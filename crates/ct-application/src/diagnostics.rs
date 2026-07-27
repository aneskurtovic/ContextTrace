//! Read-only diagnostics: the beginnings of "Context Doctor".
//!
//! Everything here is a pure function of a parsed session. Nothing is written,
//! nothing is inferred beyond what the session states, and every figure keeps
//! the confidence of the data it came from.

use ct_domain::model::event::EventKind;
use ct_domain::{AgentSession, TurnNumber};
use serde::Serialize;

/// Growth between consecutive turns large enough to be worth explaining.
///
/// 20,000 tokens is roughly the size at which a single tool output starts to
/// dominate a context window, and it is far above ordinary conversational
/// growth, so it flags the events users actually care about without burying
/// them in noise.
pub const SPIKE_THRESHOLD: u32 = 20_000;

/// A sudden jump in prompt size, with the events that plausibly caused it.
#[derive(Debug, Clone, Serialize)]
pub struct ResidualSpike {
    pub turn: u32,
    pub previous_tokens: u32,
    pub tokens: u32,
    pub growth: u32,
    /// Tool calls and outputs recorded between the two turns.
    ///
    /// Deliberately called a *candidate*, not a cause. The session records that
    /// these events occurred between the turns; it does not prove which one was
    /// responsible, and claiming otherwise would be exactly the kind of
    /// confident guess ContextTrace exists to avoid.
    pub candidates: Vec<String>,
}

/// Everything `ct doctor` reports for one session.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostics {
    /// Fraction of events successfully mapped to domain concepts, 0.0 to 1.0.
    pub fidelity: f32,
    pub total_events: usize,
    pub unrecognised_events: u32,
    /// Unrecognised agent event types with counts, worst first.
    pub unrecognised_types: Vec<(String, u32)>,
    pub turns: usize,
    pub peak_prompt_tokens: Option<u32>,
    pub peak_turn: Option<u32>,
    pub compactions: usize,
    /// Tokens reclaimed by compaction, where the agent reported both sides.
    pub compaction_reduction: Option<u32>,
    pub spikes: Vec<ResidualSpike>,
    /// Turns for which the agent reported no usage, so context size is unknown.
    pub turns_without_usage: usize,
    /// Turns whose figures the agent produced from more than one API call.
    ///
    /// Worth reporting because the log's top-level cache fields for such a turn
    /// are sums across those calls, and reading them as one prompt size is how
    /// a turn comes to claim more tokens than a context window holds.
    pub multi_call_turns: usize,
    /// Reasoning events whose text the agent stripped from the log.
    ///
    /// Their size is derived from the leftover signature, so a session with many
    /// of these is reconstructed less directly than its fidelity score suggests.
    pub redacted_reasoning: usize,
}

impl Diagnostics {
    /// True when the parse understood everything the agent wrote.
    ///
    /// Says nothing about whether the agent wrote everything down -- see
    /// [`Diagnostics::reconstruction_caveats`].
    pub fn is_fully_understood(&self) -> bool {
        self.unrecognised_events == 0
    }

    /// Ways this session is reconstructed less directly than its fidelity score
    /// suggests, phrased for a one-line summary.
    pub fn reconstruction_caveats(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.redacted_reasoning > 0 {
            out.push(format!(
                "{} reasoning event(s) were logged without their text",
                self.redacted_reasoning
            ));
        }
        if self.multi_call_turns > 0 {
            out.push(format!(
                "{} turn(s) report figures from several API calls",
                self.multi_call_turns
            ));
        }
        out
    }

    /// One-line summary for terminal output.
    ///
    /// Recognising every event is not the same as reconstructing it faithfully.
    /// A session can score full fidelity while hundreds of its reasoning blocks
    /// were logged without their text and its turns report figures summed across
    /// several API calls. Since this line is the one people quote, it must not
    /// claim more than the parse actually established.
    pub fn headline(&self) -> String {
        if self.is_fully_understood() {
            let caveats = self.reconstruction_caveats();
            return match caveats.is_empty() {
                true => format!("{} events, all recognised", self.total_events),
                false => format!(
                    "{} events, all recognised - but {}",
                    self.total_events,
                    caveats.join(" and ")
                ),
            };
        }
        {
            format!(
                "{} events, {} unrecognised ({:.1}% fidelity) - context reconstruction may be incomplete",
                self.total_events,
                self.unrecognised_events,
                self.fidelity * 100.0
            )
        }
    }
}

