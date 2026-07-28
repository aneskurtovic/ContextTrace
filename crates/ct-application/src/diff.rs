//! Comparing two turns: what changed, and what may honestly be subtracted.
//!
//! The workflow is "it worked yesterday and fails today on the same task".
//! Answering it means putting two reconstructions beside each other -- and that
//! is where a comparison view can quietly start lying.
//!
//! # Two sessions are measured by two instruments
//!
//! Claude Code item sizes come from a characters-per-token ratio fitted to each
//! session's *own* usage figures. Across the sessions on the machine this was
//! built on, that ratio ranges from 2.00 to 2.55 -- a 27% spread. Two sessions
//! therefore report their contents on two differently-graduated scales, and a
//! raw subtraction between them carries both the change in content *and* the
//! difference between the instruments, with no way to tell which is which.
//!
//! The residual is worst affected. It is `observed_total - sum(item estimates)`,
//! so every token the ratio difference moves in the items lands there with the
//! sign flipped. The axis most likely to be read as "this session wasted more
//! context" is the one most contaminated by the measurement.
//!
//! # What this module does about it
//!
//! It does not average the ratios, and it does not re-size one side with the
//! other's instrument -- that would make the deltas clean at the cost of making
//! each side disagree with `ct context` for its own session.
//!
//! Instead the skew between the instruments is *quantified* and carried down to
//! every row, as the largest difference the instruments alone could account for
//! ([`CategoryDelta::instrument_bound`]). A delta above its bound is a change in
//! content; a delta below it is not distinguishable from the measurement. This
//! falls out row by row rather than being asserted once in a caption, and it
//! flags the residual automatically -- its bound is set by the whole accounted
//! total, which is exactly the quantity the ratio difference moves.
//!
//! Where no skew exists to bound -- a real tokenizer on one side and a ratio
//! heuristic on the other -- the answer is a refusal rather than a wider bound.
//! See [`Comparability::Incomparable`].
//!
//! # What stays true whatever the instruments
//!
//! Prompt totals are read from the agent's own usage records, and item counts
//! and tool-call counts are counts. Nothing on those axes is estimated, so they
//! are reported unconditionally and are the right place to start reading.

use ct_domain::{AgentKind, ContextCategory, ContextSnapshot, ContextSource, TokenCount};
use serde::Serialize;
use std::collections::BTreeMap;

/// How one side's item sizes were produced.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Instrument {
    pub name: String,
    /// The ratio knob, where this instrument has one.
    ///
    /// `Some(r)` means every size it produces is a character count divided by
    /// `r`, so two instruments differing only in `r` produce figures related by
    /// a known factor. `None` means the sizes come from somewhere else -- a real
    /// tokenizer -- and no such factor exists to correct for.
    pub chars_per_token: Option<f32>,
}

impl Instrument {
    pub fn new(name: impl Into<String>, chars_per_token: Option<f32>) -> Self {
        Self {
            name: name.into(),
            chars_per_token,
        }
    }
}

/// Whether the two sides' token figures may be subtracted, and how far.
///
/// A sum type rather than a boolean or a caption, for the reason `TokenCount` is
/// one: these are not degrees of the same claim. One says the subtraction is
/// exact, one says it is exact to within a computed bound, and one says there is
/// no bound to state.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Comparability {
    /// One instrument on both sides. Token deltas are content deltas.
    Identical { estimator: String },
    /// Two ratio heuristics with different ratios. Every token figure on one
    /// side is scaled against the other by `skew`, which bounds each row.
    Skewed {
        left: String,
        right: String,
        /// Fractional difference between the two ratios, e.g. `0.27` for 27%.
        skew: f32,
    },
    /// Different kinds of instrument -- in practice a tokenizer against a
    /// heuristic, which is what comparing a Codex session with a Claude Code one
    /// means. No factor relates the two scales, so no token delta is reported at
    /// all.
    Incomparable {
        left: String,
        right: String,
        reason: String,
    },
}

impl Comparability {
    /// The fractional error the instruments alone can introduce, or `None` when
    /// that cannot be stated.
    pub fn skew(&self) -> Option<f32> {
        match self {
            Comparability::Identical { .. } => Some(0.0),
            Comparability::Skewed { skew, .. } => Some(*skew),
            Comparability::Incomparable { .. } => None,
        }
    }

    /// True when token figures may be subtracted at all.
    pub fn tokens_are_comparable(&self) -> bool {
        self.skew().is_some()
    }

