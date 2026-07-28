//! Prompt size over the life of a session.
//!
//! The cheapest view in the tool, and deliberately so. Every figure here is the
//! agent's own `usage` record, read straight off the parsed turns -- no
//! reconstruction, no estimator, no calibration. `ct residual` reconstructs
//! every turn and costs seconds on a long session; this costs a pass over a
//! vector. Keeping them separate means the question "when did this session get
//! big" is answerable without paying for the question "what was in it".
//!
//! # Three things a timeline must not do
//!
//! **A turn with no usage record is not a turn of size zero.** Both agents omit
//! usage on some turns. Plotting those as a floor draws a sawtooth that never
//! happened, and -- worse -- makes the session look like it repeatedly shed
//! context it never shed. They are gaps, counted and named.
//!
//! **A compaction that the agent did not place is not a compaction that did not
//! happen.** Compaction events do not always carry a turn number. One that
//! cannot be positioned is reported as a count beside the chart rather than
//! dropped, because a timeline missing the event that explains its largest fall
//! is worse than one that admits it.
//!
//! **A column is not a turn.** Any session longer than the terminal is wide gets
//! bucketed, and a bucket showing its largest turn can hide the fall that
//! followed. That is the reason compactions are marked independently of the bars
//! rather than left to be inferred from a dip: the mark survives the bucketing
//! that would swallow the dip.

use ct_domain::model::event::EventKind;
use ct_domain::AgentSession;
use serde::Serialize;
use std::collections::BTreeMap;

/// A compaction the agent placed at a specific turn.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CompactionAt {
    /// The log line that recorded it -- the identity `ct trace` also uses,
    /// because a turn number is not always present and never unique.
    pub line_no: u32,
    pub trigger: Option<String>,
    /// Tokens the agent said this reclaimed, where it reported both sides.
    pub reclaimed: Option<u32>,
}

/// One turn's place on the timeline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GrowthPoint {
    pub turn: u32,
    /// The agent's own prompt size. `None` where it recorded no usage, which is
    /// a gap in the record and never a zero.
    pub prompt_tokens: Option<u32>,
    pub compaction: Option<CompactionAt>,
}

/// A run of consecutive turns collapsed into one column.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Bucket {
    pub from_turn: u32,
    pub to_turn: u32,
    /// The largest measured prompt in the run, or `None` if none was measured.
    ///
    /// The largest rather than the mean, because this view exists to find
    /// spikes and a mean hides them. The cost is that a fall inside the bucket
    /// is invisible, which is what [`Bucket::compactions`] is for.
    pub peak: Option<u32>,
    pub compactions: usize,
}

/// A change in prompt size between two consecutive measured turns.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Jump {
    /// The later of the two turns -- where the new size was first seen.
    pub turn: u32,
    pub from: u32,
    pub to: u32,
    /// Turns between the two whose size the agent did not record.
    ///
    /// Non-zero means the change happened *somewhere* across those turns, not
    /// necessarily at this one. Reported so a caller can say which it has.
    pub skipped: u32,
}

impl Jump {
    pub fn growth(&self) -> i64 {
        self.to as i64 - self.from as i64
    }
}

/// A session's prompt size, turn by turn.
#[derive(Debug, Clone, Serialize)]
pub struct GrowthTimeline {
    pub points: Vec<GrowthPoint>,
    /// Compactions the agent recorded without a turn we could place them at.
    /// Counted rather than discarded -- see the module docs.
    pub unplaced_compactions: usize,
    pub context_window: Option<u32>,
}

impl GrowthTimeline {
    /// Turns whose size the agent actually recorded.
    pub fn measured(&self) -> usize {
        self.points.iter().filter(|p| p.prompt_tokens.is_some()).count()
    }

    /// Turns present in the session but absent from every figure here.
    pub fn gaps(&self) -> usize {
        self.points.len() - self.measured()
    }

    /// The high-water mark and where it was reached.
    pub fn peak(&self) -> Option<(u32, u32)> {
        self.points
            .iter()
            .filter_map(|p| p.prompt_tokens.map(|t| (p.turn, t)))
            .max_by_key(|(_, tokens)| *tokens)
    }

