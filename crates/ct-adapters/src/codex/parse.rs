//! Translating Codex's JSONL vocabulary into domain events.

use crate::jsonl::{self, LineRecord};
use ct_domain::model::event::{CompactionFacts, EventLinks};
use ct_domain::ports::{PortError, PortResult};
use ct_domain::{
    AgentKind, AgentSession, Event, EventId, EventKind, FileId, MessageRole, SessionId,
    SessionMetadata, SourceRef, TokenUsage, Turn, TurnNumber,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// The handful of fields worth reading from a session's first line.
#[derive(Default)]
pub struct Header {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
}

/// Read only the `session_meta` header line.
pub fn read_header(path: &Path) -> PortResult<Header> {
    let file = File::open(path).map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;
    let mut line = String::new();
    // 1 MiB cap: a header line is tiny, and refusing to read an unbounded first
    // line protects against a corrupt file with no newlines at all. The cap goes
    // on the `File` so the `BufReader` wrapping it still offers `read_line`.
    let mut reader = BufReader::new(file.take(1024 * 1024));
    reader
        .read_line(&mut line)
        .map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;

    let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
        return Ok(Header::default());
    };
    let payload = value.get("payload").unwrap_or(&Value::Null);

    Ok(Header {
        session_id: str_field(payload, "session_id").or_else(|| str_field(payload, "id")),
        cwd: str_field(payload, "cwd"),
        timestamp: parse_time(value.get("timestamp")),
    })
}

/// Parse a full Codex session.
pub fn load(path: &Path, id: SessionId) -> PortResult<AgentSession> {
    let mut events: Vec<Event> = Vec::new();
    let mut metadata = SessionMetadata::default();
    let mut unrecognised: BTreeMap<String, u32> = BTreeMap::new();

    jsonl::read_lines(path, |record| {
        let event = translate(&record, &mut metadata);
        if matches!(event.kind, EventKind::Unrecognised) {
            *unrecognised.entry(event.raw_type.clone()).or_insert(0) += 1;
        }
        events.push(event);
    })?;

    let turns = derive_turns(&mut events, &metadata);

    if metadata.last_activity.is_none() {
        metadata.last_activity = events.iter().rev().find_map(|e| e.timestamp);
    }

    Ok(AgentSession::new(
        id,
        AgentKind::Codex,
        metadata,
        events,
        turns,
        unrecognised.into_iter().collect(),
    ))
}

