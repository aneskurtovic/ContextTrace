//! Item lifecycle: when one context item entered the prompt, and when it left.
//!
//! `ct context` sees a single turn, so it cannot answer "how long has this been
//! sitting there". This module answers it the only way that stays honest:
//! reconstruct every turn and record where the item actually appears. The
//! alternative -- inferring a lifespan from the turn an item's line was written
//! in -- assumes a session is a straight line, which neither supported agent's
//! log is.
//!
//! # Three things a lifecycle view must not say
//!
//! **Absent is not unknown.** A turn whose reconstruction fails tells us nothing
//! about the item. Counting it as absence invents a departure, and splits one
//! run of presence into two.
//!
//! **Gone is not evicted.** Claude Code's log is a DAG. A rewind puts later
//! turns on a different branch, and an item on the abandoned branch was never
//! evicted from anything -- it was simply never in those prompts.
//!
//! **A subagent's turns are not this thread's turns.** A subagent runs against
//! its own context window, so main-thread items are legitimately missing from
//! subagent turns. Treating that as absence makes every long-lived item appear
//! to flicker in and out.
//!
//! # Why presence is swept with a character probe
//!
//! Whether an item is in a turn's context is decided by reconstruction, not by
//! token counting: the estimator only sets `tokens`. So the sweep runs on
//! [`CharProbe`](crate::CharProbe), and the item's *size* is taken separately
//! from one properly calibrated snapshot -- which is also what makes the figure
//! agree with the row the user just read in `ct largest`.

use crate::CharProbe;
use crate::ContextTrace;
use ct_domain::{
    AgentKind, AgentSession, ContextCategory, ContextItemId, ContextSource,
};
use serde::Serialize;
use std::collections::BTreeMap;

/// The compaction in effect at a turn, identified by the log line that recorded
/// it.
///
/// Line number rather than turn number on purpose: a compaction event does not
/// always carry a turn, and comparing `Option<u32>` turns would silently fail to
/// notice a compaction whose turn the agent did not record. Two turns share a
/// compaction exactly when they name the same line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CompactionMark {
    pub line_no: u32,
    pub turn: Option<u32>,
    pub reclaimed: Option<u32>,
}

/// One turn as the sweep saw it.
#[derive(Debug, Clone, Serialize)]
pub struct TurnScan {
    pub turn: u32,
    /// True when this turn belongs to a subagent, which has its own context
    /// window and therefore its own population of items.
    pub sidechain: bool,
    /// The most recent compaction at or before this turn.
    pub compaction: Option<CompactionMark>,
    /// False when reconstruction failed. Presence here is unknown, not absent.
    pub readable: bool,
}

/// Everything the sweep learned about one item.
#[derive(Debug, Clone, Serialize)]
pub struct ItemRecord {
    pub id: ContextItemId,
    pub label: String,
    pub category: ContextCategory,
    pub source: ContextSource,
    /// The adapter's own note of the turn during which this item's line was
    /// written. Kept separate from the sweep's answer rather than merged with
    /// it: they answer different questions, and where they disagree that is
    /// worth showing, not reconciling.
    pub recorded_first_seen: Option<u32>,
    /// Turns whose reconstruction contained this item, ascending.
    pub present_in: Vec<u32>,
    /// Which thread this item lives on, taken from where it was first seen.
    pub sidechain: bool,
}

impl ItemRecord {
    pub fn present_at(&self, turn: u32) -> bool {
        self.present_in.binary_search(&turn).is_ok()
    }

    pub fn first_present(&self) -> Option<u32> {
        self.present_in.first().copied()
    }

    pub fn last_present(&self) -> Option<u32> {
        self.present_in.last().copied()
    }
}

/// A stretch of consecutive turns in which an item was continuously present.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TurnRun {
    pub from: u32,
    pub to: u32,
    /// Turns actually observed present. Equals `to - from + 1` unless turns on
    /// the other thread sit inside the range.
    pub turns: usize,
}

/// Why an item stopped being in the context.
///
/// A sum type rather than a sentence, for the same reason `TokenCount` is one:
/// these three are not degrees of the same claim. One is read from the log, one
/// is inferred from the shape of the DAG, and one is an admission.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Departure {
    /// A compaction between the last turn holding the item and the next one.
    /// The only departure either agent's log records as an event.
    Compaction {
        /// The compaction's own turn, where the agent recorded one.
        turn: Option<u32>,
        reclaimed: Option<u32>,
    },
    /// The next turn descends from a different branch of the DAG, so it never
    /// held this item. Claude Code only, and *not* an eviction: the item was not
    /// removed from a prompt, the conversation moved to a history that never
    /// contained it.
    BranchDiverged { turn: u32 },
    /// Present, then not, with nothing in the log accounting for it.
    ///
    /// For Codex this should be unreachable: the replay fold only clears the
    /// item list at a `compacted` event, so a departure without one is a defect
    /// in ContextTrace rather than a fact about the session.
    Unexplained { turn: u32 },
}

