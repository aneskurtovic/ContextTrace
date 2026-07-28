//! The session aggregate root.

use super::event::{Event, EventKind};
use super::identity::{SessionId, TurnNumber};
use super::provenance::SourceRef;
use super::tokens::TokenUsage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
}

impl AgentKind {
    pub fn label(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude-code",
            AgentKind::Codex => "codex",
        }
    }

    pub fn parse(s: &str) -> Option<AgentKind> {
        match s.to_ascii_lowercase().replace('_', "-").as_str() {
            "claude-code" | "claude" | "cc" => Some(AgentKind::ClaudeCode),
            "codex" | "openai-codex" => Some(AgentKind::Codex),
            _ => None,
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Everything known about a session without parsing its body.
///
/// Discovery produces these cheaply, from filenames and directory structure, so
/// `ct sessions` can list hundreds of sessions without reading hundreds of
/// megabytes. Fields fill in only if the adapter can get them cheaply; the
/// session browser tolerates every one of them being absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionDescriptor {
    pub id: SessionId,
    pub agent: AgentKind,
    /// Absolute path of the session file.
    pub path: String,
    /// Size on disk, used to warn before parsing something enormous.
    pub size_bytes: u64,
    /// Project or working directory the session ran in.
    pub project: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub last_activity: Option<DateTime<Utc>>,
}

/// Metadata extracted from a parsed session body.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub project: Option<String>,
    pub working_directory: Option<String>,
    pub model: Option<String>,
    pub agent_version: Option<String>,
    pub git_branch: Option<String>,
    pub git_commit: Option<String>,
    pub repository_url: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub last_activity: Option<DateTime<Utc>>,
    pub context_window: Option<u32>,
    /// The agent's base/system prompt, when it records it verbatim.
    /// Codex does; Claude Code does not.
    pub base_instructions: Option<SourceRef>,
}

/// One model request, and everything the agent recorded about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub number: TurnNumber,
    pub timestamp: Option<DateTime<Utc>>,
    pub model: Option<String>,
    pub usage: TokenUsage,
    /// Indices into [`AgentSession::events`] belonging to this turn.
    ///
    /// Indices rather than clones: a turn can span hundreds of events, and a
    /// session tens of thousands.
    pub event_indices: Vec<usize>,
    /// The event that carried this turn's usage report -- the anchor from which
    /// context reconstruction starts.
    pub anchor_index: Option<usize>,
}

impl Turn {
    /// Exact prompt size for this turn, when the agent reported it.
    pub fn prompt_tokens(&self) -> Option<u32> {
        self.usage.prompt_tokens()
    }
}

/// **The aggregate root.** A parsed session: its events, its turns, its
/// metadata.
///
/// All access to turns and events goes through here, so callers cannot
/// assemble their own inconsistent view of a session. Reconstruction asks the
/// session for a turn; it never hand-builds one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSession {
    id: SessionId,
    agent: AgentKind,
    metadata: SessionMetadata,
    events: Vec<Event>,
    turns: Vec<Turn>,
    /// Raw agent event-type strings we did not recognise, with counts.
    ///
    /// Surfaced rather than swallowed: this is the early-warning signal that an
    /// agent changed its log format, and it is what `ct doctor` reports.
    unrecognised: Vec<(String, u32)>,
}

impl AgentSession {
    pub fn new(
        id: SessionId,
        agent: AgentKind,
        metadata: SessionMetadata,
        events: Vec<Event>,
        turns: Vec<Turn>,
        unrecognised: Vec<(String, u32)>,
    ) -> Self {
        Self {
            id,
            agent,
            metadata,
            events,
            turns,
            unrecognised,
        }
    }