/// Map one line onto a domain event.
///
/// Every unknown shape funnels into [`EventKind::Unrecognised`] rather than an
/// error. The event keeps its [`SourceRef`], so the raw inspector can still show
/// it in full and the corpus smoke test can count it -- which is how we learn
/// that Codex changed its format.
fn translate(record: &LineRecord, metadata: &mut SessionMetadata) -> Event {
    let source = SourceRef::new(FileId(0), record.offset, record.len, record.line_no);
    let value = record.value.as_ref();
    let raw_outer = record.type_str().unwrap_or("unknown").to_string();
    let timestamp = value.and_then(|v| parse_time(v.get("timestamp")));
    let payload = value.and_then(|v| v.get("payload")).unwrap_or(&Value::Null);

    // An oversized line was never parsed, so only its outer type is known. Its
    // recorded byte length still feeds size estimation.
    if record.oversized {
        let kind = match raw_outer.as_str() {
            "compacted" => EventKind::Compacted(CompactionFacts {
                replacement_recorded: true,
                ..Default::default()
            }),
            _ => EventKind::SessionEvent {
                subtype: raw_outer.clone(),
            },
        };
        return Event {
            id: EventId::Ordinal(record.line_no),
            sequence: record.line_no.saturating_sub(1),
            timestamp,
            kind,
            source,
            raw_type: raw_outer,
            turn: None,
            links: EventLinks::default(),
        };
    }

    let inner = str_field(payload, "type");
    let raw_type = match (&raw_outer[..], inner.as_deref()) {
        ("response_item", Some(i)) | ("event_msg", Some(i)) => format!("{raw_outer}/{i}"),
        _ => raw_outer.clone(),
    };

    let kind = match raw_outer.as_str() {
        "session_meta" => {
            absorb_session_meta(payload, metadata, source, timestamp);
            // Codex records its system prompt verbatim, and that prompt really
            // was in the model's context. Emitting it as an injection rather
            // than a bare header means it flows through the same accounting as
            // everything else instead of vanishing into the residual.
            match payload.get("base_instructions") {
                Some(bi) => EventKind::ContextInjection {
                    mechanism: "base_instructions".into(),
                    label: "Codex system prompt".into(),
                    char_len: instruction_chars(bi),
                },
                None => EventKind::SessionStarted,
            }
        }
        "turn_context" => {
            if metadata.model.is_none() {
                metadata.model = str_field(payload, "model");
            }
            if metadata.working_directory.is_none() {
                metadata.working_directory = str_field(payload, "cwd");
            }
            EventKind::TurnStarted
        }
        "compacted" => EventKind::Compacted(CompactionFacts {
            replacement_recorded: payload.get("replacement_history").is_some(),
            ..Default::default()
        }),
        "response_item" => translate_response_item(payload, inner.as_deref()),
        "event_msg" => translate_event_msg(payload, inner.as_deref(), metadata),
        // Session-lifecycle records that are written to the log but never sent
        // to the model. Classified rather than left unrecognised so the
        // fidelity score stays a signal about *context* reconstruction.
        "world_state" | "inter_agent_communication_metadata" => EventKind::SessionEvent {
            subtype: raw_outer.clone(),
        },
        _ => EventKind::Unrecognised,
    };

    Event {
        id: EventId::Ordinal(record.line_no),
        sequence: record.line_no.saturating_sub(1),
        timestamp,
        kind,
        source,
        raw_type,
        turn: None,
        links: EventLinks::default(),
    }
}

/// `response_item` lines are the actual API conversation items.
fn translate_response_item(payload: &Value, inner: Option<&str>) -> EventKind {
    let char_len = content_chars(payload);
    match inner {
        Some("message") => {
            let role = match str_field(payload, "role").as_deref() {
                Some("assistant") => MessageRole::Assistant,
                Some("developer") => MessageRole::Developer,
                Some("system") => MessageRole::System,
                _ => MessageRole::User,
            };
            EventKind::Message {
                role,
                preview: preview_of(payload, 160),
                char_len,
            }
        }
        // Codex records reasoning summaries as text, so nothing is redacted.
        Some("reasoning") => EventKind::Reasoning {
            char_len,
            redacted: false,
        },
        Some("function_call") | Some("custom_tool_call") | Some("tool_search_call") => {
            EventKind::ToolCall {
                tool: str_field(payload, "name").unwrap_or_else(|| "unknown".into()),
                call_id: str_field(payload, "call_id"),
                char_len,
                // Codex encodes the arguments object as a string, so unlike
                // Claude Code this needs decoding before it can be read.
                target: str_field(payload, "arguments")
                    .as_deref()
                    .and_then(crate::tool_target::describe_encoded)
                    .or_else(|| {
                        str_field(payload, "input")
                            .as_deref()
                            .and_then(crate::tool_target::describe_encoded)
                    }),
            }
        }
        Some("function_call_output") | Some("custom_tool_call_output")
        | Some("tool_search_output") => EventKind::ToolResult {
            tool: None,
            call_id: str_field(payload, "call_id"),
            char_len,
            is_error: str_field(payload, "status").as_deref() == Some("failed"),
        },
        Some("agent_message") => EventKind::Message {
            role: MessageRole::Assistant,
            preview: preview_of(payload, 160),
            char_len,
        },
        _ => EventKind::Unrecognised,
    }
}

