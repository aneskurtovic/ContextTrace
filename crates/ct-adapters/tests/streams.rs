use ct_adapters::streams::{StreamAdapter, StreamSurface};
use ct_adapters::HeuristicEstimator;
use ct_domain::ports::AgentAdapter;
use ct_domain::{EventKind, TurnNumber};
use std::path::PathBuf;
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/streams")
        .join(name)
}
#[test]
fn each_surface_imports_only_its_own_contract_and_preserves_usage_scope() {
    for (surface, name, version, scope) in [
        (
            StreamSurface::CodexExec,
            "codex-exec-0.161.0.jsonl",
            "0.161.0",
            "turn-aggregate",
        ),
        (
            StreamSurface::CodexAppServer,
            "codex-app-server-0.161.0.jsonl",
            "0.161.0",
            "last-request-snapshot",
        ),
        (
            StreamSurface::ClaudeStream,
            "claude-stream-2.1.293.jsonl",
            "2.1.293",
            "request",
        ),
    ] {
        let adapter = StreamAdapter::new(fixture(name).parent().unwrap(), surface, version);
        let report = adapter.import(&fixture(name)).unwrap();
        assert_eq!(report.session.unrecognised_total(), 0);
        assert!(report.usage.iter().any(|u| u.scope == scope));
        assert_eq!(
            report.session.turn_count(),
            0,
            "presentation usage must not invent prompt snapshots"
        );
        assert_eq!(
            report
                .session
                .events()
                .iter()
                .filter(|e| matches!(e.kind, EventKind::Message { .. }))
                .count(),
            1
        );
        assert!(adapter
            .reconstruct(
                &report.session,
                TurnNumber::new(1).unwrap(),
                &HeuristicEstimator::for_code()
            )
            .is_err());
    }
    let wrong = StreamAdapter::new(
        fixture(".").parent().unwrap(),
        StreamSurface::CodexExec,
        "0.161.0",
    );
    assert!(wrong
        .import(&fixture("claude-stream-2.1.293.jsonl"))
        .is_err());
}
fn temporary(name: &str, text: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("ct-stream-{}-{name}.jsonl", std::process::id()));
    std::fs::write(&p, text).unwrap();
    p
}
#[test]
fn duplicate_completed_items_and_deltas_do_not_duplicate_messages() {
    let text = std::fs::read_to_string(fixture("codex-exec-0.161.0.jsonl")).unwrap();
    let item = text.lines().find(|s| s.contains("item.completed")).unwrap();
    let p = temporary(
        "duplicates",
        &format!("{text}{item}\nNOT JSON\n{{\"type\":\"future_event\"}}\n"),
    );
    let adapter = StreamAdapter::new(p.parent().unwrap(), StreamSurface::CodexExec, "0.161.0");
    let report = adapter.import(&p).unwrap();
    assert_eq!(
        report
            .session
            .events()
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Message { .. }))
            .count(),
        1
    );
    assert_eq!(report.session.unrecognised_total(), 2);
    let _ = std::fs::remove_file(p);
}
#[test]
fn mismatched_versions_and_mixed_sessions_are_rejected() {
    let adapter = StreamAdapter::new(
        fixture(".").parent().unwrap(),
        StreamSurface::ClaudeStream,
        "2.0.0",
    );
    assert!(adapter
        .import(&fixture("claude-stream-2.1.293.jsonl"))
        .is_err());
    let p=temporary("mixed","{\"type\":\"thread.started\",\"thread_id\":\"a\"}\n{\"type\":\"thread.started\",\"thread_id\":\"b\"}\n");
    assert!(
        StreamAdapter::new(p.parent().unwrap(), StreamSurface::CodexExec, "0.161.0")
            .import(&p)
            .is_err()
    );
    let _ = std::fs::remove_file(p);
}

#[test]
fn mixed_claude_blocks_preserve_tools_and_do_not_count_request_usage_twice() {
    let init = serde_json::json!({"type":"system","subtype":"init","session_id":"session","claude_code_version":"2.1.293"});
    let message = serde_json::json!({"type":"assistant","session_id":"session","request_id":"request","message":{"id":"message","content":[{"type":"text","text":"hello"},{"type":"tool_use","id":"call","name":"Read","input":{"file_path":"example.rs"}},{"type":"thinking","thinking":"summary"},{"type":"future_block","text":"unknown"}],"usage":{"input_tokens":10}}});
    let p = temporary("claude-blocks", &format!("{init}\n{message}\n{message}\n"));
    let report = StreamAdapter::new(p.parent().unwrap(), StreamSurface::ClaudeStream, "2.1.293")
        .import(&p)
        .unwrap();
    assert_eq!(report.usage.len(), 1);
    assert_eq!(report.session.unrecognised_total(), 2);
    assert!(report
        .session
        .events()
        .iter()
        .any(|e| matches!(&e.kind,EventKind::ToolCall{tool,..} if tool=="Read")));
    assert!(report
        .session
        .events()
        .windows(2)
        .all(|w| w[0].sequence < w[1].sequence));
    let _ = std::fs::remove_file(p);
}
