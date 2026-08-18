//! Reading a session back as the conversation it was.
//!
//! Every other view here answers a question *about* a session -- how big, how
//! composed, how it changed. This one answers what was said, which is the
//! question a reader arrives at once a composition view has told them that one
//! tool result took 38,000 tokens at turn 12. Without it the tool can name the
//! turn that went wrong and never show it.
//!
//! # What makes this ContextTrace's transcript rather than a chat log
//!
//! Two things. Every entry carries the turn it belongs to and the log line it
//! came from, so a reader can move between the conversation and the
//! measurements of it. And nothing is silently omitted: injected context,
//! redacted thinking and tool results are all entries, because a "conversation"
//! that showed only the human-written parts would misrepresent what the model
//! actually read.
//!
//! # Cost
//!
//! Text is fetched through [`RawEventSource`] for the requested window only.
//! Listing which events belong in a transcript is a pass over already-parsed
//! metadata and touches no bytes; a page of thirty entries costs thirty seeks.
//! A session's transcript is never materialised whole -- some local sessions
//! are 6.8 MB, and one of them being opened must not read all of it.

use ct_domain::model::event::{EventKind, MessageRole};
use ct_domain::ports::{AgentAdapter, RawEventSource};
use ct_domain::AgentSession;
use serde::Serialize;

/// The longest text carried on a page entry.
///
/// A page is a scannable list, and one 400,000-character tool result would
/// otherwise be the whole payload. Entries longer than this are truncated and
/// say so; [`entry`] serves the rest on request.
pub const PAGE_TEXT_CHARS: usize = 2_000;

/// The longest text served for one expanded entry.
///
/// Generous, because the reason to expand an entry is usually that it is huge
/// and the reader wants to see why. Still bounded: the IPC layer serialises
/// this into a webview, and an unbounded read there is an out-of-memory bug
/// waiting for the largest session on the machine.
pub const ENTRY_TEXT_CHARS: usize = 200_000;

/// The longest label on an entry. A label names a row; it is not the row.
const LABEL_CHARS: usize = 60;

/// What one entry in a transcript is.
///
/// Coarser than [`EventKind`] on purpose: a reader wants to know who was
/// speaking and whether they are looking at a request, an answer or machinery,
/// and the finer distinctions are already available in the composition views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TranscriptKind {
    User,
    Assistant,
    Reasoning,
    ToolCall,
    ToolResult,
    /// Content the harness put in the prompt: instruction files, hook output,
    /// skill listings. Shown because the model read it.
    Injection,
    /// The point where history was summarised away.
    Compaction,
}

impl TranscriptKind {
    /// Whether an entry of this kind is worth showing expanded by default.
    ///
    /// Tool results are the bulk of a session's text and almost never what a
    /// reader is scanning for, so they arrive collapsed with their size
    /// stated -- which is also the reading that makes an oversized one
    /// obvious.
    pub fn collapsed_by_default(self) -> bool {
        matches!(
            self,
            TranscriptKind::ToolResult | TranscriptKind::Injection | TranscriptKind::Reasoning
        )
    }
}

/// One thing said, injected or returned, in the order the log recorded it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntry {
    /// Position within this session's transcript, and the cursor callers page
    /// by. Not an event index: the transcript skips telemetry, and a cursor
    /// that jumped by irregular amounts would make paging arithmetic wrong.
    pub index: usize,
    pub kind: TranscriptKind,
    pub turn: Option<u32>,
    /// The tool it called, the file it injected -- whatever names this entry
    /// beyond its kind.
    pub label: Option<String>,
    /// Text as read back from the log, truncated to the caller's budget.
    pub text: String,
    /// True when [`TranscriptEntry::text`] is shorter than what is on disk.
    pub truncated: bool,
    /// Character length recorded at parse time -- the whole entry's size, not
    /// the truncated text's. An estimate of nothing: it is a count of
    /// characters, and it is what makes a collapsed row's weight visible.
    pub chars: Option<u32>,
    /// True for a subagent's own conversation, which occupies a separate
    /// context window from the thread it was spawned from.
    pub sidechain: bool,
    /// Whether the agent flagged this tool result as an error.
    pub error: bool,
    /// The line in the session file this came from.
    pub line: u32,
}