/// `event_msg` lines are UI/telemetry notifications. Most do not occupy context;
/// the exception that matters is `token_count`.
fn translate_event_msg(
    payload: &Value,
    inner: Option<&str>,
    metadata: &mut SessionMetadata,
) -> EventKind {
    match inner {
        Some("token_count") => {
            let info = payload.get("info").unwrap_or(&Value::Null);
            let last = info.get("last_token_usage").unwrap_or(&Value::Null);
            let window = u32_field(info, "model_context_window");
            if window.is_some() && metadata.context_window.is_none() {
                metadata.context_window = window;
            }
            EventKind::TokenReport(TokenUsage {
                input: u32_field(last, "input_tokens"),
                cache_creation: None,
                // Codex reports cached tokens as a *subset* of input_tokens,
                // unlike Anthropic which reports them separately. Folding it
                // into `cache_read` here would double-count, so it is left out
                // and `input_tokens` alone carries the prompt size.
                cache_read: None,
                output: u32_field(last, "output_tokens"),
                reasoning: u32_field(last, "reasoning_output_tokens"),
                context_window: window,
                // `last_token_usage` is by definition one request, which is why
                // this adapter reads it rather than `total_token_usage`.
                api_calls: Some(1),
            })
        }
        Some(other) => EventKind::SessionEvent {
            subtype: other.to_string(),
        },
        None => EventKind::Unrecognised,
    }
}

fn absorb_session_meta(
    payload: &Value,
    metadata: &mut SessionMetadata,
    source: SourceRef,
    timestamp: Option<DateTime<Utc>>,
) {
    metadata.working_directory = str_field(payload, "cwd");
    metadata.project = metadata.working_directory.clone();
    metadata.agent_version = str_field(payload, "cli_version");
    metadata.started_at = timestamp;
    if payload.get("base_instructions").is_some() {
        // Codex records its own system prompt. Point at the line rather than
        // copying it: it is large, and the raw inspector can fetch it.
        metadata.base_instructions = Some(source);
    }
    if let Some(git) = payload.get("git") {
        metadata.git_branch = str_field(git, "branch");
        metadata.git_commit = str_field(git, "commit_hash");
        metadata.repository_url = str_field(git, "repository_url");
    }
    if let Some(window) = payload.get("context_window").and_then(|w| w.as_u64()) {
        metadata.context_window = Some(window as u32);
    }
}

/// Group events into turns.
///
/// A turn is one model request, and Codex marks the end of each with a
/// `token_count` event carrying `last_token_usage`. So each `token_count`
/// anchors a turn, and the events since the previous one belong to it.
fn derive_turns(events: &mut [Event], metadata: &SessionMetadata) -> Vec<Turn> {
    let mut turns = Vec::new();
    let mut pending: Vec<usize> = Vec::new();
    let mut number = 1u32;

    for index in 0..events.len() {
        pending.push(index);
        let EventKind::TokenReport(usage) = &events[index].kind else {
            continue;
        };
        let usage = *usage;

        let Ok(turn_number) = TurnNumber::new(number) else {
            continue;
        };
        for &i in &pending {
            events[i].turn = Some(turn_number);
        }
        turns.push(Turn {
            number: turn_number,
            timestamp: events[index].timestamp,
            model: metadata.model.clone(),
            usage,
            event_indices: std::mem::take(&mut pending),
            anchor_index: Some(index),
        });
        number += 1;
    }

    // Events after the final token report belong to an in-flight turn that never
    // completed. They are left unassigned rather than invented into a turn.
    turns
}

// ---------------------------------------------------------------------------
// JSON helpers
//
// All of these tolerate absent or unexpectedly-typed fields by returning None.
// That leniency is the mechanism behind "never crash because an agent
// introduced a new JSONL field".
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

