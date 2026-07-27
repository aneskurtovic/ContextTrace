//! Translating Claude Code's JSONL vocabulary into domain events.

use crate::jsonl::{self, LineRecord};
use ct_domain::model::event::{CompactionFacts, EventLinks};
use ct_domain::ports::{PortError, PortResult};
use ct_domain::{
    AgentKind, AgentSession, Event, EventId, EventKind, FileId, MessageRole, SessionId,
    SessionMetadata, SourceRef, TokenUsage, Turn, TurnNumber,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

#[derive(Default)]
pub struct Header {
    pub cwd: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
}

/// Read enough of the first line to describe the session.
pub fn read_header(path: &Path) -> PortResult<Header> {
    let file = File::open(path).map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;
    let mut line = String::new();
    let mut reader = BufReader::new(file.take(1024 * 1024));
    reader
        .read_line(&mut line)
        .map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;

    let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
        return Ok(Header::default());
    };

    Ok(Header {
        cwd: str_field(&value, "cwd"),
        timestamp: parse_time(value.get("timestamp")),
    })
}

/// Parse a full Claude Code session.
pub fn load(path: &Path, id: SessionId) -> PortResult<AgentSession> {
    let mut events: Vec<Event> = Vec::new();
    let mut metadata = SessionMetadata::default();
    let mut unrecognised: BTreeMap<String, u32> = BTreeMap::new();
    // Request grouping data, parallel to `events`. Held in a side channel
    // rather than on `Event` because usage belongs to a *request*, and one
    // request spans several message lines -- putting it on the event would
    // imply a per-line figure that does not exist.
    let mut extras: Vec<LineExtras> = Vec::new();

    jsonl::read_lines(path, |record| {
        let (event, line_extras) = translate(&record, &mut metadata);
        if matches!(event.kind, EventKind::Unrecognised) {
            *unrecognised.entry(event.raw_type.clone()).or_insert(0) += 1;
        }
        events.push(event);
        extras.push(line_extras);
    })?;

    let turns = derive_turns(&mut events, &extras);

    if metadata.last_activity.is_none() {
        metadata.last_activity = events.iter().rev().find_map(|e| e.timestamp);
    }
    if metadata.started_at.is_none() {
        metadata.started_at = events.iter().find_map(|e| e.timestamp);
    }

    Ok(AgentSession::new(
        id,
        AgentKind::ClaudeCode,
        metadata,
        events,
        turns,
        unrecognised.into_iter().collect(),
    ))
}

/// Per-line data needed for turn grouping but not part of the domain model.
#[derive(Default, Clone)]
struct LineExtras {
    request_id: Option<String>,
    usage: Option<TokenUsage>,
    model: Option<String>,
}