/// Analyse a parsed session.
pub fn diagnose(session: &AgentSession) -> Diagnostics {
    let mut unrecognised_types: Vec<(String, u32)> = session.unrecognised().to_vec();
    unrecognised_types.sort_by(|a, b| b.1.cmp(&a.1));

    let compactions: Vec<_> = session.compactions();
    let compaction_reduction = compactions
        .iter()
        .filter_map(|(_, event)| match &event.kind {
            EventKind::Compacted(facts) => {
                let before = facts.tokens_before?;
                let after = facts.tokens_after?;
                Some(before.saturating_sub(after))
            }
            _ => None,
        })
        .reduce(|a, b| a.saturating_add(b));

    Diagnostics {
        fidelity: session.fidelity(),
        total_events: session.events().len(),
        unrecognised_events: session.unrecognised_total(),
        unrecognised_types,
        turns: session.turn_count(),
        peak_prompt_tokens: session.peak_prompt_tokens(),
        peak_turn: peak_turn(session).map(|t| t.get()),
        compactions: compactions.len(),
        compaction_reduction,
        spikes: find_spikes(session),
        turns_without_usage: session
            .turns()
            .iter()
            .filter(|t| t.prompt_tokens().is_none())
            .count(),
        multi_call_turns: session
            .turns()
            .iter()
            .filter(|t| t.usage.api_calls.is_some_and(|n| n > 1))
            .count(),
        redacted_reasoning: session
            .events()
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Reasoning { redacted: true, .. }))
            .count(),
    }
}

fn peak_turn(session: &AgentSession) -> Option<TurnNumber> {
    session
        .turns()
        .iter()
        .filter(|t| t.prompt_tokens().is_some())
        .max_by_key(|t| t.prompt_tokens().unwrap_or(0))
        .map(|t| t.number)
}

/// Find turn-to-turn prompt growth above [`SPIKE_THRESHOLD`].
///
/// Compaction *shrinks* context, so a drop is never a spike; only growth is
/// reported.
fn find_spikes(session: &AgentSession) -> Vec<ResidualSpike> {
    let mut spikes = Vec::new();
    let turns = session.turns();

    let tool_names = tool_names_by_call_id(session);

    for window in turns.windows(2) {
        let (previous, current) = (&window[0], &window[1]);
        let (Some(before), Some(after)) = (previous.prompt_tokens(), current.prompt_tokens())
        else {
            continue;
        };
        // A zero-token baseline is not a real prompt -- it comes from synthetic
        // or errored turns that record empty usage. Measuring growth from it
        // reports the whole of the next context as a "spike", which is noise
        // dressed up as a finding.
        if before == 0 {
            continue;
        }
        if after <= before {
            continue;
        }
        let growth = after - before;
        if growth < SPIKE_THRESHOLD {
            continue;
        }

        spikes.push(ResidualSpike {
            turn: current.number.get(),
            previous_tokens: before,
            tokens: after,
            growth,
            candidates: candidates_for(session, current, &tool_names),
        });
    }

    spikes.sort_by(|a, b| b.growth.cmp(&a.growth));
    spikes
}

/// Map every tool call's id to its name, so results can be named rather than
/// shown as opaque identifiers.
fn tool_names_by_call_id(session: &AgentSession) -> std::collections::HashMap<String, String> {
    session
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::ToolCall {
                tool,
                call_id: Some(id),
                ..
            } => Some((id.clone(), tool.clone())),
            _ => None,
        })
        .collect()
}