    /// Share of the model's context window at the peak.
    pub fn peak_utilisation(&self) -> Option<f32> {
        let window = self.context_window.filter(|w| *w > 0)?;
        let (_, peak) = self.peak()?;
        Some(peak as f32 / window as f32)
    }

    pub fn compactions(&self) -> usize {
        self.points.iter().filter(|p| p.compaction.is_some()).count()
    }

    /// Collapse the timeline to at most `width` columns.
    ///
    /// Buckets are equal runs of *turns*, not of measured turns, so a stretch
    /// the agent did not record occupies the width it really takes rather than
    /// being closed up -- closing it up would draw a continuous line across a
    /// hole in the record.
    pub fn buckets(&self, width: usize) -> Vec<Bucket> {
        if self.points.is_empty() || width == 0 {
            return Vec::new();
        }
        self.points
            .chunks(self.turns_per_column(width))
            .map(|chunk| Bucket {
                from_turn: chunk[0].turn,
                to_turn: chunk[chunk.len() - 1].turn,
                peak: chunk.iter().filter_map(|p| p.prompt_tokens).max(),
                compactions: chunk.iter().filter(|p| p.compaction.is_some()).count(),
            })
            .collect()
    }

    /// How many turns one column stands for at this width. `1` means the chart
    /// is per-turn and nothing was collapsed.
    pub fn turns_per_column(&self, width: usize) -> usize {
        if width == 0 {
            return 1;
        }
        self.points.len().div_ceil(width).max(1)
    }

    /// The `limit` largest changes in prompt size, biggest first.
    ///
    /// Computed between consecutive *measured* turns. A gap between them is
    /// carried on the [`Jump`] rather than smoothed over: the change is real,
    /// but which turn caused it is not established when the record skips turns.
    pub fn largest_jumps(&self, limit: usize) -> Vec<Jump> {
        let mut jumps: Vec<Jump> = Vec::new();
        let mut previous: Option<(u32, u32)> = None;

        for point in &self.points {
            let Some(tokens) = point.prompt_tokens else {
                continue;
            };
            if let Some((prev_turn, prev_tokens)) = previous {
                jumps.push(Jump {
                    turn: point.turn,
                    from: prev_tokens,
                    to: tokens,
                    skipped: point.turn.saturating_sub(prev_turn).saturating_sub(1),
                });
            }
            previous = Some((point.turn, tokens));
        }

        jumps.sort_by_key(|j| std::cmp::Reverse(j.growth().abs()));
        jumps.truncate(limit);
        jumps
    }

    /// Narrow to a turn range, keeping every other figure consistent with what
    /// is shown.
    pub fn range(&self, from: Option<u32>, to: Option<u32>) -> GrowthTimeline {
        GrowthTimeline {
            points: self
                .points
                .iter()
                .filter(|p| from.is_none_or(|f| p.turn >= f))
                .filter(|p| to.is_none_or(|t| p.turn <= t))
                .cloned()
                .collect(),
            // Deliberately carried through unchanged. An unplaced compaction has
            // no turn, so it cannot be known to fall outside the range, and
            // dropping it would let a narrower view look more complete than the
            // whole.
            unplaced_compactions: self.unplaced_compactions,
            context_window: self.context_window,
        }
    }
}

