//! Claude Code context reconstruction: the parent-chain walk.
//!
//! # Why this is not a filter over the file
//!
//! Claude Code's log is a DAG. Rewinding a conversation or editing a message
//! starts a new branch while the abandoned one stays in the file. Selecting
//! "every event before line N" therefore includes messages the model was never
//! shown, and produces a breakdown that looks plausible and is wrong.
//!
//! The context at a turn is the chain reached by following `parentUuid` back
//! from that turn's anchor to the root, then reversing into chronological
//! order. Sibling branches are excluded automatically because nothing on them
//! is an ancestor.
//!
//! # Compaction boundaries stop the walk
//!
//! A `compact_boundary` event has a null `parentUuid` and a
//! `logicalParentUuid` pointing at the pre-compaction history. It is tempting
//! to follow that link to "recover" the earlier conversation -- and it would be
//! wrong. That history was summarised away and was *not* in the prompt.
//!
//! So the walk stops at a boundary and records the compaction. The
//! `logicalParentUuid` link is for lifecycle and diff views, which answer "what
//! was dropped", not "what was present".

use ct_domain::model::event::EventKind;
use ct_domain::ports::{PortError, PortResult, ReconstructedContext, TokenEstimator};
use ct_domain::{
    AgentSession, CompactionEvent, ContextCategory, ContextItem, ContextItemId, ContextSource,
    Event, MessageRole, Provenance, TokenCount, TurnNumber,
};
use crate::tool_target::{self, CallIndex};
use std::collections::HashMap;

/// Guard against a malformed or cyclic parent chain.
///
/// Sessions in the local corpus reach a few thousand events; a chain longer
/// than this means a `parentUuid` cycle, and spinning forever is a worse
/// failure than truncating.
const MAX_CHAIN: usize = 100_000;

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
        .ok_or_else(|| PortError::Unsupported(format!("turn {turn} has no anchor event")))?;

    let by_uuid = index_by_uuid(session);
    // A subagent runs against its own context window. Reconstructing the main
    // thread must exclude its transcript, and reconstructing the subagent must
    // exclude the main thread's -- so the walk keeps only events on the same
    // side as the turn being asked about.
    let want_sidechain = session
        .event(anchor)
        .map(|e| e.links.is_sidechain)
        .unwrap_or(false);
    let (chain, compaction) = walk_ancestors(session, anchor, &by_uuid, want_sidechain);

    let events: Vec<&Event> = chain
        .into_iter()
        .filter_map(|index| session.event(index))
        .collect();

    // A tool_result carries only the id of the call it answers, so without this
    // join the largest contributor in a session reads "toolu_01Aux..." instead
    // of "Bash(npm test)" -- which defeats the point of the view.
    let tool_names = tool_names_by_call_id(&events);

    let mut items: Vec<ContextItem> = events
        .iter()
        .filter(|event| event.occupies_context())
        .filter_map(|event| to_item(event, estimator, &tool_names))
        .collect();

    promote_current_prompt(&mut items);

    Ok(ReconstructedContext {
        items,
        observed_total: turn_data.prompt_tokens().map(TokenCount::observed),
        context_window: turn_data
            .usage
            .context_window
            .or(session.metadata().context_window),
        model: turn_data
            .model
            .clone()
            .or_else(|| session.metadata().model.clone()),
        preceding_compaction: compaction,
    })
}

fn index_by_uuid(session: &AgentSession) -> HashMap<&str, usize> {
    session
        .events()
        .iter()
        .enumerate()
        .filter_map(|(i, e)| e.links.uuid.as_deref().map(|u| (u, i)))
        .collect()
}

/// Walk from `anchor` back to the root, returning indices in chronological
/// order plus any compaction that terminated the walk.
///
/// `want_sidechain` selects which thread's events count as context. Events from
/// the other side are stepped over rather than stopped at: a subagent's
/// transcript sitting between two main-thread messages does not sever the main
/// thread's history, it simply was not part of its prompt.
fn walk_ancestors(
    session: &AgentSession,
    anchor: usize,
    by_uuid: &HashMap<&str, usize>,
    want_sidechain: bool,
) -> (Vec<usize>, Option<CompactionEvent>) {
    let mut chain = Vec::new();
    let mut compaction = None;
    let mut cursor = Some(anchor);
    let mut steps = 0;

    while let Some(index) = cursor {
        steps += 1;
        if steps > MAX_CHAIN {
            break;
        }
        let Some(event) = session.event(index) else {
            break;
        };

        if let EventKind::Compacted(facts) = &event.kind {
            // Everything beyond this point was summarised away. Record it and
            // stop; do NOT follow logical_parent_uuid.
            compaction = Some(CompactionEvent {
                turn: event.turn,
                facts: facts.clone(),
                source: event.source,
            });
            break;
        }

        if event.links.is_sidechain == want_sidechain {
            chain.push(index);
        }

        cursor = event
            .links
            .parent_uuid
            .as_deref()
            .and_then(|parent| by_uuid.get(parent).copied())
            // Guard against a self-referencing parent, which would loop.
            .filter(|&next| next != index);
    }

    chain.reverse();
    (chain, compaction)
}

