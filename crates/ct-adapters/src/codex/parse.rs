//! Translating Codex's JSONL vocabulary into domain events.

use crate::fingerprint;
use crate::jsonl::{self, LineRecord};
use chrono::{DateTime, Utc};
use ct_domain::model::event::{CompactionFacts, EventLinks};
use ct_domain::ports::{PortError, PortResult};
use ct_domain::{
    AgentKind, AgentSession, Event, EventId, EventKind, FileId, MessageRole, SessionId,
    SessionMetadata, SourceRef, ThreadRole, TokenUsage, Turn, TurnNumber,
};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// The handful of fields worth reading from a session's first line.
#[derive(Default)]
pub struct Header {
    /// `payload.id`. Unique across every rollout file, including a subagent
    /// thread's -- see CT-069. This is the field a descriptor's identity is
    /// built from.
    pub id: Option<String>,
    /// `payload.session_id`. Equal to `id` for an ordinary session; on a
    /// subagent thread the harness instead writes the *parent's* id here.
    /// No longer used as identity, but kept: it is the fallback source for
    /// [`ThreadRole::Subagent`]'s parent when `parent_thread_id` itself is
    /// absent.
    pub session_id: Option<String>,
    pub parent_thread_id: Option<String>,
    pub thread_source: Option<String>,
    pub cwd: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
    pub git_branch: Option<String>,
    /// The first user message that reads as a request rather than injected
    /// harness text. Codex writes no title of its own, so this is the only
    /// name a rollout file offers.
    pub first_prompt: Option<String>,
}

/// How far past the header to look for the session's first real prompt.
///
/// Codex's `session_meta` line alone can run to tens of kilobytes -- it embeds
/// the entire base instructions -- and the opening user messages that follow
/// are frequently injected `AGENTS.md` and orchestration blocks rather than
/// anything typed. This budget is what bounds that search; a rollout whose
/// first authored message lands beyond it is listed without a title rather
/// than titled from a guess.
const PROMPT_BYTES: u64 = 256 * 1024;

/// The longest title kept, matching the Claude Code adapter's limit.
const TITLE_CHARS: usize = 120;

/// The shortest prompt worth naming a session after. See the Claude Code
/// adapter's constant of the same name: sessions really do open with `Yes`,
/// `continue`, `Yes please`, and a catalog full of those identifies nothing.
const MIN_TITLE_CHARS: usize = 16;

/// Read the `session_meta` header line, and enough after it to name the
/// session.
pub fn read_header(path: &Path) -> PortResult<Header> {
    let file = File::open(path).map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;
    // The cap goes on the `File` so the `BufReader` wrapping it still offers
    // `read_line`. It also protects against a corrupt file with no newlines at
    // all, where a single `read_line` would otherwise pull in the whole file.
    let mut reader = BufReader::new(file.take(PROMPT_BYTES));
    let io = |e: std::io::Error| PortError::Io(format!("{}: {e}", path.display()));

    let mut line = String::new();
    reader.read_line(&mut line).map_err(io)?;

    let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
        return Ok(Header::default());
    };
    let payload = value.get("payload").unwrap_or(&Value::Null);
    let git = payload.get("git").unwrap_or(&Value::Null);

    let mut header = Header {
        id: str_field(payload, "id"),
        session_id: str_field(payload, "session_id"),
        parent_thread_id: str_field(payload, "parent_thread_id"),
        thread_source: str_field(payload, "thread_source"),
        cwd: str_field(payload, "cwd"),
        timestamp: parse_time(value.get("timestamp")),
        git_branch: str_field(git, "branch"),
        first_prompt: None,
    };

    while header.first_prompt.is_none() {
        line.clear();
        if reader.read_line(&mut line).map_err(io)? == 0 {
            break;
        }
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        header.first_prompt = authored_prompt(&value);
    }
    Ok(header)
}

/// Blocks Codex's harness injects through the user role.
///
/// A rollout's opening user messages are routinely an `AGENTS.md` copy, a
/// tool-mode directive or an orchestration brief -- none of them typed by the
/// person whose session this is. Matched at the start of the trimmed text,
/// so a message that merely quotes one still names its session.
const INJECTED_PROMPT_MARKERS: [&str; 7] = [
    "# AGENTS.md instructions for",
    "<user_instructions>",
    "<environment_context>",
    "<multi_agent_mode>",
    "<INSTRUCTIONS>",
    "<recommended_plugins>",
    // The harness's own template when a run is asked to assess another
    // agent's request. Observed opening seven local rollouts.
    "The following is the Codex agent history",
];

/// The text of a user message a person plausibly wrote, or `None`.
fn authored_prompt(value: &Value) -> Option<String> {
    if str_field(value, "type").as_deref() != Some("response_item") {
        return None;
    }
    let payload = value.get("payload")?;
    if str_field(payload, "type").as_deref() != Some("message")
        || str_field(payload, "role").as_deref() != Some("user")
    {
        return None;
    }
    let text = content_text(payload)?;
    let trimmed = text.trim();
    if INJECTED_PROMPT_MARKERS
        .iter()
        .any(|marker| trimmed.starts_with(marker))
    {
        return None;
    }
    let collapsed = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    (collapsed.chars().count() >= MIN_TITLE_CHARS)
        .then(|| ct_domain::ports::truncate_chars(&collapsed, TITLE_CHARS))
}