    fn of(left: &Instrument, right: &Instrument, agents_differ: bool) -> Self {
        match (left.chars_per_token, right.chars_per_token) {
            (Some(l), Some(r)) if l > 0.0 && r > 0.0 => {
                // Compared on the ratio, never on the name: the name rounds to
                // one decimal, so `chars/2.17` and `chars/2.18` print
                // identically while being half a percent apart.
                if l == r && left.name == right.name {
                    Comparability::Identical {
                        estimator: left.name.clone(),
                    }
                } else {
                    Comparability::Skewed {
                        left: left.name.clone(),
                        right: right.name.clone(),
                        skew: (l / r - 1.0).abs(),
                    }
                }
            }
            (None, None) if left.name == right.name => Comparability::Identical {
                estimator: left.name.clone(),
            },
            _ => Comparability::Incomparable {
                left: left.name.clone(),
                right: right.name.clone(),
                reason: if agents_differ {
                    // Two independent reasons, and the second survives even if
                    // the estimators were somehow matched: Codex logs its own
                    // system prompt as a context item, so its residual is not
                    // the same quantity as Claude Code's.
                    "one side is measured with a tokenizer and the other estimated from a \
                     ratio, and the two agents do not log the same things -- Codex records \
                     its system prompt, so the residuals are not the same quantity"
                        .into()
                } else {
                    "the two sides were sized by different kinds of instrument, and no \
                     factor relates their scales"
                        .into()
                },
            },
        }
    }
}

/// One side of the comparison, as supplied by the caller.
///
/// The instrument is passed in rather than inferred because choosing it is a
/// composition-root decision -- which estimator a session gets is settled once,
/// where the adapters are wired, and this module must not re-decide it.
pub struct Side<'a> {
    pub snapshot: &'a ContextSnapshot,
    pub instrument: Instrument,
}

/// What was compared, restated so a reader never has to assume it.
#[derive(Debug, Clone, Serialize)]
pub struct SideSummary {
    pub session_id: String,
    pub agent: AgentKind,
    /// Always stated. Two sides defaulting to their own peak turn can be turn 5
    /// against turn 400, and depth would then read as difference.
    pub turn: u32,
    pub model: Option<String>,
    pub total: TokenCount,
    pub items: usize,
    pub residual: u32,
    pub instrument: Instrument,
}

/// One category's figures on both sides.
#[derive(Debug, Clone, Serialize)]
pub struct CategoryDelta {
    pub category: ContextCategory,
    pub left: u32,
    pub right: u32,
    /// `right - left`. Meaningful only above [`CategoryDelta::instrument_bound`].
    pub delta: i64,
    pub left_items: usize,
    pub right_items: usize,
    /// The largest difference the instruments alone could produce on a row this
    /// size. `None` when no bound can be stated, in which case `delta` is
    /// arithmetic without a claim attached and must not be rendered as a finding.
    pub instrument_bound: Option<u32>,
}

impl CategoryDelta {
    /// True when this row's difference is larger than the measurement could
    /// explain by itself.
    pub fn is_meaningful(&self) -> bool {
        self.instrument_bound
            .is_some_and(|bound| self.delta.unsigned_abs() > bound as u64)
    }

    /// Change in how many items make up this category. A count, so it is
    /// unaffected by either instrument.
    pub fn item_delta(&self) -> i64 {
        self.right_items as i64 - self.left_items as i64
    }
}

/// One tool's usage on both sides.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDelta {
    pub tool: String,
    pub left_calls: usize,
    pub right_calls: usize,
    pub left_tokens: u32,
    pub right_tokens: u32,
    pub instrument_bound: Option<u32>,
}

impl ToolDelta {
    pub fn call_delta(&self) -> i64 {
        self.right_calls as i64 - self.left_calls as i64
    }

    pub fn token_delta(&self) -> i64 {
        self.right_tokens as i64 - self.left_tokens as i64
    }

    pub fn tokens_are_meaningful(&self) -> bool {
        self.instrument_bound
            .is_some_and(|bound| self.token_delta().unsigned_abs() > bound as u64)
    }
}

/// The comparison.
#[derive(Debug, Clone, Serialize)]
pub struct SessionDiff {
    pub left: SideSummary,
    pub right: SideSummary,
    pub comparability: Comparability,
    pub categories: Vec<CategoryDelta>,
    pub tools: Vec<ToolDelta>,
}

