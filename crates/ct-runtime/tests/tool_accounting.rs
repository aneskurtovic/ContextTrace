use ct_adapters::claude_code::ClaudeCodeAdapter;
use ct_application::{Corpus, NotificationEngine, NotificationInputs};
use ct_domain::ports::AgentAdapter;
use ct_domain::{
    AgentKind, EventKind, NotificationEvidence, NotificationRuleId, NotificationSettings,
    SessionDescriptor, SessionId, SessionNotificationCheckpoint, ThreadRole,
};

#[test]
fn claude_multi_tool_fixture_preserves_accounting_and_later_error() {
    let descriptor = SessionDescriptor {
        id: SessionId::new("chat-claude").unwrap(),
        agent: AgentKind::ClaudeCode,
        path: concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/claude_code/chat-blocks.jsonl"
        )
        .into(),
        size_bytes: 0,
        project: None,
        title: None,
        git_branch: None,
        started_at: None,
        last_activity: None,
        thread_role: ThreadRole::Root,
    };
    let session = ClaudeCodeAdapter::new().load(&descriptor).unwrap();
    assert_eq!(session.events().len(), 3);
    assert_eq!(session.turn_count(), 1);
    assert_eq!(session.total_output_tokens(), 10);
    assert_eq!(session.events()[1].links.uuid.as_deref(), Some("a1"));
    assert_eq!(session.events()[2].links.parent_uuid.as_deref(), Some("a1"));
    let calls: Vec<_> = session.events()[1].tool_operations().collect();
    assert_eq!(calls.len(), 2);
    assert!(
        matches!(calls[1], EventKind::ToolCall { call_id: Some(id), target: Some(target), .. } if id == "call-b" && target.contains("b.rs"))
    );
    let results: Vec<_> = session.events()[2].tool_operations().collect();
    assert_eq!(results.len(), 2);
    assert!(
        matches!(results[1], EventKind::ToolResult { call_id: Some(id), is_error: true, .. } if id == "call-b")
    );
    let expected_chars: u64 = results
        .iter()
        .map(|kind| match kind {
            EventKind::ToolResult { char_len, .. } => u64::from(*char_len),
            _ => unreachable!(),
        })
        .sum();
    // Explanatory user text/unknown blocks belong to the line, not either tool.
    assert!(expected_chars < u64::from(session.events()[2].char_len().unwrap()));
    let mut corpus = Corpus::new();
    corpus.add(&descriptor, &session);
    let report = corpus.finish();
    assert_eq!(report.tool_calls, 2);
    assert_eq!(report.tool_errors, 1);
    let read = report
        .by_tool
        .iter()
        .find(|tool| tool.tool == "Read")
        .unwrap();
    assert_eq!(
        (read.calls, read.errors, read.result_chars),
        (2, 1, expected_chars)
    );
    let mut settings = NotificationSettings {
        enabled: true,
        ..Default::default()
    };
    settings.tool_error_streak.count = 1;
    let prior = SessionNotificationCheckpoint::baseline(session.agent(), session.id().clone());
    let evaluation =
        NotificationEngine::evaluate(&session, &prior, &settings, NotificationInputs::default());
    let alert = evaluation
        .candidates
        .iter()
        .find(|alert| alert.rule == NotificationRuleId::ToolErrorStreak)
        .unwrap();
    assert_eq!(alert.location.line, Some(3));
    assert!(
        matches!(&alert.evidence, NotificationEvidence::ToolErrorStreak { tool, streak: 1 } if tool == "Read")
    );
    let next = NotificationEngine::evaluate(
        &session,
        &evaluation.checkpoint,
        &settings,
        NotificationInputs::default(),
    );
    assert!(next.candidates.is_empty());
}
