//! The reconstructed context: what the model was looking at, and where it came
//! from.
//!
//! [`ContextSnapshot`] is an aggregate. Its invariant -- that the parts sum to
//! the whole -- is enforced at construction, so no consumer has to trust that
//! whichever adapter produced it remembered to reconcile its numbers.

use super::event::CompactionFacts;
use super::filter::{FilteredView, ItemFilter};
use super::identity::{ContextItemId, SessionId, TurnNumber};
use super::provenance::{Confidence, Provenance, SourceRef};
use super::session::AgentKind;
use super::tokens::TokenCount;
use serde::{Deserialize, Serialize};
use std::fmt;

/// What kind of thing a context item is. Drives the composition breakdown.
// Kebab-case on the wire so the JSON name and the `--category` argument are the
// same word. One vocabulary; nothing to translate between the two surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContextCategory {
    /// The agent's own base/system prompt.
    SystemInstructions,
    /// Instructions injected by the harness beneath the system prompt.
    DeveloperInstructions,
    /// AGENTS.md, CLAUDE.md and friends.
    RepositoryInstructions,
    /// JSON schemas for the tools the model may call. Usually invisible in
    /// logs, and therefore usually part of the residual.
    ToolDefinitions,
    UserMessages,
    AssistantMessages,
    Reasoning,
    ToolCalls,
    ToolOutputs,
    /// File contents pulled in by reads or attachments.
    FileContents,
    /// Compaction summaries and other replacements for dropped history.
    Summaries,
    /// The prompt that triggered this specific turn.
    CurrentPrompt,
    /// Context we know is present because the totals demand it, but cannot
    /// attribute to anything visible in the log.
    Unattributed,
    Other,
}

impl ContextCategory {
    pub const ALL: [ContextCategory; 14] = [
        ContextCategory::SystemInstructions,
        ContextCategory::DeveloperInstructions,
        ContextCategory::RepositoryInstructions,
        ContextCategory::ToolDefinitions,
        ContextCategory::UserMessages,
        ContextCategory::AssistantMessages,
        ContextCategory::Reasoning,
        ContextCategory::ToolCalls,
        ContextCategory::ToolOutputs,
        ContextCategory::FileContents,
        ContextCategory::Summaries,
        ContextCategory::CurrentPrompt,
        ContextCategory::Unattributed,
        ContextCategory::Other,
    ];

    /// The machine-facing name: what a user types, and what `--json` emits.
    ///
    /// Derived from the label rather than written out twice, so a category
    /// cannot be renamed in one place and left stale in the other.
    pub fn slug(&self) -> String {
        self.label().to_ascii_lowercase().replace(' ', "-")
    }

    pub fn parse(s: &str) -> Option<ContextCategory> {
        let norm = s.trim().to_ascii_lowercase().replace('_', "-");
        ContextCategory::ALL.into_iter().find(|c| c.slug() == norm)
    }

    pub fn label(&self) -> &'static str {
        match self {
            ContextCategory::SystemInstructions => "System instructions",
            ContextCategory::DeveloperInstructions => "Developer instructions",
            ContextCategory::RepositoryInstructions => "Repository instructions",
            ContextCategory::ToolDefinitions => "Tool definitions",
            ContextCategory::UserMessages => "User messages",
            ContextCategory::AssistantMessages => "Assistant messages",
            ContextCategory::Reasoning => "Reasoning",
            ContextCategory::ToolCalls => "Tool calls",
            ContextCategory::ToolOutputs => "Tool outputs",
            ContextCategory::FileContents => "File contents",
            ContextCategory::Summaries => "Summaries",
            ContextCategory::CurrentPrompt => "Current prompt",
            ContextCategory::Unattributed => "Unattributed",
            ContextCategory::Other => "Other",
        }
    }
}

/// Where a context item originated -- the answer to "why is this in my prompt?"
///
/// This is the instruction-tracing feature expressed as a type. For Claude Code
/// most of these are [`Confidence::Observed`], because the harness labels its
/// own injections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "origin", rename_all = "snake_case")]
pub enum ContextSource {
    /// The agent's built-in system prompt.
    AgentSystemPrompt,
    /// An instruction file discovered in the repository.
    InstructionFile { path: String },
    /// Configuration supplied by the project or user settings.
    ProjectConfig { path: Option<String> },
    /// Injected at runtime by the harness: a hook, a skill listing, MCP server
    /// instructions, a plan file, a queued command.
    HarnessInjection { mechanism: String },
    /// Typed by the human.
    UserPrompt,
    /// Produced by running a tool.
    ToolExecution { tool: String },
    /// Content of a file read into context.
    FileRead { path: String },
    /// A summary standing in for compacted history.
    CompactionSummary,
    /// Generated by the model.
    ModelOutput,
    /// We cannot say.
    Unknown,
}