impl SessionDiff {
    /// Change in the prompt size the agents themselves reported.
    ///
    /// Free of every instrument question the rest of this type has to manage --
    /// both figures are read out of the sessions' own usage records. Where that
    /// holds, [`SessionDiff::totals_are_observed`] is true and this is the one
    /// number in the view that needs no caveat.
    pub fn prompt_delta(&self) -> i64 {
        self.right.total.tokens() as i64 - self.left.total.tokens() as i64
    }

    pub fn totals_are_observed(&self) -> bool {
        self.left.total.is_trustworthy() && self.right.total.is_trustworthy()
    }

    pub fn same_session(&self) -> bool {
        self.left.session_id == self.right.session_id
    }
}

/// Compare two reconstructed turns.
///
/// Pure: it reads two finished snapshots and touches no port. That is what lets
/// it be tested against hand-built snapshots, which is worth more here than
/// anywhere else in this crate -- the interesting cases are ratio combinations,
/// and reproducing those from real sessions would mean shipping a corpus.
pub fn compare(left: Side<'_>, right: Side<'_>) -> SessionDiff {
    let agents_differ = left.snapshot.agent() != right.snapshot.agent();
    let comparability = Comparability::of(&left.instrument, &right.instrument, agents_differ);
    let skew = comparability.skew();

    let categories = compare_categories(left.snapshot, right.snapshot, skew);
    let tools = compare_tools(left.snapshot, right.snapshot, skew);

    SessionDiff {
        left: summarise(left),
        right: summarise(right),
        comparability,
        categories,
        tools,
    }
}

fn summarise(side: Side<'_>) -> SideSummary {
    SideSummary {
        session_id: side.snapshot.session_id().to_string(),
        agent: side.snapshot.agent(),
        turn: side.snapshot.turn().get(),
        model: side.snapshot.model().map(str::to_string),
        total: side.snapshot.total(),
        items: side.snapshot.items().len(),
        residual: side.snapshot.residual(),
        instrument: side.instrument,
    }
}

/// The largest difference two instruments alone could produce on a row of this
/// size.
///
/// Uses the larger side, which is the conservative choice: re-sizing the right
/// side with the left's ratio moves it by `right * skew`, and the reverse moves
/// the left by `left * skew`, so taking the maximum bounds both directions.
///
/// This is an upper bound and is deliberately loose in one regime. When item
/// estimates *overflow* a turn's observed total the calibrator scales them all
/// down to fit, which cancels much of the ratio difference; the bound does not
/// model that, and will call some real differences unproven. Erring that way is
/// the right error for this tool to make.
fn instrument_bound(skew: Option<f32>, left: u32, right: u32) -> Option<u32> {
    let skew = skew?;
    Some((left.max(right) as f32 * skew).ceil() as u32)
}

fn compare_categories(
    left: &ContextSnapshot,
    right: &ContextSnapshot,
    skew: Option<f32>,
) -> Vec<CategoryDelta> {
    // Both sides' rows come from `by_category`, so the residual arrives as an
    // `Unattributed` row like any other and cannot be forgotten here.
    let mut rows: BTreeMap<ContextCategory, (u32, usize, u32, usize)> = BTreeMap::new();
    for row in left.by_category() {
        let entry = rows.entry(row.category).or_default();
        entry.0 = row.tokens;
        entry.1 = row.item_count;
    }
    for row in right.by_category() {
        let entry = rows.entry(row.category).or_default();
        entry.2 = row.tokens;
        entry.3 = row.item_count;
    }

    // The residual does not get the bound its own size implies. It is
    // `observed_total - sum(item estimates)`, and the observed total is fixed --
    // so every token the ratio difference moves in the items lands here, and the
    // quantity that bounds it is the *accounted* total, not the remainder.
    // Sizing this row like any other understated its bound by more than half on
    // the case that first exercised it.
    let left_accounted = left.total().tokens().saturating_sub(left.residual());
    let right_accounted = right.total().tokens().saturating_sub(right.residual());

    let mut out: Vec<CategoryDelta> = rows
        .into_iter()
        .map(
            |(category, (left_tokens, left_items, right_tokens, right_items))| CategoryDelta {
                category,
                left: left_tokens,
                right: right_tokens,
                delta: right_tokens as i64 - left_tokens as i64,
                left_items,
                right_items,
                instrument_bound: match category {
                    ContextCategory::Unattributed => {
                        instrument_bound(skew, left_accounted, right_accounted)
                    }
                    _ => instrument_bound(skew, left_tokens, right_tokens),
                },
            },
        )
        .collect();

    // Largest change first where changes can be read at all; largest row first
    // where they cannot, since ranking by a quantity we have just declined to
    // interpret would be presenting it as a finding by the back door.
    if skew.is_some() {
        out.sort_by(|a, b| b.delta.abs().cmp(&a.delta.abs()));
    } else {
        out.sort_by_key(|d| std::cmp::Reverse(d.left.max(d.right)));
    }
    out
}