/// Derive a session's place in its thread group from the fields Codex's
/// harness already records.
///
/// `thread_source` decides the shape outright: anything other than the exact
/// string `"subagent"` -- including its absence, which is every rollout file
/// written before the harness recorded this at all -- is a root. Only inside
/// that branch does a parent get looked for, so a stray `parent_thread_id` on
/// an ordinary session (never observed locally, but the field is free-form)
/// cannot make this a subagent thread by itself.
///
/// `parent_thread_id` is the primary source once `thread_source` says
/// `"subagent"`. `session_id` is the fallback: on a subagent thread it holds
/// the parent's id even when `parent_thread_id` is missing, which is the same
/// fact CT-069 was filed about, read the other way round. Every subagent
/// thread in the local corpus carries `parent_thread_id`, so the fallback is
/// a safety net rather than the normal path. If neither field yields a usable
/// id, the session is reported as a root rather than as a subagent with no
/// parent to name -- that combination has no honest representation.
///
/// A thread that resolves to *itself* as parent is a root, not a cycle. The
/// `session_id` fallback exists because a subagent thread writes its parent's
/// id there; when a rollout instead writes its own -- observed once locally,
/// where the catalog then read `[subagent of 01a00fe2]` on session `01a00fe2`
/// -- that fallback found nothing, and saying so is the honest reading.
pub fn thread_role(header: &Header) -> ThreadRole {
    if header.thread_source.as_deref() != Some("subagent") {
        return ThreadRole::Root;
    }
    header
        .parent_thread_id
        .clone()
        .or_else(|| header.session_id.clone())
        .filter(|parent| Some(parent) != header.id.as_ref())
        .and_then(|parent| SessionId::new(parent).ok())
        .map(|parent| ThreadRole::Subagent { parent })
        .unwrap_or(ThreadRole::Root)
}

