//! What the whole local corpus says, rather than one session.
//!
//! Every other use case here is scoped to a session, which answers "why did
//! *that* happen" and never "where did my month go". This one sweeps every
//! discovered session and reports the totals, the distributions and the
//! repeat offenders across all of them.
//!
//! # Why a full parse is affordable
//!
//! [`ContextTrace::sweep_drift`](crate::ContextTrace::sweep_drift) already
//! walks every binding's `discover()` and `load()`, and did 816 sessions and
//! 148,970 events in 5.88 seconds (`docs/MVP-STATUS.md`). This sweep is that
//! walk with more counted per session, so it is bounded by the same parse.
//!
//! What keeps it there is what it declines to do. No content analysis: no
//! payload is hashed or compressed. No reconstruction: no turn's context is
//! rebuilt, which is the expensive per-turn path the composition views run.
//! Everything below is read from turn usage records and event metadata that
//! parsing produced anyway.
//!
//! # What it will not claim
//!
//! Cost is summed from each session's own report, and the turns those reports
//! could not price are carried through as a count rather than dropped. A
//! corpus total that quietly omitted a fifth of the turns would be the most
//! confidently wrong number this tool could print.

use crate::cost;
use ct_domain::model::event::EventKind;
use ct_domain::ports::AgentAdapter;
use ct_domain::{AgentKind, AgentSession, SessionDescriptor};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How many entries each ranking keeps. A ranking is read to find the
/// offenders, not to enumerate the corpus.
const TOP_N: usize = 12;

/// Utilization bands a session's peak prompt falls into.
///
/// Bands rather than an average: an average utilization across a corpus is a
/// number no session ever had, while "nine sessions ran past 90% of their
/// window" is a fact about nine specific sessions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PressureBands {
    /// Sessions whose peak prompt stayed under half the window.
    pub comfortable: usize,
    /// Half to three quarters.
    pub warming: usize,
    /// Three quarters to ninety percent.
    pub tight: usize,
    /// Ninety percent and above.
    pub critical: usize,
    /// Sessions where either the peak or the window was never recorded, so no
    /// band applies. Counted rather than dropped: the share of the corpus this
    /// question cannot be asked of is part of the answer.
    pub unmeasured: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTotals {
    pub agent: String,
    pub sessions: usize,
    pub turns: usize,
    pub events: usize,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTotals {
    pub project: String,
    pub sessions: usize,
    pub turns: usize,
    pub output_tokens: u64,
    /// Micro-dollars, summed from each session's own priced turns.
    pub cost_micros: u64,
}

/// A tool's whole footprint across the corpus.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTotals {
    pub tool: String,
    pub calls: usize,
    pub errors: usize,
    /// Characters returned by this tool's results. Characters, not tokens: no
    /// estimator ran here, and calling this a token count would be inventing
    /// precision the sweep did not buy.
    pub result_chars: u64,
}

/// Sessions and turns on one calendar day, from turn timestamps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayTotals {
    /// `YYYY-MM-DD`, UTC.
    pub day: String,
    pub sessions: usize,
    pub turns: usize,
    pub output_tokens: u64,
}

/// A session worth looking at, and the figure that earned it the place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRank {
    pub id: String,
    pub agent: String,
    pub title: Option<String>,
    pub project: Option<String>,
    pub turns: usize,
    pub peak_prompt_tokens: Option<u32>,
    pub output_tokens: u64,
    pub cost_micros: u64,
}

/// Everything the sweep found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusReport {
    pub sessions: usize,
    pub turns: usize,
    pub events: usize,
    pub output_tokens: u64,
    /// Sessions that could not be parsed at all. One bad file must not be able
    /// to hide the other eight hundred, and it must not vanish either.
    pub unreadable: usize,
    /// Events this build did not recognise, corpus-wide -- the same early
    /// warning `ct doctor --dir` reports, in the view people will actually
    /// have open.
    pub unrecognised_events: u32,
    pub compactions: usize,
    pub sessions_with_compaction: usize,
    /// Compactions that recorded both a before and an after size.
    ///
    /// The denominator for [`CorpusReport::reclaimed_tokens`], and not a
    /// detail: every compaction in the local corpus is a Codex one, and Codex
    /// records the replacement history rather than the two token counts. So
    /// the reclaimed total is legitimately zero *because nothing measured it*,
    /// which reads identically to "compaction freed nothing" unless this
    /// number is beside it.
    pub compactions_measured: usize,
    /// Tokens compaction reclaimed, summed over the compactions that recorded
    /// both a before and an after.
    pub reclaimed_tokens: u64,
    pub tool_calls: usize,
    pub tool_errors: usize,
    pub cost_micros: u64,
    /// Turns no local rate could price. The denominator for every cost figure
    /// above, and the reason they are a floor rather than a total.
    pub unpriced_turns: usize,
    pub pressure: PressureBands,
    pub by_agent: Vec<AgentTotals>,
    pub by_project: Vec<ProjectTotals>,
    pub by_day: Vec<DayTotals>,
    pub by_tool: Vec<ToolTotals>,
    pub models: Vec<(String, usize)>,
    /// Sessions ranked by peak prompt: the ones that ran closest to their
    /// ceiling.
    pub largest_sessions: Vec<SessionRank>,
    /// Sessions ranked by cost.
    pub costliest_sessions: Vec<SessionRank>,
}