/// One item's history across a session.
#[derive(Debug, Clone, Serialize)]
pub struct ItemLifecycle {
    pub id: ContextItemId,
    pub label: String,
    pub category: ContextCategory,
    pub source: ContextSource,
    /// Stretches of continuous presence, earliest first. More than one means the
    /// item left and came back, which is worth seeing rather than smoothing.
    pub runs: Vec<TurnRun>,
    /// Set only when the item is gone *and* the reason is establishable.
    pub departure: Option<Departure>,
    /// True when the item was still in context at the last turn on its thread.
    pub still_present: bool,
    /// Turns on this item's thread whose reconstruction failed. Presence there
    /// is unknown; it is neither counted as present nor read as a gap.
    pub unknown_turns: Vec<u32>,
    /// Turns scanned on this item's thread.
    pub scanned_turns: usize,
    /// Turns belonging to the other thread, excluded from the analysis.
    pub other_thread_turns: usize,
    /// Last turn scanned on this item's thread.
    pub last_scanned_turn: Option<u32>,
    pub recorded_first_seen: Option<u32>,
}

impl ItemLifecycle {
    pub fn first_present(&self) -> Option<u32> {
        self.runs.first().map(|r| r.from)
    }

    pub fn last_present(&self) -> Option<u32> {
        self.runs.last().map(|r| r.to)
    }

    pub fn turns_present(&self) -> usize {
        self.runs.iter().map(|r| r.turns).sum()
    }

    /// True when the adapter's recorded first turn disagrees with the turn the
    /// item is first *observed* in a prompt.
    ///
    /// The sweep is authoritative for this view -- it reads reconstructions,
    /// while `first_seen_turn` records when a line was written. They can differ
    /// legitimately, and a view that quietly picked one would be hiding the fact
    /// that they answer different questions.
    pub fn first_seen_disagrees(&self) -> bool {
        matches!(
            (self.recorded_first_seen, self.first_present()),
            (Some(recorded), Some(observed)) if recorded != observed
        )
    }
}

/// A candidate when a reference matches more than one item.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub id: ContextItemId,
    pub label: String,
    pub first_present: Option<u32>,
    pub last_present: Option<u32>,
    pub turns_present: usize,
}

