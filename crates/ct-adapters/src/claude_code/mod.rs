//! Anti-corruption layer for Claude Code.
//!
//! # The foreign model
//!
//! Claude Code writes `~/.claude/projects/<slugified-cwd>/<session-uuid>.jsonl`.
//! Three properties drive the whole design, and each contradicts a
//! reasonable-sounding assumption:
//!
//! ## 1. The file is a DAG, not a transcript
//!
//! Every line carries `uuid` and `parentUuid`. Rewinds and edits create sibling
//! branches *in the same file*, so line order is not conversation order.
//! Reading top to bottom and calling it "the context" silently includes
//! abandoned branches the model was never shown. The context at a turn is the
//! parent chain walked back from that turn's assistant message.
//!
//! `compact_boundary` events carry `logicalParentUuid`, pointing at the
//! pre-compaction event they continue from. That link must **not** be followed
//! when reconstructing membership -- the history it points at was summarised
//! away and was not in the prompt. It is for lifecycle and diff views, which
//! answer "what was dropped", not "what was present".
//!
//! ## 2. One API response spans several lines
//!
//! A single model request is written as multiple `assistant` lines -- one for
//! thinking, one for text, one per tool call -- all sharing a `requestId` and
//! all carrying an *identical* `usage` object. Treating each line as a turn
//! would inflate the turn count and report the same prompt size repeatedly, so
//! lines must be grouped by `requestId`.
//!
//! ## 3. Injected context is labelled at the source
//!
//! `attachment` lines record *why* something entered the prompt. The subtypes
//! observed in a 709-session corpus, and their domain mapping:
//!
//! | attachment type | category | source |
//! |---|---|---|
//! | `nested_memory` | repository instructions | the CLAUDE.md path |
//! | `file`, `edited_text_file` | file contents | the file path |
//! | `skill_listing`, `invoked_skills`, `dynamic_skill` | developer instructions | harness |
//! | `agent_listing_delta`, `mcp_instructions_delta`, `deferred_tools_delta` | tool definitions | harness |
//! | `hook_additional_context`, `hook_success` | developer instructions | the hook |
//! | `plan_file_reference`, `plan_mode*` | developer instructions | harness |
//! | `compact_file_reference` | summaries | compaction |
//! | `task_reminder`, `queued_command`, `command_permissions`, `date_change`, `auto_mode` | other | harness |
//!
//! This is instruction provenance as *observed data*, which is why it can ship
//! at P0 rather than being a research project.
//!
//! # What we cannot do
//!
//! Anthropic ships no local tokenizer, so per-item counts are always estimates.
//! The per-turn *total* is exact -- `usage.input_tokens +
//! cache_creation_input_tokens + cache_read_input_tokens` -- and
//! [`TokenCalibrator`](ct_domain::services::TokenCalibrator) reconciles the two.

mod parse;
mod reconstruct;

use crate::home_dir;
use crate::walk::{find_files, has_extension};
use ct_domain::ports::{AgentAdapter, PortError, PortResult, ReconstructedContext, TokenEstimator};
use ct_domain::{
    AgentKind, AgentSession, SessionDescriptor, SessionId, SessionTitle, ThreadRole, TurnNumber,
};
use std::path::{Path, PathBuf};

/// Reads Claude Code sessions.
pub struct ClaudeCodeAdapter {
    /// The `.claude` directory. Honours `CLAUDE_CONFIG_DIR`.
    home: Option<PathBuf>,
}

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        let home = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join(".claude")));
        Self { home }
    }

    /// Point the adapter at an explicit directory, for fixture tests that must
    /// not depend on what happens to be installed on the machine.
    pub fn with_home(home: impl Into<PathBuf>) -> Self {
        Self {
            home: Some(home.into()),
        }
    }

    fn projects_dir(&self) -> Option<PathBuf> {
        self.home
            .as_ref()
            .map(|h| h.join("projects"))
            .filter(|p| p.is_dir())
    }
}

impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentAdapter for ClaudeCodeAdapter {
    fn agent(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }

    fn roots(&self) -> Vec<String> {
        self.projects_dir()
            .map(|p| vec![p.display().to_string()])
            .unwrap_or_default()
    }

    fn discover(&self) -> PortResult<Vec<SessionDescriptor>> {
        let Some(dir) = self.projects_dir() else {
            return Ok(Vec::new());
        };

        let mut out = Vec::new();
        for path in find_files(&dir, |p| has_extension(p, "jsonl")) {
            // One unreadable file must not hide every other session.
            if let Ok(descriptor) = describe(&path) {
                out.push(descriptor);
            }
        }
        out.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
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

    fn transcript_text(&self, raw_line: &str) -> Option<String> {
        parse::transcript_text(raw_line)
    }
}

/// Describe a session without parsing its body.
///
/// The session id is the filename stem and the project comes from the recorded
/// `cwd` (falling back to the directory name), so listing hundreds of sessions
/// costs one `stat` plus one short read each rather than hundreds of megabytes.
fn describe(path: &Path) -> PortResult<SessionDescriptor> {
    let metadata =
        std::fs::metadata(path).map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();

    let header = parse::read_header(path)?;

    // Not every `.jsonl` under `~/.claude/projects` is a conversation. Workflow
    // bookkeeping lives at `subagents/workflows/<id>/journal.jsonl` and holds
    // `{"type":"started"|"result"}` lines, which made `ct sessions` list four
    // entries called `journal` and `ct doctor --dir` report 466 unrecognised
    // events. The rule is stated from what a session *is* rather than from that
    // filename: no `uuid` anywhere means no node for the ancestor walk to start
    // from. Excluding by name would also have been wrong the other way — the
    // 625 local `agent-<hex>.jsonl` subagent transcripts are real sessions.
    if !header.has_conversation {
        return Err(PortError::Malformed {
            path: path.display().to_string(),
            detail: "no line carries a `uuid`, so this is not a session transcript".into(),
        });
    }

    let project = header
        .cwd
        .clone()
        .or_else(|| project_slug(path).map(unslug));
    let role = thread_role(path);

    Ok(SessionDescriptor {
        id: SessionId::new(stem).map_err(|e| PortError::Malformed {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?,
        agent: AgentKind::ClaudeCode,
        path: path.display().to_string(),
        size_bytes: metadata.len(),
        project,
        // A subagent transcript is entirely sidechain, and the brief it opens
        // with is what that file is; a main session's sidechain messages are
        // briefs it *handed out*, and naming it after one would describe the
        // wrong conversation. Which applies is settled by the role, so it is
        // chosen here rather than inside the reader.
        title: header
            .ai_title
            .map(SessionTitle::agent_generated)
            .or_else(|| {
                match role {
                    ThreadRole::Subagent { .. } => header.first_sidechain_prompt,
                    ThreadRole::Root => header.first_prompt,
                }
                .map(SessionTitle::first_prompt)
            }),
        git_branch: header.git_branch,
        started_at: header.timestamp,
        last_activity: metadata
            .modified()
            .ok()
            .map(chrono::DateTime::<chrono::Utc>::from),
        thread_role: role,
    })
}

/// The slugified project directory a session file sits under.
///
/// A main session is `<project>/<id>.jsonl`, so its parent is the project. A
/// subagent transcript is `<project>/<parent-id>/subagents/agent-<hex>.jsonl`,
/// where the same reading gives `subagents` -- which was shown as the project
/// name for every subagent whose log did not happen to record a `cwd`. Two
/// levels are skipped when the file is in a `subagents` directory, and the
/// caller only reaches this at all when the log itself said nothing.
fn project_slug(path: &Path) -> Option<&str> {
    let parent = path.parent()?;
    let dir = if parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("subagents"))
    {
        parent.parent()?.parent()?
    } else {
        parent
    };
    dir.file_name().and_then(|name| name.to_str())
}

/// Read a session's place in its thread group out of where the file sits.
///
/// Claude Code's *in-session* subagent activity is per-event (CT-015's
/// sidechains) and has no separate file. Its *spawned* subagents do: the
/// harness writes them to
/// `<project>/<parent-session-id>/subagents/agent-<hex>.jsonl`, and every one
/// of the 24 local transcripts follows that layout exactly. So the parent this
/// adapter once reported as unknowable is in the path -- not inferred from it,
/// but named by the directory the harness chose.
///
/// Anything not under a `subagents` directory is a root, and a `subagents`
/// directory whose grandparent is not a usable session id is a root too: a
/// subagent that cannot name its parent has no honest representation here (see
/// [`ThreadRole`]).
fn thread_role(path: &Path) -> ThreadRole {
    let subagents = path.parent().filter(|dir| {
        dir.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("subagents"))
    });
    subagents
        .and_then(Path::parent)
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .and_then(|parent| SessionId::new(parent).ok())
        .map(|parent| ThreadRole::Subagent { parent })
        .unwrap_or(ThreadRole::Root)
}

/// Best-effort reversal of Claude Code's directory slugification.
///
/// The scheme replaces path separators and colons with `-`, which is lossy:
/// `C--Users-tester-source-repos-my-project` could have come from either
/// `my-project` or `my/project`. So this is a display fallback only, used when
/// the session's own recorded `cwd` is unavailable, and never as an identifier.
fn unslug(slug: &str) -> String {
    match slug.split_once("--") {
        Some((drive, rest)) if drive.len() == 1 => {
            format!(
                "{}:\\{}",
                drive.to_ascii_uppercase(),
                rest.replace('-', "\\")
            )
        }
        _ => slug.replace('-', "/"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::TitleSource;
    use std::io::Write;

    fn temp_session(name: &str, contents: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("ct-discovery-{name}.jsonl"));
        std::fs::File::create(&path)
            .unwrap()
            .write_all(contents.as_bytes())
            .unwrap();
        path
    }

    #[test]
    fn a_workflow_journal_is_not_a_session() {
        // The four local `subagents/workflows/<id>/journal.jsonl` files, which
        // put 466 unrecognised events into the drift sweep and four entries
        // called `journal` into `ct sessions`.
        let path = temp_session(
            "journal",
            "{\"type\":\"started\",\"key\":\"v2:abc\",\"agentId\":\"a1\"}\n\
             {\"type\":\"result\",\"key\":\"v2:abc\"}\n",
        );
        let described = describe(&path);
        let _ = std::fs::remove_file(&path);

        assert!(
            matches!(described, Err(PortError::Malformed { .. })),
            "a file with no uuid anywhere has no node for the walk to start from"
        );
    }

    #[test]
    fn header_fields_come_from_the_first_line_that_has_them() {
        // Reading line 1 alone left 74 of 707 local sessions with no
        // `started_at`, and `ct sessions --since` deliberately waves
        // timestamp-less sessions through — so every one of them leaked past
        // every date filter.
        let path = temp_session(
            "late-header",
            "{\"type\":\"last-prompt\",\"prompt\":\"hi\"}\n\
             {\"type\":\"mode\",\"mode\":\"default\"}\n\
             {\"type\":\"user\",\"uuid\":\"u1\",\"cwd\":\"C:\\\\src\",\
               \"timestamp\":\"2026-07-20T09:15:00.000Z\"}\n",
        );
        let described = describe(&path);
        let _ = std::fs::remove_file(&path);

        let d = described.expect("this is a session");
        assert_eq!(d.project.as_deref(), Some("C:\\src"));
        assert!(d.started_at.is_some(), "the timestamp is three lines down");
    }

    #[test]
    fn a_session_opening_with_sidecar_lines_is_still_a_session() {
        // The regression the obvious rule would have caused: 90 of 711 local
        // sessions open with a line that carries no `uuid`, and judging by the
        // first line alone would have discarded every one of them.
        let path = temp_session(
            "sidecars",
            "{\"type\":\"last-prompt\",\"prompt\":\"hi\"}\n\
             {\"type\":\"mode\",\"mode\":\"default\"}\n\
             {\"type\":\"ai-title\",\"title\":\"x\"}\n\
             {\"type\":\"user\",\"uuid\":\"u1\",\"parentUuid\":null,\"cwd\":\"C:\\\\src\"}\n",
        );
        let described = describe(&path);
        let _ = std::fs::remove_file(&path);

        assert!(described.is_ok(), "got {described:?}");
    }

    #[test]
    fn a_subagent_transcript_is_a_session_despite_its_name() {
        // 625 of the 711 local Claude Code sessions are named `agent-<hex>`,
        // so any rule keyed on a UUID filename would have discarded most of
        // the corpus while fixing four files.
        let path = temp_session(
            "agent-a0ba077117b8bf6b2",
            "{\"type\":\"user\",\"uuid\":\"u1\",\"parentUuid\":null}\n",
        );
        let described = describe(&path);
        let _ = std::fs::remove_file(&path);

        assert!(described.is_ok(), "got {described:?}");
    }

    /// A session file nested the way the harness nests a subagent transcript.
    fn temp_subagent(project: &str, parent: &str, name: &str, contents: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ct-discovery-subagents")
            .join(project)
            .join(parent)
            .join("subagents");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.jsonl"));
        std::fs::File::create(&path)
            .unwrap()
            .write_all(contents.as_bytes())
            .unwrap();
        path
    }

    #[test]
    fn a_session_is_named_by_the_title_the_agent_wrote_for_it() {
        // The `ai-title` line sits some way into the file -- 30 to 63 KiB in
        // the seven local sessions this budget covers -- so a first-line read
        // would never see it. It outranks the prompt below it.
        let path = temp_session(
            "titled",
            "{\"type\":\"user\",\"uuid\":\"u1\",\"cwd\":\"C:\\\\src\",\"gitBranch\":\"feat/naming\",\
               \"timestamp\":\"2026-08-17T09:15:00.000Z\",\
               \"message\":{\"content\":\"Rename every session in the catalog\"}}\n\
             {\"type\":\"ai-title\",\"aiTitle\":\"Organize features and improve session naming\"}\n",
        );
        let described = describe(&path);
        let _ = std::fs::remove_file(&path);

        let d = described.expect("this is a session");
        let title = d.title.expect("a titled session");
        assert_eq!(title.text, "Organize features and improve session naming");
        assert_eq!(title.source, TitleSource::AgentGenerated);
        assert_eq!(d.git_branch.as_deref(), Some("feat/naming"));
    }

    #[test]
    fn an_untitled_session_falls_back_to_its_first_real_prompt() {
        // Every line before the prompt is something the harness wrote through
        // the user role. Naming a session `<command-name>/clear` would look
        // like a title while identifying nothing -- worse than no title.
        let path = temp_session(
            "scaffolding",
            "{\"type\":\"user\",\"uuid\":\"u1\",\"cwd\":\"C:\\\\src\",\
               \"message\":{\"content\":\"<local-command-caveat>Caveat: the messages below\"}}\n\
             {\"type\":\"user\",\"uuid\":\"u2\",\"message\":{\"content\":\"<command-name>/clear</command-name>\"}}\n\
             {\"type\":\"user\",\"uuid\":\"u3\",\"message\":{\"content\":\"continue\"}}\n\
             {\"type\":\"user\",\"uuid\":\"u4\",\
               \"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Fix the toast\\ndelivery status\"}]}}\n",
        );
        let described = describe(&path);
        let _ = std::fs::remove_file(&path);

        let title = described.expect("a session").title.expect("a title");
        assert_eq!(
            title.text, "Fix the toast delivery status",
            "scaffolding and one-word prompts are skipped, and newlines collapse"
        );
        assert_eq!(title.source, TitleSource::FirstPrompt);
    }

    #[test]
    fn a_subagent_transcript_names_its_parent_and_itself() {
        // The claim this replaces was that Claude Code records no parent link
        // between session files. It records it in the path: all 24 local
        // subagent transcripts live under `<parent-id>/subagents/`. Their own
        // brief is a sidechain message, which is exactly what such a file is.
        let parent = "da2d970a-526f-435e-b8fa-050b778d4270";
        let path = temp_subagent(
            "C--Users-anes-src",
            parent,
            "agent-a2356c6d94cfaa975",
            "{\"type\":\"user\",\"uuid\":\"u1\",\"isSidechain\":true,\
               \"message\":{\"content\":\"You are editing exactly ONE file and no other\"}}\n",
        );
        let described = describe(&path);
        let _ = std::fs::remove_file(&path);

        let d = described.expect("a session");
        assert_eq!(
            d.thread_role,
            ThreadRole::Subagent {
                parent: SessionId::new(parent).unwrap()
            }
        );
        assert_eq!(
            d.title.expect("a title").text,
            "You are editing exactly ONE file and no other"
        );
        assert_eq!(
            d.project.as_deref(),
            Some("C:\\Users\\anes\\src"),
            "the project is the slug two levels up, never the `subagents` directory"
        );
    }

    #[test]
    fn a_main_session_is_not_named_after_a_brief_it_handed_out() {
        // The mirror of the case above. A sidechain message inside a main
        // session is work it delegated, and titling the session with it would
        // describe the wrong conversation.
        let path = temp_session(
            "delegating",
            "{\"type\":\"user\",\"uuid\":\"u1\",\"isSidechain\":true,\
               \"message\":{\"content\":\"You are a subagent. Do the thing.\"}}\n",
        );
        let described = describe(&path);
        let _ = std::fs::remove_file(&path);

        assert!(
            described.expect("a session").title.is_none(),
            "an untitled session stays untitled rather than borrowing a subagent's brief"
        );
    }

    #[test]
    fn unslugs_windows_project_directories() {
        assert_eq!(
            unslug("C--Users-tester-source-repos-ContextTrace"),
            "C:\\Users\\tester\\source\\repos\\ContextTrace"
        );
    }

    #[test]
    fn unslugs_posix_style_directories() {
        assert_eq!(unslug("home-anes-code"), "home/anes/code");
    }

    #[test]
    fn missing_claude_home_yields_no_roots_rather_than_an_error() {
        let adapter = ClaudeCodeAdapter::with_home("Z:/definitely/not/here");
        assert!(adapter.roots().is_empty());
        assert!(adapter.discover().unwrap().is_empty());
    }
}