/// A bounded window into a session's transcript.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptPage {
    pub entries: Vec<TranscriptEntry>,
    /// Entries in the whole transcript, so a caller can never mistake a page
    /// for the conversation.
    pub total: usize,
    pub offset: usize,
    pub has_more: bool,
}

/// Read one window of a session's transcript.
///
/// `offset` and `limit` address the transcript, not the log: entry 0 is the
/// first thing said, whatever line it happens to sit on.
pub fn page(
    session: &AgentSession,
    adapter: &dyn AgentAdapter,
    raw: &dyn RawEventSource,
    offset: usize,
    limit: usize,
) -> TranscriptPage {
    let visible = visible_events(session);
    let total = visible.len();
    let offset = offset.min(total);
    let end = offset.saturating_add(limit).min(total);

    let entries = visible[offset..end]
        .iter()
        .enumerate()
        .map(|(position, &event_index)| {
            build(
                session,
                adapter,
                raw,
                event_index,
                offset + position,
                PAGE_TEXT_CHARS,
            )
        })
        .collect();

    TranscriptPage {
        entries,
        total,
        offset,
        has_more: end < total,
    }
}

/// Read one entry in full, for a reader who expanded it.
pub fn entry(
    session: &AgentSession,
    adapter: &dyn AgentAdapter,
    raw: &dyn RawEventSource,
    index: usize,
) -> Option<TranscriptEntry> {
    let event_index = *visible_events(session).get(index)?;
    Some(build(
        session,
        adapter,
        raw,
        event_index,
        index,
        ENTRY_TEXT_CHARS,
    ))
}

/// Event indices that belong in a transcript, in log order.
///
/// Everything that occupied the model's context, plus compactions -- which
/// occupy none but are the reason the text on either side of them does not
/// join up. Token reports and lifecycle markers are excluded: they were
/// written to the log and never sent.
fn visible_events(session: &AgentSession) -> Vec<usize> {
    session
        .events()
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            event.occupies_context() || matches!(event.kind, EventKind::Compacted(_))
        })
        .map(|(index, _)| index)
        .collect()
}

fn build(
    session: &AgentSession,
    adapter: &dyn AgentAdapter,
    raw: &dyn RawEventSource,
    event_index: usize,
    index: usize,
    budget: usize,
) -> TranscriptEntry {
    let event = &session.events()[event_index];
    let (kind, label, error) = classify(&event.kind);

    // The preview recorded at parse time is the fallback, not the plan: it is
    // 160 characters and exists for list rendering. A failed read leaves the
    // reader with something rather than an empty row, and the length beside it
    // still says how much they are not seeing.
    let full = adapter
        .transcript_text(&raw.fetch(event.source).unwrap_or_default())
        .unwrap_or_else(|| preview_of(&event.kind));
    // Compared against the budget rather than against the rendered text:
    // `truncate_chars` appends an ellipsis, so a payload of exactly
    // `budget + 1` characters produces a string of the same length as itself
    // and would report as complete.
    let truncated = full.chars().count() > budget;
    let text = ct_domain::ports::truncate_chars(&full, budget);

    TranscriptEntry {
        index,
        kind,
        turn: event.turn.map(|turn| turn.get()),
        label,
        truncated,
        text,
        chars: event.char_len(),
        sidechain: event.links.is_sidechain,
        error,
        line: event.source.line_no,
    }
}