/// Map each tool call's id to its name and target, so results can be named.
fn tool_names_by_call_id<'a>(events: &[&'a Event]) -> CallIndex<'a> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::ToolCall {
                tool,
                call_id: Some(id),
                target,
                ..
            } => Some((id.as_str(), (tool.as_str(), target.as_deref()))),
            _ => None,
        })
        .collect()
}

fn to_item(
    event: &Event,
    estimator: &dyn TokenEstimator,
    tool_names: &CallIndex<'_>,
) -> Option<ContextItem> {
    let char_len = event.char_len().unwrap_or(0);
    let (category, source, label) = classify(event, tool_names)?;

    Some(ContextItem {
        id: ContextItemId::new(format!("claude:{}", event.source.line_no)),
        category,
        label,
        source,
        tokens: estimator.estimate_from_chars(char_len),
        first_seen_turn: event.turn,
        // Membership is observed: this event is genuinely on the ancestor chain
        // of the request. Its *size* is estimated, which `tokens` carries.
        provenance: Provenance::observed(event.source),
        preview: match &event.kind {
            EventKind::Message { preview, .. } if !preview.is_empty() => Some(preview.clone()),
            _ => None,
        },
    })
}

fn classify(
    event: &Event,
    tool_names: &CallIndex<'_>,
) -> Option<(ContextCategory, ContextSource, String)> {
    Some(match &event.kind {
        EventKind::Message { role, preview, .. } => match role {
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
            MessageRole::Developer | MessageRole::System => (
                ContextCategory::DeveloperInstructions,
                ContextSource::HarnessInjection {
                    mechanism: "system message".into(),
                },
                "System message".to_string(),
            ),
        },
        EventKind::Reasoning { redacted, .. } => (
            ContextCategory::Reasoning,
            ContextSource::ModelOutput,
            if *redacted {
                // Named so it is obvious in `ct largest` that this row's size is
                // derived from a signature rather than measured from text.
                "Thinking (redacted, size derived)".to_string()
            } else {
                "Thinking".to_string()
            },
        ),
        EventKind::ToolCall { tool, target, .. } => (
            ContextCategory::ToolCalls,
            ContextSource::ToolExecution { tool: tool.clone() },
            tool_target::label("Tool call", tool, target.as_deref()),
        ),
        EventKind::ToolResult { tool, call_id, .. } => {
            // A result records only the id of the call it answers, so both the
            // tool's name and what it acted on have to be looked up. Falling
            // back to the raw id keeps the row addressable when the matching
            // call is not on this chain.
            let matched = call_id.as_deref().and_then(|id| tool_names.get(id));
            let name = tool
                .clone()
                .or_else(|| matched.map(|(n, _)| n.to_string()))
                .or_else(|| call_id.clone())
                .unwrap_or_else(|| "tool".into());
            let target = matched.and_then(|(_, t)| *t);
            (
                ContextCategory::ToolOutputs,
                // The source stays the bare tool name: it is what `--source
                // tool:Read` matches, and a filter over paths is `--source
                // file:` territory rather than this.
                ContextSource::ToolExecution { tool: name.clone() },
                tool_target::label("Tool output", &name, target),
            )
        }
        EventKind::ContextInjection {
            mechanism, label, ..
        } => {
            let (category, source) = classify_injection(mechanism, label);
            (category, source, label.clone())
        }
        _ => return None,
    })
}

