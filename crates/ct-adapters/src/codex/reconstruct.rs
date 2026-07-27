//! Codex context reconstruction: replay.
//!
//! Because `response_item` lines are the literal API items, "what was in
//! context at turn N" is answered by folding those items forward from the start
//! of the session to turn N's anchor. Membership is therefore
//! [`Confidence::Observed`] -- we are not inferring that an item was present, we
//! are reading the list that was sent.
//!
//! The one place the fold is not a simple append is compaction: a `compacted`
//! event replaces the accumulated history wholesale with its
//! `replacement_history`. We model that as clearing the live list and inserting
//! a single summary item, which is what the model actually saw afterwards.

use ct_domain::model::event::EventKind;
use ct_domain::ports::{PortError, PortResult, ReconstructedContext, TokenEstimator};
use ct_domain::{
    AgentSession, CompactionEvent, Confidence, ContextCategory, ContextItem, ContextItemId,
    ContextSource, Event, MessageRole, Provenance, TokenCount, TurnNumber,
};

pub fn reconstruct(
    session: &AgentSession,
    turn: TurnNumber,
    estimator: &dyn TokenEstimator,
) -> PortResult<ReconstructedContext> {
    let turn_data = session
        .turn(turn)
        .ok_or_else(|| PortError::NotFound(format!("turn {turn} in session {}", session.id())))?;

    let anchor = turn_data
        .anchor_index
        .unwrap_or_else(|| session.events().len().saturating_sub(1));

    // The live item list, rebuilt by replaying events in order.
    let mut live: Vec<ContextItem> = Vec::new();
    let mut preceding_compaction: Option<CompactionEvent> = None;

    for (index, event) in session.events().iter().enumerate() {
        if index > anchor {
            break;
        }

        if let EventKind::Compacted(facts) = &event.kind {
            // Everything before this point stopped being in context.
            live.clear();
            preceding_compaction = Some(CompactionEvent {
                turn: event.turn,
                facts: facts.clone(),
                source: event.source,
            });
            live.push(compaction_summary_item(event, estimator));
            continue;
        }

        if !event.occupies_context() {
            continue;
        }

        if let Some(item) = to_item(event, estimator) {
            live.push(item);
        }
    }

    promote_current_prompt(&mut live);

    let observed_total = turn_data.prompt_tokens().map(TokenCount::observed);
    let context_window = turn_data
        .usage
        .context_window
        .or(session.metadata().context_window);

    Ok(ReconstructedContext {
        items: live,
        observed_total,
        context_window,
        model: turn_data.model.clone().or_else(|| session.metadata().model.clone()),
        preceding_compaction,
    })
}

/// Translate one context-occupying event into a context item.
fn to_item(event: &Event, estimator: &dyn TokenEstimator) -> Option<ContextItem> {
    let char_len = event.char_len().unwrap_or(0);
    let (category, source, label) = classify(event)?;

    Some(ContextItem {
        id: ContextItemId::new(format!("codex:{}", event.source.line_no)),
        category,
        label,
        source,
        tokens: estimator.estimate_from_chars(char_len),
        first_seen_turn: event.turn,
        // Membership is observed -- this item is in the replayed API item list.
        // Its *size* is a separate question, carried by `tokens`.
        provenance: Provenance::observed(event.source),
        preview: preview_for(event),
    })
}