/// One part of a payload that occupies context.
///
/// The split is not cosmetic: it decides whether an item can be counted
/// exactly. [`Component::Text`] is text the model tokenizes as text, so a real
/// tokenizer measures it. [`Component::Opaque`] occupies context but is not
/// tokenizable text we hold:
///
/// - `encrypted_content` on reasoning items is a blob we cannot read;
/// - `image_url` data URLs are charged as image patches, not as BPE tokens
///   over their base64;
/// - a structured `output` would have to be re-serialized before we could read
///   it, in *serde_json's* key order and spacing rather than the ones Codex
///   sent.
///
/// All three are genuinely replayed into the request, so their length still
/// counts. But running a tokenizer over them would produce a wrong number
/// wearing an `Exact` label, which is the one thing this project must not do.
pub(crate) enum Component<'a> {
    Text(&'a str),
    Opaque(Cow<'a, str>),
}

impl Component<'_> {
    fn chars(&self) -> usize {
        match self {
            Component::Text(s) => s.chars().count(),
            Component::Opaque(s) => s.chars().count(),
        }
    }
}

/// Walk the parts of a payload that occupy context.
///
/// Single traversal shared by [`content_chars`] and [`content_text`], so the
/// exact path can never measure a different selection from the estimated one.
fn visit_content(payload: &Value, f: &mut dyn FnMut(Component<'_>)) {
    if let Some(content) = payload.get("content").and_then(Value::as_array) {
        for block in content {
            for key in ["text", "input_text", "output_text"] {
                if let Some(s) = block.get(key).and_then(Value::as_str) {
                    f(Component::Text(s));
                }
            }
            // Inline images are sent as data URLs and are enormous; count them.
            if let Some(s) = block.get("image_url").and_then(Value::as_str) {
                f(Component::Opaque(Cow::Borrowed(s)));
            }
        }
    }

    for key in ["arguments", "input"] {
        if let Some(s) = payload.get(key).and_then(Value::as_str) {
            f(Component::Text(s));
        }
    }

    if let Some(s) = payload.get("encrypted_content").and_then(Value::as_str) {
        f(Component::Opaque(Cow::Borrowed(s)));
    }

    match payload.get("output") {
        Some(Value::String(s)) => f(Component::Text(s)),
        Some(other @ Value::Object(_)) | Some(other @ Value::Array(_)) => {
            f(Component::Opaque(Cow::Owned(other.to_string())))
        }
        _ => {}
    }

    if let Some(summary) = payload.get("summary").and_then(Value::as_array) {
        for block in summary {
            if let Some(s) = block.get("text").and_then(Value::as_str) {
                f(Component::Text(s));
            }
        }
    }
}

/// Character count of the parts of a payload that occupy context.
pub(crate) fn content_chars(payload: &Value) -> u32 {
    let mut total: usize = 0;
    visit_content(payload, &mut |part| total += part.chars());
    total.min(u32::MAX as usize) as u32
}

/// The model-visible text of a payload, when *all* of it is text.
///
/// `None` is a refusal, not a failure: it means the payload holds at least one
/// [`Component::Opaque`] part, so no exact count of the whole item exists and
/// the character estimate remains the honest answer. An empty result is `None`
/// for the same reason -- there is nothing to have counted.
pub(crate) fn content_text(payload: &Value) -> Option<String> {
    let mut text = String::new();
    let mut opaque = false;
    visit_content(payload, &mut |part| match part {
        Component::Text(s) => text.push_str(s),
        Component::Opaque(_) => opaque = true,
    });
    (!opaque && !text.is_empty()).then_some(text)
}

/// `base_instructions`, which Codex writes either as a bare string or as
/// `{text: "..."}` depending on version. `None` for any shape we do not
/// recognise, which is then sized by its serialized length instead.
pub(crate) fn instruction_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Object(_) => value
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn instruction_chars(value: &Value) -> u32 {
    let len = match instruction_text(value) {
        Some(text) => text.chars().count(),
        None => value.to_string().chars().count(),
    };
    len.min(u32::MAX as usize) as u32
}

/// Short human-readable excerpt for list rendering.
fn preview_of(payload: &Value, max: usize) -> String {
    let text = payload
        .get("content")
        .and_then(Value::as_array)
        .and_then(|blocks| {
            blocks.iter().find_map(|b| {
                ["text", "input_text", "output_text"]
                    .iter()
                    .find_map(|k| b.get(*k).and_then(Value::as_str))
            })
        })
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .unwrap_or_default();

    ct_domain::ports::truncate_chars(text.trim(), max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn counts_text_blocks_and_inline_images() {
        let payload = json!({
            "content": [
                {"type": "input_text", "text": "hello"},
                {"type": "input_image", "image_url": "data:image/png;base64,AAAA"}
            ]
        });
        // "hello" plus the full data URL, which is counted because inline
        // images really do occupy context.
        assert_eq!(content_chars(&payload), 5 + "data:image/png;base64,AAAA".len() as u32);
    }

    #[test]
    fn counts_encrypted_reasoning_because_it_is_replayed_to_the_model() {
        let payload = json!({"type": "reasoning", "encrypted_content": "abcdefghij"});
        assert_eq!(content_chars(&payload), 10);
    }

    #[test]
    fn counts_structured_tool_output_as_serialised_length() {
        let payload = json!({"output": {"stdout": "hi"}});
        assert!(content_chars(&payload) > 2);
    }

    #[test]
    fn unexpected_field_types_do_not_panic() {
        let payload = json!({"content": "not-an-array", "arguments": 42, "output": true});
        assert_eq!(content_chars(&payload), 0);
        assert_eq!(str_field(&payload, "arguments"), None);
    }

    #[test]
    fn tool_calls_are_recognised_across_codex_spellings() {
        for kind in ["function_call", "custom_tool_call", "tool_search_call"] {
            let payload = json!({"type": kind, "name": "shell", "call_id": "c1", "arguments": "ls"});
            let ev = translate_response_item(&payload, Some(kind));
            assert!(
                matches!(ev, EventKind::ToolCall { ref tool, .. } if tool == "shell"),
                "{kind} should map to a tool call, got {ev:?}"
            );
        }
    }

    #[test]
    fn a_shell_call_records_the_command_it_ran() {
        // Codex encodes the arguments object as a string, so the target has to
        // survive a second round of decoding that Claude Code does not need.
        let payload = json!({
            "type": "function_call",
            "name": "shell",
            "call_id": "c1",
            "arguments": "{\"command\":[\"bash\",\"-lc\",\"cargo test\"]}"
        });
        match translate_response_item(&payload, Some("function_call")) {
            EventKind::ToolCall { target, .. } => {
                assert_eq!(target.as_deref(), Some("bash -lc cargo test"))
            }
            other => panic!("expected a tool call, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_response_item_becomes_unrecognised_not_an_error() {
        let payload = json!({"type": "brand_new_thing_from_the_future"});
        let ev = translate_response_item(&payload, Some("brand_new_thing_from_the_future"));
        assert_eq!(ev, EventKind::Unrecognised);
    }

    #[test]
    fn token_report_takes_the_last_usage_not_the_cumulative_total() {
        let payload = json!({
            "type": "token_count",
            "info": {
                "total_token_usage": {"input_tokens": 999_999, "output_tokens": 1},
                "last_token_usage": {"input_tokens": 17_268, "output_tokens": 234,
                                     "reasoning_output_tokens": 46},
                "model_context_window": 258_400
            }
        });
        let mut meta = SessionMetadata::default();
        let kind = translate_event_msg(&payload, Some("token_count"), &mut meta);
        match kind {
            EventKind::TokenReport(u) => {
                assert_eq!(u.prompt_tokens(), Some(17_268), "cumulative totals must not leak in");
                assert_eq!(u.output, Some(234));
                assert_eq!(u.reasoning, Some(46));
                assert_eq!(u.context_window, Some(258_400));
            }
            other => panic!("expected a token report, got {other:?}"),
        }
        assert_eq!(meta.context_window, Some(258_400));
    }
}