/// Map an attachment mechanism onto category and provenance.
///
/// This table is the instruction-tracing feature. Claude Code labels its own
/// injections, so these classifications are read from the log rather than
/// inferred -- which is why "where did this instruction come from?" can ship at
/// P0 instead of being a research project.
fn classify_injection(mechanism: &str, label: &str) -> (ContextCategory, ContextSource) {
    match mechanism {
        "nested_memory" => (
            ContextCategory::RepositoryInstructions,
            ContextSource::InstructionFile {
                path: label.to_string(),
            },
        ),
        "file" | "edited_text_file" => (
            ContextCategory::FileContents,
            ContextSource::FileRead {
                path: label.to_string(),
            },
        ),
        "compact_file_reference" => (ContextCategory::Summaries, ContextSource::CompactionSummary),
        // Tool and agent listings are literally the tool schemas, which is the
        // component most often invisible in other tools.
        "deferred_tools_delta" | "agent_listing_delta" | "mcp_instructions_delta" => (
            ContextCategory::ToolDefinitions,
            ContextSource::HarnessInjection {
                mechanism: mechanism.to_string(),
            },
        ),
        "skill_listing" | "invoked_skills" | "dynamic_skill" | "hook_additional_context"
        | "hook_success" | "plan_mode" | "plan_mode_exit" | "plan_mode_reentry"
        | "plan_file_reference" => (
            ContextCategory::DeveloperInstructions,
            ContextSource::HarnessInjection {
                mechanism: mechanism.to_string(),
            },
        ),
        _ => (
            ContextCategory::Other,
            ContextSource::HarnessInjection {
                mechanism: mechanism.to_string(),
            },
        ),
    }
}

