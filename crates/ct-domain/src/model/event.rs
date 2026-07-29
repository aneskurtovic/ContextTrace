//! Events: the normalized unit an adapter produces per line of an agent log.

use super::analysis::ContentMeasurement;
use super::identity::{EventId, TurnNumber};
use super::provenance::SourceRef;
use super::tokens::TokenUsage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    /// Codex's third role, carrying instructions injected between the system
    /// prompt and the conversation.
    Developer,
    System,
}

/// One normalized event from an agent session.
///
/// The struct is intentionally thin: identity, position, provenance and a
/// classified [`EventKind`]. Content is *not* here -- it lives behind
/// [`Event::source`], fetched on demand through the
/// [`RawEventSource`](crate::ports::RawEventSource) port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    /// Position in the file, 0-based. The only ordering Codex gives us.
    pub sequence: u32,
    pub timestamp: Option<DateTime<Utc>>,
    pub kind: EventKind,
    pub source: SourceRef,
    /// The agent's own type string, always preserved verbatim.
    ///
    /// Kept even for events we classify confidently, because it is what makes
    /// the raw inspector useful when an agent changes its schema: the histogram
    /// of these strings tells us what upstream started emitting.
    pub raw_type: String,
    /// Which model request this event belongs to, once turns are resolved.
    pub turn: Option<TurnNumber>,
    /// Claude Code threads its log as a DAG via these. Codex leaves them empty.
    pub links: EventLinks,
    /// Fixed-size analysis of the model-visible payload, when the log exposed
    /// it and the caller opted into content diagnostics.
    ///
    /// Kept out of serialized session views: it exists only to compare items
    /// without retaining their potentially huge content.
    #[serde(skip)]
    pub content_measurement: Option<ContentMeasurement>,
}

/// Graph edges between events.
///
/// Claude Code does not write a linear conversation: rewinds and edits create
/// sibling branches in one file, so line order is *not* conversation order. The
/// context at an assistant turn is the parent chain walked back from it, which
/// is why these links are first-class rather than adapter-private.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLinks {
    /// This event's own graph identity.
    pub uuid: Option<String>,
    /// The event this one directly followed.
    pub parent_uuid: Option<String>,
    /// Bridges a compaction boundary: the pre-compaction event this one
    /// logically continues from, even though `parent_uuid` is null because the
    /// summarised history was cut away.
    pub logical_parent_uuid: Option<String>,
    /// True for subagent activity, which occupies a *separate* context window
    /// from the main thread and must never be folded into it.
    pub is_sidechain: bool,
}

/// What an event is, in domain terms.
///
/// Adapters map their agent's vocabulary onto these. Anything unrecognised
/// becomes [`EventKind::Unrecognised`] rather than an error -- the brief's
/// "never crash because an agent introduced a new JSONL field" is implemented
/// by this variant existing and by adapters navigating JSON leniently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    /// Session-level header: cwd, model provider, versions, git state.
    SessionStarted,
    /// Per-turn configuration: model, approval policy, working directory.
    TurnStarted,
    TurnCompleted,
    Message {
        role: MessageRole,
        /// Short preview for list rendering. Full text stays behind `source`.
        preview: String,
        /// Character length of the full content, used for heuristic token
        /// estimation without loading the content itself.
        char_len: u32,
    },
    /// Model reasoning, where the agent records it.
    Reasoning {
        char_len: u32,
        /// True when the agent wrote the block but stripped its text, leaving
        /// only an opaque signature.
        ///
        /// This is the normal case for Claude Code extended thinking -- 99.2% of
        /// thinking blocks in the local corpus -- and it matters because the
        /// reasoning still occupied the model's context. `char_len` for a
        /// redacted block is *derived from the signature's length*, so it is a
        /// weaker figure than a measured one and diagnostics say so.
        redacted: bool,
    },
    ToolCall {
        tool: String,
        call_id: Option<String>,
        char_len: u32,
        /// What the call acted on: the path it read, the command it ran.
        ///
        /// Taken from the call's own arguments, and `None` whenever they do not
        /// say. Without it a context breakdown reports which *tool* consumed
        /// 14,805 tokens but not which *file*, which is one question short of
        /// the one being asked.
        target: Option<String>,
    },
    ToolResult {
        tool: Option<String>,
        call_id: Option<String>,
        char_len: u32,
        /// Whether the agent flagged this result as an error.
        is_error: bool,
    },
    /// A response item too large to materialise as a JSON tree.
    ///
    /// Membership and the presence of inline images are observed by a narrow
    /// streaming scan. `non_image_chars` covers a decoded plain-text output or,
    /// for structured output, its serialised representation with image URL
    /// values removed. Base64 length is deliberately not treated as a token
    /// count; any image charge remains unattributed during observed-total
    /// reconciliation.
    OversizedToolResult {
        call_id: Option<String>,
        non_image_chars: u32,
        image_count: u32,
        image_payload_chars: u32,
    },
    /// Content injected into the prompt by the harness rather than authored by
    /// user or model: instruction files, skill listings, tool schemas, hook
    /// output, file reads.
    ///
    /// This variant is why instruction tracing is a P0 feature rather than a
    /// research project: Claude Code labels these injections with their origin,
    /// so provenance is observed rather than inferred.
    ContextInjection {
        mechanism: String,
        label: String,
        char_len: u32,
    },
    /// History was summarised and replaced.
    Compacted(CompactionFacts),
    /// The agent reported token usage.
    TokenReport(TokenUsage),
    /// A recognised session-lifecycle event we do not model further.
    SessionEvent {
        subtype: String,
    },
    /// An event type this version of ContextTrace does not know.
    ///
    /// Not a failure. The event keeps its `source`, so the raw inspector shows
    /// it in full, and the corpus smoke test reports these as a histogram --
    /// which is how we discover that an agent changed its format.
    Unrecognised,
}