/// Map one line onto a domain event, plus the grouping data it carries.
fn translate(record: &LineRecord, metadata: &mut SessionMetadata) -> (Event, LineExtras) {
    let source = SourceRef::new(FileId(0), record.offset, record.len, record.line_no);
    let raw_type = record.type_str().unwrap_or("unknown").to_string();
    let value = record.value.as_ref();
    let timestamp = value.and_then(|v| parse_time(v.get("timestamp")));

    let links = value
        .map(|v| EventLinks {
            uuid: str_field(v, "uuid"),
            parent_uuid: str_field(v, "parentUuid"),
            logical_parent_uuid: str_field(v, "logicalParentUuid"),
            is_sidechain: v
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .unwrap_or_default();

    let mut extras = LineExtras::default();

    let kind = match value {
        // Oversized or unparseable: keep it as a session event so its recorded
        // byte length still contributes, rather than losing it entirely.
        None => EventKind::SessionEvent {
            subtype: raw_type.clone(),
        },
        Some(v) => {
            absorb_metadata(v, metadata, timestamp);
            match raw_type.as_str() {
                "assistant" => {
                    extras.request_id = str_field(v, "requestId")
                        .or_else(|| v.get("message").and_then(|m| str_field(m, "id")));
                    extras.usage = usage_from_message(v);
                    extras.model = v.get("message").and_then(|m| str_field(m, "model"));
                    assistant_kind(v)
                }
                "user" => user_kind(v),
                "attachment" => attachment_kind(v),
                "system" => system_kind(v),
                // Recognised sidecar metadata. These lines are written to the
                // log but never sent to the model, so they are classified (not
                // left unrecognised) while contributing nothing to context.
                "file-history-snapshot" | "file-history-delta" | "pr-link" | "frame-link"
                | "ai-title" | "custom-title" | "agent-name" | "mode" | "permission-mode"
                | "agent-setting" | "last-prompt" | "queue-operation" | "worktree-state"
                | "bridge-session" | "relocated" | "summary" => EventKind::SessionEvent {
                    subtype: raw_type.clone(),
                },
                _ => EventKind::Unrecognised,
            }
        }
    };

    let event = Event {
        id: links
            .uuid
            .clone()
            .map(EventId::Uuid)
            .unwrap_or(EventId::Ordinal(record.line_no)),
        sequence: record.line_no.saturating_sub(1),
        timestamp,
        kind,
        source,
        raw_type,
        turn: None,
        links,
    };

    (event, extras)
}

/// Classify an `assistant` line.
///
/// One line holds one *content block group* of a response. Priority order
/// matters: a line carrying a tool call is most usefully shown as a tool call,
/// even when it also carries text.
fn assistant_kind(v: &Value) -> EventKind {
    let message = v.get("message").unwrap_or(&Value::Null);
    let blocks = message.get("content").and_then(Value::as_array);
    let char_len = blocks.map(|b| content_chars(b)).unwrap_or(0);

    if let Some(blocks) = blocks {
        if let Some(tool) = blocks.iter().find(|b| block_type(b) == Some("tool_use")) {
            return EventKind::ToolCall {
                tool: str_field(tool, "name").unwrap_or_else(|| "unknown".into()),
                call_id: str_field(tool, "id"),
                char_len,
            };
        }
        let has_thinking = blocks.iter().any(|b| block_type(b) == Some("thinking"));
        let has_text = blocks.iter().any(|b| block_type(b) == Some("text"));
        if has_thinking && !has_text {
            return EventKind::Reasoning { char_len };
        }
    }

    EventKind::Message {
        role: MessageRole::Assistant,
        preview: first_text(blocks, 160),
        char_len,
    }
}

/// Classify a `user` line, which carries either a human prompt or tool results.
fn user_kind(v: &Value) -> EventKind {
    let message = v.get("message").unwrap_or(&Value::Null);
    let content = message.get("content");

    // A plain string is always a human prompt.
    if let Some(Value::String(s)) = content {
        return EventKind::Message {
            role: MessageRole::User,
            preview: ct_domain::ports::truncate_chars(s.trim(), 160),
            char_len: s.chars().count() as u32,
        };
    }

    let blocks = content.and_then(Value::as_array);
    let char_len = blocks.map(|b| content_chars(b)).unwrap_or(0);

    if let Some(blocks) = blocks {
        if let Some(result) = blocks.iter().find(|b| block_type(b) == Some("tool_result")) {
            return EventKind::ToolResult {
                tool: None,
                call_id: str_field(result, "tool_use_id"),
                char_len,
                is_error: result
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            };
        }
    }

    EventKind::Message {
        role: MessageRole::User,
        preview: first_text(blocks, 160),
        char_len,
    }
}

/// Classify an `attachment` line -- the injected-context surface.
///
/// The attachment's own `type` is carried through as the mechanism, which is
/// what makes instruction provenance *observed*: the harness already told us
/// why this content is in the prompt.
fn attachment_kind(v: &Value) -> EventKind {
    let attachment = v.get("attachment").unwrap_or(&Value::Null);
    let mechanism = str_field(attachment, "type").unwrap_or_else(|| "attachment".into());
    let label = attachment_label(attachment, &mechanism);

    EventKind::ContextInjection {
        mechanism,
        label,
        char_len: attachment_chars(attachment),
    }
}

/// Human-facing name for an injected item, preferring a real path when present.
fn attachment_label(attachment: &Value, mechanism: &str) -> String {
    for key in ["displayPath", "path", "filename", "planFilePath", "skillDir"] {
        if let Some(p) = str_field(attachment, key) {
            return p;
        }
    }
    match mechanism {
        "skill_listing" => "Skill listing".into(),
        "invoked_skills" => "Invoked skills".into(),
        "agent_listing_delta" => "Agent listing".into(),
        "mcp_instructions_delta" => "MCP server instructions".into(),
        "deferred_tools_delta" => "Deferred tool definitions".into(),
        "task_reminder" => "Task reminder".into(),
        "queued_command" => "Queued command".into(),
        "command_permissions" => "Command permissions".into(),
        "date_change" => "Date change".into(),
        other => other.replace('_', " "),
    }
}

/// Classify a `system` line. The one that matters is `compact_boundary`.
fn system_kind(v: &Value) -> EventKind {
    let subtype = str_field(v, "subtype").unwrap_or_else(|| "system".into());
    if subtype != "compact_boundary" {
        return EventKind::SessionEvent { subtype };
    }

    let meta = v.get("compactMetadata").unwrap_or(&Value::Null);
    EventKind::Compacted(CompactionFacts {
        trigger: str_field(meta, "trigger"),
        tokens_before: u32_field(meta, "preTokens"),
        tokens_after: u32_field(meta, "postTokens"),
        cumulative_dropped: u32_field(meta, "cumulativeDroppedTokens"),
        duration_ms: meta.get("durationMs").and_then(Value::as_u64),
        // Claude Code records the size of what was dropped but not its content,
        // unlike Codex which stores the full replacement history.
        replacement_recorded: false,
    })
}

fn absorb_metadata(v: &Value, metadata: &mut SessionMetadata, timestamp: Option<DateTime<Utc>>) {
    if metadata.working_directory.is_none() {
        metadata.working_directory = str_field(v, "cwd");
        metadata.project = metadata.working_directory.clone();
    }
    if metadata.git_branch.is_none() {
        metadata.git_branch = str_field(v, "gitBranch");
    }
    if metadata.agent_version.is_none() {
        metadata.agent_version = str_field(v, "version");
    }
    if metadata.model.is_none() {
        if let Some(model) = v.get("message").and_then(|m| str_field(m, "model")) {
            // `<synthetic>` marks harness-generated messages, not a real model.
            if !model.starts_with('<') {
                metadata.model = Some(model);
            }
        }
    }
    if metadata.started_at.is_none() {
        metadata.started_at = timestamp;
    }
}

/// Group assistant lines into turns.
///
/// **This is the subtle part.** One API response is written as several
/// `assistant` lines -- thinking, text, and one per tool call -- all sharing a
/// `requestId` and all carrying an *identical* `usage` object. Treating each
/// line as a turn would inflate the turn count several-fold and report the same
/// prompt size repeatedly, so lines are grouped by request id and the group's
/// last line anchors the turn.
fn derive_turns(events: &mut [Event], extras: &[LineExtras]) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    let mut current_request: Option<String> = None;
    let mut pending: Vec<usize> = Vec::new();
    let mut number = 1u32;

    for index in 0..events.len() {
        pending.push(index);

        // Only assistant lines carry usage, and only they end a request.
        let Some(usage) = extras[index].usage else {
            continue;
        };
        let request = extras[index].request_id.clone();

        // A continuation of the request we are already in: absorb these lines
        // into the open turn and move its anchor forward, rather than opening a
        // second turn reporting the same prompt size.
        let continues = request.is_some() && request == current_request;
        if continues {
            if let Some(open) = turns.last_mut() {
                let absorbed: Vec<usize> = std::mem::take(&mut pending);
                for &i in &absorbed {
                    events[i].turn = Some(open.number);
                }
                open.event_indices.extend(absorbed);
                open.anchor_index = Some(index);
                continue;
            }
        }

        let Ok(turn_number) = TurnNumber::new(number) else {
            continue;
        };
        let indices: Vec<usize> = std::mem::take(&mut pending);
        for &i in &indices {
            events[i].turn = Some(turn_number);
        }
        turns.push(Turn {
            number: turn_number,
            timestamp: events[index].timestamp,
            model: extras[index]
                .model
                .clone()
                .filter(|m| !m.starts_with('<')),
            usage,
            event_indices: indices,
            anchor_index: Some(index),
        });
        current_request = request;
        number += 1;
    }

    turns
}

// ---------------------------------------------------------------------------
// JSON helpers. All tolerate absent or unexpectedly-typed fields by returning
// None, which is the mechanism behind "never crash on a new JSONL field".
// ---------------------------------------------------------------------------

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(str::to_string)
}