impl fmt::Display for ContextSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextSource::AgentSystemPrompt => f.write_str("agent system prompt"),
            ContextSource::InstructionFile { path } => write!(f, "instruction file {path}"),
            ContextSource::ProjectConfig { path: Some(p) } => write!(f, "project config {p}"),
            ContextSource::ProjectConfig { path: None } => f.write_str("project config"),
            ContextSource::HarnessInjection { mechanism } => write!(f, "harness: {mechanism}"),
            ContextSource::UserPrompt => f.write_str("user prompt"),
            ContextSource::ToolExecution { tool } => write!(f, "tool: {tool}"),
            ContextSource::FileRead { path } => write!(f, "file: {path}"),
            ContextSource::CompactionSummary => f.write_str("compaction summary"),
            ContextSource::ModelOutput => f.write_str("model output"),
            ContextSource::Unknown => f.write_str("unknown"),
        }
    }
}

/// One addressable thing occupying space in the model's context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextItem {
    /// Stable across turns, so an item can be tracked through its lifecycle.
    pub id: ContextItemId,
    pub category: ContextCategory,
    /// Human-facing name: a filename, a tool invocation, a message summary.
    pub label: String,
    pub source: ContextSource,
    pub tokens: TokenCount,
    /// Turn at which this item first entered the context.
    pub first_seen_turn: Option<TurnNumber>,
    pub provenance: Provenance,
    /// Short excerpt for display. Full content is re-read via `provenance.source`.
    pub preview: Option<String>,
}

impl ContextItem {
    /// Overall confidence in this item: no stronger than either our belief that
    /// it is present or our belief in how large it is.
    pub fn confidence(&self) -> Confidence {
        self.provenance.confidence.weakest(self.tokens.confidence())
    }
}

/// A category's share of the context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryBreakdown {
    pub category: ContextCategory,
    pub tokens: u32,
    pub share: f32,
    pub item_count: usize,
    /// Weakest confidence among the contributing items.
    pub confidence: Confidence,
}

/// A single large context consumer, for the "largest contributors" view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contributor {
    pub id: ContextItemId,
    pub label: String,
    pub category: ContextCategory,
    pub source: ContextSource,
    pub tokens: u32,
    pub share: f32,
    pub confidence: Confidence,
}

/// A compaction that happened during the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionEvent {
    pub turn: Option<TurnNumber>,
    pub facts: CompactionFacts,
    pub source: SourceRef,
}

impl CompactionEvent {
    /// Tokens removed, when both sides were reported.
    pub fn reduction(&self) -> Option<u32> {
        let before = self.facts.tokens_before?;
        let after = self.facts.tokens_after?;
        Some(before.saturating_sub(after))
    }
}