/// Re-label the most recent user message as the prompt driving this turn.
fn promote_current_prompt(items: &mut [ContextItem]) {
    if let Some(last_user) = items
        .iter_mut()
        .rev()
        .find(|i| i.category == ContextCategory::UserMessages)
    {
        last_user.category = ContextCategory::CurrentPrompt;
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
        AgentKind, EventId, FileId, SessionId, SessionMetadata, SourceRef, TokenUsage, Turn,
    };

    /// Build an event with explicit DAG links.
    fn ev(line: u32, uuid: &str, parent: Option<&str>, kind: EventKind) -> Event {
        Event {
            id: EventId::Uuid(uuid.into()),
            sequence: line - 1,
            timestamp: None,
            kind,
            source: SourceRef::new(FileId(0), line as u64 * 100, 100, line),
            raw_type: "assistant".into(),
            turn: Some(TurnNumber::FIRST),
            links: EventLinks {
                uuid: Some(uuid.into()),
                parent_uuid: parent.map(String::from),
                logical_parent_uuid: None,
                is_sidechain: false,
            },
        }
    }

    fn msg(role: MessageRole, chars: u32) -> EventKind {
        EventKind::Message {
            role,
            preview: String::new(),
            char_len: chars,
        }
    }

    fn session(events: Vec<Event>, anchor: usize, prompt: u32) -> AgentSession {
        let turn = Turn {
            number: TurnNumber::FIRST,
            timestamp: None,
            model: Some("claude-opus-4-8".into()),
            usage: TokenUsage {
                input: Some(prompt),
                ..Default::default()
            },
            event_indices: (0..events.len()).collect(),
            anchor_index: Some(anchor),
        };
        AgentSession::new(
            SessionId::new("cc-1").unwrap(),
            AgentKind::ClaudeCode,
            SessionMetadata::default(),
            events,
            vec![turn],
            vec![],
        )
    }

    #[test]
    fn abandoned_branches_are_excluded() {
        // a -> b (abandoned)
        //  \-> c -> d (live, anchored at d)
        let events = vec![
            ev(1, "a", None, msg(MessageRole::User, 100)),
            ev(2, "b", Some("a"), msg(MessageRole::Assistant, 9_999)),
            ev(3, "c", Some("a"), msg(MessageRole::Assistant, 200)),
            ev(4, "d", Some("c"), msg(MessageRole::User, 300)),
        ];
        let s = session(events, 3, 5000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();

        assert_eq!(r.items.len(), 3, "the abandoned branch must not be included");
        assert!(
            r.items.iter().all(|i| i.tokens.tokens() < 1000),
            "the 9,999-char abandoned message leaked into the context"
        );
    }

    #[test]
    fn the_chain_is_returned_in_chronological_order() {
        let events = vec![
            ev(1, "a", None, msg(MessageRole::User, 10)),
            ev(2, "b", Some("a"), msg(MessageRole::Assistant, 20)),
            ev(3, "c", Some("b"), msg(MessageRole::User, 30)),
        ];
        let s = session(events, 2, 100);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();

        let lines: Vec<u32> = r
            .items
            .iter()
            .map(|i| i.provenance.source.unwrap().line_no)
            .collect();
        assert_eq!(lines, vec![1, 2, 3]);
    }

    #[test]
    fn the_walk_stops_at_a_compaction_rather_than_resurrecting_dropped_history() {
        let mut boundary = ev(
            3,
            "boundary",
            None,
            EventKind::Compacted(CompactionFacts {
                trigger: Some("auto".into()),
                tokens_before: Some(165_223),
                tokens_after: Some(17_542),
                ..Default::default()
            }),
        );
        // The real format sets this; following it would be the bug.
        boundary.links.logical_parent_uuid = Some("b".into());

        let events = vec![
            ev(1, "a", None, msg(MessageRole::User, 50_000)),
            ev(2, "b", Some("a"), msg(MessageRole::Assistant, 50_000)),
            boundary,
            ev(4, "post", Some("boundary"), msg(MessageRole::User, 100)),
        ];
        let s = session(events, 3, 20_000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();

        assert_eq!(r.items.len(), 1, "pre-compaction history must not be resurrected");
        let compaction = r.preceding_compaction.expect("compaction must be reported");
        assert_eq!(compaction.facts.tokens_before, Some(165_223));
        assert_eq!(compaction.reduction(), Some(147_681));
    }

    #[test]
    fn a_subagent_transcript_never_enters_the_main_thread_context() {
        // A subagent runs against its own context window. Folding its transcript
        // into the main thread inflates every figure for the main thread, and
        // nothing in the corpus would catch it: no session on this machine uses
        // subagents, so only a fixture can hold the line.
        let mut sub = ev(2, "sub", Some("a"), msg(MessageRole::Assistant, 500_000));
        sub.links.is_sidechain = true;

        let events = vec![
            ev(1, "a", None, msg(MessageRole::User, 100)),
            sub,
            ev(3, "c", Some("sub"), msg(MessageRole::Assistant, 200)),
        ];
        let s = session(events, 2, 5_000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();

        assert_eq!(
            r.items.len(),
            2,
            "the subagent's 500,000 characters are not in the main thread's prompt"
        );
        assert!(
            r.items.iter().all(|i| i.tokens.tokens() < 1_000),
            "subagent content leaked into the main thread"
        );
    }

    #[test]
    fn a_subagent_turn_sees_its_own_thread_and_not_the_main_one() {
        // The exclusion has to run both ways, or asking about the subagent's own
        // turn would return an empty context.
        let mut sub_a = ev(2, "sa", Some("a"), msg(MessageRole::User, 300));
        sub_a.links.is_sidechain = true;
        let mut sub_b = ev(3, "sb", Some("sa"), msg(MessageRole::Assistant, 400));
        sub_b.links.is_sidechain = true;

        let events = vec![
            ev(1, "a", None, msg(MessageRole::User, 900_000)),
            sub_a,
            sub_b,
        ];
        let s = session(events, 2, 1_000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();

        assert_eq!(r.items.len(), 2, "the subagent's own two events are its context");
        assert!(
            r.items.iter().all(|i| i.tokens.tokens() < 1_000),
            "the main thread's 900,000 characters are not in the subagent's prompt"
        );
    }

    #[test]
    fn a_parent_cycle_terminates_instead_of_hanging() {
        let events = vec![
            ev(1, "a", Some("b"), msg(MessageRole::User, 10)),
            ev(2, "b", Some("a"), msg(MessageRole::Assistant, 10)),
        ];
        let s = session(events, 1, 100);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();
        assert!(!r.items.is_empty());
    }

    #[test]
    fn a_self_referencing_parent_does_not_loop() {
        let events = vec![ev(1, "a", Some("a"), msg(MessageRole::User, 10))];
        let s = session(events, 0, 100);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();
        assert_eq!(r.items.len(), 1);
    }

    #[test]
    fn tool_outputs_are_named_from_their_matching_call() {
        let events = vec![
            ev(
                1,
                "a",
                None,
                EventKind::ToolCall {
                    tool: "Bash".into(),
                    call_id: Some("toolu_01".into()),
                    char_len: 50,
                    target: Some("npm test".into()),
                },
            ),
            ev(
                2,
                "b",
                Some("a"),
                EventKind::ToolResult {
                    tool: None,
                    call_id: Some("toolu_01".into()),
                    char_len: 120_000,
                    is_error: false,
                },
            ),
        ];
        let s = session(events, 1, 40_000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_code()).unwrap();

        let output = r
            .items
            .iter()
            .find(|i| i.category == ContextCategory::ToolOutputs)
            .expect("the tool output must be present");
        assert_eq!(
            output.label, "Tool output: Bash npm test",
            "an opaque toolu_ id here defeats the whole 'find the giant tool result' workflow"
        );
        // The source stays the bare tool name, because that is what
        // `--source tool:Bash` matches. The target belongs in the label.
        assert_eq!(
            output.source,
            ContextSource::ToolExecution {
                tool: "Bash".into()
            }
        );
    }

    #[test]
    fn a_call_whose_arguments_name_nothing_keeps_the_bare_tool_name() {
        // TodoWrite and friends carry no path, command or pattern. A label
        // invented from some other argument would be worse than none.
        let events = vec![
            ev(
                1,
                "a",
                None,
                EventKind::ToolCall {
                    tool: "TodoWrite".into(),
                    call_id: Some("toolu_09".into()),
                    char_len: 50,
                    target: None,
                },
            ),
            ev(
                2,
                "b",
                Some("a"),
                EventKind::ToolResult {
                    tool: None,
                    call_id: Some("toolu_09".into()),
                    char_len: 120_000,
                    is_error: false,
                },
            ),
        ];
        let s = session(events, 1, 40_000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_code()).unwrap();
        let output = r
            .items
            .iter()
            .find(|i| i.category == ContextCategory::ToolOutputs)
            .expect("the tool output must be present");
        assert_eq!(output.label, "Tool output: TodoWrite");
    }

    #[test]
    fn an_unmatched_tool_result_falls_back_to_its_id_rather_than_vanishing() {
        let events = vec![ev(
            1,
            "a",
            None,
            EventKind::ToolResult {
                tool: None,
                call_id: Some("toolu_orphan".into()),
                char_len: 100,
                is_error: false,
            },
        )];
        let s = session(events, 0, 1000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_code()).unwrap();
        assert_eq!(r.items[0].label, "Tool output: toolu_orphan");
    }

    #[test]
    fn claude_md_injections_are_traced_to_their_file() {
        let events = vec![ev(
            1,
            "a",
            None,
            EventKind::ContextInjection {
                mechanism: "nested_memory".into(),
                label: "server\\CLAUDE.md".into(),
                char_len: 4000,
            },
        )];
        let s = session(events, 0, 2000);
        let r = reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).unwrap();

        let item = &r.items[0];
        assert_eq!(item.category, ContextCategory::RepositoryInstructions);
        assert_eq!(
            item.source,
            ContextSource::InstructionFile {
                path: "server\\CLAUDE.md".into()
            }
        );
        assert_eq!(item.provenance.confidence, ct_domain::Confidence::Observed);
    }

    #[test]
    fn tool_schema_injections_are_categorised_as_tool_definitions() {
        for mechanism in ["deferred_tools_delta", "agent_listing_delta", "mcp_instructions_delta"] {
            let (category, _) = classify_injection(mechanism, "x");
            assert_eq!(
                category,
                ContextCategory::ToolDefinitions,
                "{mechanism} should count as tool definitions"
            );
        }
    }

    #[test]
    fn an_unknown_injection_mechanism_is_kept_rather_than_dropped() {
        let (category, source) = classify_injection("some_future_thing", "x");
        assert_eq!(category, ContextCategory::Other);
        assert_eq!(
            source,
            ContextSource::HarnessInjection {
                mechanism: "some_future_thing".into()
            }
        );
    }

    #[test]
    fn a_turn_without_an_anchor_is_reported_rather_than_guessed() {
        let mut s = session(vec![ev(1, "a", None, msg(MessageRole::User, 10))], 0, 100);
        // Rebuild with no anchor.
        s = AgentSession::new(
            SessionId::new("cc-2").unwrap(),
            AgentKind::ClaudeCode,
            SessionMetadata::default(),
            s.events().to_vec(),
            vec![Turn {
                number: TurnNumber::FIRST,
                timestamp: None,
                model: None,
                usage: TokenUsage::default(),
                event_indices: vec![0],
                anchor_index: None,
            }],
            vec![],
        );
        assert!(reconstruct(&s, TurnNumber::FIRST, &HeuristicEstimator::for_prose()).is_err());
    }
}
