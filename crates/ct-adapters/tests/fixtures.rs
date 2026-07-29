//! Adapter tests against committed synthetic session files.
//!
//! These fixtures are hand-authored, never captured. Real session logs contain
//! source code, prompts, terminal output and potentially secrets, so they stay
//! out of the repository entirely -- the corpus smoke test reads them locally
//! and gitignored.
//!
//! Each fixture is built to encode a specific way the real formats can mislead a
//! reader, so a regression shows up as a named failing test rather than as a
//! number that quietly drifts:
//!
//! - a rewound branch that must never appear in a reconstruction;
//! - a compaction boundary the walk must stop at rather than reach through;
//! - one response spanning several lines under a shared `requestId`;
//! - a turn whose cache figures are the *sum* of several API calls;
//! - a thinking block with its text stripped and only a signature left;
//! - a tool result whose full output was persisted to disk and not sent;
//! - an event type from the future, which must be counted and not crash.

use ct_adapters::tokenizers::HeuristicEstimator;
use ct_adapters::{ClaudeCodeAdapter, CodexAdapter};
use ct_domain::model::event::EventKind;
use ct_domain::ports::AgentAdapter;
use ct_domain::{
    AgentKind, AgentSession, ContextCategory, SessionDescriptor, SessionId, TurnNumber,
};
use std::path::PathBuf;

fn fixture(agent: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(agent)
        .join(name)
}

fn descriptor(path: PathBuf, agent: AgentKind, id: &str) -> SessionDescriptor {
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    SessionDescriptor {
        id: SessionId::new(id).unwrap(),
        agent,
        path: path.to_string_lossy().into_owned(),
        size_bytes: size,
        project: None,
        started_at: None,
        last_activity: None,
    }
}

fn claude() -> (ClaudeCodeAdapter, AgentSession) {
    let adapter = ClaudeCodeAdapter::new();
    let d = descriptor(
        fixture("claude_code", "session.jsonl"),
        AgentKind::ClaudeCode,
        "fixture-cc",
    );
    let session = adapter.load(&d).expect("the fixture must parse");
    (adapter, session)
}

fn codex() -> (CodexAdapter, AgentSession) {
    let adapter = CodexAdapter::new();
    let d = descriptor(
        fixture("codex", "rollout.jsonl"),
        AgentKind::Codex,
        "fixture-codex",
    );
    let session = adapter.load(&d).expect("the fixture must parse");
    (adapter, session)
}

#[test]
fn content_measurements_are_opt_in_to_the_analyses_that_use_them() {
    let adapter = CodexAdapter::new();
    let d = descriptor(
        fixture("codex", "rollout.jsonl"),
        AgentKind::Codex,
        "fixture-codex-content-analysis",
    );

    let ordinary = adapter.load(&d).expect("the fixture must parse");
    assert!(
        ordinary
            .events()
            .iter()
            .all(|event| event.content_measurement.is_none()),
        "ordinary inspection and corpus sweeps must not pay to analyse content"
    );

    let analysed = adapter
        .load_with_content_analysis(&d)
        .expect("the fixture must parse with content analysis");
    assert!(
        analysed
            .events()
            .iter()
            .any(|event| event.content_measurement.is_some()),
        "the context analysis path must retain fixed-size content measurements"
    );
}

// ---------------------------------------------------------------------------
// Claude Code
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_event_type_is_counted_rather_than_fatal() {
    let (_, session) = claude();
    assert_eq!(session.unrecognised_total(), 1);
    assert!(
        session
            .unrecognised()
            .iter()
            .any(|(name, _)| name == "some-future-event-type"),
        "the agent's own type string must survive so the histogram is actionable"
    );
    assert!(
        session.fidelity() < 1.0 && session.fidelity() > 0.8,
        "one unknown event should dent fidelity, not destroy it: {}",
        session.fidelity()
    );
}

#[test]
fn lines_sharing_a_request_id_form_one_turn() {
    let (_, session) = claude();
    // Three requests: req_A (two lines), req_B, req_C.
    assert_eq!(
        session.turn_count(),
        3,
        "grouping by requestId is what stops one response counting as several turns"
    );
}

#[test]
fn a_turn_built_from_several_api_calls_reports_the_largest_call() {
    let (_, session) = claude();
    let turn = session.turn(TurnNumber::new(2).unwrap()).unwrap();
    assert_eq!(
        turn.prompt_tokens(),
        Some(76_603),
        "the top-level fields sum to 148,236, which is three prompts added together"
    );
    assert_eq!(turn.usage.api_calls, Some(3));
}

#[test]
fn a_rewound_branch_never_enters_a_reconstruction() {
    let (adapter, session) = claude();
    for turn in session.turns() {
        let context = adapter
            .reconstruct(&session, turn.number, &HeuristicEstimator::for_code())
            .expect("every turn reconstructs");
        assert!(
            !context.items.iter().any(|i| i.label.contains("REWOUND")),
            "turn {} pulled in an abandoned branch",
            turn.number
        );
    }
}