fn compare_tools(
    left: &ContextSnapshot,
    right: &ContextSnapshot,
    skew: Option<f32>,
) -> Vec<ToolDelta> {
    let mut rows: BTreeMap<String, (usize, u32, usize, u32)> = BTreeMap::new();
    for (tool, calls, tokens) in tool_usage(left) {
        let entry = rows.entry(tool).or_default();
        entry.0 = calls;
        entry.1 = tokens;
    }
    for (tool, calls, tokens) in tool_usage(right) {
        let entry = rows.entry(tool).or_default();
        entry.2 = calls;
        entry.3 = tokens;
    }

    let mut out: Vec<ToolDelta> = rows
        .into_iter()
        .map(
            |(tool, (left_calls, left_tokens, right_calls, right_tokens))| ToolDelta {
                tool,
                left_calls,
                right_calls,
                left_tokens,
                right_tokens,
                instrument_bound: instrument_bound(skew, left_tokens, right_tokens),
            },
        )
        .collect();

    // Ranked by the call delta, which is a count and therefore says the same
    // thing whatever sized the items.
    out.sort_by(|a, b| {
        b.call_delta()
            .abs()
            .cmp(&a.call_delta().abs())
            .then(b.right_calls.max(b.left_calls).cmp(&a.right_calls.max(a.left_calls)))
    });
    out
}