/// Parse a full Codex session.
pub fn load(
    path: &Path,
    id: SessionId,
    include_content_analysis: bool,
) -> PortResult<AgentSession> {
    let mut events: Vec<Event> = Vec::new();
    let mut metadata = SessionMetadata::default();
    let mut unrecognised: BTreeMap<String, u32> = BTreeMap::new();

    jsonl::read_lines(path, |record, raw| {
        let event = translate(&record, raw, &mut metadata, include_content_analysis);
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
fn translate(
    record: &LineRecord,
    raw: &[u8],
    metadata: &mut SessionMetadata,
    include_content_analysis: bool,
) -> Event {
    let source = SourceRef::new(FileId(0), record.offset, record.len, record.line_no);
    let value = record.value.as_ref();
    let raw_outer = record.type_str().unwrap_or("unknown").to_string();
    let timestamp = value.and_then(|v| parse_time(v.get("timestamp")));
    let payload = value.and_then(|v| v.get("payload")).unwrap_or(&Value::Null);

    // Oversized response items are not materialised as JSON trees. Recover only
    // the fields reconstruction needs, and apply the same inline-image policy
    // used for parsed tool outputs: base64 data URLs are not text-token input.
    if record.oversized {
        let payload_start = find_field_value(raw, b"payload", 0);
        let inner = payload_start.and_then(|start| string_field(raw, b"type", start));
        let raw_type = match (&raw_outer[..], inner.as_deref()) {
            ("response_item", Some(i)) | ("event_msg", Some(i)) => format!("{raw_outer}/{i}"),
            _ => raw_outer.clone(),
        };
        let kind = match (raw_outer.as_str(), inner.as_deref(), payload_start) {
            ("compacted", _, _) => EventKind::Compacted(CompactionFacts {
                replacement_recorded: true,
                ..Default::default()
            }),
            (
                "response_item",
                Some("function_call_output" | "custom_tool_call_output" | "tool_search_output"),
                Some(payload_start),
            ) => match oversized_output_facts(raw, payload_start) {
                Some((non_image_chars, image_count, image_payload_chars)) => {
                    EventKind::OversizedToolResult {
                        call_id: string_field(raw, b"call_id", payload_start),
                        non_image_chars,
                        image_count,
                        image_payload_chars,
                    }
                }
                None => EventKind::OversizedToolResult {
                    call_id: string_field(raw, b"call_id", payload_start),
                    non_image_chars: 0,
                    image_count: 0,
                    image_payload_chars: 0,
                },
            },
            _ => EventKind::SessionEvent {
                subtype: raw_type.clone(),
            },
        };
        return Event {
            id: EventId::Ordinal(record.line_no),
            sequence: record.line_no.saturating_sub(1),
            timestamp,
            kind,
            source,
            raw_type,
            turn: None,
            links: EventLinks::default(),
            content_measurement: None,
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
    let content_measurement = match (include_content_analysis, raw_outer.as_str()) {
        (true, "session_meta") => payload
            .get("base_instructions")
            .and_then(instruction_measurement),
        (true, "response_item") => response_item_measurement(payload, inner.as_deref()),
        _ => None,
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
        content_measurement,
    }
}

/// Locate one JSON object's field value without materialising the object.
///
/// Strings are skipped as lexical units, so `"output"` inside a tool's text
/// cannot be mistaken for a field name. This scanner exists only for oversized
/// lines; ordinary records still go through `serde_json`.
fn find_field_value(raw: &[u8], field: &[u8], from: usize) -> Option<usize> {
    let mut i = skip_ascii_space(raw, from);
    if raw.get(i) != Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    while i < raw.len() {
        match raw[i] {
            b'{' | b'[' => {
                depth += 1;
                i += 1;
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return None;
                }
                i += 1;
            }
            b'"' => {
                let token = scan_json_string(raw, i)?;
                let mut after = skip_ascii_space(raw, token.end);
                if depth == 1
                    && !token.escaped
                    && &raw[token.content_start..token.content_end] == field
                    && raw.get(after) == Some(&b':')
                {
                    after = skip_ascii_space(raw, after + 1);
                    return Some(after);
                }
                i = token.end;
            }
            _ => i += 1,
        }
    }
    None
}

fn string_field(raw: &[u8], field: &[u8], from: usize) -> Option<String> {
    let start = find_field_value(raw, field, from)?;
    let token = scan_json_string(raw, start)?;
    if !token.escaped {
        return std::str::from_utf8(&raw[token.content_start..token.content_end])
            .ok()
            .map(str::to_string);
    }
    // Only the small metadata token is parsed, never the oversized payload.
    serde_json::from_slice(&raw[start..token.end]).ok()
}

fn skip_ascii_space(raw: &[u8], mut at: usize) -> usize {
    while raw.get(at).is_some_and(|byte| byte.is_ascii_whitespace()) {
        at += 1;
    }
    at
}

#[derive(Debug, Clone, Copy)]
struct ScannedString {
    content_start: usize,
    content_end: usize,
    end: usize,
    chars: usize,
    escaped: bool,
}

/// Scan one JSON string and count decoded Unicode scalar values without
/// allocating its contents.
fn scan_json_string(raw: &[u8], quote: usize) -> Option<ScannedString> {
    if raw.get(quote) != Some(&b'"') {
        return None;
    }
    let content_start = quote + 1;
    let mut segment = content_start;
    let mut chars = 0usize;
    let mut escaped = false;
    let mut i = content_start;

    while i < raw.len() {
        match raw[i] {
            b'"' => {
                chars = chars
                    .checked_add(std::str::from_utf8(&raw[segment..i]).ok()?.chars().count())?;
                return Some(ScannedString {
                    content_start,
                    content_end: i,
                    end: i + 1,
                    chars,
                    escaped,
                });
            }
            b'\\' => {
                chars = chars
                    .checked_add(std::str::from_utf8(&raw[segment..i]).ok()?.chars().count())?;
                escaped = true;
                let escape = *raw.get(i + 1)?;
                if escape == b'u' {
                    let high = parse_hex_quad(raw.get(i + 2..i + 6)?)?;
                    i += 6;
                    if (0xd800..=0xdbff).contains(&high) && raw.get(i..i + 2) == Some(b"\\u") {
                        let low = parse_hex_quad(raw.get(i + 2..i + 6)?)?;
                        if (0xdc00..=0xdfff).contains(&low) {
                            i += 6;
                        }
                    }
                } else if matches!(
                    escape,
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't'
                ) {
                    i += 2;
                } else {
                    return None;
                }
                chars = chars.checked_add(1)?;
                segment = i;
            }
            byte if byte < 0x20 => return None,
            _ => i += 1,
        }
    }
    None
}

fn parse_hex_quad(bytes: &[u8]) -> Option<u16> {
    (bytes.len() == 4).then_some(())?;
    bytes.iter().try_fold(0u16, |value, byte| {
        let digit = match byte {
            b'0'..=b'9' => (byte - b'0') as u16,
            b'a'..=b'f' => (byte - b'a' + 10) as u16,
            b'A'..=b'F' => (byte - b'A' + 10) as u16,
            _ => return None,
        };
        Some((value << 4) | digit)
    })
}

/// Size an oversized output while excluding inline-image URL string values.
///
/// Plain strings are measured as decoded text. Arrays/objects retain their
/// serialised structural cost, matching the ordinary parser's opaque-output
/// proxy, but subtract inline-image URL values in full. This is deliberately
/// the same policy as [`parsed_tool_output_facts`]: image patches remain
/// observed but unattributed, never converted from base64 into BPE text.
fn oversized_output_facts(raw: &[u8], payload_start: usize) -> Option<(u32, u32, u32)> {
    let start = find_field_value(raw, b"output", payload_start)?;
    if raw.get(start) == Some(&b'"') {
        let token = scan_json_string(raw, start)?;
        let is_image = scanned_string_starts_with_inline_image(raw, token);
        return Some(if is_image {
            (0, 1, saturating_u32(token.chars))
        } else {
            (saturating_u32(token.chars), 0, 0)
        });
    }
    if !matches!(raw.get(start), Some(b'[' | b'{')) {
        return None;
    }

    let mut depth = 0usize;
    let mut i = start;
    let mut image_count = 0usize;
    let mut image_payload_chars = 0usize;
    let mut excluded_raw_chars = 0usize;
    let end = loop {
        match *raw.get(i)? {
            b'[' | b'{' => {
                depth += 1;
                i += 1;
            }
            b']' | b'}' => {
                depth = depth.checked_sub(1)?;
                i += 1;
                if depth == 0 {
                    break i;
                }
            }
            b'"' => {
                let key = scan_json_string(raw, i)?;
                let after = skip_ascii_space(raw, key.end);
                if !key.escaped
                    && &raw[key.content_start..key.content_end] == b"image_url"
                    && raw.get(after) == Some(&b':')
                {
                    let value_start = skip_ascii_space(raw, after + 1);
                    let value = scan_json_string(raw, value_start)?;
                    let is_image = scanned_string_starts_with_inline_image(raw, value);
                    if is_image {
                        image_count = image_count.checked_add(1)?;
                        image_payload_chars = image_payload_chars.checked_add(value.chars)?;
                        excluded_raw_chars = excluded_raw_chars.checked_add(
                            std::str::from_utf8(&raw[value_start..value.end])
                                .ok()?
                                .chars()
                                .count(),
                        )?;
                    }
                    i = value.end;
                } else {
                    i = key.end;
                }
            }
            _ => i += 1,
        }
    };

    let serialized_chars = std::str::from_utf8(&raw[start..end]).ok()?.chars().count();
    Some((
        saturating_u32(serialized_chars.saturating_sub(excluded_raw_chars)),
        saturating_u32(image_count),
        saturating_u32(image_payload_chars),
    ))
}

/// Check the first decoded bytes of a scanned JSON string without allocating
/// its (potentially multi-megabyte) base64 payload. JSON writers may escape the
/// colon in `data:image`, and that transport choice must not change accounting
/// at the parse cap.
fn scanned_string_starts_with_inline_image(raw: &[u8], token: ScannedString) -> bool {
    let mut at = token.content_start;
    for expected in b"data:image" {
        let Some((byte, next)) = decoded_ascii_byte(raw, at, token.content_end) else {
            return false;
        };
        if byte != *expected {
            return false;
        }
        at = next;
    }
    true
}

fn decoded_ascii_byte(raw: &[u8], at: usize, end: usize) -> Option<(u8, usize)> {
    let byte = *raw.get(at)?;
    if at >= end {
        return None;
    }
    if byte != b'\\' {
        return Some((byte, at + 1));
    }

    let escape = *raw.get(at + 1)?;
    let decoded = match escape {
        b'"' | b'\\' | b'/' => escape,
        b'b' => 0x08,
        b'f' => 0x0c,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'u' => u8::try_from(parse_hex_quad(raw.get(at + 2..at + 6)?)?).ok()?,
        _ => return None,
    };
    Some((decoded, if escape == b'u' { at + 6 } else { at + 2 }))
}

/// Account for one parsed tool output under the inline-image policy.
///
/// Both parser paths preserve the output's non-image representation: a plain
/// output is decoded text, while a structured output is a serialization proxy.
/// Every `data:image...` value is reported separately and excluded from that
/// proxy. The API charges image patches by their visual representation, so
/// base64 is neither a heuristic-text input nor eligible for exact BPE counts.
///
/// `None` means the output is not image-bearing and can use the ordinary
/// [`EventKind::ToolResult`] representation.
fn parsed_tool_output_facts(output: &Value) -> Option<(u32, u32, u32)> {
    match output {
        Value::String(text) if is_inline_image_url(text) => {
            Some((0, 1, saturating_u32(text.chars().count())))
        }
        Value::String(_) => None,
        Value::Array(_) | Value::Object(_) => {
            let mut image_count = 0usize;
            let mut image_payload_chars = 0usize;
            let mut excluded_serialized_chars = 0usize;
            visit_inline_images(
                output,
                &mut image_count,
                &mut image_payload_chars,
                &mut excluded_serialized_chars,
            );

            (image_count > 0).then(|| {
                let serialized_chars = output.to_string().chars().count();
                (
                    saturating_u32(serialized_chars.saturating_sub(excluded_serialized_chars)),
                    saturating_u32(image_count),
                    saturating_u32(image_payload_chars),
                )
            })
        }
        _ => None,
    }
}

fn visit_inline_images(
    value: &Value,
    image_count: &mut usize,
    image_payload_chars: &mut usize,
    excluded_serialized_chars: &mut usize,
) {
    match value {
        Value::Array(values) => {
            for value in values {
                visit_inline_images(
                    value,
                    image_count,
                    image_payload_chars,
                    excluded_serialized_chars,
                );
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                if key == "image_url" {
                    if let Some(url) = value.as_str() {
                        if is_inline_image_url(url) {
                            *image_count = image_count.saturating_add(1);
                            *image_payload_chars =
                                image_payload_chars.saturating_add(url.chars().count());
                            // `Value::to_string` is also the normal structured-output
                            // proxy, so subtract the exact JSON string representation,
                            // including its quotes and escapes.
                            *excluded_serialized_chars = excluded_serialized_chars.saturating_add(
                                Value::String(url.to_string()).to_string().chars().count(),
                            );
                        }
                    }
                }
                visit_inline_images(
                    value,
                    image_count,
                    image_payload_chars,
                    excluded_serialized_chars,
                );
            }
        }
        _ => {}
    }
}

fn is_inline_image_url(value: &str) -> bool {
    value.starts_with("data:image")
}

fn saturating_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

/// Identity of the payload the API item contributes, excluding transport ids.
///
/// A retried tool result has a new `call_id` but the same `output`; comparing
/// the whole JSON object would miss exactly the duplicate CT-023 is about.
fn response_item_measurement(
    payload: &Value,
    inner: Option<&str>,
) -> Option<ct_domain::ContentMeasurement> {
    let value = match inner {
        Some("message") | Some("agent_message") => {
            if let Some(text) = content_text(payload) {
                return Some(fingerprint::text(&text));
            }
            payload.get("content")?
        }
        Some("function_call_output")
        | Some("custom_tool_call_output")
        | Some("tool_search_output") => payload.get("output")?,
        Some("web_search_call") => payload.get("action")?,
        Some("reasoning") => {
            // Both fields are replayed. Hashing the complete item would also
            // admit non-content ids if a future format adds them.
            let mut content = serde_json::Map::new();
            if let Some(summary) = payload.get("summary") {
                content.insert("summary".into(), summary.clone());
            }
            if let Some(encrypted) = payload.get("encrypted_content") {
                content.insert("encrypted_content".into(), encrypted.clone());
            }
            return (!content.is_empty()).then(|| fingerprint::value(&Value::Object(content)));
        }
        Some("function_call") | Some("custom_tool_call") | Some("tool_search_call") => {
            let mut content = serde_json::Map::new();
            if let Some(name) = payload.get("name") {
                content.insert("name".into(), name.clone());
            }
            for key in ["arguments", "input"] {
                if let Some(value) = payload.get(key) {
                    content.insert(key.into(), value.clone());
                }
            }
            return (!content.is_empty()).then(|| fingerprint::value(&Value::Object(content)));
        }
        _ => return None,
    };

    value_has_content(value).then(|| fingerprint::value(value))
}

fn instruction_measurement(value: &Value) -> Option<ct_domain::ContentMeasurement> {
    match instruction_text(value) {
        Some(text) if !text.is_empty() => Some(fingerprint::text(&text)),
        Some(_) => None,
        None if value_has_content(value) => Some(fingerprint::value(value)),
        None => None,
    }
}

fn value_has_content(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Bool(_) | Value::Number(_) => true,
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
        // A built-in web search. Unlike a function call it carries no `name`
        // and no `arguments`: what it did lives in `action`, shaped
        // `{type: "search", query, queries}` or `{type: "open_page", url}`.
        // The tool name is derived from the item type rather than read, because
        // the payload does not carry one.
        //
        // No `web_search_call_output` item exists anywhere in the local corpus,
        // so the results the model read are not in the log. They are real
        // context and they land in the unattributed remainder; saying more than
        // that would be guessing at how the harness replays them.
        Some("web_search_call") => EventKind::ToolCall {
            tool: "web_search".into(),
            call_id: str_field(payload, "id"),
            char_len,
            // `action` needs no special knowledge here: `tool_target`'s key list
            // already ranks `url` above `query`, so both shapes name themselves.
            target: payload.get("action").and_then(crate::tool_target::describe),
        },
        Some("function_call_output")
        | Some("custom_tool_call_output")
        | Some("tool_search_output") => {
            // The image-aware event is not a size classification: it is the
            // common accounting representation for any tool output carrying
            // inline images, including a fully parsed line below the 4 MiB cap.
            if let Some((non_image_chars, image_count, image_payload_chars)) =
                payload.get("output").and_then(parsed_tool_output_facts)
            {
                EventKind::OversizedToolResult {
                    call_id: str_field(payload, "call_id"),
                    non_image_chars,
                    image_count,
                    image_payload_chars,
                }
            } else {
                EventKind::ToolResult {
                    tool: None,
                    call_id: str_field(payload, "call_id"),
                    char_len,
                    is_error: str_field(payload, "status").as_deref() == Some("failed"),
                }
            }
        }
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
            // The ordinary path keeps the historical serialized-character
            // proxy, so inline image bytes are charged here. The oversized
            // lexical path excludes image payloads because it cannot assign
            // them an honest text-token estimate; the two paths are therefore
            // intentionally different until the image-accounting follow-up.
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

    // A web search's `action` is what that item carries. Sized from its
    // serialized form and marked opaque for the same reason a structured
    // `output` is: `query` and `queries[0]` are usually the same string, so
    // counting both over-counts and counting one may under-count, and nothing
    // in the log says which the API replays. A declared proxy beats a guess
    // wearing an exact label.
    if let Some(action @ Value::Object(_)) = payload.get("action") {
        f(Component::Opaque(Cow::Owned(action.to_string())));
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
    use std::fs;

    fn oversized_record() -> LineRecord {
        LineRecord {
            offset: 0,
            len: (jsonl::MAX_PARSE_BYTES + 1) as u32,
            line_no: 1,
            value: None,
            oversized: true,
            sniffed_type: Some("response_item".into()),
        }
    }

    #[test]
    fn oversized_plain_output_counts_decoded_text_without_parsing_the_line() {
        let raw = r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"A\n\uD83D\uDE00é"}}"#
            .as_bytes();
        let event = translate(
            &oversized_record(),
            raw,
            &mut SessionMetadata::default(),
            false,
        );

        assert_eq!(event.raw_type, "response_item/function_call_output");
        match event.kind {
            EventKind::OversizedToolResult {
                call_id,
                non_image_chars,
                image_count,
                image_payload_chars,
            } => {
                assert_eq!(call_id.as_deref(), Some("c1"));
                assert_eq!(non_image_chars, 4, "A, newline, emoji, and é");
                assert_eq!(image_count, 0);
                assert_eq!(image_payload_chars, 0);
            }
            other => panic!("expected an oversized tool result, got {other:?}"),
        }
    }

    #[test]
    fn oversized_structured_output_excludes_inline_image_payloads() {
        let first = "data:image/png;base64,AAAA";
        let second = "data:image/jpeg;base64,BBBBBB";
        let raw = format!(
            r#"{{"type":"response_item","payload":{{"type":"custom_tool_call_output","call_id":"c2","metadata":{{"output":"not the direct field"}},"output":[{{"type":"input_text","text":"small result"}},{{"type":"input_image","image_url":"{first}"}},{{"type":"input_image","image_url":"{second}"}}]}}}}"#
        );
        let event = translate(
            &oversized_record(),
            raw.as_bytes(),
            &mut SessionMetadata::default(),
            false,
        );

        match event.kind {
            EventKind::OversizedToolResult {
                non_image_chars,
                image_count,
                image_payload_chars,
                ..
            } => {
                assert_eq!(image_count, 2);
                assert_eq!(
                    image_payload_chars,
                    (first.chars().count() + second.chars().count()) as u32
                );
                assert!(
                    non_image_chars < raw.chars().count() as u32 / 2,
                    "image payloads must not enter the text-token proxy"
                );
                assert!(non_image_chars > "small result".len() as u32);
            }
            other => panic!("expected an oversized tool result, got {other:?}"),
        }
    }

    #[test]
    fn oversized_output_recognises_an_escaped_inline_image_url() {
        let raw = br#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c2","output":[{"image_url":"data\u003aimage/png;base64,AAAA"}]}}"#;
        let event = translate(
            &oversized_record(),
            raw,
            &mut SessionMetadata::default(),
            false,
        );

        assert!(matches!(
            event.kind,
            EventKind::OversizedToolResult {
                image_count: 1,
                image_payload_chars: 26,
                ..
            }
        ));
    }

    #[test]
    fn parsed_structured_output_uses_the_same_inline_image_policy() {
        let first = "data:image/png;base64,AAAA";
        let second = "data:image/jpeg;base64,BBBBBB";
        let payload = json!({
            "type": "custom_tool_call_output",
            "call_id": "c2",
            "output": [
                {"type": "input_text", "text": "small result"},
                {"type": "input_image", "image_url": first},
                {"type": "input_image", "image_url": second}
            ]
        });

        match translate_response_item(&payload, Some("custom_tool_call_output")) {
            EventKind::OversizedToolResult {
                non_image_chars,
                image_count,
                image_payload_chars,
                ..
            } => {
                assert_eq!(image_count, 2);
                assert_eq!(
                    image_payload_chars,
                    (first.chars().count() + second.chars().count()) as u32
                );
                let expected = payload["output"].to_string().chars().count()
                    - Value::String(first.to_string()).to_string().chars().count()
                    - Value::String(second.to_string())
                        .to_string()
                        .chars()
                        .count();
                assert_eq!(
                    non_image_chars, expected as u32,
                    "the parsed path must remove the same base64 JSON strings as the oversized path"
                );
            }
            other => panic!("expected image-aware tool result, got {other:?}"),
        }
    }

    fn inline_image_fixture(filler_len: usize) -> String {
        format!(
            r#"{{"type":"response_item","payload":{{"type":"function_call_output","call_id":"cap","output":[{{"type":"input_text","text":"small result"}},{{"type":"input_image","image_url":"data:image/png;base64,{}"}}]}}}}"#,
            "A".repeat(filler_len)
        )
    }

    fn parse_inline_image_fixture(filler_len: usize, suffix: &str) -> Event {
        let line = inline_image_fixture(filler_len);
        let path = std::env::temp_dir().join(format!(
            "ct-codex-inline-image-{suffix}-{}.jsonl",
            std::process::id()
        ));
        fs::write(&path, line).unwrap();
        let session = load(
            &path,
            SessionId::new(format!("inline-image-{suffix}")).unwrap(),
            false,
        )
        .unwrap();
        let _ = fs::remove_file(path);
        session.events()[0].clone()
    }

    fn image_facts(event: &Event) -> (u32, u32, u32) {
        match &event.kind {
            EventKind::OversizedToolResult {
                non_image_chars,
                image_count,
                image_payload_chars,
                ..
            } => (*non_image_chars, *image_count, *image_payload_chars),
            other => panic!("expected image-aware tool result, got {other:?}"),
        }
    }

    #[test]
    fn inline_image_accounting_does_not_change_at_the_four_mib_parse_cap() {
        let overhead = inline_image_fixture(0).len();
        let below_filler = jsonl::MAX_PARSE_BYTES - overhead;
        let above_filler = below_filler + 1;

        let below = parse_inline_image_fixture(below_filler, "below-cap");
        let above = parse_inline_image_fixture(above_filler, "above-cap");
        assert_eq!(below.source.byte_len as usize, jsonl::MAX_PARSE_BYTES);
        assert_eq!(above.source.byte_len as usize, jsonl::MAX_PARSE_BYTES + 1);

        let below_facts = image_facts(&below);
        let above_facts = image_facts(&above);
        assert_eq!(below_facts.0, above_facts.0);
        assert_eq!(below_facts.1, 1);
        assert_eq!(above_facts.1, 1);
        assert_eq!(
            above_facts.2,
            below_facts.2 + 1,
            "only observed image payload metadata changes across the cap"
        );
    }

    #[test]
    fn malformed_oversized_output_stays_in_context_as_unmeasured() {
        let raw = br#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c3","output":{"image_url":42}}}"#;
        let event = translate(
            &oversized_record(),
            raw,
            &mut SessionMetadata::default(),
            false,
        );

        assert!(matches!(event.kind, EventKind::OversizedToolResult {
            call_id: Some(ref id),
            non_image_chars: 0,
            image_count: 0,
            image_payload_chars: 0,
        } if id == "c3"));
        assert!(event.occupies_context());
    }

    #[test]
    fn unrelated_oversized_telemetry_does_not_enter_context() {
        let raw =
            br#"{"type":"event_msg","payload":{"type":"image_generation_end","image":"AAAA"}}"#;
        let mut record = oversized_record();
        record.sniffed_type = Some("event_msg".into());
        let event = translate(&record, raw, &mut SessionMetadata::default(), false);

        assert_eq!(event.raw_type, "event_msg/image_generation_end");
        assert!(matches!(event.kind, EventKind::SessionEvent { .. }));
        assert!(!event.occupies_context());
    }

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
        assert_eq!(
            content_chars(&payload),
            5 + "data:image/png;base64,AAAA".len() as u32
        );
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
    fn a_web_search_is_named_by_what_it_searched_for() {
        let payload = json!({
            "type": "web_search_call",
            "id": "ws_1",
            "status": "completed",
            "action": {"type": "search", "query": "rust tokenizer crate", "queries": ["rust tokenizer crate"]}
        });
        match translate_response_item(&payload, Some("web_search_call")) {
            EventKind::ToolCall {
                tool,
                call_id,
                target,
                char_len,
            } => {
                assert_eq!(tool, "web_search");
                assert_eq!(call_id.as_deref(), Some("ws_1"));
                assert_eq!(target.as_deref(), Some("rust tokenizer crate"));
                assert!(char_len > 0, "the action is what this item carries");
            }
            other => panic!("expected a tool call, got {other:?}"),
        }
    }

    #[test]
    fn opening_a_page_is_named_by_its_url_and_a_bare_action_by_neither() {
        // `url` outranks `query` in the shared key list, so the two action
        // shapes name themselves without this module knowing either exists.
        let opened = json!({
            "type": "web_search_call",
            "action": {"type": "open_page", "url": "https://docs.rs/tiktoken-rs"}
        });
        match translate_response_item(&opened, Some("web_search_call")) {
            EventKind::ToolCall { target, .. } => {
                assert_eq!(target.as_deref(), Some("https://docs.rs/tiktoken-rs"))
            }
            other => panic!("expected a tool call, got {other:?}"),
        }

        // 2 of the 38 in the corpus carry an action with nothing but its type.
        // The bare tool name is honest; an invented label would not be.
        let bare = json!({"type": "web_search_call", "action": {"type": "open_page"}});
        match translate_response_item(&bare, Some("web_search_call")) {
            EventKind::ToolCall { target, .. } => assert_eq!(target, None),
            other => panic!("expected a tool call, got {other:?}"),
        }
    }

    #[test]
    fn a_search_action_is_sized_but_never_counted_as_exact_text() {
        // `query` and `queries[0]` are usually the same string, so counting
        // both over-counts and counting one may under-count. Sized from the
        // serialized action and marked opaque, like a structured output.
        let payload = json!({
            "action": {"type": "search", "query": "abc", "queries": ["abc", "def"]}
        });
        assert!(content_chars(&payload) > 3);
        assert_eq!(
            content_text(&payload),
            None,
            "an action must not reach the tokenizer as if it were plain text"
        );
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
            let payload =
                json!({"type": kind, "name": "shell", "call_id": "c1", "arguments": "ls"});
            let ev = translate_response_item(&payload, Some(kind));
            assert!(
                matches!(ev, EventKind::ToolCall { ref tool, .. } if tool == "shell"),
                "{kind} should map to a tool call, got {ev:?}"
            );
        }
    }

    #[test]
    fn retried_tool_outputs_match_even_when_call_ids_change() {
        let first = json!({
            "type": "function_call_output",
            "call_id": "call-1",
            "output": "the same file contents"
        });
        let retry = json!({
            "type": "function_call_output",
            "call_id": "call-2",
            "output": "the same file contents"
        });

        assert_eq!(
            response_item_measurement(&first, Some("function_call_output")),
            response_item_measurement(&retry, Some("function_call_output"))
        );
    }

    #[test]
    fn near_duplicate_tool_outputs_do_not_match() {
        let first = json!({"output": "same"});
        let newline = json!({"output": "same\n"});
        assert_ne!(
            response_item_measurement(&first, Some("function_call_output")),
            response_item_measurement(&newline, Some("function_call_output"))
        );
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
                assert_eq!(
                    u.prompt_tokens(),
                    Some(17_268),
                    "cumulative totals must not leak in"
                );
                assert_eq!(u.output, Some(234));
                assert_eq!(u.reasoning, Some(46));
                assert_eq!(u.context_window, Some(258_400));
            }
            other => panic!("expected a token report, got {other:?}"),
        }
        assert_eq!(meta.context_window, Some(258_400));
    }

    // -----------------------------------------------------------------------
    // Thread role (CT-069)
    // -----------------------------------------------------------------------

    #[test]
    fn a_rollout_is_named_by_its_first_typed_message_not_its_injected_ones() {
        // Codex writes no title of its own, and its opening user messages are
        // routinely an AGENTS.md copy or an orchestration brief. Naming a
        // session after one of those would make every session in a repository
        // share the same name -- exactly the failure a title is meant to fix.
        // `Yes` is real and useless; the scan keeps going.
        let meta = r#"{"timestamp":"2026-08-17T14:56:40.465Z","type":"session_meta","payload":{"id":"01a01039","cwd":"C:\\work","git":{"branch":"feat/naming"}}}"#;
        // `r##` rather than `r#`: the payload itself contains `"#`, which would
        // otherwise close the literal mid-string.
        let injected = r##"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions for C:\\work"}]}}"##;
        let terse = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Yes"}]}}"#;
        let assistant = r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Reviewing the notification pipeline now"}]}}"#;
        let typed = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Review\tthe   toast delivery path"}]}}"#;

        let path =
            std::env::temp_dir().join(format!("ct-codex-title-{}.jsonl", std::process::id()));
        fs::write(
            &path,
            format!("{meta}\n{injected}\n{terse}\n{assistant}\n{typed}\n"),
        )
        .unwrap();
        let header = read_header(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(
            header.first_prompt.as_deref(),
            Some("Review the toast delivery path"),
            "an assistant message is not a prompt, and whitespace collapses"
        );
        assert_eq!(header.git_branch.as_deref(), Some("feat/naming"));
        assert_eq!(header.cwd.as_deref(), Some("C:\\work"));
    }

    #[test]
    fn a_session_with_no_thread_source_is_a_root() {
        // Every rollout file written before the harness recorded this field
        // at all. `thread_source` absent must mean "not a subagent", not
        // "unknown" -- a typed absence, not a guess.
        let header = Header {
            id: Some("s".into()),
            session_id: Some("s".into()),
            parent_thread_id: None,
            thread_source: None,
            cwd: None,
            timestamp: None,
            ..Header::default()
        };
        assert_eq!(thread_role(&header), ThreadRole::Root);
    }

    #[test]
    fn a_subagent_thread_names_its_parent_from_parent_thread_id() {
        let header = Header {
            id: Some("child".into()),
            session_id: Some("root".into()),
            parent_thread_id: Some("root".into()),
            thread_source: Some("subagent".into()),
            cwd: None,
            timestamp: None,
            ..Header::default()
        };
        assert_eq!(
            thread_role(&header),
            ThreadRole::Subagent {
                parent: SessionId::new("root").unwrap()
            }
        );
    }

    #[test]
    fn a_subagent_thread_falls_back_to_session_id_when_parent_thread_id_is_missing() {
        // The same fact CT-069 was filed about, used deliberately: a subagent
        // thread's `session_id` holds the parent's id even when
        // `parent_thread_id` itself is absent.
        let header = Header {
            id: Some("child".into()),
            session_id: Some("root".into()),
            parent_thread_id: None,
            thread_source: Some("subagent".into()),
            cwd: None,
            timestamp: None,
            ..Header::default()
        };
        assert_eq!(
            thread_role(&header),
            ThreadRole::Subagent {
                parent: SessionId::new("root").unwrap()
            }
        );
    }

    #[test]
    fn a_non_subagent_thread_source_is_a_root_even_with_a_parent_thread_id() {
        // The state this type must not represent: `Some(parent)` next to a
        // `thread_source` that names something other than a subagent. A
        // stray `parent_thread_id` on an ordinary session must not promote it
        // to a subagent thread.
        let header = Header {
            id: Some("s".into()),
            session_id: Some("s".into()),
            parent_thread_id: Some("root".into()),
            thread_source: Some("user".into()),
            cwd: None,
            timestamp: None,
            ..Header::default()
        };
        assert_eq!(thread_role(&header), ThreadRole::Root);
    }

    #[test]
    fn a_thread_naming_itself_as_its_parent_is_a_root() {
        // Observed once in the local corpus, where `ct sessions` printed
        // `[subagent of 01a00fe2]` on session `01a00fe2`. The `session_id`
        // fallback is only meaningful when it holds someone else's id.
        let header = Header {
            id: Some("01a00fe2".into()),
            session_id: Some("01a00fe2".into()),
            parent_thread_id: None,
            thread_source: Some("subagent".into()),
            cwd: None,
            timestamp: None,
            ..Header::default()
        };
        assert_eq!(thread_role(&header), ThreadRole::Root);
    }

    #[test]
    fn a_subagent_thread_with_no_derivable_parent_is_reported_as_root() {
        // Neither field yields a usable id. There is no honest way to state
        // "subagent with an unnamed parent", so this falls back to root
        // rather than fabricating one or panicking.
        let header = Header {
            id: Some("child".into()),
            session_id: None,
            parent_thread_id: None,
            thread_source: Some("subagent".into()),
            cwd: None,
            timestamp: None,
            ..Header::default()
        };
        assert_eq!(thread_role(&header), ThreadRole::Root);
    }

    #[test]
    fn read_header_reads_the_thread_fields_of_a_subagent_line() {
        let raw = concat!(
            r#"{"timestamp":"2026-08-01T10:00:00.000Z","type":"session_meta","payload":{"#,
            r#""id":"child-id","session_id":"root-id","parent_thread_id":"root-id","#,
            r#""thread_source":"subagent","cwd":"C:\\repos\\demo"}}"#,
        );
        let path = std::env::temp_dir().join(format!(
            "ct-codex-subagent-header-{}.jsonl",
            std::process::id()
        ));
        fs::write(&path, raw).unwrap();
        let header = read_header(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(header.id.as_deref(), Some("child-id"));
        assert_eq!(header.session_id.as_deref(), Some("root-id"));
        assert_eq!(header.parent_thread_id.as_deref(), Some("root-id"));
        assert_eq!(header.thread_source.as_deref(), Some("subagent"));
        assert_eq!(
            thread_role(&header),
            ThreadRole::Subagent {
                parent: SessionId::new("root-id").unwrap()
            }
        );
    }
}