/// Map an event onto the coarser vocabulary a reader works in, along with
/// whatever names it beyond its kind.
fn classify(kind: &EventKind) -> (TranscriptKind, Option<String>, bool) {
    match kind {
        EventKind::Message { role, .. } => (
            match role {
                MessageRole::Assistant => TranscriptKind::Assistant,
                // Developer and system messages are instructions wearing the
                // message envelope, and a reader scanning for what *they*
                // asked should not find them under their own name.
                MessageRole::User => TranscriptKind::User,
                MessageRole::Developer | MessageRole::System => TranscriptKind::Injection,
            },
            None,
            false,
        ),
        EventKind::Reasoning { redacted, .. } => (
            TranscriptKind::Reasoning,
            redacted.then(|| "recorded without its text".to_string()),
            false,
        ),
        EventKind::ToolCall { tool, target, .. } => (
            TranscriptKind::ToolCall,
            Some(match target {
                // Bounded, because a target is taken from the call's own
                // arguments and a shell invocation's arguments run to
                // hundreds of characters. The whole thing is the entry's
                // text, immediately below; this is the row's name.
                Some(target) => format!(
                    "{tool} · {}",
                    ct_domain::ports::truncate_chars(target, LABEL_CHARS)
                ),
                None => tool.clone(),
            }),
            false,
        ),
        EventKind::ToolResult { tool, is_error, .. } => {
            (TranscriptKind::ToolResult, tool.clone(), *is_error)
        }
        EventKind::OversizedToolResult { image_count, .. } => (
            TranscriptKind::ToolResult,
            (*image_count > 0).then(|| format!("{image_count} inline image(s)")),
            false,
        ),
        EventKind::ContextInjection { label, .. } => {
            (TranscriptKind::Injection, Some(label.clone()), false)
        }
        EventKind::Compacted(facts) => (TranscriptKind::Compaction, facts.trigger.clone(), false),
        // `visible_events` admits nothing else; a future kind that slips
        // through is shown as an injection rather than hidden from the reader.
        _ => (TranscriptKind::Injection, None, false),
    }
}