/// What an agent told us about a compaction.
///
/// Both supported agents record compaction well, and better than the initial
/// brief assumed. Claude Code writes exact before/after token counts; Codex
/// writes the literal replacement history, which means the discarded content is
/// *derivable by diffing* rather than merely inferable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionFacts {
    /// "manual", "auto", or whatever the agent calls it.
    pub trigger: Option<String>,
    pub tokens_before: Option<u32>,
    pub tokens_after: Option<u32>,
    /// Running total of tokens dropped across all compactions this session.
    pub cumulative_dropped: Option<u32>,
    pub duration_ms: Option<u64>,
    /// True when the agent recorded the post-compaction history verbatim, so
    /// the dropped content can be derived exactly instead of guessed at.
    pub replacement_recorded: bool,
}

impl Event {
    /// Character length of this event's content, where known.
    ///
    /// The basis for heuristic token estimation. Recorded at parse time so the
    /// estimator never has to re-read a multi-megabyte payload.
    pub fn char_len(&self) -> Option<u32> {
        match &self.kind {
            EventKind::Message { char_len, .. }
            | EventKind::Reasoning { char_len, .. }
            | EventKind::ToolCall { char_len, .. }
            | EventKind::ToolResult { char_len, .. }
            | EventKind::ContextInjection { char_len, .. } => Some(*char_len),
            EventKind::OversizedToolResult {
                non_image_chars, ..
            } => Some(*non_image_chars),
            _ => None,
        }
    }

    /// Whether this event contributes text to the model's prompt.
    ///
    /// Excludes pure telemetry (token reports, lifecycle markers) which are
    /// written to the log but never sent to the model.
    pub fn occupies_context(&self) -> bool {
        matches!(
            self.kind,
            EventKind::Message { .. }
                | EventKind::Reasoning { .. }
                | EventKind::ToolCall { .. }
                | EventKind::ToolResult { .. }
                | EventKind::OversizedToolResult { .. }
                | EventKind::ContextInjection { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::identity::FileId;

    fn ev(kind: EventKind) -> Event {
        Event {
            id: EventId::Ordinal(0),
            sequence: 0,
            timestamp: None,
            kind,
            source: SourceRef::new(FileId(0), 0, 0, 1),
            raw_type: "test".into(),
            turn: None,
            links: EventLinks::default(),
            content_measurement: None,
        }
    }

    #[test]
    fn telemetry_does_not_occupy_context() {
        assert!(!ev(EventKind::TokenReport(TokenUsage::default())).occupies_context());
        assert!(!ev(EventKind::Unrecognised).occupies_context());
        assert!(!ev(EventKind::TurnStarted).occupies_context());
    }

    #[test]
    fn content_events_occupy_context_and_expose_length() {
        let e = ev(EventKind::ToolResult {
            tool: Some("Bash".into()),
            call_id: None,
            char_len: 4096,
            is_error: false,
        });
        assert!(e.occupies_context());
        assert_eq!(e.char_len(), Some(4096));

        let oversized = ev(EventKind::OversizedToolResult {
            call_id: Some("call-1".into()),
            non_image_chars: 83,
            image_count: 2,
            image_payload_chars: 8_000_000,
        });
        assert!(oversized.occupies_context());
        assert_eq!(oversized.char_len(), Some(83));
    }
}