fn classify(event: &Event) -> Option<(ContextCategory, ContextSource, String)> {
    Some(match &event.kind {
        EventKind::Message { role, preview, .. } => match role {
            MessageRole::Developer | MessageRole::System => (
                ContextCategory::DeveloperInstructions,
                ContextSource::HarnessInjection {
                    mechanism: "developer message".into(),
                },
                "Developer instructions".to_string(),
            ),
            MessageRole::User => (
                ContextCategory::UserMessages,
                ContextSource::UserPrompt,
                summarise("User message", preview),
            ),
            MessageRole::Assistant => (
                ContextCategory::AssistantMessages,
                ContextSource::ModelOutput,
                summarise("Assistant message", preview),
            ),
        },
        EventKind::Reasoning { .. } => (
            ContextCategory::Reasoning,
            ContextSource::ModelOutput,
            "Reasoning".to_string(),
        ),
        EventKind::ToolCall { tool, .. } => (
            ContextCategory::ToolCalls,
            ContextSource::ToolExecution { tool: tool.clone() },
            format!("Tool call: {tool}"),
        ),
        EventKind::ToolResult { tool, call_id, .. } => {
            let name = tool
                .clone()
                .or_else(|| call_id.clone())
                .unwrap_or_else(|| "tool".into());
            (
                ContextCategory::ToolOutputs,
                ContextSource::ToolExecution { tool: name.clone() },
                format!("Tool output: {name}"),
            )
        }
        EventKind::ContextInjection {
            mechanism, label, ..
        } => (
            if mechanism == "base_instructions" {
                ContextCategory::SystemInstructions
            } else {
                ContextCategory::DeveloperInstructions
            },
            if mechanism == "base_instructions" {
                ContextSource::AgentSystemPrompt
            } else {
                ContextSource::HarnessInjection {
                    mechanism: mechanism.clone(),
                }
            },
            label.clone(),
        ),
        _ => return None,
    })
}

/// Represent post-compaction history as a single summary item.
///
/// Sized from the compaction line's own byte length. That is a proxy, not a
/// measurement -- the line is JSON-encoded and may hold base64 images that were
/// never text -- so the item is explicitly [`Confidence::Estimated`] even though
/// its *presence* is certain.
fn compaction_summary_item(event: &Event, estimator: &dyn TokenEstimator) -> ContextItem {
    ContextItem {
        id: ContextItemId::new(format!("codex:compaction:{}", event.source.line_no)),
        category: ContextCategory::Summaries,
        label: "Compacted history".to_string(),
        source: ContextSource::CompactionSummary,
        tokens: estimator.estimate_from_chars(event.source.byte_len),
        first_seen_turn: event.turn,
        provenance: Provenance {
            confidence: Confidence::Estimated,
            source: Some(event.source),
        },
        preview: None,
    }
}

/// Re-label the most recent user message as the prompt that drove this turn.
///
/// Distinguishing "the question being asked now" from "conversation history"
/// is what makes the composition view answer *why this request looks like
/// this*, rather than just how big it is.
fn promote_current_prompt(items: &mut [ContextItem]) {
    if let Some(last_user) = items
        .iter_mut()
        .rev()
        .find(|i| i.category == ContextCategory::UserMessages)
    {
        last_user.category = ContextCategory::CurrentPrompt;
    }
}

fn preview_for(event: &Event) -> Option<String> {
    match &event.kind {
        EventKind::Message { preview, .. } if !preview.is_empty() => Some(preview.clone()),
        _ => None,
    }
}