    pub fn id(&self) -> &SessionId {
        &self.id
    }
    pub fn agent(&self) -> AgentKind {
        self.agent
    }
    pub fn metadata(&self) -> &SessionMetadata {
        &self.metadata
    }
    pub fn events(&self) -> &[Event] {
        &self.events
    }
    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }
    pub fn unrecognised(&self) -> &[(String, u32)] {
        &self.unrecognised
    }

    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }

    pub fn turn(&self, number: TurnNumber) -> Option<&Turn> {
        self.turns.iter().find(|t| t.number == number)
    }

    pub fn event(&self, index: usize) -> Option<&Event> {
        self.events.get(index)
    }

    /// Look up an event by its graph uuid. Used when walking Claude Code's
    /// parent chains.
    pub fn event_by_uuid(&self, uuid: &str) -> Option<(usize, &Event)> {
        self.events
            .iter()
            .enumerate()
            .find(|(_, e)| e.links.uuid.as_deref() == Some(uuid))
    }

    /// Total tokens billed across the session, summing per-turn output and
    /// fresh input. Cache reads are excluded because they are re-presentations
    /// of context already counted, not new consumption.
    pub fn total_output_tokens(&self) -> u32 {
        self.turns
            .iter()
            .filter_map(|t| t.usage.output)
            .fold(0u32, |a, b| a.saturating_add(b))
    }

    /// Largest prompt the session ever sent -- its high-water context mark.
    pub fn peak_prompt_tokens(&self) -> Option<u32> {
        self.turns.iter().filter_map(|t| t.prompt_tokens()).max()
    }

    /// The turn that sent that prompt.
    ///
    /// `None` when *no* turn reported a usable size, which is a different
    /// statement from "this session has no turns" and must stay that way. The
    /// filter is the whole point: ranking on `prompt_tokens().unwrap_or(0)`
    /// gives every unmeasured turn the same key, and `max_by_key` then returns
    /// whichever happened to come last -- a turn nobody chose, handed back as
    /// the largest. Callers that want a starting point regardless are free to
    /// pick one, but they have to do it in the open.
    ///
    /// Ties keep the earliest turn, so a session that plateaus at its peak
    /// reports where the plateau began rather than where it ended.
    pub fn peak_turn(&self) -> Option<TurnNumber> {
        self.turns
            .iter()
            .filter_map(|t| Some((t.prompt_tokens()?, t.number)))
            .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
            .map(|(_, number)| number)
    }

    /// Number of events this version of ContextTrace could not classify.
    pub fn unrecognised_total(&self) -> u32 {
        self.unrecognised
            .iter()
            .map(|(_, count)| *count)
            .fold(0u32, |a, b| a.saturating_add(b))
    }

    /// Fraction of events successfully mapped to domain concepts, 0.0 to 1.0.
    ///
    /// The early-warning signal for upstream format changes. A score of 1.0
    /// means every line was understood; a drop means the agent started emitting
    /// something new, and context reconstruction may be incomplete. Reporting
    /// this is what turns "the agent changed its log format" from a silent
    /// wrong answer into a visible number.
    ///
    /// An empty session scores 1.0: nothing was misunderstood.
    pub fn fidelity(&self) -> f32 {
        let total = self.events.len();
        if total == 0 {
            return 1.0;
        }
        let unrecognised = self.unrecognised_total().min(total as u32);
        1.0 - (unrecognised as f32 / total as f32)
    }

    /// Every compaction in the session, in order.
    pub fn compactions(&self) -> Vec<(usize, &Event)> {
        self.events
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e.kind, EventKind::Compacted(_)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_kind_parses_common_spellings() {
        assert_eq!(AgentKind::parse("claude"), Some(AgentKind::ClaudeCode));
        assert_eq!(AgentKind::parse("Claude_Code"), Some(AgentKind::ClaudeCode));
        assert_eq!(AgentKind::parse("codex"), Some(AgentKind::Codex));
        assert_eq!(AgentKind::parse("cursor"), None);
    }

    fn session_with_turns(prompts: &[u32]) -> AgentSession {
        let turns = prompts
            .iter()
            .enumerate()
            .map(|(i, p)| Turn {
                number: TurnNumber::new(i as u32 + 1).unwrap(),
                timestamp: None,
                model: None,
                usage: TokenUsage {
                    input: Some(*p),
                    output: Some(10),
                    ..Default::default()
                },
                event_indices: vec![],
                anchor_index: None,
            })
            .collect();
        AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::Codex,
            SessionMetadata::default(),
            vec![],
            turns,
            vec![],
        )
    }

    #[test]
    fn peak_prompt_is_the_high_water_mark() {
        let s = session_with_turns(&[100, 90_000, 4_000]);
        assert_eq!(s.peak_prompt_tokens(), Some(90_000));
        assert_eq!(s.turn_count(), 3);
    }

    #[test]
    fn turns_are_addressed_by_one_based_number() {
        let s = session_with_turns(&[10, 20]);
        assert_eq!(s.turn(TurnNumber::FIRST).unwrap().prompt_tokens(), Some(10));
        assert!(s.turn(TurnNumber::new(9).unwrap()).is_none());
    }

    #[test]
    fn fidelity_reports_the_share_of_understood_events() {
        let events = vec![];
        let mut s = AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::ClaudeCode,
            SessionMetadata::default(),
            events,
            vec![],
            vec![],
        );
        assert_eq!(s.fidelity(), 1.0, "an empty session misunderstood nothing");

        // 10 events, 2 of which are unrecognised.
        let ten = (0..10)
            .map(|i| Event {
                id: crate::model::identity::EventId::Ordinal(i),
                sequence: i,
                timestamp: None,
                kind: EventKind::SessionStarted,
                source: crate::model::provenance::SourceRef::new(
                    crate::model::identity::FileId(0),
                    0,
                    0,
                    i + 1,
                ),
                raw_type: "x".into(),
                turn: None,
                links: Default::default(),
                content_fingerprint: None,
            })
            .collect();
        s = AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::ClaudeCode,
            SessionMetadata::default(),
            ten,
            vec![],
            vec![("brand_new_type".into(), 2)],
        );
        assert_eq!(s.unrecognised_total(), 2);
        assert!((s.fidelity() - 0.8).abs() < 1e-6, "got {}", s.fidelity());
    }

    #[test]
    fn empty_session_has_no_peak() {
        let s = session_with_turns(&[]);
        assert_eq!(s.peak_prompt_tokens(), None);
        assert_eq!(s.peak_turn(), None);
        assert_eq!(s.total_output_tokens(), 0);
    }

    #[test]
    fn peak_turn_is_the_turn_that_sent_the_peak_prompt() {
        let s = session_with_turns(&[100, 90_000, 4_000]);
        assert_eq!(s.peak_turn(), Some(TurnNumber::new(2).unwrap()));
    }

    #[test]
    fn a_session_nobody_measured_has_no_peak_turn_rather_than_its_last_one() {
        // Three real turns, none with a usable size. Ranking on
        // `prompt_tokens().unwrap_or(0)` gives all three the same key and hands
        // back turn 3 -- a turn nobody chose, described as the largest.
        let s = session_with_turns(&[0, 0, 0]);
        assert_eq!(s.turn_count(), 3, "the turns exist");
        assert_eq!(s.peak_prompt_tokens(), None);
        assert_eq!(
            s.peak_turn(),
            None,
            "no turn was measured, so no turn is the largest"
        );
    }

    #[test]
    fn a_plateau_reports_where_it_began() {
        // Ties keep the earliest turn: a session that sits at its ceiling for
        // a while should point at the turn that got there.
        let s = session_with_turns(&[500, 90_000, 90_000, 90_000]);
        assert_eq!(s.peak_turn(), Some(TurnNumber::new(2).unwrap()));
    }
}