#[test]
fn the_walk_stops_at_a_compaction_instead_of_reaching_through_it() {
    let (adapter, session) = claude();
    // Turn 3 sits after the compact_boundary, whose logicalParentUuid points
    // back at the summarised history. Following that link would resurrect
    // content that was explicitly removed from the prompt.
    let context = adapter
        .reconstruct(
            &session,
            TurnNumber::new(3).unwrap(),
            &HeuristicEstimator::for_code(),
        )
        .expect("the post-compaction turn reconstructs");

    assert!(
        context
            .items
            .iter()
            .all(|i| !i.label.contains("health check endpoint")),
        "pre-compaction history was resurrected"
    );
    let compaction = context
        .preceding_compaction
        .expect("the compaction must be reported, not silently swallowed");
    assert_eq!(compaction.facts.tokens_before, Some(165_223));
    assert_eq!(compaction.reduction(), Some(147_681));
}

#[test]
fn injected_instructions_are_traced_to_the_file_they_came_from() {
    let (adapter, session) = claude();
    let context = adapter
        .reconstruct(
            &session,
            TurnNumber::new(1).unwrap(),
            &HeuristicEstimator::for_code(),
        )
        .unwrap();

    let memory = context
        .items
        .iter()
        .find(|i| i.category == ContextCategory::RepositoryInstructions)
        .expect("the CLAUDE.md injection must be classified as repository instructions");
    assert_eq!(memory.label, "server\\CLAUDE.md");

    assert!(
        context
            .items
            .iter()
            .any(|i| i.category == ContextCategory::ToolDefinitions),
        "deferred_tools_delta is the tool schema surface and must be visible"
    );
}

#[test]
fn output_persisted_to_disk_is_not_counted_as_context() {
    let (_, session) = claude();
    let result = session
        .events()
        .iter()
        .find(|e| matches!(e.kind, EventKind::ToolResult { .. }))
        .expect("the fixture has a tool result");

    // The model saw message.content; toolUseResult.stdout was written to a file
    // and never sent. Counting it would inflate the largest category there is.
    let sent = "line one\nline two\nC:\\repos\\demo\\server\\app.py";
    assert_eq!(
        result.char_len(),
        Some(sent.chars().count() as u32),
        "only what was sent counts, and JSON escaping is not part of it"
    );
}

#[test]
fn redacted_thinking_is_flagged_and_still_carries_weight() {
    let (_, session) = claude();
    let reasoning = session
        .events()
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::Reasoning { char_len, redacted } => Some((*char_len, *redacted)),
            _ => None,
        })
        .expect("the fixture has a thinking block");
    assert!(
        reasoning.1,
        "an empty thinking block with a signature is redacted"
    );
    assert!(
        reasoning.0 > 0,
        "a redacted block still occupied context and must not weigh zero"
    );
}

#[test]
fn a_tool_output_is_named_after_the_call_it_answers() {
    let (adapter, session) = claude();
    let context = adapter
        .reconstruct(
            &session,
            TurnNumber::new(2).unwrap(),
            &HeuristicEstimator::for_code(),
        )
        .unwrap();
    // Both halves matter. The tool name comes from the matching call, because
    // the result records only a `tool_use_id`; the path comes from that call's
    // arguments, because a turn holding four `Read` results needs to say which
    // file each one was.
    assert!(
        context
            .items
            .iter()
            .any(|i| i.label == "Read server/app.py"),
        "an opaque toolu_ id defeats the whole 'find the giant tool result' workflow"
    );
}

// ---------------------------------------------------------------------------
// Codex
// ---------------------------------------------------------------------------

#[test]
fn codex_survives_an_unknown_event_type() {
    let (_, session) = codex();
    assert_eq!(session.unrecognised_total(), 1);
}

#[test]
fn the_codex_system_prompt_is_accounted_for_rather_than_hidden() {
    let (adapter, session) = codex();
    let context = adapter
        .reconstruct(
            &session,
            TurnNumber::new(1).unwrap(),
            &HeuristicEstimator::for_code(),
        )
        .unwrap();
    let prompt = context
        .items
        .iter()
        .find(|i| i.source == ct_domain::ContextSource::AgentSystemPrompt)
        .expect(
            "session_meta.base_instructions is the literal system prompt, observable for Codex",
        );
    assert_eq!(prompt.category, ContextCategory::SystemInstructions);
    assert!(
        prompt.tokens.tokens() > 0,
        "the system prompt is the context component other tools lose entirely"
    );
}

#[test]
fn codex_reads_the_per_request_usage_not_the_running_total() {
    let (_, session) = codex();
    let turn = session.turn(TurnNumber::new(1).unwrap()).unwrap();
    assert_eq!(
        turn.prompt_tokens(),
        Some(12_480),
        "total_token_usage accumulates across the session and is not a prompt size"
    );
}

#[test]
fn a_codex_compaction_replaces_the_item_list() {
    let (adapter, session) = codex();
    let context = adapter
        .reconstruct(
            &session,
            TurnNumber::new(2).unwrap(),
            &HeuristicEstimator::for_code(),
        )
        .unwrap();
    assert!(
        context
            .items
            .iter()
            .any(|i| i.category == ContextCategory::Summaries),
        "replacement_history should appear as the summary that replaced the history"
    );
}