/// Accumulates one session at a time so the caller can report progress.
///
/// Separate from [`CorpusReport`] because the two have different shapes: this
/// is keyed maps built for insertion, that one is sorted, truncated vectors
/// built for reading. [`Corpus::finish`] is the one place the conversion
/// happens.
#[derive(Debug, Default)]
pub struct Corpus {
    report: CorpusReport,
    agents: BTreeMap<AgentKind, AgentTotals>,
    projects: BTreeMap<String, ProjectTotals>,
    days: BTreeMap<String, DayTotals>,
    tools: BTreeMap<String, ToolTotals>,
    models: BTreeMap<String, usize>,
    ranked: Vec<SessionRank>,
}

impl Corpus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a session that could not be parsed.
    pub fn add_unreadable(&mut self) {
        self.report.unreadable += 1;
    }

    /// Fold one parsed session into the totals.
    pub fn add(&mut self, descriptor: &SessionDescriptor, session: &AgentSession) {
        let agent = descriptor.agent;
        let turns = session.turn_count();
        let output = u64::from(session.total_output_tokens());

        self.report.sessions += 1;
        self.report.turns += turns;
        self.report.events += session.events().len();
        self.report.output_tokens += output;
        self.report.unrecognised_events = self
            .report
            .unrecognised_events
            .saturating_add(session.unrecognised_total());

        let agent_totals = self.agents.entry(agent).or_insert_with(|| AgentTotals {
            agent: agent.to_string(),
            ..AgentTotals::default()
        });
        agent_totals.sessions += 1;
        agent_totals.turns += turns;
        agent_totals.events += session.events().len();
        agent_totals.output_tokens += output;

        let report = cost::project(session);
        self.report.cost_micros += report.total.0;
        self.report.unpriced_turns += report.unpriced.len();

        let project = descriptor
            .project
            .clone()
            .unwrap_or_else(|| "unknown project".into());
        let project_totals =
            self.projects
                .entry(project.clone())
                .or_insert_with(|| ProjectTotals {
                    project,
                    ..ProjectTotals::default()
                });
        project_totals.sessions += 1;
        project_totals.turns += turns;
        project_totals.output_tokens += output;
        project_totals.cost_micros += report.total.0;

        self.add_activity(session);
        self.add_events(session);
        self.add_pressure(session);

        self.ranked.push(SessionRank {
            id: descriptor.id.to_string(),
            agent: agent.to_string(),
            title: descriptor.title.as_ref().map(|title| title.text.clone()),
            project: descriptor.project.clone(),
            turns,
            peak_prompt_tokens: session.peak_prompt_tokens(),
            output_tokens: output,
            cost_micros: report.total.0,
        });
    }

    /// Turns per day, from the timestamps the turns themselves carry.
    ///
    /// A session is counted on the day its first *timestamped* turn ran, not
    /// on its file's modification time: a session left open overnight would
    /// otherwise land on the day it was closed.
    fn add_activity(&mut self, session: &AgentSession) {
        let mut first_day: Option<String> = None;
        for turn in session.turns() {
            let Some(timestamp) = turn.timestamp else {
                continue;
            };
            let day = timestamp.format("%Y-%m-%d").to_string();
            let totals = self.days.entry(day.clone()).or_insert_with(|| DayTotals {
                day: day.clone(),
                ..DayTotals::default()
            });
            totals.turns += 1;
            totals.output_tokens += u64::from(turn.usage.output.unwrap_or(0));
            first_day.get_or_insert(day);
        }
        if let Some(totals) = first_day.and_then(|day| self.days.get_mut(&day)) {
            totals.sessions += 1;
        }
        for turn in session.turns() {
            if let Some(model) = &turn.model {
                *self.models.entry(model.clone()).or_default() += 1;
            }
        }
    }

    fn add_events(&mut self, session: &AgentSession) {
        // Result sizes are attributed to the tool that was *called*, which
        // means resolving a result back to its call: both agents put the tool
        // name on the call and only sometimes on the result.
        let mut called: BTreeMap<&str, &str> = BTreeMap::new();
        for event in session.events() {
            if let EventKind::ToolCall {
                tool,
                call_id: Some(id),
                ..
            } = &event.kind
            {
                called.insert(id.as_str(), tool.as_str());
            }
        }

        for event in session.events() {
            match &event.kind {
                EventKind::ToolCall { tool, .. } => {
                    self.report.tool_calls += 1;
                    self.tools
                        .entry(tool.clone())
                        .or_insert_with(|| ToolTotals {
                            tool: tool.clone(),
                            ..ToolTotals::default()
                        })
                        .calls += 1;
                }
                EventKind::ToolResult {
                    tool,
                    call_id,
                    char_len,
                    is_error,
                } => {
                    let name = tool
                        .as_deref()
                        .or_else(|| call_id.as_deref().and_then(|id| called.get(id).copied()))
                        .unwrap_or("unattributed");
                    let totals = self
                        .tools
                        .entry(name.to_string())
                        .or_insert_with(|| ToolTotals {
                            tool: name.to_string(),
                            ..ToolTotals::default()
                        });
                    totals.result_chars += u64::from(*char_len);
                    if *is_error {
                        totals.errors += 1;
                        self.report.tool_errors += 1;
                    }
                }
                EventKind::Compacted(facts) => {
                    self.report.compactions += 1;
                    if let (Some(before), Some(after)) = (facts.tokens_before, facts.tokens_after) {
                        self.report.compactions_measured += 1;
                        self.report.reclaimed_tokens += u64::from(before.saturating_sub(after));
                    }
                }
                _ => {}
            }
        }
        if session
            .events()
            .iter()
            .any(|event| matches!(event.kind, EventKind::Compacted(_)))
        {
            self.report.sessions_with_compaction += 1;
        }
    }

    /// Which band this session's closest approach to its ceiling falls in.
    fn add_pressure(&mut self, session: &AgentSession) {
        let bands = &mut self.report.pressure;
        let Some(window) = session
            .metadata()
            .context_window
            .filter(|window| *window > 0)
        else {
            bands.unmeasured += 1;
            return;
        };
        let Some(peak) = session.peak_prompt_tokens() else {
            bands.unmeasured += 1;
            return;
        };
        let share = f64::from(peak) / f64::from(window);
        if share >= 0.9 {
            bands.critical += 1;
        } else if share >= 0.75 {
            bands.tight += 1;
        } else if share >= 0.5 {
            bands.warming += 1;
        } else {
            bands.comfortable += 1;
        }
    }

    /// Sort, truncate and hand back the finished report.
    pub fn finish(mut self) -> CorpusReport {
        self.report.by_agent = self.agents.into_values().collect();

        let mut projects: Vec<_> = self.projects.into_values().collect();
        projects.sort_by(|left, right| {
            right
                .turns
                .cmp(&left.turns)
                .then_with(|| left.project.cmp(&right.project))
        });
        projects.truncate(TOP_N);
        self.report.by_project = projects;

        // Chronological, not ranked: this one is read as a series.
        self.report.by_day = self.days.into_values().collect();

        let mut tools: Vec<_> = self.tools.into_values().collect();
        tools.sort_by(|left, right| {
            right
                .result_chars
                .cmp(&left.result_chars)
                .then_with(|| left.tool.cmp(&right.tool))
        });
        tools.truncate(TOP_N);
        self.report.by_tool = tools;

        let mut models: Vec<_> = self.models.into_iter().collect();
        models.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        self.report.models = models;

        let mut largest = self.ranked.clone();
        // Sessions with no measured peak sort last rather than as zero: an
        // unmeasured session is not a small one.
        largest.sort_by(|left, right| right.peak_prompt_tokens.cmp(&left.peak_prompt_tokens));
        largest.retain(|rank| rank.peak_prompt_tokens.is_some());
        largest.truncate(TOP_N);
        self.report.largest_sessions = largest;

        let mut costliest = self.ranked;
        costliest.sort_by(|left, right| right.cost_micros.cmp(&left.cost_micros));
        costliest.retain(|rank| rank.cost_micros > 0);
        costliest.truncate(TOP_N);
        self.report.costliest_sessions = costliest;

        self.report
    }
}