/// Calls and tokens per tool in one turn's context.
fn tool_usage(snapshot: &ContextSnapshot) -> Vec<(String, usize, u32)> {
    let mut by_tool: BTreeMap<String, (usize, u32)> = BTreeMap::new();
    for item in snapshot.items() {
        if let ContextSource::ToolExecution { tool } = &item.source {
            let entry = by_tool.entry(tool.clone()).or_default();
            entry.0 += 1;
            entry.1 = entry.1.saturating_add(item.tokens.tokens());
        }
    }
    by_tool
        .into_iter()
        .map(|(tool, (calls, tokens))| (tool, calls, tokens))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::model::identity::FileId;
    use ct_domain::{
        ContextItem, ContextItemId, Provenance, SessionId, SourceRef, TurnNumber,
    };

    fn item(label: &str, category: ContextCategory, tokens: u32, source: ContextSource) -> ContextItem {
        ContextItem {
            id: ContextItemId::new(label),
            category,
            label: label.into(),
            source,
            tokens: TokenCount::estimated(tokens),
            first_seen_turn: None,
            provenance: Provenance::observed(SourceRef::new(FileId(0), 0, 0, 1)),
            preview: None,
            content_measurement: None,
        }
    }

    fn snapshot(
        id: &str,
        agent: AgentKind,
        turn: u32,
        items: Vec<ContextItem>,
        residual: u32,
    ) -> ContextSnapshot {
        let total: u32 = items.iter().map(|i| i.tokens.tokens()).sum::<u32>() + residual;
        ContextSnapshot::assemble(
            SessionId::new(id).unwrap(),
            agent,
            TurnNumber::new(turn).unwrap(),
            Some("m".into()),
            items,
            TokenCount::observed(total),
            residual,
            Some(200_000),
            None,
        )
        .expect("test snapshots must balance")
    }

    fn heuristic(ratio: f32) -> Instrument {
        Instrument::new(format!("heuristic:chars/{ratio:.1}"), Some(ratio))
    }

    fn tokenizer() -> Instrument {
        Instrument::new("o200k_base", None)
    }

    fn tool(name: &str) -> ContextSource {
        ContextSource::ToolExecution { tool: name.into() }
    }

    #[test]
    fn one_session_compared_with_itself_has_no_skew_to_bound() {
        // Turn against turn inside one session is the case that needs no
        // caveats: one instrument sized both sides.
        let a = snapshot("s", AgentKind::ClaudeCode, 3, vec![item("x", ContextCategory::ToolOutputs, 500, tool("Bash"))], 100);
        let b = snapshot("s", AgentKind::ClaudeCode, 9, vec![item("x", ContextCategory::ToolOutputs, 900, tool("Bash"))], 100);

        let diff = compare(
            Side { snapshot: &a, instrument: heuristic(2.2) },
            Side { snapshot: &b, instrument: heuristic(2.2) },
        );

        assert!(matches!(diff.comparability, Comparability::Identical { .. }));
        assert_eq!(diff.comparability.skew(), Some(0.0));
        assert!(diff.same_session());

        let row = diff.categories.iter().find(|c| c.category == ContextCategory::ToolOutputs).unwrap();
        assert_eq!(row.delta, 400);
        assert_eq!(row.instrument_bound, Some(0), "one instrument bounds nothing away");
        assert!(row.is_meaningful());
    }

    #[test]
    fn a_delta_smaller_than_the_ratio_difference_is_not_reported_as_a_change() {
        // The defect this module exists to prevent. Two Claude Code sessions
        // fitted at 2.00 and 2.55 -- the real spread on the machine this was
        // built on -- differ by 27% before any content differs at all.
        let a = snapshot("a", AgentKind::ClaudeCode, 1, vec![item("x", ContextCategory::ToolOutputs, 10_000, tool("Bash"))], 5_000);
        let b = snapshot("b", AgentKind::ClaudeCode, 1, vec![item("x", ContextCategory::ToolOutputs, 11_000, tool("Bash"))], 5_000);

        let diff = compare(
            Side { snapshot: &a, instrument: heuristic(2.0) },
            Side { snapshot: &b, instrument: heuristic(2.55) },
        );

        let skew = diff.comparability.skew().expect("two ratios bound each other");
        assert!((skew - 0.2157).abs() < 0.01, "skew was {skew}");

        let row = diff.categories.iter().find(|c| c.category == ContextCategory::ToolOutputs).unwrap();
        assert_eq!(row.delta, 1_000);
        assert!(
            !row.is_meaningful(),
            "1,000 tokens is inside the {:?} the instruments alone explain",
            row.instrument_bound
        );
    }

    #[test]
    fn a_delta_larger_than_the_ratio_difference_survives_it() {
        let a = snapshot("a", AgentKind::ClaudeCode, 1, vec![item("x", ContextCategory::ToolOutputs, 10_000, tool("Bash"))], 0);
        let b = snapshot("b", AgentKind::ClaudeCode, 1, vec![item("x", ContextCategory::ToolOutputs, 48_000, tool("Bash"))], 0);

        let diff = compare(
            Side { snapshot: &a, instrument: heuristic(2.0) },
            Side { snapshot: &b, instrument: heuristic(2.55) },
        );

        let row = diff.categories.iter().find(|c| c.category == ContextCategory::ToolOutputs).unwrap();
        assert!(row.is_meaningful(), "38,000 tokens is far outside the bound");
        assert_eq!(diff.categories[0].category, ContextCategory::ToolOutputs, "biggest change first");
    }

    #[test]
    fn the_residual_is_bounded_by_the_accounted_total_not_by_its_own_size() {
        // The bug this test found. The residual is `total - sum(estimates)`, so
        // a ratio moving the items by 9% moves the residual by 9% *of the
        // items* -- 5,455 tokens here, not 9% of the residual's own 23,000.
        // Bounding it like an ordinary row understated it by more than half and
        // would have presented a 3,000-token move as a finding.
        let a = snapshot("a", AgentKind::ClaudeCode, 1, vec![item("x", ContextCategory::ToolOutputs, 60_000, tool("Bash"))], 20_000);
        let b = snapshot("b", AgentKind::ClaudeCode, 1, vec![item("x", ContextCategory::ToolOutputs, 60_000, tool("Bash"))], 23_000);

        let diff = compare(
            Side { snapshot: &a, instrument: heuristic(2.0) },
            Side { snapshot: &b, instrument: heuristic(2.2) },
        );

        let row = diff.categories.iter().find(|c| c.category == ContextCategory::Unattributed).unwrap();
        assert_eq!(row.delta, 3_000);
        let bound = row.instrument_bound.expect("two ratios bound each other");
        assert!(
            bound > 5_000,
            "the bound must come from the 60,000 accounted tokens, not the 23,000 residual; got {bound}"
        );
        assert!(!row.is_meaningful(), "3,000 is inside what a 2.0-vs-2.2 fit explains");
    }

    #[test]
    fn a_tokenizer_against_a_heuristic_refuses_rather_than_widening_the_bound() {
        // Codex against Claude Code. No factor relates a measured count to a
        // ratio estimate, so there is no honest bound to state -- and Codex logs
        // its own system prompt, so the residuals are not even the same quantity.
        let a = snapshot("a", AgentKind::Codex, 1, vec![item("x", ContextCategory::ToolOutputs, 10_000, tool("shell"))], 3_000);
        let b = snapshot("b", AgentKind::ClaudeCode, 1, vec![item("x", ContextCategory::ToolOutputs, 40_000, tool("Bash"))], 3_000);

        let diff = compare(
            Side { snapshot: &a, instrument: tokenizer() },
            Side { snapshot: &b, instrument: heuristic(2.2) },
        );

        assert!(matches!(diff.comparability, Comparability::Incomparable { .. }));
        assert!(!diff.comparability.tokens_are_comparable());
        for row in &diff.categories {
            assert_eq!(row.instrument_bound, None);
            assert!(!row.is_meaningful(), "nothing may be claimed about {:?}", row.category);
        }
        assert_eq!(
            diff.categories[0].category,
            ContextCategory::ToolOutputs,
            "ranked by size, not by a delta we declined to interpret"
        );
    }

    #[test]
    fn counts_and_observed_totals_survive_an_incomparable_pairing() {
        // The point of separating the axes: when token sizes cannot be compared,
        // the view is not empty. Prompt totals are read from the agents' own
        // usage records and calls are calls.
        let a = snapshot(
            "a",
            AgentKind::Codex,
            1,
            vec![
                item("r1", ContextCategory::ToolOutputs, 100, tool("shell")),
                item("r2", ContextCategory::ToolOutputs, 100, tool("shell")),
            ],
            0,
        );
        let b = snapshot(
            "b",
            AgentKind::ClaudeCode,
            1,
            vec![item("r1", ContextCategory::ToolOutputs, 900, tool("shell"))],
            0,
        );

        let diff = compare(
            Side { snapshot: &a, instrument: tokenizer() },
            Side { snapshot: &b, instrument: heuristic(2.2) },
        );

        assert!(diff.totals_are_observed());
        assert_eq!(diff.prompt_delta(), 700);
        let shell = diff.tools.iter().find(|t| t.tool == "shell").unwrap();
        assert_eq!(shell.call_delta(), -1, "one fewer shell call, whatever sized it");
        assert!(!shell.tokens_are_meaningful());
        let row = diff.categories.iter().find(|c| c.category == ContextCategory::ToolOutputs).unwrap();
        assert_eq!(row.item_delta(), -1);
    }

    #[test]
    fn a_category_present_on_one_side_only_still_appears() {
        // An item kind that vanished between the two turns is the most
        // interesting thing a diff can find, and a naive zip over one side's
        // rows would drop it.
        let a = snapshot("a", AgentKind::ClaudeCode, 1, vec![item("f", ContextCategory::FileContents, 4_000, ContextSource::Unknown)], 0);
        let b = snapshot("b", AgentKind::ClaudeCode, 1, vec![item("t", ContextCategory::ToolOutputs, 4_000, tool("Bash"))], 0);

        let diff = compare(
            Side { snapshot: &a, instrument: heuristic(2.2) },
            Side { snapshot: &b, instrument: heuristic(2.2) },
        );

        let gone = diff.categories.iter().find(|c| c.category == ContextCategory::FileContents).unwrap();
        assert_eq!((gone.left, gone.right), (4_000, 0));
        let arrived = diff.categories.iter().find(|c| c.category == ContextCategory::ToolOutputs).unwrap();
        assert_eq!((arrived.left, arrived.right), (0, 4_000));
    }

    #[test]
    fn two_ratios_that_print_alike_are_still_compared_on_their_values() {
        // `heuristic:chars/2.2` is what both 2.17 and 2.18 render as. Comparing
        // instruments by name would call that one instrument and bound nothing.
        let a = snapshot("a", AgentKind::ClaudeCode, 1, vec![item("x", ContextCategory::ToolOutputs, 100_000, tool("Bash"))], 0);
        let b = snapshot("b", AgentKind::ClaudeCode, 1, vec![item("x", ContextCategory::ToolOutputs, 100_400, tool("Bash"))], 0);

        let diff = compare(
            Side { snapshot: &a, instrument: heuristic(2.17) },
            Side { snapshot: &b, instrument: heuristic(2.18) },
        );

        assert!(matches!(diff.comparability, Comparability::Skewed { .. }));
        let row = diff.categories.iter().find(|c| c.category == ContextCategory::ToolOutputs).unwrap();
        assert!(
            !row.is_meaningful(),
            "400 tokens is inside a half-percent fit difference on 100,000"
        );
    }
}