/// Read a session's prompt size, turn by turn.
pub fn timeline(session: &AgentSession) -> GrowthTimeline {
    // Keyed by turn so a compaction lands on its own turn rather than on
    // whichever one happens to be adjacent in the event list.
    let mut placed: BTreeMap<u32, CompactionAt> = BTreeMap::new();
    let mut unplaced = 0usize;

    for (_, event) in session.compactions() {
        let EventKind::Compacted(facts) = &event.kind else {
            continue;
        };
        let mark = CompactionAt {
            line_no: event.source.line_no,
            trigger: facts.trigger.clone(),
            reclaimed: match (facts.tokens_before, facts.tokens_after) {
                (Some(before), Some(after)) => Some(before.saturating_sub(after)),
                _ => None,
            },
        };
        match event.turn {
            Some(turn) => {
                placed.insert(turn.get(), mark);
            }
            None => unplaced += 1,
        }
    }

    let points: Vec<GrowthPoint> = session
        .turns()
        .iter()
        .map(|turn| {
            let number = turn.number.get();
            GrowthPoint {
                turn: number,
                prompt_tokens: turn.prompt_tokens(),
                compaction: placed.remove(&number),
            }
        })
        .collect();

    GrowthTimeline {
        points,
        // Whatever is left named a turn the session does not have. It happened;
        // we simply cannot say where, which is the same situation as one that
        // named no turn at all.
        unplaced_compactions: unplaced + placed.len(),
        context_window: session.metadata().context_window,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::model::event::{CompactionFacts, EventLinks};
    use ct_domain::{
        AgentKind, Event, EventId, FileId, SessionId, SessionMetadata, SourceRef, TokenUsage, Turn,
        TurnNumber,
    };

    fn compaction_event(line_no: u32, turn: Option<u32>, before: u32, after: u32) -> Event {
        Event {
            id: EventId::Ordinal(line_no),
            sequence: line_no,
            timestamp: None,
            kind: EventKind::Compacted(CompactionFacts {
                trigger: Some("auto".into()),
                tokens_before: Some(before),
                tokens_after: Some(after),
                ..Default::default()
            }),
            source: SourceRef::new(FileId(0), 0, 0, line_no),
            raw_type: "compacted".into(),
            turn: turn.map(|t| TurnNumber::new(t).unwrap()),
            links: EventLinks::default(),
            content_measurement: None,
        }
    }

    /// `None` in `prompts` is a turn the agent recorded no usage for.
    fn session(prompts: &[Option<u32>], events: Vec<Event>, window: Option<u32>) -> AgentSession {
        let turns = prompts
            .iter()
            .enumerate()
            .map(|(i, p)| Turn {
                number: TurnNumber::new(i as u32 + 1).unwrap(),
                timestamp: None,
                model: None,
                usage: TokenUsage {
                    input: *p,
                    ..Default::default()
                },
                event_indices: vec![],
                anchor_index: None,
            })
            .collect();

        AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::ClaudeCode,
            SessionMetadata {
                context_window: window,
                ..Default::default()
            },
            events,
            turns,
            vec![],
        )
    }

    #[test]
    fn a_turn_with_no_usage_record_is_a_gap_not_a_zero() {
        // Plotting these as a floor draws a sawtooth that never happened, and
        // makes the session look like it repeatedly shed context it never shed.
        let t = timeline(&session(&[Some(1_000), None, Some(3_000)], vec![], None));
        assert_eq!(t.points[1].prompt_tokens, None);
        assert_eq!(t.measured(), 2);
        assert_eq!(t.gaps(), 1);
        assert_eq!(t.peak(), Some((3, 3_000)));
    }

    #[test]
    fn a_bucket_with_nothing_measured_reports_nothing_rather_than_zero() {
        let t = timeline(&session(&[None, None, Some(500), Some(900)], vec![], None));
        let buckets = t.buckets(2);
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].peak, None, "no measurement is not a measurement of none");
        assert_eq!(buckets[1].peak, Some(900));
    }

    #[test]
    fn a_compaction_the_agent_did_not_place_is_counted_not_dropped() {
        // A timeline missing the event that explains its largest fall is worse
        // than one that admits it cannot position it.
        let t = timeline(&session(
            &[Some(100_000), Some(20_000)],
            vec![compaction_event(7, None, 100_000, 20_000)],
            None,
        ));
        assert_eq!(t.compactions(), 0, "it could not be placed on a turn");
        assert_eq!(t.unplaced_compactions, 1);
    }

    #[test]
    fn a_compaction_naming_a_turn_the_session_lacks_is_also_unplaced() {
        // Same situation as naming no turn at all: it happened, and we cannot
        // say where. Silently indexing it to the nearest turn would put a
        // definite mark on a guess.
        let t = timeline(&session(
            &[Some(100_000), Some(20_000)],
            vec![compaction_event(7, Some(99), 100_000, 20_000)],
            None,
        ));
        assert_eq!(t.compactions(), 0);
        assert_eq!(t.unplaced_compactions, 1);
    }

    #[test]
    fn a_placed_compaction_lands_on_its_own_turn_with_what_it_reclaimed() {
        let t = timeline(&session(
            &[Some(100_000), Some(20_000), Some(30_000)],
            vec![compaction_event(41, Some(2), 100_000, 20_000)],
            Some(200_000),
        ));
        assert_eq!(t.unplaced_compactions, 0);
        let mark = t.points[1].compaction.as_ref().expect("turn 2 holds it");
        assert_eq!(mark.reclaimed, Some(80_000));
        assert_eq!(mark.line_no, 41);
        assert_eq!(t.peak_utilisation(), Some(0.5));
    }

    #[test]
    fn a_compaction_survives_the_bucketing_that_swallows_its_dip() {
        // The reason compactions are marked independently of the bars: a bucket
        // shows its largest turn, so the fall after a compaction can vanish
        // inside it. Here the whole session collapses to one column whose peak
        // is the pre-compaction size -- and the mark is the only evidence left.
        let t = timeline(&session(
            &[Some(180_000), Some(20_000), Some(25_000), Some(30_000)],
            vec![compaction_event(9, Some(2), 180_000, 20_000)],
            None,
        ));
        let buckets = t.buckets(1);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].peak, Some(180_000), "the dip is hidden by design");
        assert_eq!(buckets[0].compactions, 1, "and the mark is what carries it");
    }

    #[test]
    fn buckets_cover_every_turn_exactly_once() {
        let prompts: Vec<Option<u32>> = (1..=100).map(|i| Some(i * 100)).collect();
        let t = timeline(&session(&prompts, vec![], None));
        let buckets = t.buckets(30);

        assert!(buckets.len() <= 30, "asked for 30 columns, got {}", buckets.len());
        assert_eq!(buckets[0].from_turn, 1);
        assert_eq!(buckets[buckets.len() - 1].to_turn, 100);
        for pair in buckets.windows(2) {
            assert_eq!(
                pair[1].from_turn,
                pair[0].to_turn + 1,
                "columns must neither overlap nor skip turns"
            );
        }
    }

    #[test]
    fn a_session_shorter_than_the_chart_is_drawn_turn_by_turn() {
        let t = timeline(&session(&[Some(1), Some(2), Some(3)], vec![], None));
        assert_eq!(t.turns_per_column(60), 1);
        assert_eq!(t.buckets(60).len(), 3);
    }

    #[test]
    fn jumps_are_ranked_by_size_and_disclose_a_skipped_turn() {
        // Turn 3 has no record, so the change seen at turn 4 happened somewhere
        // across two turns. Reporting it as turn 4's jump without saying so
        // would name a cause the record does not establish.
        let t = timeline(&session(
            &[Some(10_000), Some(12_000), None, Some(60_000)],
            vec![],
            None,
        ));
        let jumps = t.largest_jumps(5);
        assert_eq!(jumps.len(), 2);
        assert_eq!(jumps[0].turn, 4);
        assert_eq!(jumps[0].growth(), 48_000);
        assert_eq!(jumps[0].skipped, 1);
        assert_eq!(jumps[1].skipped, 0);
    }

    #[test]
    fn a_fall_is_as_much_a_jump_as_a_rise() {
        // Ranked on magnitude: the largest single change in a compacted session
        // is the compaction, and a view of "what changed" that omitted it would
        // be answering a different question.
        let t = timeline(&session(&[Some(150_000), Some(20_000), Some(24_000)], vec![], None));
        let jumps = t.largest_jumps(1);
        assert_eq!(jumps[0].growth(), -130_000);
    }

    #[test]
    fn narrowing_the_range_keeps_the_unplaced_count() {
        // An unplaced compaction has no turn, so it cannot be known to fall
        // outside the range. Dropping it would let a narrower view look more
        // complete than the whole.
        let t = timeline(&session(
            &[Some(10), Some(20), Some(30), Some(40)],
            vec![compaction_event(3, None, 100, 50)],
            None,
        ));
        let narrowed = t.range(Some(2), Some(3));
        assert_eq!(narrowed.points.len(), 2);
        assert_eq!(narrowed.unplaced_compactions, 1);
        assert_eq!(narrowed.peak(), Some((3, 30)));
    }

    #[test]
    fn an_empty_session_yields_an_empty_chart_rather_than_a_panic() {
        let t = timeline(&session(&[], vec![], None));
        assert!(t.buckets(40).is_empty());
        assert!(t.largest_jumps(5).is_empty());
        assert_eq!(t.peak(), None);
        assert_eq!(t.peak_utilisation(), None);
    }
}
