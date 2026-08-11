//! Shared composition root for every ContextTrace user interface.
//!
//! The CLI and desktop app are both driving adapters. Keeping concrete agent
//! and tokenizer selection here ensures they execute the same use cases with
//! the same measurement policy.

use std::path::{Path, PathBuf};

pub use ct_adapters::FileArchiveStore;
use ct_adapters::{
    ClaudeCodeAdapter, CodexAdapter, FileRawEventSource, HeuristicEstimator, Sha256ContentHasher,
    TiktokenEstimator,
};
use ct_application::{AgentBinding, ContextTrace};
use ct_domain::ports::TokenEstimator;
use ct_domain::services::DerivedRatio;
use ct_domain::{AgentKind, AgentSession};

/// A ready-to-use application service plus non-fatal startup notices.
pub struct Runtime {
    pub app: ContextTrace,
    pub warnings: Vec<String>,
}

/// Wire every supported agent to the estimator appropriate to its models.
pub fn build() -> Runtime {
    let mut warnings = Vec::new();
    let codex_estimator: Box<dyn TokenEstimator> = match TiktokenEstimator::o200k() {
        Ok(tokenizer) => Box::new(tokenizer),
        Err(error) => {
            warnings.push(format!(
                "o200k tokenizer unavailable ({error}); using the code-density heuristic"
            ));
            Box::new(HeuristicEstimator::for_code())
        }
    };

    let app = ContextTrace::new(vec![
        AgentBinding::new(
            Box::new(ClaudeCodeAdapter::new()),
            // Anthropic ships no public local tokenizer. Coding-agent sessions
            // are code/output heavy, so the denser heuristic is the honest
            // default until a session-specific ratio can be derived.
            Box::new(HeuristicEstimator::for_code()),
        ),
        AgentBinding::new(Box::new(CodexAdapter::new()), codex_estimator),
    ]);

    Runtime { app, warnings }
}

/// Choose a session-specific estimator where the evidence supports one.
///
/// Claude Code has no public tokenizer and does not expose its system prompt,
/// so fitting a local character ratio to its own usage records improves the
/// heuristic. Codex records its system prompt and has a public tokenizer; its
/// remaining gap is not a ratio to fit.
pub fn calibrate_session(
    app: &ContextTrace,
    session: &AgentSession,
    binding: usize,
) -> (Option<HeuristicEstimator>, Option<DerivedRatio>) {
    let ratio = (session.agent() == AgentKind::ClaudeCode)
        .then(|| app.derive_ratio(session, binding))
        .flatten();
    let estimator = ratio.map(|ratio| HeuristicEstimator::with_ratio(ratio.chars_per_token));
    (estimator, ratio)
}

/// Recreate the concrete heuristic selected by [`calibrate_session`].
///
/// Desktop clients cache the small ratio rather than a tokenizer object so
/// cached sessions remain simple data and can be invalidated cheaply.
pub fn heuristic_estimator(chars_per_token: f32) -> HeuristicEstimator {
    HeuristicEstimator::with_ratio(chars_per_token)
}

/// Open one session's JSONL as a lazy raw-record source.
///
/// Kept in the shared composition root so desktop and CLI analyses use the
/// same filesystem adapter without making UI code construct driven adapters.
pub fn raw_event_source(path: &str) -> impl ct_domain::ports::RawEventSource {
    FileRawEventSource::for_session(path)
}

/// The content identity used by parsers and instruction-file comparisons.
pub fn content_hasher() -> impl ct_domain::ports::ContentHasher {
    Sha256ContentHasher
}

/// The one store ContextTrace writes session copies to.
///
/// Here rather than at each call site because "where does this tool write"
/// is a claim `ct roots` makes out loud, and a second interface answering it
/// from its own constructor is how that claim quietly stops being true. The
/// desktop cannot construct this itself in any case: it depends on this crate
/// and not on `ct-adapters`, which is the boundary working as intended.
pub fn archive_store() -> FileArchiveStore {
    FileArchiveStore::new()
}

/// Where an exported session is written, given the archive root.
///
/// Nested under the archive root deliberately. `ct roots` names *one* written
/// directory, and an export landing somewhere else would falsify that sentence
/// rather than extend it -- so exports become a subdirectory of the directory
/// already disclosed, not a second disclosure to keep in sync.
pub fn export_dir(archive_root: &str) -> PathBuf {
    Path::new(archive_root).join("exports")
}
