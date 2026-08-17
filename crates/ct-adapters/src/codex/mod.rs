//! Anti-corruption layer for OpenAI Codex CLI.
//!
//! # The foreign model
//!
//! Codex writes `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`, one
//! JSON object per line, each shaped `{timestamp, type, payload}`.
//!
//! The property that makes Codex the easier of the two agents: lines of type
//! `response_item` **are the literal OpenAI Responses API items** appended to
//! the conversation. Replaying them in order reproduces the request body, so
//! context membership here is *observed*, not inferred.
//!
//! Two further gifts:
//!
//! - `session_meta.base_instructions` stores the system prompt verbatim, so the
//!   usual invisible chunk of context is visible.
//! - `compacted` payloads carry `replacement_history`: the post-compaction item
//!   list in full. Discarded content is therefore *derivable by diffing*, which
//!   is stronger than the original brief assumed was possible.

mod compaction;
mod exact;
mod parse;
mod reconstruct;

use crate::home_dir;
use crate::walk::{find_files, has_extension};
use ct_domain::ports::{
    AgentAdapter, ExactRecount, PortError, PortResult, RawEventSource, ReconstructedContext,
    TokenEstimator,
};
use ct_domain::{
    AgentKind, AgentSession, ContextItem, SessionDescriptor, SessionId, SessionTitle, TurnNumber,
};
use std::path::{Path, PathBuf};

/// Reads Codex CLI sessions.
pub struct CodexAdapter {
    /// The `.codex` directory. Honours `CODEX_HOME`, as Codex itself does.
    home: Option<PathBuf>,
}

