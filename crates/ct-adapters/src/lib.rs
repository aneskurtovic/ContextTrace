//! # ContextTrace driven adapters
//!
//! The outer edge of the hexagon. Everything here implements a port declared in
//! [`ct_domain::ports`] and depends inward only -- this crate does not know that
//! `ct-application` or any user interface exists.
//!
//! ## Anti-corruption layers
//!
//! [`codex`] and [`claude_code`] are ACLs in the strict sense. Each owns the
//! whole of one agent's foreign model: where its files live, how its JSONL is
//! shaped, which of its event types matter, and -- crucially -- what "the
//! context at turn N" means for that agent, which genuinely differs:
//!
//! | | Codex CLI | Claude Code |
//! |---|---|---|
//! | Log shape | flat list of API items | DAG linked by `parentUuid` |
//! | Reconstruction | replay the item list | walk the parent chain |
//! | Per-turn total | `token_count.last_token_usage` | `usage` input + cache fields |
//! | Per-item counts | estimated from `char_len`; `tiktoken` available but unused (CT-035) | estimated (no public tokenizer) |
//! | Compaction | `replacement_history` recorded verbatim | before/after token counts |
//!
//! Neither agent's vocabulary escapes its module. The domain sees only
//! [`Event`](ct_domain::Event), [`Turn`](ct_domain::Turn) and
//! [`ContextItem`](ct_domain::ContextItem).
//!
//! ## Adding an agent
//!
//! Implement [`AgentAdapter`](ct_domain::ports::AgentAdapter) in a new module
//! and register it in the composition root. No domain, application or UI change
//! is required -- that is the property the hexagon buys.

pub mod archive;
pub mod claude_code;
pub mod codex;
mod fingerprint;
pub mod jsonl;
pub mod raw_source;
pub mod tokenizers;
pub mod tool_target;
pub mod walk;

pub use archive::FileArchiveStore;
pub use claude_code::ClaudeCodeAdapter;
pub use codex::CodexAdapter;
pub use fingerprint::Sha256ContentHasher;
pub use raw_source::FileRawEventSource;
pub use tokenizers::{HeuristicEstimator, TiktokenEstimator};

use std::path::PathBuf;

/// Resolve a user's home directory without pulling in a dependency for it.
///
/// Checks `USERPROFILE` then `HOME`, so it works under both native Windows and
/// the Git Bash/MSYS environments this project is developed in.
pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}