fn summarise(prefix: &str, preview: &str) -> String {
    if preview.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}: {preview}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizers::HeuristicEstimator;
    use ct_domain::model::event::{CompactionFacts, EventLinks};
    use ct_domain::{
        AgentKind, AgentSession, EventId, FileId, SessionId, SessionMetadata, SourceRef, TokenUsage,
        Turn,
    };

    fn event(line: u32, kind: EventKind, turn: Option<TurnNumber>) -> Event {
        Event {
            id: EventId::Ordinal(line),
            sequence: line - 1,
            timestamp: None,
            kind,
            source: SourceRef::new(FileId(0), line as u64 * 100, 100, line),
            raw_type: "response_item".into(),
            turn,
            links: EventLinks::default(),
        }
    }

    fn message(line: u32, role: MessageRole, chars: u32) -> Event {
        event(
            line,
            EventKind::Message {
                role,
                preview: "hi".into(),
                char_len: chars,
            },
            Some(TurnNumber::FIRST),
        )
    }

    fn session(events: Vec<Event>, anchor: usize, prompt_tokens: u32) -> AgentSession {
        let turn = Turn {
            number: TurnNumber::FIRST,
            timestamp: None,
            model: Some("gpt-5".into()),
            usage: TokenUsage {
                input: Some(prompt_tokens),
                context_window: Some(258_400),
                ..Default::default()
            },
            event_indices: (0..events.len()).collect(),
            anchor_index: Some(anchor),
        };
        AgentSession::new(
            SessionId::new("codex-1").unwrap(),
            AgentKind::Codex,
            SessionMetadata::default(),
            events,
            vec![turn],
            vec![],
        )
    }

    #[test]
    fn replay_includes_everything_up_to_the_anchor_and_nothing_after() {
        let events = vec![
            message(1, MessageRole::User, 100),
            message(2, MessageRole::Assistant, 100),
            event(3, EventKind::TokenReport(TokenUsage::default()), None),
            message(4, MessageRole::User, 100), // after the anchor
        ];
        let s = session(events, 2, 5000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();

        assert_eq!(r.items.len(), 2, "the post-anchor message must not appear");
        assert_eq!(r.observed_total.unwrap().tokens(), 5000);
        assert_eq!(r.context_window, Some(258_400));
    }

    #[test]
    fn membership_is_observed_even_though_sizes_are_estimated() {
        let s = session(vec![message(1, MessageRole::User, 400)], 0, 1000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();
        let item = &r.items[0];
        assert_eq!(item.provenance.confidence, Confidence::Observed);
        assert_eq!(item.tokens.confidence(), Confidence::Estimated);
        // The combination is only as strong as its weakest part.
        assert_eq!(item.confidence(), Confidence::Estimated);
    }

    #[test]
    fn compaction_clears_prior_context_and_leaves_a_summary() {
        let events = vec![
            message(1, MessageRole::User, 10_000),
            message(2, MessageRole::Assistant, 10_000),
            event(
                3,
                EventKind::Compacted(CompactionFacts {
                    replacement_recorded: true,
                    ..Default::default()
                }),
                Some(TurnNumber::FIRST),
            ),
            message(4, MessageRole::User, 50),
        ];
        let s = session(events, 3, 2000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();

        assert_eq!(r.items.len(), 2, "pre-compaction messages must be dropped");
        assert_eq!(r.items[0].category, ContextCategory::Summaries);
        assert!(r.preceding_compaction.is_some());
        assert!(r.preceding_compaction.unwrap().facts.replacement_recorded);
    }

    #[test]
    fn the_latest_user_message_becomes_the_current_prompt() {
        let events = vec![
            message(1, MessageRole::User, 100),
            message(2, MessageRole::Assistant, 100),
            message(3, MessageRole::User, 100),
        ];
        let s = session(events, 2, 1000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();

        assert_eq!(r.items[0].category, ContextCategory::UserMessages, "history stays history");
        assert_eq!(r.items[2].category, ContextCategory::CurrentPrompt);
    }

    #[test]
    fn the_system_prompt_is_accounted_for_rather_than_left_to_the_residual() {
        let events = vec![
            event(
                1,
                EventKind::ContextInjection {
                    mechanism: "base_instructions".into(),
                    label: "Codex system prompt".into(),
                    char_len: 9000,
                },
                Some(TurnNumber::FIRST),
            ),
            message(2, MessageRole::User, 100),
        ];
        let s = session(events, 1, 5000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();

        let sys = &r.items[0];
        assert_eq!(sys.category, ContextCategory::SystemInstructions);
        assert_eq!(sys.source, ContextSource::AgentSystemPrompt);
        assert!(sys.tokens.tokens() > 1000);
    }

    #[test]
    fn a_missing_turn_is_an_error_not_an_empty_context() {
        let s = session(vec![message(1, MessageRole::User, 10)], 0, 100);
        let missing = TurnNumber::new(99).unwrap();
        assert!(reconstruct(&s, missing, &HeuristicEstimator::for_prose()).is_err());
    }
}