/// Sweep every session one adapter discovered, folding each into `corpus`.
///
/// `progress` is called after each session with the number completed, so a
/// caller can report a sweep that takes seconds rather than appearing to hang.
/// One unreadable session never stops it: finding out that four hundred
/// sessions are fine and one is not is the point of running it.
pub fn sweep_adapter(
    adapter: &dyn AgentAdapter,
    corpus: &mut Corpus,
    mut progress: impl FnMut(usize),
) {
    let Ok(descriptors) = adapter.discover() else {
        return;
    };
    for (done, descriptor) in descriptors.into_iter().enumerate() {
        match adapter.load(&descriptor) {
            Ok(session) => corpus.add(&descriptor, &session),
            Err(_) => corpus.add_unreadable(),
        }
        progress(done + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::model::event::{CompactionFacts, Event, EventLinks};
    use ct_domain::{
        EventId, FileId, SessionId, SessionMetadata, SessionTitle, SourceRef, ThreadRole,
        TokenUsage, Turn, TurnNumber,
    };

    fn descriptor(id: &str, project: &str) -> SessionDescriptor {
        SessionDescriptor {
            id: SessionId::new(id).unwrap(),
            agent: AgentKind::Codex,
            path: format!("{id}.jsonl"),
            size_bytes: 10,
            project: Some(project.into()),
            title: Some(SessionTitle::agent_generated("Named")),
            git_branch: None,
            started_at: None,
            last_activity: None,
            thread_role: ThreadRole::Root,
        }
    }

    fn event(sequence: u32, kind: EventKind) -> Event {
        Event {
            id: EventId::Ordinal(sequence),
            sequence,
            timestamp: None,
            kind,
            source: SourceRef::new(FileId(0), 0, 0, sequence + 1),
            raw_type: "test".into(),
            turn: None,
            links: EventLinks::default(),
            content_measurement: None,
        }
    }

    fn turn(number: u32, input: Option<u32>, output: Option<u32>) -> Turn {
        Turn {
            number: TurnNumber::new(number).unwrap(),
            timestamp: Some(
                chrono::DateTime::parse_from_rfc3339("2026-08-17T10:00:00Z")
                    .unwrap()
                    .into(),
            ),
            model: Some("gpt-5.4".into()),
            usage: TokenUsage {
                input,
                output,
                ..Default::default()
            },
            event_indices: Vec::new(),
            anchor_index: None,
        }
    }

    fn session(window: Option<u32>, turns: Vec<Turn>, events: Vec<Event>) -> AgentSession {
        AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::Codex,
            SessionMetadata {
                context_window: window,
                ..SessionMetadata::default()
            },
            events,
            turns,
            Vec::new(),
        )
    }

    #[test]
    fn totals_and_rankings_come_from_turn_usage_and_event_metadata() {
        let mut corpus = Corpus::new();
        corpus.add(
            &descriptor("a", "C:\\work\\alpha"),
            &session(
                Some(100_000),
                vec![
                    turn(1, Some(10_000), Some(500)),
                    turn(2, Some(95_000), Some(700)),
                ],
                vec![
                    event(
                        0,
                        EventKind::ToolCall {
                            tool: "Read".into(),
                            call_id: Some("c1".into()),
                            char_len: 20,
                            target: None,
                        },
                    ),
                    // No tool name on the result: it must still be attributed
                    // to Read, or the biggest consumer in a corpus reads as
                    // "unattributed".
                    event(
                        1,
                        EventKind::ToolResult {
                            tool: None,
                            call_id: Some("c1".into()),
                            char_len: 38_000,
                            is_error: true,
                        },
                    ),
                    event(
                        2,
                        EventKind::Compacted(CompactionFacts {
                            tokens_before: Some(90_000),
                            tokens_after: Some(20_000),
                            ..CompactionFacts::default()
                        }),
                    ),
                ],
            ),
        );
        corpus.add_unreadable();

        let report = corpus.finish();

        assert_eq!(report.sessions, 1);
        assert_eq!(report.turns, 2);
        assert_eq!(report.output_tokens, 1_200);
        assert_eq!(report.unreadable, 1);
        assert_eq!(report.compactions, 1);
        assert_eq!(report.sessions_with_compaction, 1);
        assert_eq!(report.compactions_measured, 1);
        assert_eq!(report.reclaimed_tokens, 70_000);
        assert_eq!(report.tool_calls, 1);
        assert_eq!(report.tool_errors, 1);
        assert_eq!(
            report.by_tool,
            vec![ToolTotals {
                tool: "Read".into(),
                calls: 1,
                errors: 1,
                result_chars: 38_000,
            }],
            "a result with no tool name is attributed through its call id"
        );
        assert_eq!(report.by_agent[0].sessions, 1);
        assert_eq!(report.by_project[0].project, "C:\\work\\alpha");
        assert_eq!(report.by_day[0].day, "2026-08-17");
        assert_eq!(report.by_day[0].sessions, 1);
        assert_eq!(report.by_day[0].turns, 2);
        assert_eq!(report.models, vec![("gpt-5.4".into(), 2)]);
        assert_eq!(
            report.largest_sessions[0].peak_prompt_tokens,
            Some(95_000),
            "the ranking is by the largest prompt the session ever sent"
        );
        assert_eq!(report.largest_sessions[0].title.as_deref(), Some("Named"));
    }

    #[test]
    fn a_compaction_that_recorded_no_sizes_is_counted_but_never_measured() {
        // Every compaction in the local corpus is a Codex one, and Codex
        // records the replacement history rather than a before and an after.
        // Reporting `reclaimed 0` alone would read as "compaction freed
        // nothing" when the truth is that nothing measured it.
        let mut corpus = Corpus::new();
        corpus.add(
            &descriptor("a", "p"),
            &session(
                None,
                Vec::new(),
                vec![event(0, EventKind::Compacted(CompactionFacts::default()))],
            ),
        );

        let report = corpus.finish();

        assert_eq!(report.compactions, 1);
        assert_eq!(report.compactions_measured, 0);
        assert_eq!(report.reclaimed_tokens, 0);
    }

    #[test]
    fn pressure_bands_count_sessions_they_cannot_measure_rather_than_guessing() {
        let mut corpus = Corpus::new();
        // 95% of its window.
        corpus.add(
            &descriptor("tight", "p"),
            &session(
                Some(100_000),
                vec![turn(1, Some(95_000), Some(1))],
                Vec::new(),
            ),
        );
        // A window, but no turn ever reported a prompt size.
        corpus.add(
            &descriptor("unsized", "p"),
            &session(Some(100_000), vec![turn(1, None, Some(1))], Vec::new()),
        );
        // A measured prompt, but the agent never recorded its window.
        corpus.add(
            &descriptor("windowless", "p"),
            &session(None, vec![turn(1, Some(50_000), Some(1))], Vec::new()),
        );

        let bands = corpus.finish().pressure;

        assert_eq!(bands.critical, 1);
        assert_eq!(
            bands.unmeasured, 2,
            "a session missing either half of the ratio is unmeasured, not comfortable"
        );
        assert_eq!(bands.comfortable + bands.warming + bands.tight, 0);
    }

    #[test]
    fn an_unpriced_corpus_reports_the_turns_it_could_not_price() {
        // A model no bundled rate covers. The cost is zero and the sweep says
        // why, rather than presenting zero as a measurement.
        let mut unpriced = turn(1, Some(1_000), Some(100));
        unpriced.model = Some("a-model-nobody-prices".into());
        let mut corpus = Corpus::new();
        corpus.add(
            &descriptor("a", "p"),
            &session(None, vec![unpriced], Vec::new()),
        );

        let report = corpus.finish();

        assert_eq!(report.cost_micros, 0);
        assert_eq!(report.unpriced_turns, 1);
        assert!(
            report.costliest_sessions.is_empty(),
            "a session with no priced turn does not belong in a cost ranking"
        );
    }
    #[test]
    fn a_fingerprint_compares_equal_after_a_round_trip_through_json() {
        // The desktop decides whether a remembered sweep is still valid by
        // comparing a fingerprint it wrote to disk against one it just
        // computed. A timestamp that lost precision on the way out would make
        // that comparison fail forever, and the symptom would be a slow launch
        // rather than an error -- which nobody reads as a bug.
        let fingerprints = vec![ct_domain::SessionFingerprint {
            path: "/logs/a.jsonl".into(),
            size_bytes: 46_652,
            last_activity: Some(
                "2026-07-28T12:56:17.529662300Z"
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .expect("a fixed timestamp parses"),
            ),
        }];

        let json = serde_json::to_string(&fingerprints).expect("serialises");
        let read: Vec<ct_domain::SessionFingerprint> =
            serde_json::from_str(&json).expect("reads back");

        assert_eq!(read, fingerprints);
    }

    #[test]
    fn a_report_survives_a_round_trip_through_json() {
        // The desktop remembers a sweep on disk so a launch does not have to
        // repeat it. That only works while every part of the report can be
        // read back: a field added with `Serialize` alone would compile, ship,
        // and silently turn the cache into a permanent miss.
        let mut corpus = Corpus::new();
        corpus.add(
            &descriptor("a", "p"),
            &session(None, vec![turn(1, Some(1_000), Some(100))], Vec::new()),
        );
        let report = corpus.finish();

        let json = serde_json::to_string(&report).expect("a report serialises");
        let read: CorpusReport = serde_json::from_str(&json).expect("and reads back");

        assert_eq!(read, report);
    }
}