#[derive(Debug, Clone)]
pub enum ResolveError {
    NotFound(String),
    /// Labels are not unique -- two `Read BACKLOG.md` calls are two items -- so
    /// this is an ordinary outcome, and the candidates carry the ids needed to
    /// disambiguate.
    Ambiguous {
        needle: String,
        candidates: Vec<Candidate>,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound(needle) => write!(
                f,
                "no context item in this session matches '{needle}'. \
                 Item references are an id like `claude:4821` or any part of a label; \
                 `ct largest <id>` lists both."
            ),
            ResolveError::Ambiguous { needle, candidates } => write!(
                f,
                "'{needle}' matches {} items. Re-run with one of their ids: {}",
                candidates.len(),
                candidates
                    .iter()
                    .take(8)
                    .map(|c| c.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Presence of every context item at every turn, from one pass over the
/// session.
///
/// One pass rather than one per item: resolving `--item BACKLOG.md` needs the
/// labels anyway, and the sweep that produces them is the same sweep that
/// produces the history.
#[derive(Debug, Clone, Serialize)]
pub struct LifecycleSweep {
    turns: Vec<TurnScan>,
    items: BTreeMap<ContextItemId, ItemRecord>,
    agent: AgentKind,
}

impl LifecycleSweep {
    pub fn turns(&self) -> &[TurnScan] {
        &self.turns
    }

    pub fn items(&self) -> impl Iterator<Item = &ItemRecord> {
        self.items.values()
    }

    pub fn item(&self, id: &ContextItemId) -> Option<&ItemRecord> {
        self.items.get(id)
    }

    /// Find one item by id or by any part of its label.
    ///
    /// Ids are exact and win outright; label matching is case-insensitive
    /// substring, because the label is what the user just read in `ct largest`
    /// and retyping a whole Windows path to trace it would be absurd.
    pub fn resolve(&self, needle: &str) -> Result<&ItemRecord, ResolveError> {
        let trimmed = needle.trim();
        if let Some(record) = self.items.get(&ContextItemId::new(trimmed)) {
            return Ok(record);
        }

        let lowered = trimmed.to_lowercase();
        let mut matched: Vec<&ItemRecord> = self
            .items
            .values()
            .filter(|r| r.label.to_lowercase().contains(&lowered))
            .collect();

        match matched.len() {
            0 => Err(ResolveError::NotFound(trimmed.to_string())),
            1 => Ok(matched.remove(0)),
            _ => {
                // Longest-lived first: when a label matches several items the
                // one worth naming is usually the one that has been in context
                // the longest.
                matched.sort_by(|a, b| b.present_in.len().cmp(&a.present_in.len()));
                Err(ResolveError::Ambiguous {
                    needle: trimmed.to_string(),
                    candidates: matched
                        .iter()
                        .map(|r| Candidate {
                            id: r.id.clone(),
                            label: r.label.clone(),
                            first_present: r.first_present(),
                            last_present: r.last_present(),
                            turns_present: r.present_in.len(),
                        })
                        .collect(),
                })
            }
        }
    }

    /// Derive one item's history from the sweep.
    pub fn lifecycle_of(&self, record: &ItemRecord) -> ItemLifecycle {
        let scanned: Vec<&TurnScan> = self
            .turns
            .iter()
            .filter(|t| t.sidechain == record.sidechain)
            .collect();

        let mut runs: Vec<TurnRun> = Vec::new();
        let mut unknown_turns: Vec<u32> = Vec::new();
        let mut open: Option<TurnRun> = None;

        for scan in &scanned {
            if !scan.readable {
                // Unknown, so the run cannot be continued across it *or* closed
                // as a departure. Ending it here is the conservative reading:
                // the view then reports a gap it cannot explain rather than
                // asserting continuity it did not observe.
                unknown_turns.push(scan.turn);
                if let Some(run) = open.take() {
                    runs.push(run);
                }
                continue;
            }
            if record.present_at(scan.turn) {
                match &mut open {
                    Some(run) => {
                        run.to = scan.turn;
                        run.turns += 1;
                    }
                    None => {
                        open = Some(TurnRun {
                            from: scan.turn,
                            to: scan.turn,
                            turns: 1,
                        })
                    }
                }
            } else if let Some(run) = open.take() {
                runs.push(run);
            }
        }

        let still_present = open.is_some();
        if let Some(run) = open.take() {
            runs.push(run);
        }

        let departure = match (still_present, runs.last()) {
            (false, Some(last)) => departure_after(&scanned, last.to, self.agent),
            _ => None,
        };

        ItemLifecycle {
            id: record.id.clone(),
            label: record.label.clone(),
            category: record.category,
            source: record.source.clone(),
            runs,
            departure,
            still_present,
            unknown_turns,
            scanned_turns: scanned.len(),
            other_thread_turns: self.turns.len() - scanned.len(),
            last_scanned_turn: scanned.last().map(|s| s.turn),
            recorded_first_seen: record.recorded_first_seen,
        }
    }
}

/// What removed an item that was present at `last_present` and gone afterwards.
///
/// Returns `None` when the following turn could not be reconstructed: a
/// departure asserted from an unreadable turn would be an inference from missing
/// data, which is the one thing this view exists not to do.
fn departure_after(scanned: &[&TurnScan], last_present: u32, agent: AgentKind) -> Option<Departure> {
    let index = scanned.iter().position(|s| s.turn == last_present)?;
    let held = scanned[index].compaction.map(|c| c.line_no);
    let next = scanned.get(index + 1)?;
    if !next.readable {
        return None;
    }

    match next.compaction {
        // A compaction the previous turn was not already living under: it
        // happened in between, and it rewrote the prompt.
        Some(mark) if Some(mark.line_no) != held => Some(Departure::Compaction {
            turn: mark.turn,
            reclaimed: mark.reclaimed,
        }),
        _ if agent == AgentKind::ClaudeCode => Some(Departure::BranchDiverged { turn: next.turn }),
        _ => Some(Departure::Unexplained { turn: next.turn }),
    }
}

impl ContextTrace {
    /// Reconstruct every turn once and record which items each one held.
    ///
    /// Every turn, not only those with recorded usage: presence does not depend
    /// on the agent having reported a token count, and skipping unmeasured turns
    /// would punch holes in a history for a reason unrelated to the history.
    pub fn sweep_lifecycles(&self, session: &AgentSession, binding: usize) -> LifecycleSweep {
        let probe = CharProbe;
        let adapter = &self.bindings[binding].adapter;
        let mut turns: Vec<TurnScan> = Vec::with_capacity(session.turn_count());
        let mut items: BTreeMap<ContextItemId, ItemRecord> = BTreeMap::new();

        for turn in session.turns() {
            let number = turn.number.get();
            let sidechain = turn
                .anchor_index
                .and_then(|i| session.event(i))
                .is_some_and(|e| e.links.is_sidechain);

            let Ok(context) = adapter.reconstruct(session, turn.number, &probe) else {
                turns.push(TurnScan {
                    turn: number,
                    sidechain,
                    compaction: None,
                    readable: false,
                });
                continue;
            };

            for item in &context.items {
                let record = items.entry(item.id.clone()).or_insert_with(|| ItemRecord {
                    id: item.id.clone(),
                    label: item.label.clone(),
                    category: item.category,
                    source: item.source.clone(),
                    recorded_first_seen: item.first_seen_turn.map(|t| t.get()),
                    present_in: Vec::new(),
                    sidechain,
                });
                // Turns are swept in ascending order, so this stays sorted --
                // which `present_at` relies on for its binary search.
                if record.present_in.last() != Some(&number) {
                    record.present_in.push(number);
                }
            }

            turns.push(TurnScan {
                turn: number,
                sidechain,
                compaction: context.preceding_compaction.as_ref().map(|c| CompactionMark {
                    line_no: c.source.line_no,
                    turn: c.turn.map(|t| t.get()),
                    reclaimed: c.reduction(),
                }),
                readable: true,
            });
        }

        LifecycleSweep {
            turns,
            items,
            agent: session.agent(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(turn: u32, readable: bool, compaction: Option<u32>) -> TurnScan {
        TurnScan {
            turn,
            sidechain: false,
            compaction: compaction.map(|line_no| CompactionMark {
                line_no,
                turn: Some(turn),
                reclaimed: Some(120_000),
            }),
            readable,
        }
    }

    fn sweep(turns: Vec<TurnScan>, present_in: Vec<u32>, agent: AgentKind) -> LifecycleSweep {
        let record = ItemRecord {
            id: ContextItemId::new("claude:42"),
            label: "Read BACKLOG.md".into(),
            category: ContextCategory::ToolOutputs,
            source: ContextSource::ToolExecution { tool: "Read".into() },
            recorded_first_seen: present_in.first().copied(),
            present_in,
            sidechain: false,
        };
        let mut items = BTreeMap::new();
        items.insert(record.id.clone(), record);
        LifecycleSweep {
            turns,
            items,
            agent,
        }
    }

    fn lifecycle(sweep: &LifecycleSweep) -> ItemLifecycle {
        let record = sweep.item(&ContextItemId::new("claude:42")).unwrap();
        sweep.lifecycle_of(record)
    }

    #[test]
    fn an_item_present_to_the_end_has_no_departure() {
        let turns = (1..=5).map(|t| scan(t, true, None)).collect();
        let life = lifecycle(&sweep(turns, vec![2, 3, 4, 5], AgentKind::ClaudeCode));

        assert_eq!(life.first_present(), Some(2));
        assert_eq!(life.turns_present(), 4);
        assert!(life.still_present);
        assert!(life.departure.is_none());
    }

    #[test]
    fn a_compaction_between_two_turns_is_named_as_the_cause() {
        let turns = vec![
            scan(1, true, None),
            scan(2, true, None),
            scan(3, true, Some(880)),
            scan(4, true, Some(880)),
        ];
        let life = lifecycle(&sweep(turns, vec![1, 2], AgentKind::ClaudeCode));

        assert!(!life.still_present);
        assert_eq!(
            life.departure,
            Some(Departure::Compaction {
                turn: Some(3),
                reclaimed: Some(120_000)
            })
        );
    }

    #[test]
    fn a_compaction_already_in_force_is_not_blamed_again() {
        // Both turns live under the same compaction, so it cannot be what
        // removed an item that survived into the first of them. Comparing the
        // compaction's identity rather than its mere presence is what keeps a
        // post-compaction rewind from being misreported as a second compaction.
        let turns = vec![
            scan(1, true, Some(880)),
            scan(2, true, Some(880)),
            scan(3, true, Some(880)),
        ];
        let life = lifecycle(&sweep(turns, vec![1, 2], AgentKind::ClaudeCode));

        assert_eq!(life.departure, Some(Departure::BranchDiverged { turn: 3 }));
    }

    #[test]
    fn disappearing_without_a_compaction_is_a_branch_change_not_an_eviction() {
        // The regression this guards: reporting "evicted at turn 4" for an item
        // that was never in turn 4's prompt because the conversation was rewound.
        let turns = (1..=4).map(|t| scan(t, true, None)).collect();
        let life = lifecycle(&sweep(turns, vec![1, 2, 3], AgentKind::ClaudeCode));

        assert_eq!(life.departure, Some(Departure::BranchDiverged { turn: 4 }));
    }

    #[test]
    fn codex_cannot_lose_an_item_without_a_compaction() {
        // Codex's replay fold only clears the item list at a `compacted` event,
        // so this shape is a defect in ContextTrace rather than a session fact,
        // and the view must say so instead of inventing a branch that Codex's
        // linear log cannot have.
        let turns = (1..=4).map(|t| scan(t, true, None)).collect();
        let life = lifecycle(&sweep(turns, vec![1, 2, 3], AgentKind::Codex));

        assert_eq!(life.departure, Some(Departure::Unexplained { turn: 4 }));
    }

    #[test]
    fn an_unreadable_turn_is_a_gap_rather_than_a_departure() {
        let turns = vec![
            scan(1, true, None),
            scan(2, true, None),
            scan(3, false, None),
            scan(4, true, None),
        ];
        let life = lifecycle(&sweep(turns, vec![1, 2, 4], AgentKind::ClaudeCode));

        assert_eq!(life.unknown_turns, vec![3]);
        assert_eq!(life.runs.len(), 2, "an unknown turn must not be read as continuity");
        assert!(life.still_present);
        assert!(life.departure.is_none());
    }

    #[test]
    fn a_departure_is_not_claimed_from_an_unreadable_next_turn() {
        let turns = vec![scan(1, true, None), scan(2, true, None), scan(3, false, None)];
        let life = lifecycle(&sweep(turns, vec![1, 2], AgentKind::ClaudeCode));

        assert!(!life.still_present);
        assert!(
            life.departure.is_none(),
            "the next turn could not be read, so nothing may be said about why"
        );
    }

    #[test]
    fn subagent_turns_do_not_make_a_main_thread_item_flicker() {
        // A subagent has its own context window, so main-thread items are
        // legitimately absent from its turns. Counting those as gaps would break
        // one run into three and manufacture a departure.
        let mut turns: Vec<TurnScan> = (1..=6).map(|t| scan(t, true, None)).collect();
        turns[2].sidechain = true;
        turns[3].sidechain = true;
        let life = lifecycle(&sweep(turns, vec![1, 2, 5, 6], AgentKind::ClaudeCode));

        assert_eq!(life.runs.len(), 1, "expected one uninterrupted run: {:?}", life.runs);
        assert_eq!(life.other_thread_turns, 2);
        assert!(life.still_present);
    }

    #[test]
    fn an_id_resolves_exactly_and_a_label_by_substring() {
        let s = sweep(vec![scan(1, true, None)], vec![1], AgentKind::ClaudeCode);
        assert_eq!(s.resolve("claude:42").unwrap().id.as_str(), "claude:42");
        assert_eq!(s.resolve("backlog").unwrap().id.as_str(), "claude:42");
        assert!(matches!(s.resolve("nothing here"), Err(ResolveError::NotFound(_))));
    }

    #[test]
    fn two_items_with_the_same_label_are_a_choice_not_an_error_message() {
        // Labels are not unique: two `Read BACKLOG.md` calls are two items. The
        // candidates carry the ids needed to pick one.
        let mut s = sweep(vec![scan(1, true, None)], vec![1], AgentKind::ClaudeCode);
        let mut twin = s.items.values().next().unwrap().clone();
        twin.id = ContextItemId::new("claude:99");
        s.items.insert(twin.id.clone(), twin);

        match s.resolve("BACKLOG") {
            Err(ResolveError::Ambiguous { candidates, .. }) => {
                assert_eq!(candidates.len(), 2);
                assert!(candidates.iter().all(|c| c.label.contains("BACKLOG")));
            }
            other => panic!("expected an ambiguous match, got {other:?}"),
        }
    }

    #[test]
    fn the_recorded_first_turn_and_the_observed_one_are_reported_separately() {
        let turns = (1..=4).map(|t| scan(t, true, None)).collect();
        let mut s = sweep(turns, vec![3, 4], AgentKind::ClaudeCode);
        s.items
            .values_mut()
            .next()
            .unwrap()
            .recorded_first_seen = Some(1);

        let life = lifecycle(&s);
        assert_eq!(life.first_present(), Some(3));
        assert_eq!(life.recorded_first_seen, Some(1));
        assert!(
            life.first_seen_disagrees(),
            "a disagreement must stay visible, not be reconciled away"
        );
    }
}
