//! Provenance: how much we know about a fact, and where it came from.

use super::identity::FileId;
use serde::{Deserialize, Serialize};
use std::fmt;

/// How much trust a piece of information deserves.
///
/// ContextTrace's central promise is that it never presents a reconstruction as
/// a measurement. Every fact not read verbatim from a session file carries one
/// of these, and the ordering is meaningful: `Observed < Derived < Estimated`,
/// weakest-last, so `max` selects the *least* trustworthy input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Read directly from the agent's own log. The agent asserted it.
    /// Example: `usage.input_tokens` on a Claude Code assistant message.
    Observed,
    /// Computed deterministically from observed data by a defensible rule.
    /// Example: tokenizing text we can see, or diffing a Codex compaction
    /// `replacement_history` against the pre-compaction item list to learn
    /// exactly what was dropped.
    Derived,
    /// A heuristic. Directionally useful; not a measurement.
    /// Example: per-item token attribution for Claude Code, where no local
    /// tokenizer exists.
    Estimated,
}

impl Confidence {
    pub const ALL: [Confidence; 3] = [
        Confidence::Observed,
        Confidence::Derived,
        Confidence::Estimated,
    ];

    pub fn parse(s: &str) -> Option<Confidence> {
        let norm = s.trim().to_ascii_lowercase();
        Confidence::ALL.into_iter().find(|c| c.label() == norm)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Confidence::Observed => "observed",
            Confidence::Derived => "derived",
            Confidence::Estimated => "estimated",
        }
    }

    /// The weakest of two confidences.
    ///
    /// Named for its meaning in the domain (least confident) rather than its
    /// implementation (numerically greatest). Combining an observed fact with
    /// an estimated one yields an estimate -- confidence never launders upward,
    /// which is the property that keeps aggregate figures honest.
    pub fn weakest(self, other: Confidence) -> Confidence {
        if self > other {
            self
        } else {
            other
        }
    }

    /// The weakest confidence across an iterator, or `Observed` when empty.
    pub fn weakest_of(items: impl IntoIterator<Item = Confidence>) -> Confidence {
        items
            .into_iter()
            .fold(Confidence::Observed, Confidence::weakest)
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A byte-range pointer back into the originating session file.
///
/// This is how the domain satisfies "preserve raw events for future analysis"
/// without holding them in memory. The raw inspector re-reads these bytes on
/// demand, so the original JSON is always available verbatim -- *including
/// fields this version of ContextTrace does not understand yet*.
///
/// It is also the performance strategy. Real sessions reach 55 MB with single
/// JSONL lines megabytes wide (Codex embeds base64 images inline). Holding
/// offsets rather than payloads is what keeps them tractable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    pub file: FileId,
    /// Byte offset of the start of the line within the file.
    pub byte_offset: u64,
    /// Byte length of the line, excluding any trailing newline.
    pub byte_len: u32,
    /// 1-based line number, for human-facing "look at line N" messages.
    pub line_no: u32,
}

impl SourceRef {
    pub fn new(file: FileId, byte_offset: u64, byte_len: u32, line_no: u32) -> Self {
        Self {
            file,
            byte_offset,
            byte_len,
            line_no,
        }
    }
}

/// Confidence plus the location backing it.
///
/// Pairing these is what lets any figure in the UI be challenged: click it,
/// and the raw inspector opens the exact bytes the claim rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub confidence: Confidence,
    /// Absent when a fact is synthesised across many events rather than read
    /// from one -- for instance the unattributed residual, which by definition
    /// corresponds to no line in any file.
    pub source: Option<SourceRef>,
}

impl Provenance {
    pub fn observed(source: SourceRef) -> Self {
        Self {
            confidence: Confidence::Observed,
            source: Some(source),
        }
    }

    pub fn derived(source: SourceRef) -> Self {
        Self {
            confidence: Confidence::Derived,
            source: Some(source),
        }
    }

    pub fn estimated(source: Option<SourceRef>) -> Self {
        Self {
            confidence: Confidence::Estimated,
            source,
        }
    }

    /// Provenance for a fact with no single backing line.
    pub fn synthesised(confidence: Confidence) -> Self {
        Self {
            confidence,
            source: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_never_launders_upward() {
        assert_eq!(
            Confidence::Observed.weakest(Confidence::Estimated),
            Confidence::Estimated
        );
        assert_eq!(
            Confidence::Estimated.weakest(Confidence::Observed),
            Confidence::Estimated
        );
        assert_eq!(
            Confidence::Observed.weakest(Confidence::Derived),
            Confidence::Derived
        );
    }

    #[test]
    fn empty_aggregate_is_observed() {
        assert_eq!(Confidence::weakest_of([]), Confidence::Observed);
    }

    #[test]
    fn aggregate_takes_the_weakest_member() {
        let mixed = [
            Confidence::Observed,
            Confidence::Derived,
            Confidence::Estimated,
        ];
        assert_eq!(Confidence::weakest_of(mixed), Confidence::Estimated);
    }
}