fn u32_field(v: &Value, key: &str) -> Option<u32> {
    v.get(key)?.as_u64().map(|n| n.min(u32::MAX as u64) as u32)
}

fn parse_time(v: Option<&Value>) -> Option<DateTime<Utc>> {
    let s = v?.as_str()?;
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn block_type(block: &Value) -> Option<&str> {
    block.get("type")?.as_str()
}

/// Characters across a message's content blocks.
pub(crate) fn content_chars(blocks: &[Value]) -> u32 {
    let mut total: usize = 0;
    for block in blocks {
        for key in ["text", "thinking"] {
            if let Some(s) = block.get(key).and_then(Value::as_str) {
                total += s.chars().count();
            }
        }
        // Tool call arguments and tool results are objects or strings.
        for key in ["input", "content"] {
            match block.get(key) {
                Some(Value::String(s)) => total += s.chars().count(),
                Some(other @ (Value::Object(_) | Value::Array(_))) => {
                    total += other.to_string().chars().count()
                }
                _ => {}
            }
        }
        // Inline images are sent as base64 and are large; count them.
        if let Some(src) = block.get("source").and_then(|s| s.get("data")) {
            if let Some(s) = src.as_str() {
                total += s.chars().count();
            }
        }
    }
    total.min(u32::MAX as usize) as u32
}

/// Size of an attachment's injected content.
fn attachment_chars(attachment: &Value) -> u32 {
    let mut total: usize = 0;
    let mut counted = false;

    for key in [
        "content",
        "stdout",
        "planContent",
        "snippet",
        "prompt",
        "skills",
        "addedLines",
        "addedBlocks",
    ] {
        match attachment.get(key) {
            Some(Value::String(s)) => {
                total += s.chars().count();
                counted = true;
            }
            Some(other @ (Value::Object(_) | Value::Array(_))) => {
                total += other.to_string().chars().count();
                counted = true;
            }
            _ => {}
        }
    }

    // Nothing recognised: fall back to the serialised attachment so an unknown
    // injection type still contributes its approximate weight rather than
    // silently counting as zero.
    if !counted {
        total = attachment.to_string().chars().count();
    }

    total.min(u32::MAX as usize) as u32
}

fn first_text(blocks: Option<&Vec<Value>>, max: usize) -> String {
    let text = blocks
        .and_then(|bs| {
            bs.iter()
                .find_map(|b| b.get("text").and_then(Value::as_str))
        })
        .unwrap_or_default();
    ct_domain::ports::truncate_chars(text.trim(), max)
}

/// Extract usage from an assistant line's `message.usage`.
pub(crate) fn usage_from_message(v: &Value) -> Option<TokenUsage> {
    let usage = v.get("message")?.get("usage")?;
    Some(TokenUsage {
        input: u32_field(usage, "input_tokens"),
        cache_creation: u32_field(usage, "cache_creation_input_tokens"),
        cache_read: u32_field(usage, "cache_read_input_tokens"),
        output: u32_field(usage, "output_tokens"),
        reasoning: None,
        context_window: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prompt_size_includes_cache_fields() {
        let line = json!({
            "type": "assistant",
            "message": {"usage": {
                "input_tokens": 2,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 279_943,
                "output_tokens": 1785
            }}
        });
        let usage = usage_from_message(&line).unwrap();
        assert_eq!(
            usage.prompt_tokens(),
            Some(279_945),
            "reading input_tokens alone would report 2 for a 280k-token prompt"
        );
    }

    #[test]
    fn a_tool_call_line_is_classified_as_a_tool_call() {
        let line = json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "text", "text": "let me check"},
                {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "ls"}}
            ]}
        });
        match assistant_kind(&line) {
            EventKind::ToolCall { tool, call_id, .. } => {
                assert_eq!(tool, "Bash");
                assert_eq!(call_id.as_deref(), Some("toolu_1"));
            }
            other => panic!("expected a tool call, got {other:?}"),
        }
    }

    #[test]
    fn a_thinking_only_line_is_reasoning_not_a_message() {
        let line = json!({
            "type": "assistant",
            "message": {"content": [{"type": "thinking", "thinking": "hmm"}]}
        });
        assert!(matches!(assistant_kind(&line), EventKind::Reasoning { .. }));
    }

    #[test]
    fn tool_results_are_distinguished_from_human_prompts() {
        let result = json!({
            "type": "user",
            "message": {"content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": "output here"}
            ]}
        });
        assert!(matches!(user_kind(&result), EventKind::ToolResult { .. }));

        let prompt = json!({"type": "user", "message": {"content": "do the thing"}});
        match user_kind(&prompt) {
            EventKind::Message { role, char_len, .. } => {
                assert_eq!(role, MessageRole::User);
                assert_eq!(char_len, 12);
            }
            other => panic!("expected a user message, got {other:?}"),
        }
    }

    #[test]
    fn an_errored_tool_result_is_flagged() {
        let line = json!({
            "type": "user",
            "message": {"content": [
                {"type": "tool_result", "tool_use_id": "t", "content": "boom", "is_error": true}
            ]}
        });
        assert!(matches!(user_kind(&line), EventKind::ToolResult { is_error: true, .. }));
    }

    #[test]
    fn nested_memory_attachments_carry_their_path() {
        let line = json!({
            "type": "attachment",
            "attachment": {
                "type": "nested_memory",
                "path": "C:\\repo\\server\\CLAUDE.md",
                "displayPath": "server\\CLAUDE.md",
                "content": "# Server rules"
            }
        });
        match attachment_kind(&line) {
            EventKind::ContextInjection { mechanism, label, char_len } => {
                assert_eq!(mechanism, "nested_memory");
                assert_eq!(label, "server\\CLAUDE.md", "provenance is observed, not guessed");
                assert_eq!(char_len, 14);
            }
            other => panic!("expected a context injection, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_attachment_type_still_contributes_weight() {
        let line = json!({
            "type": "attachment",
            "attachment": {"type": "some_future_injection", "mystery": "abcdefghij"}
        });
        match attachment_kind(&line) {
            EventKind::ContextInjection { mechanism, char_len, .. } => {
                assert_eq!(mechanism, "some_future_injection");
                assert!(char_len > 0, "unknown injections must not count as zero tokens");
            }
            other => panic!("expected a context injection, got {other:?}"),
        }
    }

    #[test]
    fn compact_boundaries_carry_observed_before_and_after_totals() {
        let line = json!({
            "type": "system",
            "subtype": "compact_boundary",
            "compactMetadata": {
                "trigger": "manual",
                "preTokens": 165_223,
                "postTokens": 17_542,
                "cumulativeDroppedTokens": 479_060,
                "durationMs": 125_281
            }
        });
        match system_kind(&line) {
            EventKind::Compacted(facts) => {
                assert_eq!(facts.trigger.as_deref(), Some("manual"));
                assert_eq!(facts.tokens_before, Some(165_223));
                assert_eq!(facts.tokens_after, Some(17_542));
                assert!(!facts.replacement_recorded, "Claude Code records sizes, not content");
            }
            other => panic!("expected a compaction, got {other:?}"),
        }
    }

    #[test]
    fn other_system_subtypes_are_recognised_not_flagged_as_unknown() {
        let line = json!({"type": "system", "subtype": "turn_duration"});
        assert!(matches!(system_kind(&line), EventKind::SessionEvent { .. }));
    }

    #[test]
    fn synthetic_models_do_not_become_the_session_model() {
        let mut meta = SessionMetadata::default();
        absorb_metadata(&json!({"message": {"model": "<synthetic>"}}), &mut meta, None);
        assert_eq!(meta.model, None);
        absorb_metadata(&json!({"message": {"model": "claude-opus-4-8"}}), &mut meta, None);
        assert_eq!(meta.model.as_deref(), Some("claude-opus-4-8"));
    }

    #[test]
    fn content_chars_counts_inline_images() {
        let blocks = vec![json!({
            "type": "image",
            "source": {"type": "base64", "data": "AAAABBBB"}
        })];
        assert_eq!(content_chars(&blocks), 8);
    }
}
