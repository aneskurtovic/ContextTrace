//! Ports: the domain's requirements of the outside world.
//!
//! These traits are declared here, in the centre of the hexagon, and
//! implemented out at the edges in `ct-adapters`. The domain therefore never
//! learns that JSONL, tiktoken or the filesystem exist, and adding an agent
//! means implementing [`AgentAdapter`] rather than editing anything inward.
//!
//! Note what is *not* a port: calibration. Turning raw estimates into a
//! balanced [`ContextSnapshot`] is a domain rule
//! ([`TokenCalibrator`](crate::services::TokenCalibrator)), deliberately kept
//! out of adapter hands so a future agent integration cannot accidentally
//! publish numbers that do not add up.

use crate::model::context::{CompactionEvent, ContextItem};
use crate::model::identity::TurnNumber;
use crate::model::provenance::SourceRef;
use crate::model::session::{AgentKind, AgentSession, SessionDescriptor};
use crate::model::tokens::TokenCount;
use std::fmt;

/// Failure at a port boundary.
#[derive(Debug)]
pub enum PortError {
    /// The underlying store could not be reached or read.
    Io(String),
    /// A session file exists but could not be understood well enough to use.
    ///
    /// Deliberately rare: unknown *event types* are not errors, they become
    /// [`EventKind::Unrecognised`](crate::model::event::EventKind::Unrecognised).
    /// This is for a file that is not a session at all.
    Malformed { path: String, detail: String },
    NotFound(String),
    /// The request was valid but this adapter cannot serve it.
    Unsupported(String),
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortError::Io(m) => write!(f, "i/o error: {m}"),
            PortError::Malformed { path, detail } => {
                write!(f, "malformed session file {path}: {detail}")
            }
            PortError::NotFound(what) => write!(f, "not found: {what}"),
            PortError::Unsupported(what) => write!(f, "unsupported: {what}"),
        }
    }
}

impl std::error::Error for PortError {}

pub type PortResult<T> = Result<T, PortError>;

/// What an adapter reconstructed for one turn, before the domain balances it.
///
/// Items carry whatever counts the adapter could honestly produce. Today that
/// is `Estimated` for both agents: adapters size items from the character count
/// recorded at parse time, via [`TokenEstimator::estimate_from_chars`], because
/// counting exactly would mean re-reading and re-parsing every line of a session
/// that can reach 55 MB. Codex *could* be exact -- `tiktoken` applies to
/// GPT-family models and [`TokenEstimator::count_text`] implements it -- but
/// nothing calls that path, and the docs must say what the code does rather than
/// what the design allows.
///
/// The adapter does *not* decide the headline total or the residual.
#[derive(Debug, Clone)]
pub struct ReconstructedContext {
    pub items: Vec<ContextItem>,
    /// The agent's own report of this turn's prompt size, if it made one.
    /// When present this becomes the snapshot's authoritative total.
    pub observed_total: Option<TokenCount>,
    pub context_window: Option<u32>,
    pub model: Option<String>,
    pub preceding_compaction: Option<CompactionEvent>,
}

/// A driven port: one agent's anti-corruption layer.
///
/// Each implementation owns the whole of its agent's foreign model -- where its
/// files live, how its JSONL is shaped, and what "the context at turn N" means
/// for it. Those semantics genuinely differ: Codex logs the literal API item
/// list, so reconstruction is a replay; Claude Code logs a DAG, so
/// reconstruction is a walk back up the parent chain. Both translate into the
/// same domain vocabulary, and neither leaks its foreign concepts inward.
pub trait AgentAdapter: Send + Sync {
    fn agent(&self) -> AgentKind;

    /// Directories this adapter reads.
    ///
    /// Exposed because the brief requires ContextTrace to state plainly which
    /// local paths it touches. Users handing a tool their raw agent logs are
    /// owed that.
    fn roots(&self) -> Vec<String>;

    /// Enumerate sessions without parsing their bodies.
    fn discover(&self) -> PortResult<Vec<SessionDescriptor>>;

    /// Parse one session into the aggregate.
    fn load(&self, descriptor: &SessionDescriptor) -> PortResult<AgentSession>;

    /// Reconstruct the context present at `turn`.
    fn reconstruct(
        &self,
        session: &AgentSession,
        turn: TurnNumber,
        estimator: &dyn TokenEstimator,
    ) -> PortResult<ReconstructedContext>;
}

/// A driven port: counting tokens.
///
/// Two implementations exist because the supported agents differ in what is
/// knowable -- a real tokenizer for GPT-family models, and a heuristic for
/// Anthropic models, which ship no local tokenizer. The return type carries
/// that difference, so callers cannot lose track of which they were handed.
pub trait TokenEstimator: Send + Sync {
    /// Count the tokens in text we hold.
    fn count_text(&self, text: &str) -> TokenCount;

    /// Approximate the tokens in content we have measured but not loaded.
    ///
    /// The reason this exists: attributing a 40 MB session must not require
    /// reading 40 MB of tool output back off disk. Adapters record character
    /// counts at parse time and estimate from those.
    fn estimate_from_chars(&self, char_len: u32) -> TokenCount;

    /// Identifier for display, e.g. `o200k_base` or `heuristic:chars/3.6`.
    fn name(&self) -> &str;
}

/// A driven port: fetching original bytes behind a [`SourceRef`].
///
/// Backs the raw inspector and any operation needing full content. Separate
/// from [`AgentAdapter`] because it is agent-independent -- a byte range in a
/// file is a byte range in a file -- and because it is the one place that
/// re-reads session data, which keeps the read-only guarantee auditable.
pub trait RawEventSource: Send + Sync {
    /// Return the exact bytes of the referenced line as UTF-8 text.
    fn fetch(&self, source: SourceRef) -> PortResult<String>;

    /// Fetch and truncate to `max_chars`, for previews.
    fn fetch_preview(&self, source: SourceRef, max_chars: usize) -> PortResult<String> {
        let full = self.fetch(source)?;
        Ok(truncate_chars(&full, max_chars))
    }
}

/// Truncate on a character boundary, appending an ellipsis when shortened.
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('\u{2026}');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_respects_char_boundaries() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        assert_eq!(truncate_chars("hello world", 5), "hello\u{2026}");
        // Multi-byte input must not panic or split a character.
        assert_eq!(truncate_chars("héllo wörld", 5), "héllo\u{2026}");
    }
}
