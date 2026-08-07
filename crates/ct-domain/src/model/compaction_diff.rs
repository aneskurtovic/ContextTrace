//! A structural accounting of what one compaction replaced.
//!
//! The values here intentionally contain no copied session content.  An item is
//! identified only by its API type and role, and points back to the source line
//! that supports the claim.  This keeps a compaction report useful in a shell
//! or an export without turning it into another way to print private prompts.

use super::event::MessageRole;
use super::identity::TurnNumber;
use super::provenance::{Provenance, SourceRef};
use super::tokens::TokenCount;
use serde::{Deserialize, Serialize};

/// The outcome of inspecting one compaction's raw replacement history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CompactionDiff {
    Available {
        source: SourceRef,
        turn: Option<TurnNumber>,
        items: Vec<CompactionDiffItem>,
    },
    Unavailable {
        source: SourceRef,
        turn: Option<TurnNumber>,
        reason: CompactionDiffUnavailable,
    },
}

impl CompactionDiff {
    pub fn source(&self) -> SourceRef {
        match self {
            Self::Available { source, .. } | Self::Unavailable { source, .. } => *source,
        }
    }
}

/// Why an exact structural diff could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionDiffUnavailable {
    MissingReplacementHistory,
    OversizedRawLine,
    UnavailableRawLine,
    MalformedRawLine,
    MalformedPrecedingItem,
    UnknownPrecedingHistory,
}

/// One item participating in the replacement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionDiffItem {
    /// Codex Responses API item type, for example `message` or
    /// `function_call_output`.
    pub item_type: String,
    /// Present only on message items.
    pub role: Option<MessageRole>,
    pub disposition: CompactionItemDisposition,
    /// Size after serializing the parsed JSON item into a normalized compact
    /// representation. This is derived, not a token estimate or a claim about
    /// the original request wire bytes.
    pub normalized_json_bytes: u32,
    /// A tokenizer measurement only when the complete model-visible item was
    /// plain text. Opaque blobs and structured outputs deliberately remain
    /// `None` rather than being tokenized as transport JSON.
    pub text_tokens: Option<TokenCount>,
    /// The line supporting this item's presence. Replacement entries share the
    /// compaction line because they live inside its `replacement_history`.
    pub provenance: Provenance,
}

/// How the item relates to the history immediately before the compaction, and
/// its position in whichever list(s) it appears in.
///
/// Two unrelated lists are in play — the pre-compaction history and the
/// `replacement_history` array — and a bare `ordinal` field could not say
/// which one a number indexed, nor record that a preserved item occupies a
/// position in both. Naming the index on the variant that needs it makes
/// that distinction part of the type rather than something a reader has to
/// remember.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompactionItemDisposition {
    /// Present only in the pre-compaction history; absent from the
    /// replacement.
    Dropped {
        /// Zero-based position in the pre-compaction history.
        history_index: u32,
    },
    /// Present in both lists. Recording both positions lets a report say
    /// where the item moved to, not just that it survived.
    Preserved {
        /// Zero-based position in the pre-compaction history.
        history_index: u32,
        /// Zero-based position in `replacement_history`.
        replacement_index: u32,
    },
    /// An entry introduced by `replacement_history`, such as Codex's opaque
    /// `compaction` blob. It has no identical predecessor to call preserved.
    AddedByReplacement {
        /// Zero-based position in `replacement_history`.
        replacement_index: u32,
    },
}