/// Name the tool activity recorded within a turn, largest first.
fn candidates_for(
    session: &AgentSession,
    turn: &ct_domain::Turn,
    tool_names: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let mut found: Vec<(u32, String)> = turn
        .event_indices
        .iter()
        .filter_map(|&i| session.event(i))
        .filter_map(|event| {
            let chars = event.char_len().unwrap_or(0);
            match &event.kind {
                EventKind::ToolResult { tool, call_id, .. } => {
                    let name = tool
                        .clone()
                        .or_else(|| {
                            call_id
                                .as_deref()
                                .and_then(|id| tool_names.get(id))
                                .cloned()
                        })
                        .or_else(|| call_id.clone())
                        .unwrap_or_else(|| "tool".into());
                    Some((chars, format!("tool output: {name}")))
                }
                EventKind::ToolCall { tool, .. } => Some((chars, format!("tool call: {tool}"))),
                EventKind::ContextInjection { label, .. } => {
                    Some((chars, format!("injected: {label}")))
                }
                _ => None,
            }
        })
        .collect();

    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().take(3).map(|(_, name)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::model::event::{CompactionFacts, EventLinks};
    use ct_domain::{
        AgentKind, Event, EventId, FileId, SessionId, SessionMetadata, SourceRef, TokenUsage, Turn,
    };

    fn event(line: u32, kind: EventKind) -> Event {
        Event {
            id: EventId::Ordinal(line),
            sequence: line - 1,
            timestamp: None,
            kind,
            source: SourceRef::new(FileId(0), 0, 0, line),
            raw_type: "x".into(),
            turn: None,
            links: EventLinks::default(),
        }
    }

    fn turn(number: u32, prompt: u32, indices: Vec<usize>) -> Turn {
        Turn {
            number: TurnNumber::new(number).unwrap(),
            timestamp: None,
            model: None,
            usage: TokenUsage {
                input: Some(prompt),
                ..Default::default()
            },
            event_indices: indices,
            anchor_index: indices_last(&[]),
        }
    }

    fn indices_last(_: &[usize]) -> Option<usize> {
        None
    }

    fn session(events: Vec<Event>, turns: Vec<Turn>, unrecognised: Vec<(String, u32)>) -> AgentSession {
        AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::ClaudeCode,
            SessionMetadata::default(),
            events,
            turns,
            unrecognised,
        )
    }

    #[test]
    fn a_large_jump_is_flagged_and_attributed_to_candidates() {
        let events = vec![
            event(1, EventKind::Message { role: ct_domain::MessageRole::User, preview: String::new(), char_len: 10 }),
            event(
                2,
                EventKind::ToolResult {
                    tool: Some("Bash(npm test)".into()),
                    call_id: None,
                    char_len: 180_000,
                    is_error: false,
                },
            ),
        ];
        let turns = vec![turn(1, 5_000, vec![0]), turn(2, 52_291, vec![1])];
        let d = diagnose(&session(events, turns, vec![]));

        assert_eq!(d.spikes.len(), 1);
        let spike = &d.spikes[0];
        assert_eq!(spike.turn, 2);
        assert_eq!(spike.growth, 47_291);
        assert_eq!(spike.candidates, vec!["tool output: Bash(npm test)"]);
    }

    #[test]
    fn a_zero_token_baseline_does_not_manufacture_a_spike() {
        // Synthetic and errored turns record empty usage. Measuring growth from
        // one reports an entire context as a spike, which is noise dressed up
        // as a finding.
        let mut empty = turn(1, 0, vec![]);
        empty.usage = TokenUsage {
            input: Some(0),
            cache_read: Some(0),
            ..Default::default()
        };
        let turns = vec![empty, turn(2, 192_281, vec![])];
        let d = diagnose(&session(vec![], turns, vec![]));
        assert!(
            d.spikes.is_empty(),
            "growth measured from a zero baseline is not a real spike"
        );
    }

    #[test]
    fn spike_candidates_resolve_tool_ids_to_tool_names() {
        let events = vec![
            event(
                1,
                EventKind::ToolCall {
                    tool: "Bash".into(),
                    call_id: Some("toolu_017".into()),
                    char_len: 40,
                },
            ),
            event(
                2,
                EventKind::ToolResult {
                    tool: None,
                    call_id: Some("toolu_017".into()),
                    char_len: 150_000,
                    is_error: false,
                },
            ),
        ];
        let turns = vec![turn(1, 5_000, vec![0]), turn(2, 45_000, vec![1])];
        let d = diagnose(&session(events, turns, vec![]));

        assert_eq!(d.spikes.len(), 1);
        assert_eq!(d.spikes[0].candidates, vec!["tool output: Bash"]);
    }

    #[test]
    fn ordinary_growth_is_not_reported_as_a_spike() {
        let turns = vec![turn(1, 5_000, vec![]), turn(2, 6_200, vec![])];
        let d = diagnose(&session(vec![], turns, vec![]));
        assert!(d.spikes.is_empty());
    }

    #[test]
    fn a_drop_is_never_a_spike() {
        // Compaction shrinks the context; that is the opposite of a problem.
        let turns = vec![turn(1, 165_000, vec![]), turn(2, 17_000, vec![])];
        let d = diagnose(&session(vec![], turns, vec![]));
        assert!(d.spikes.is_empty());
    }

    #[test]
    fn fidelity_and_headline_reflect_unrecognised_events() {
        let events = (1..=10).map(|i| event(i, EventKind::SessionStarted)).collect();
        let d = diagnose(&session(events, vec![], vec![("new_type".into(), 2)]));

        assert_eq!(d.unrecognised_events, 2);
        assert!((d.fidelity - 0.8).abs() < 1e-6);
        assert!(!d.is_fully_understood());
        assert!(d.headline().contains("unrecognised"));
    }

    #[test]
    fn a_clean_parse_says_so() {
        let events = (1..=3).map(|i| event(i, EventKind::SessionStarted)).collect();
        let d = diagnose(&session(events, vec![], vec![]));
        assert!(d.is_fully_understood());
        assert_eq!(d.headline(), "3 events, all recognised");
    }

    #[test]
    fn compaction_reduction_sums_observed_before_and_after() {
        let events = vec![
            event(
                1,
                EventKind::Compacted(CompactionFacts {
                    tokens_before: Some(165_223),
                    tokens_after: Some(17_542),
                    ..Default::default()
                }),
            ),
            event(
                2,
                EventKind::Compacted(CompactionFacts {
                    tokens_before: Some(100_000),
                    tokens_after: Some(20_000),
                    ..Default::default()
                }),
            ),
        ];
        let d = diagnose(&session(events, vec![], vec![]));
        assert_eq!(d.compactions, 2);
        assert_eq!(d.compaction_reduction, Some(147_681 + 80_000));
    }

    #[test]
    fn turns_missing_usage_are_counted_rather_than_assumed_zero() {
        let mut t = turn(1, 0, vec![]);
        t.usage = TokenUsage::default();
        let d = diagnose(&session(vec![], vec![t], vec![]));
        assert_eq!(d.turns_without_usage, 1);
        assert_eq!(d.peak_prompt_tokens, None);
    }
}