impl CodexAdapter {
    pub fn new() -> Self {
        let home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join(".codex")));
        Self { home }
    }

    /// Point the adapter at an explicit directory. Used by fixture tests, which
    /// must never depend on what happens to be installed on the machine.
    pub fn with_home(home: impl Into<PathBuf>) -> Self {
        Self {
            home: Some(home.into()),
        }
    }

    /// Directories holding session files. `archived_sessions` is included
    /// because Codex moves older sessions there and users still want them.
    fn session_dirs(&self) -> Vec<PathBuf> {
        let Some(home) = &self.home else {
            return Vec::new();
        };
        ["sessions", "archived_sessions"]
            .iter()
            .map(|d| home.join(d))
            .filter(|p| p.is_dir())
            .collect()
    }
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentAdapter for CodexAdapter {
    fn agent(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn roots(&self) -> Vec<String> {
        self.session_dirs()
            .iter()
            .map(|p| p.display().to_string())
            .collect()
    }

    fn discover(&self) -> PortResult<Vec<SessionDescriptor>> {
        let mut out = Vec::new();
        for dir in self.session_dirs() {
            for path in find_files(&dir, |p| has_extension(p, "jsonl")) {
                // One unreadable file must not hide every other session.
                if let Ok(descriptor) = describe(&path) {
                    out.push(descriptor);
                }
            }
        }
        out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(out)
    }

    fn load(&self, descriptor: &SessionDescriptor) -> PortResult<AgentSession> {
        parse::load(Path::new(&descriptor.path), descriptor.id.clone(), false)
    }

    fn load_with_content_analysis(
        &self,
        descriptor: &SessionDescriptor,
    ) -> PortResult<AgentSession> {
        parse::load(Path::new(&descriptor.path), descriptor.id.clone(), true)
    }

    fn reconstruct(
        &self,
        session: &AgentSession,
        turn: TurnNumber,
        estimator: &dyn TokenEstimator,
    ) -> PortResult<ReconstructedContext> {
        reconstruct::reconstruct(session, turn, estimator)
    }

    /// Codex is the one agent where this is a measurement rather than a slower
    /// guess: its models use `o200k_base`, which is public.
    fn recount_exact(
        &self,
        _session: &AgentSession,
        items: &mut [ContextItem],
        raw: &dyn RawEventSource,
        estimator: &dyn TokenEstimator,
    ) -> PortResult<ExactRecount> {
        Ok(exact::recount(items, raw, estimator))
    }

    fn compaction_diffs(
        &self,
        session: &AgentSession,
        raw: &dyn RawEventSource,
        estimator: &dyn TokenEstimator,
    ) -> PortResult<Vec<ct_domain::CompactionDiff>> {
        Ok(compaction::diff(session, raw, estimator))
    }
}

/// Build a descriptor from a session file cheaply.
///
/// Reads only the first line (the `session_meta` header) rather than parsing
/// the body, so listing hundreds of sessions costs kilobytes instead of
/// hundreds of megabytes.
fn describe(path: &Path) -> PortResult<SessionDescriptor> {
    let metadata =
        std::fs::metadata(path).map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;

    let header = parse::read_header(path)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    // `payload.id` is the file's own identity and is unique across every
    // local rollout file, including a subagent thread's -- unlike
    // `session_id`, which a subagent thread reports as its *parent's* id (see
    // CT-069). Prefer it; fall back to `session_id` only when a line predates
    // `id` entirely, then to the uuid in the filename.
    let id = header
        .id
        .clone()
        .or_else(|| header.session_id.clone())
        .or_else(|| id_from_filename(stem))
        .unwrap_or_else(|| stem.to_string());

    Ok(SessionDescriptor {
        id: SessionId::new(id).map_err(|e| PortError::Malformed {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?,
        agent: AgentKind::Codex,
        path: path.display().to_string(),
        size_bytes: metadata.len(),
        project: header.cwd.clone(),
        // Codex writes no title of its own, so a first prompt is the only
        // candidate and its weaker provenance travels with it.
        title: header.first_prompt.clone().map(SessionTitle::first_prompt),
        git_branch: header.git_branch.clone(),
        started_at: header.timestamp,
        last_activity: header.timestamp,
        thread_role: parse::thread_role(&header),
    })
}

/// Extract the UUID tail of `rollout-2026-07-27T13-26-32-019fa353-8393-...`.
///
/// The timestamp itself contains hyphens, so splitting on them is not enough;
/// a UUID is the last five hyphen-separated groups.
fn id_from_filename(stem: &str) -> Option<String> {
    let parts: Vec<&str> = stem.split('-').collect();
    if parts.len() < 5 {
        return None;
    }
    let uuid = parts[parts.len() - 5..].join("-");
    (uuid.len() == 36).then_some(uuid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::ThreadRole;

    #[test]
    fn extracts_uuid_from_rollout_filename() {
        assert_eq!(
            id_from_filename("rollout-2026-07-27T13-26-32-019fa353-8393-7272-8980-5a2558f68c04"),
            Some("019fa353-8393-7272-8980-5a2558f68c04".into())
        );
    }

    #[test]
    fn rejects_filenames_without_a_uuid_tail() {
        assert_eq!(id_from_filename("rollout-nonsense"), None);
        assert_eq!(id_from_filename("short"), None);
    }

    #[test]
    fn missing_codex_home_yields_no_roots_rather_than_an_error() {
        let adapter = CodexAdapter::with_home("Z:/definitely/not/here");
        assert!(adapter.roots().is_empty());
        assert!(adapter.discover().unwrap().is_empty());
    }

    fn temp_rollout(name: &str, first_line: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ct-codex-describe-{name}-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, first_line).unwrap();
        path
    }

    // CT-069: a subagent thread's `session_id` names its *parent*, not
    // itself. Before the fix, `describe` reported `session_id` as identity,
    // so every thread in a group collapsed onto the same id and only one file
    // per group was reachable.
    #[test]
    fn describe_uses_payload_id_not_the_parents_session_id() {
        let path = temp_rollout(
            "child",
            r#"{"timestamp":"2026-08-01T10:00:00.000Z","type":"session_meta","payload":{"id":"child-id","session_id":"root-id","parent_thread_id":"root-id","thread_source":"subagent","cwd":"C:\\repos\\demo"}}"#,
        );
        let descriptor = describe(&path).expect("a minimal session_meta line parses");
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            descriptor.id.as_str(),
            "child-id",
            "identity must be the file's own id, not its parent's"
        );
        assert_eq!(
            descriptor.thread_role,
            ThreadRole::Subagent {
                parent: SessionId::new("root-id").unwrap()
            }
        );
    }

    #[test]
    fn describe_reports_an_ordinary_session_as_a_root() {
        let path = temp_rollout(
            "root",
            r#"{"timestamp":"2026-08-01T10:00:00.000Z","type":"session_meta","payload":{"id":"root-id","session_id":"root-id","cwd":"C:\\repos\\demo"}}"#,
        );
        let descriptor = describe(&path).expect("a minimal session_meta line parses");
        let _ = std::fs::remove_file(&path);

        assert_eq!(descriptor.id.as_str(), "root-id");
        assert_eq!(descriptor.thread_role, ThreadRole::Root);
    }
}