/// **The aggregate.** ContextTrace's best reconstruction of what one model
/// request contained.
///
/// Construct via [`ContextSnapshot::assemble`], which enforces the invariant
/// that item tokens plus residual equal the reported total. That is what makes
/// the percentages in the composition view meaningful: they are shares of a
/// number the agent itself reported, not of a number we invented by adding up
/// our own guesses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextSnapshot {
    session_id: SessionId,
    agent: AgentKind,
    turn: TurnNumber,
    model: Option<String>,
    items: Vec<ContextItem>,
    total: TokenCount,
    /// Context present but unattributable -- in practice the hidden system
    /// prompt and tool JSON schemas.
    ///
    /// Naming this rather than smearing it across the visible categories is the
    /// difference between an honest breakdown and a flattering one.
    residual: u32,
    context_window: Option<u32>,
    /// The most recent compaction at or before this turn.
    preceding_compaction: Option<CompactionEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// The parts do not sum to the whole.
    Unbalanced {
        items_total: u64,
        residual: u32,
        declared_total: u32,
    },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::Unbalanced {
                items_total,
                residual,
                declared_total,
            } => write!(
                f,
                "context snapshot does not balance: items ({items_total}) + residual ({residual}) != total ({declared_total})"
            ),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl ContextSnapshot {
    /// Build a snapshot, rejecting any breakdown that does not add up.
    ///
    /// Normally reached through
    /// [`TokenCalibrator`](crate::services::TokenCalibrator), which produces
    /// balanced input by construction. The check is kept here anyway: the
    /// aggregate defends its own invariant regardless of who calls it.
    #[allow(clippy::too_many_arguments)]
    pub fn assemble(
        session_id: SessionId,
        agent: AgentKind,
        turn: TurnNumber,
        model: Option<String>,
        items: Vec<ContextItem>,
        total: TokenCount,
        residual: u32,
        context_window: Option<u32>,
        preceding_compaction: Option<CompactionEvent>,
    ) -> Result<Self, SnapshotError> {
        let items_total: u64 = items.iter().map(|i| i.tokens.tokens() as u64).sum();
        let declared_total = total.tokens();
        if items_total + residual as u64 != declared_total as u64 {
            return Err(SnapshotError::Unbalanced {
                items_total,
                residual,
                declared_total,
            });
        }
        Ok(Self {
            session_id,
            agent,
            turn,
            model,
            items,
            total,
            residual,
            context_window,
            preceding_compaction,
        })
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }
    pub fn agent(&self) -> AgentKind {
        self.agent
    }
    pub fn turn(&self) -> TurnNumber {
        self.turn
    }
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }
    pub fn items(&self) -> &[ContextItem] {
        &self.items
    }
    pub fn total(&self) -> TokenCount {
        self.total
    }
    pub fn residual(&self) -> u32 {
        self.residual
    }
    pub fn context_window(&self) -> Option<u32> {
        self.context_window
    }
    pub fn preceding_compaction(&self) -> Option<&CompactionEvent> {
        self.preceding_compaction.as_ref()
    }

    /// Fraction of the model's context window this turn consumed.
    pub fn utilisation(&self) -> Option<f32> {
        let window = self.context_window?;
        (window > 0).then(|| self.total.tokens() as f32 / window as f32)
    }

    /// How far heuristic estimates had to be moved to fit the observed total.
    ///
    /// `Some(0.78)` means the estimator guessed 28% high and everything was
    /// scaled down to fit; `Some(1.0)` means it landed on the nose. `None` means
    /// nothing needed calibrating.
    ///
    /// This matters for reading the rest of the snapshot honestly. When the
    /// factor is well below 1.0 the estimates *saturated* the observed total,
    /// which drives [`ContextSnapshot::residual`] to zero -- and a zero residual
    /// then means "our guesses filled the budget", **not** "there is no hidden
    /// context". The agent's system prompt and tool schemas are still in that
    /// total; they have simply been absorbed into the visible rows. Surfacing
    /// the factor is what keeps that distinction visible instead of quietly
    /// overstating every category.
    pub fn calibration_scale(&self) -> Option<f32> {
        let mut scaled = 0u64;
        let mut raw = 0u64;
        for item in &self.items {
            if let TokenCount::Calibrated {
                tokens,
                raw_estimate,
            } = item.tokens
            {
                scaled += tokens as u64;
                raw += raw_estimate as u64;
            }
        }
        (raw > 0).then(|| scaled as f32 / raw as f32)
    }

    /// True when the residual is large enough to be worth interpreting as
    /// unlogged context rather than as arithmetic rounding.
    ///
    /// Below this, describing it as "the system prompt and tool schemas" would
    /// be an overclaim.
    pub fn residual_is_meaningful(&self) -> bool {
        let total = self.total.tokens();
        total > 0 && (self.residual as f32 / total as f32) >= 0.005
    }

    /// Narrow this snapshot to the items matching `filter`.
    ///
    /// The returned view keeps a borrow of the whole snapshot, so its
    /// percentages remain shares of the turn rather than of the subset. See
    /// [`filter`](crate::model::filter) for why that is enforced structurally
    /// rather than by convention.
    pub fn filtered<'a>(&'a self, filter: &'a ItemFilter) -> FilteredView<'a> {
        FilteredView::new(self, filter)
    }

    /// Composition by category, largest first, including the residual as an
    /// explicit [`ContextCategory::Unattributed`] row when non-zero.
    ///
    /// The unfiltered case of [`ContextSnapshot::filtered`], not a parallel
    /// implementation -- so the two can never disagree about the denominator.
    pub fn by_category(&self) -> Vec<CategoryBreakdown> {
        self.filtered(&ItemFilter::ALL).by_category()
    }

    /// One named item's row, if this turn held it.
    ///
    /// Its share is of the turn's total, exactly as in the ranked view, so the
    /// two agree **at a given turn**. They routinely pick different ones -- the
    /// ranked view defaults to the session's peak turn, while a lifecycle view
    /// sizes at the last turn holding the item -- and the same item is then a
    /// different share of two different totals. That is why every caller prints
    /// the turn it sized at: identical token counts against unequal
    /// denominators are only confusing when the denominator is left implicit.
    pub fn contributor(&self, id: &ContextItemId) -> Option<Contributor> {
        let item = self.items.iter().find(|i| &i.id == id)?;
        Some(Contributor {
            id: item.id.clone(),
            label: item.label.clone(),
            category: item.category,
            source: item.source.clone(),
            tokens: item.tokens.tokens(),
            share: item.tokens.tokens() as f32 / self.total.tokens().max(1) as f32,
            confidence: item.confidence(),
        })
    }

    /// The `limit` biggest individual context consumers.
    ///
    /// The workflow this exists for: a turn ballooned, and you want the 38k-token
    /// garbage tool result responsible, by name, in one glance.
    pub fn largest_contributors(&self, limit: usize) -> Vec<Contributor> {
        self.filtered(&ItemFilter::ALL).largest_contributors(limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::identity::FileId;

    fn item(label: &str, category: ContextCategory, tokens: u32) -> ContextItem {
        ContextItem {
            id: ContextItemId::new(label),
            category,
            label: label.into(),
            source: ContextSource::Unknown,
            tokens: TokenCount::calibrated(tokens, tokens),
            first_seen_turn: None,
            provenance: Provenance::observed(SourceRef::new(FileId(0), 0, 0, 1)),
            preview: None,
        }
    }

    fn assemble(items: Vec<ContextItem>, total: u32, residual: u32) -> Result<ContextSnapshot, SnapshotError> {
        ContextSnapshot::assemble(
            SessionId::new("s1").unwrap(),
            AgentKind::ClaudeCode,
            TurnNumber::FIRST,
            Some("claude-opus-4-8".into()),
            items,
            TokenCount::observed(total),
            residual,
            Some(200_000),
            None,
        )
    }

    #[test]
    fn unbalanced_snapshots_cannot_be_constructed() {
        let err = assemble(vec![item("a", ContextCategory::ToolOutputs, 100)], 500, 0)
            .expect_err("must reject a breakdown that does not add up");
        assert!(matches!(err, SnapshotError::Unbalanced { .. }));
    }

    #[test]
    fn balanced_snapshot_with_residual_is_accepted() {
        let snap = assemble(vec![item("a", ContextCategory::ToolOutputs, 400)], 500, 100).unwrap();
        assert_eq!(snap.total().tokens(), 500);
        assert_eq!(snap.residual(), 100);
    }

    #[test]
    fn residual_appears_as_an_explicit_category_row() {
        let snap = assemble(
            vec![
                item("big", ContextCategory::ToolOutputs, 600),
                item("small", ContextCategory::UserMessages, 200),
            ],
            1000,
            200,
        )
        .unwrap();

        let rows = snap.by_category();
        assert_eq!(rows[0].category, ContextCategory::ToolOutputs);
        assert_eq!(rows[0].tokens, 600);
        assert!((rows[0].share - 0.6).abs() < f32::EPSILON);

        let unattributed = rows
            .iter()
            .find(|r| r.category == ContextCategory::Unattributed)
            .expect("residual must be shown, never hidden");
        assert_eq!(unattributed.tokens, 200);

        // Shares are shares of the observed total, so they sum to 1.
        let sum: f32 = rows.iter().map(|r| r.share).sum();
        assert!((sum - 1.0).abs() < 1e-5, "category shares must sum to 1, got {sum}");
    }

    #[test]
    fn largest_contributors_ranks_by_size() {
        let snap = assemble(
            vec![
                item("npm test output", ContextCategory::ToolOutputs, 520),
                item("schema.ts", ContextCategory::FileContents, 300),
                item("prompt", ContextCategory::CurrentPrompt, 180),
            ],
            1000,
            0,
        )
        .unwrap();

        let top = snap.largest_contributors(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].label, "npm test output");
        assert_eq!(top[1].label, "schema.ts");
        assert!((top[0].share - 0.52).abs() < 1e-6);
    }

    #[test]
    fn one_item_looked_up_by_id_reports_its_share_of_the_whole_turn() {
        // `ct trace` prints this figure for an item the user just read in
        // `ct largest`. Computing it against anything but the turn's total
        // would make the two views disagree about the same item.
        let snap = assemble(
            vec![
                item("npm test output", ContextCategory::ToolOutputs, 520),
                item("schema.ts", ContextCategory::FileContents, 300),
            ],
            1000,
            180,
        )
        .unwrap();

        let row = snap
            .contributor(&ContextItemId::new("schema.ts"))
            .expect("an item present in the turn must be found by id");
        assert_eq!(row.tokens, 300);
        assert!((row.share - 0.3).abs() < 1e-6, "share was {}", row.share);
        assert!(snap.contributor(&ContextItemId::new("absent")).is_none());
    }

    #[test]
    fn utilisation_uses_the_observed_total() {
        let snap = assemble(vec![item("a", ContextCategory::ToolOutputs, 100_000)], 100_000, 0).unwrap();
        assert_eq!(snap.utilisation(), Some(0.5));
    }
}