fn preview_of(kind: &EventKind) -> String {
    match kind {
        EventKind::Message { preview, .. } => preview.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::model::event::{CompactionFacts, Event, EventLinks};
    use ct_domain::ports::{PortError, PortResult, ReconstructedContext, TokenEstimator};
    use ct_domain::{
        AgentKind, EventId, FileId, SessionDescriptor, SessionId, SessionMetadata, SourceRef,
        TokenUsage, TurnNumber,
    };

    /// Serves one canned line per source line number, so these tests exercise
    /// the paging and classification without a filesystem.
    struct Lines(Vec<&'static str>);

    impl RawEventSource for Lines {
        fn fetch(&self, source: SourceRef) -> PortResult<String> {
            self.0
                .get(source.line_no as usize - 1)
                .map(|line| (*line).to_string())
                .ok_or_else(|| PortError::NotFound(format!("line {}", source.line_no)))
        }
    }

    /// An adapter that reads its lines as `role: text`, which is enough to
    /// prove the transcript asks the adapter rather than inventing text.
    struct Echo;

    impl AgentAdapter for Echo {
        fn agent(&self) -> AgentKind {
            AgentKind::Codex
        }
        fn roots(&self) -> Vec<String> {
            Vec::new()
        }
        fn discover(&self) -> PortResult<Vec<SessionDescriptor>> {
            Ok(Vec::new())
        }
        fn load(&self, _descriptor: &SessionDescriptor) -> PortResult<AgentSession> {
            Err(PortError::Unsupported("test adapter".into()))
        }
        fn reconstruct(
            &self,
            _session: &AgentSession,
            _turn: TurnNumber,
            _estimator: &dyn TokenEstimator,
        ) -> PortResult<ReconstructedContext> {
            Err(PortError::Unsupported("test adapter".into()))
        }
        fn transcript_text(&self, raw_line: &str) -> Option<String> {
            raw_line.split_once(": ").map(|(_, text)| text.to_string())
        }
    }

    fn event(line: u32, kind: EventKind) -> Event {
        Event {
            id: EventId::Ordinal(line),
            sequence: line,
            timestamp: None,
            kind,
            source: SourceRef::new(FileId(0), 0, 0, line),
            raw_type: "test".into(),
            turn: Some(TurnNumber::FIRST),
            links: EventLinks::default(),
            content_measurement: None,
        }
    }

    fn session(events: Vec<Event>) -> AgentSession {
        AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::Codex,
            SessionMetadata::default(),
            events,
            Vec::new(),
            Vec::new(),
        )
    }

    fn message(line: u32, role: MessageRole) -> Event {
        event(
            line,
            EventKind::Message {
                role,
                preview: "preview".into(),
                char_len: 7,
            },
        )
    }

    #[test]
    fn telemetry_is_not_part_of_the_conversation_but_a_compaction_is() {
        let session = session(vec![
            message(1, MessageRole::User),
            event(2, EventKind::TokenReport(TokenUsage::default())),
            event(3, EventKind::SessionStarted),
            event(4, EventKind::Compacted(CompactionFacts::default())),
            message(5, MessageRole::Assistant),
        ]);
        let raw = Lines(vec![
            "user: hello",
            "report: -",
            "start: -",
            "compacted: summary",
            "assistant: hi",
        ]);

        let page = page(&session, &Echo, &raw, 0, 10);

        assert_eq!(
            page.total, 3,
            "two messages and the compaction between them"
        );
        assert_eq!(
            page.entries
                .iter()
                .map(|entry| entry.kind)
                .collect::<Vec<_>>(),
            vec![
                TranscriptKind::User,
                TranscriptKind::Compaction,
                TranscriptKind::Assistant
            ]
        );
        assert_eq!(page.entries[0].text, "hello");
        assert_eq!(page.entries[1].text, "summary");
    }

    #[test]
    fn paging_addresses_the_transcript_rather_than_the_log() {
        // Line numbers 1..=5 hold three transcript entries. A cursor that
        // addressed log lines would skip entries or repeat them.
        let session = session(vec![
            message(1, MessageRole::User),
            event(2, EventKind::TokenReport(TokenUsage::default())),
            message(3, MessageRole::Assistant),
            event(4, EventKind::SessionStarted),
            message(5, MessageRole::User),
        ]);
        let raw = Lines(vec![
            "user: first",
            "report: -",
            "assistant: second",
            "start: -",
            "user: third",
        ]);

        let second = page(&session, &Echo, &raw, 1, 1);

        assert_eq!(second.entries.len(), 1);
        assert_eq!(second.entries[0].index, 1);
        assert_eq!(second.entries[0].text, "second");
        assert!(second.has_more);
        assert_eq!(second.total, 3);

        let last = page(&session, &Echo, &raw, 2, 10);
        assert_eq!(last.entries[0].text, "third");
        assert!(!last.has_more);
    }

    #[test]
    fn a_long_entry_is_truncated_on_the_page_and_served_whole_on_request() {
        let long: &'static str = Box::leak(format!("user: {}", "x".repeat(3_000)).into_boxed_str());
        let session = session(vec![message(1, MessageRole::User)]);
        let raw = Lines(vec![long]);

        let page = page(&session, &Echo, &raw, 0, 10);
        assert!(page.entries[0].truncated);
        assert_eq!(
            page.entries[0].text.chars().count(),
            PAGE_TEXT_CHARS + 1,
            "the budget's worth of text, plus the ellipsis that says so"
        );

        let whole = entry(&session, &Echo, &raw, 0).expect("an entry at 0");
        assert!(!whole.truncated);
        assert_eq!(whole.text.chars().count(), 3_000);
        assert!(entry(&session, &Echo, &raw, 9).is_none());
    }

    #[test]
    fn an_unreadable_line_falls_back_to_its_preview_rather_than_an_empty_row() {
        // The file changed under us, or the line is gone. The entry still
        // states its size, so the reader is not told the turn was empty.
        let session = session(vec![message(9, MessageRole::User)]);
        let raw = Lines(Vec::new());

        let page = page(&session, &Echo, &raw, 0, 10);

        assert_eq!(page.entries[0].text, "preview");
        assert_eq!(page.entries[0].chars, Some(7));
    }

    #[test]
    fn a_tool_call_is_named_by_what_it_acted_on() {
        let session = session(vec![
            event(
                1,
                EventKind::ToolCall {
                    tool: "Read".into(),
                    call_id: None,
                    char_len: 20,
                    target: Some("src/main.rs".into()),
                },
            ),
            event(
                2,
                EventKind::ToolResult {
                    tool: Some("Read".into()),
                    call_id: None,
                    char_len: 38_000,
                    is_error: true,
                },
            ),
        ]);
        let raw = Lines(vec!["call: {\"path\":\"src/main.rs\"}", "result: boom"]);

        let page = page(&session, &Echo, &raw, 0, 10);

        assert_eq!(page.entries[0].label.as_deref(), Some("Read · src/main.rs"));
        assert_eq!(page.entries[1].kind, TranscriptKind::ToolResult);
        assert!(page.entries[1].error);
        assert!(TranscriptKind::ToolResult.collapsed_by_default());
        assert!(!TranscriptKind::User.collapsed_by_default());
    }
}
