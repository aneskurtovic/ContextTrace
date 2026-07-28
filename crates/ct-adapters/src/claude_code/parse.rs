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

/// How much of a file to read while deciding what it is.
///
/// A budget, not a structural claim. Refusing to read an unbounded prelude
/// protects against a corrupt file with no newlines at all, and every real
/// session answers both questions below within a few kilobytes.
const PRELUDE_BYTES: u64 = 1024 * 1024;

#[derive(Default)]
pub struct Header {
    pub cwd: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
    /// Whether any line in the prelude carries a `uuid`.
    ///
    /// The test for "is this a session transcript at all". A Claude Code
    /// session is a DAG of `uuid`-keyed events, so a file with no `uuid`
    /// anywhere has no node the ancestor walk could start from — discovery
    /// offering it would be offering a file the adapter cannot use.
    pub has_conversation: bool,
}

/// Read enough of a file's prelude to describe it, and to tell whether it is a
/// session at all.
///
/// Not a first-line read, because 90 of 711 local sessions open with a sidecar
/// line — `last-prompt`, `mode`, `queue-operation`, `ai-title` — that carries no
/// `uuid`. Judging by the first line alone would discard all of them. The first
/// `uuid`-bearing line sits at depth 1 in the median case and 10 at the worst
/// observed, while the four files that are not sessions have none at any depth.
pub fn read_header(path: &Path) -> PortResult<Header> {
    let file = File::open(path).map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;
    let mut reader = BufReader::new(file.take(PRELUDE_BYTES));
    let io = |e: std::io::Error| PortError::Io(format!("{}: {e}", path.display()));

    // Descriptor metadata still comes from the first line only, unchanged: the
    // directory slug is the established fallback where it is absent.
    let mut first = String::new();
    let mut consumed = reader.read_line(&mut first).map_err(io)? as u64;
    let head = serde_json::from_str::<Value>(first.trim()).ok();

    let mut has_conversation = head.as_ref().is_some_and(|v| v.get("uuid").is_some());
    let mut line = String::new();
    while !has_conversation {
        line.clear();
        match reader.read_line(&mut line).map_err(io)? {
            0 => break,
            n => consumed += n as u64,
        }
        if let Ok(value) = serde_json::from_str::<Value>(line.trim()) {
            has_conversation = value.get("uuid").is_some();
        }
    }

    Ok(Header {
        cwd: head.as_ref().and_then(|v| str_field(v, "cwd")),
        timestamp: head.as_ref().and_then(|v| parse_time(v.get("timestamp"))),
        // Exhausting the budget establishes nothing, so it fails open. Only a
        // file read to its end without a `uuid` is *known* not to be a
        // transcript; one whose first megabyte is a single enormous pasted
        // message is merely unread, and discarding it would be a worse error
        // than keeping the four journals.
        has_conversation: has_conversation || consumed >= PRELUDE_BYTES,
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
                target: tool.get("input").and_then(crate::tool_target::describe),
            };
        }
        let has_thinking = blocks.iter().any(|b| block_type(b) == Some("thinking"));
        let has_text = blocks.iter().any(|b| block_type(b) == Some("text"));
        if has_thinking && !has_text {
            return EventKind::Reasoning {
                char_len,
                redacted: blocks.iter().any(is_redacted_thinking),
            };
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

/// Signature characters per character of thinking text.
///
/// Extended-thinking blocks are written to the log with their text removed and
/// only an opaque `signature` left behind -- 5,820 of 5,869 thinking blocks in
/// the local corpus, some 27 million characters of signature in total. Counting
/// those blocks as zero would silently drop the largest single category of
/// unlogged context.
///
/// # Why a regression slope and not the median ratio
///
/// 49 blocks in the corpus retained both their text and their signature. The
/// obvious statistic -- the median of `signature / thinking` -- gives 2.09, and
/// it is *wrong to apply here*: those ratios are strongly size-dependent (6.07
/// at 60 characters of thinking, 2.49 at 5,957), so a median drawn from a sample
/// whose typical block is 936 characters cannot be applied to redacted blocks
/// averaging 3,454.
///
/// Regressing signature length on thinking length instead gives slope 2.353 with
/// an intercept of -175 and an R² of 0.9707: near-proportional, tightly fitted,
/// and stable across the size range that matters. Using the median would inflate
/// every redacted block by about 12%.
///
/// This is a measurement of an observed relationship, not a claim about how the
/// signature is encoded, and it only ever produces an *estimate*.
const SIGNATURE_CHARS_PER_THINKING_CHAR: f32 = 2.353;

/// Character budget standing in for an image's ~1,600-token ceiling.
///
/// Anthropic's documented cost is about `(width * height) / 750` tokens after
/// resizing to at most 1,568 pixels a side, which tops out near 1,600 tokens.
/// These sessions run between two and four characters per token, so four
/// thousand characters is a deliberately generous equivalent -- generous because
/// over-stating a rare item is safer than hiding it.
const IMAGE_MAX_EQUIVALENT_CHARS: usize = 4_000;

/// Characters of *model-visible text* across a message's content blocks.
///
/// # Why this is not `to_string()` on the JSON
///
/// The model reads the text inside the blocks, not the JSON that transports it.
/// Serialising a block counts key names, braces, and -- much worse -- escaping:
/// every newline in a tool result becomes `\n`, and every Windows path
/// separator becomes `\\`. In agent transcripts, which are mostly code and
/// paths, that inflates the count substantially and it inflates it *unevenly*,
/// so it distorts the proportions rather than cancelling out in calibration.
///
/// The exception is `tool_use.input`, which the model genuinely does see as
/// serialised JSON, escapes included.
pub(crate) fn content_chars(blocks: &[Value]) -> u32 {
    let total: usize = blocks.iter().map(block_chars).sum();
    total.min(u32::MAX as usize) as u32
}

fn block_chars(block: &Value) -> usize {
    match block_type(block) {
        Some("thinking") => {
            let text = block
                .get("thinking")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !text.is_empty() {
                return text.chars().count();
            }
            // Redacted: derive the size from what is left.
            let signature = block
                .get("signature")
                .and_then(Value::as_str)
                .map(|s| s.chars().count())
                .unwrap_or(0);
            (signature as f32 / SIGNATURE_CHARS_PER_THINKING_CHAR) as usize
        }
        Some("text") => block
            .get("text")
            .and_then(Value::as_str)
            .map(|s| s.chars().count())
            .unwrap_or(0),
        // Tool arguments really are sent as JSON.
        Some("tool_use") => block
            .get("input")
            .map(|v| match v {
                Value::String(s) => s.chars().count(),
                other => other.to_string().chars().count(),
            })
            .unwrap_or(0),
        Some("tool_result") => block.get("content").map(text_chars).unwrap_or(0),
        // Inline images are sent as base64 and are large, but they do not cost
        // tokens like text does: Anthropic resizes to at most 1,568x1568 and
        // charges roughly (width x height) / 750, so a single image can never
        // exceed about 1,600 tokens however many base64 characters it occupies.
        // Passing the raw base64 length through a characters-per-token ratio
        // would price one screenshot at tens of thousands of tokens.
        Some("image") => block
            .get("source")
            .and_then(|s| s.get("data"))
            .and_then(Value::as_str)
            .map(|s| s.chars().count().min(IMAGE_MAX_EQUIVALENT_CHARS))
            .unwrap_or(0),
        // An unfamiliar block still contributes its text rather than nothing,
        // so a new block type shows up as weight instead of vanishing.
        _ => text_chars(block),
    }
}

/// Keys that carry structure or identifiers rather than text the model reads.
const NON_TEXT_KEYS: [&str; 7] = [
    "type",
    "signature",
    "id",
    "tool_use_id",
    "cache_control",
    "is_error",
    "name",
];

/// Total length of the string leaves of a JSON value.
fn text_chars(value: &Value) -> usize {
    text_chars_at(value, 0)
}

fn text_chars_at(value: &Value, depth: u8) -> usize {
    // Bounded because these documents are attacker-adjacent: they are whatever
    // an agent happened to write, and recursion depth should not depend on it.
    if depth > 12 {
        return 0;
    }
    match value {
        Value::String(s) => s.chars().count(),
        Value::Array(items) => items.iter().map(|v| text_chars_at(v, depth + 1)).sum(),
        Value::Object(map) => map
            .iter()
            .filter(|(k, _)| !NON_TEXT_KEYS.contains(&k.as_str()))
            .map(|(_, v)| text_chars_at(v, depth + 1))
            .sum(),
        _ => 0,
    }
}

/// Whether a content block's thinking text was redacted from the log.
pub(crate) fn is_redacted_thinking(block: &Value) -> bool {
    block_type(block) == Some("thinking")
        && block
            .get("thinking")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        && block.get("signature").is_some()
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
        if let Some(value) = attachment.get(key) {
            total += text_chars(value);
            counted = true;
        }
    }

    // Nothing recognised: fall back to the attachment's own text so an unknown
    // injection type still contributes its approximate weight rather than
    // silently counting as zero.
    if !counted {
        total = text_chars(attachment);
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
///
/// # Why the top-level figures are not always a prompt size
///
/// One assistant message can be produced by several API calls, which the log
/// records in an `iterations` array. When it does, the *top-level*
/// `cache_creation_input_tokens` and `cache_read_input_tokens` are the sums
/// across those calls -- verified on 466 of 474 multi-iteration records in the
/// local corpus, where both fields matched the sum exactly.
///
/// Summing them therefore reports a prompt far larger than any that was sent:
/// one record totals 844,611 tokens while its largest single call is 429,328.
///
/// The prompt we want is the biggest single request, because "what was in the
/// model's context at this turn" is a property of one call. The last call is
/// usually within a fraction of a percent of the largest, but the largest is the
/// high-water mark the rest of the tool talks about.
pub(crate) fn usage_from_message(v: &Value) -> Option<TokenUsage> {
    let usage = v.get("message")?.get("usage")?;

    let mut chosen = usage;
    let mut calls = 1u32;
    if let Some(iterations) = usage.get("iterations").and_then(Value::as_array) {
        if iterations.len() > 1 {
            calls = iterations.len().min(u32::MAX as usize) as u32;
            if let Some(largest) = iterations.iter().max_by_key(|it| prompt_sum(it)) {
                chosen = largest;
            }
        }
    }

    Some(TokenUsage {
        input: u32_field(chosen, "input_tokens"),
        cache_creation: u32_field(chosen, "cache_creation_input_tokens"),
        cache_read: u32_field(chosen, "cache_read_input_tokens"),
        // Output is genuinely produced by every call, so the total is the sum
        // the log already gives us -- unlike the input side, it is not a
        // repeated prefix being counted several times.
        output: u32_field(usage, "output_tokens"),
        reasoning: None,
        context_window: None,
        api_calls: Some(calls),
    })
}

/// Prompt size of one usage object, for picking the largest call.
fn prompt_sum(v: &Value) -> u64 {
    ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"]
        .iter()
        .filter_map(|k| v.get(*k).and_then(Value::as_u64))
        .sum()
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
            EventKind::ToolCall {
                tool,
                call_id,
                target,
                ..
            } => {
                assert_eq!(tool, "Bash");
                assert_eq!(call_id.as_deref(), Some("toolu_1"));
                // Without this the biggest row in a context breakdown reads
                // "Tool output: Bash" and says nothing about which command.
                assert_eq!(target.as_deref(), Some("ls"));
            }
            other => panic!("expected a tool call, got {other:?}"),
        }
    }

    #[test]
    fn a_file_read_records_the_path_it_read() {
        let line = json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "tool_use", "id": "toolu_2", "name": "Read",
                 "input": {"file_path": "src/schema.ts", "limit": 200}}
            ]}
        });
        match assistant_kind(&line) {
            EventKind::ToolCall { target, .. } => {
                assert_eq!(target.as_deref(), Some("src/schema.ts"))
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
    fn a_multi_call_response_reports_one_call_not_their_sum() {
        // Modelled on a real record: three API calls behind one assistant
        // message, whose top-level cache figures are the sum across all three.
        let line = json!({
            "type": "assistant",
            "message": {"usage": {
                "input_tokens": 4,
                "cache_creation_input_tokens": 10_700,
                "cache_read_input_tokens": 137_532,
                "output_tokens": 2000,
                "iterations": [
                    {"input_tokens": 76_603, "cache_creation_input_tokens": 0,
                     "cache_read_input_tokens": 0},
                    {"input_tokens": 2, "cache_creation_input_tokens": 5_924,
                     "cache_read_input_tokens": 65_804},
                    {"input_tokens": 2, "cache_creation_input_tokens": 4_776,
                     "cache_read_input_tokens": 71_728}
                ]
            }}
        });
        let usage = usage_from_message(&line).unwrap();
        assert_eq!(
            usage.prompt_tokens(),
            Some(76_603),
            "the top-level sum (148,236) is three prompts added together, not one prompt"
        );
        assert_eq!(usage.api_calls, Some(3), "the aggregation must stay visible");
    }

    #[test]
    fn a_single_call_response_is_read_from_the_top_level() {
        let line = json!({
            "type": "assistant",
            "message": {"usage": {
                "input_tokens": 131,
                "cache_creation_input_tokens": 2_011,
                "cache_read_input_tokens": 913_787,
                "iterations": [
                    {"input_tokens": 131, "cache_creation_input_tokens": 2_011,
                     "cache_read_input_tokens": 913_787}
                ]
            }}
        });
        let usage = usage_from_message(&line).unwrap();
        assert_eq!(usage.prompt_tokens(), Some(915_929));
        assert_eq!(usage.api_calls, Some(1));
    }

    #[test]
    fn json_punctuation_and_escaping_are_not_counted_as_model_input() {
        // The model reads the text inside the block. Serialising the block would
        // additionally count the key names, the braces, and the backslashes that
        // escaping adds to every newline and Windows path separator.
        let text = "line one\nline two\nC:\\repo\\file.rs";
        let blocks = vec![json!({
            "type": "tool_result",
            "tool_use_id": "toolu_0123456789abcdef",
            "content": [{"type": "text", "text": text}]
        })];
        assert_eq!(
            content_chars(&blocks),
            text.chars().count() as u32,
            "only the text itself is model input"
        );
    }

    #[test]
    fn tool_arguments_are_counted_as_the_json_the_model_actually_sees() {
        let input = json!({"command": "ls -la", "description": "list"});
        let blocks = vec![json!({
            "type": "tool_use", "id": "toolu_1", "name": "Bash", "input": input
        })];
        let expected = json!({"command": "ls -la", "description": "list"})
            .to_string()
            .chars()
            .count() as u32;
        assert_eq!(content_chars(&blocks), expected);
    }

    #[test]
    fn redacted_thinking_is_sized_from_its_signature_rather_than_counted_as_zero() {
        // 99.2% of thinking blocks in the corpus look like this: no text, a
        // large signature, and context that was genuinely occupied.
        let signature: String = "s".repeat(23_530);
        let blocks = vec![json!({
            "type": "thinking", "thinking": "", "signature": signature
        })];
        let chars = content_chars(&blocks);
        assert_eq!(
            chars, 10_000,
            "signature length divided by the regressed 2.353 slope"
        );

        let line = json!({"type": "assistant", "message": {"content": blocks}});
        match assistant_kind(&line) {
            EventKind::Reasoning { redacted, char_len } => {
                assert!(redacted, "the derivation must be visible to diagnostics");
                assert_eq!(char_len, 10_000);
            }
            other => panic!("expected reasoning, got {other:?}"),
        }
    }

    #[test]
    fn thinking_that_kept_its_text_is_measured_not_derived() {
        let blocks = vec![json!({
            "type": "thinking",
            "thinking": "twelve chars",
            "signature": "ignored-because-the-text-is-here"
        })];
        assert_eq!(content_chars(&blocks), 12);

        let line = json!({"type": "assistant", "message": {"content": blocks}});
        assert!(matches!(
            assistant_kind(&line),
            EventKind::Reasoning { redacted: false, .. }
        ));
    }

    #[test]
    fn an_unfamiliar_block_type_still_contributes_its_text() {
        let blocks = vec![json!({
            "type": "some_future_block", "body": "abcdefghij", "id": "not-text"
        })];
        assert_eq!(
            content_chars(&blocks),
            10,
            "a new block type must not silently weigh nothing"
        );
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
