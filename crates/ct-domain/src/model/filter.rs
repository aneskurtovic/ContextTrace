//! Selecting part of a context snapshot without lying about the whole.
//!
//! The workflow this exists for is the one in the brief: a turn ballooned, and
//! the 38k-token garbage tool result responsible has to be found among two
//! hundred items. Narrowing by provenance and size is how you find it.
//!
//! # The honesty problem filtering creates
//!
//! Every share in this crate is a share of the *observed* turn total -- a number
//! the agent itself reported. The moment a subset is displayed, there is an
//! obvious and wrong thing to do: recompute the percentages against the subset,
//! so that four tool outputs "account for 100% of the context". They do not.
//! They account for 31% of it, and the other 69% is exactly what the person
//! filtering needs to keep in view.
//!
//! [`FilteredView`] answers that structurally. It borrows the snapshot rather
//! than owning a copy of the matching items, so `total()` is always reached
//! through the aggregate and there is no subset sum available to divide by. The
//! wrong version is not merely discouraged; it cannot be written without adding
//! a field.

use super::context::{
    CategoryBreakdown, ContextCategory, ContextItem, ContextSnapshot, ContextSource, Contributor,
};
use super::identity::{SessionId, TurnNumber};
use super::provenance::Confidence;
use super::session::AgentKind;
use super::tokens::TokenCount;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Which [`ContextSource`] variant to match, and optionally what it must say.
///
/// The payload half is what makes this useful rather than decorative:
/// `tool:Bash` and `file:schema.ts` are the queries someone actually types.
/// Matching is a case-insensitive substring so a partial path works.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePattern {
    pub kind: SourceKind,
    /// Substring the variant's payload must contain. `None` matches any payload.
    pub detail: Option<String>,
}

/// The discriminant half of a [`SourcePattern`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    SystemPrompt,
    InstructionFile,
    ProjectConfig,
    Harness,
    User,
    Tool,
    File,
    CompactionSummary,
    Model,
    Unknown,
}

impl SourceKind {
    pub fn slug(self) -> &'static str {
        match self {
            SourceKind::SystemPrompt => "system-prompt",
            SourceKind::InstructionFile => "instruction-file",
            SourceKind::ProjectConfig => "project-config",
            SourceKind::Harness => "harness",
            SourceKind::User => "user",
            SourceKind::Tool => "tool",
            SourceKind::File => "file",
            SourceKind::CompactionSummary => "compaction-summary",
            SourceKind::Model => "model",
            SourceKind::Unknown => "unknown",
        }
    }

    pub const ALL: [SourceKind; 10] = [
        SourceKind::SystemPrompt,
        SourceKind::InstructionFile,
        SourceKind::ProjectConfig,
        SourceKind::Harness,
        SourceKind::User,
        SourceKind::Tool,
        SourceKind::File,
        SourceKind::CompactionSummary,
        SourceKind::Model,
        SourceKind::Unknown,
    ];

    pub fn parse(s: &str) -> Option<SourceKind> {
        let norm = s.trim().to_ascii_lowercase().replace('_', "-");
        SourceKind::ALL.into_iter().find(|k| k.slug() == norm)
    }

    /// The kind a concrete source belongs to.
    pub fn of(source: &ContextSource) -> SourceKind {
        match source {
            ContextSource::AgentSystemPrompt => SourceKind::SystemPrompt,
            ContextSource::InstructionFile { .. } => SourceKind::InstructionFile,
            ContextSource::ProjectConfig { .. } => SourceKind::ProjectConfig,
            ContextSource::HarnessInjection { .. } => SourceKind::Harness,
            ContextSource::UserPrompt => SourceKind::User,
            ContextSource::ToolExecution { .. } => SourceKind::Tool,
            ContextSource::FileRead { .. } => SourceKind::File,
            ContextSource::CompactionSummary => SourceKind::CompactionSummary,
            ContextSource::ModelOutput => SourceKind::Model,
            ContextSource::Unknown => SourceKind::Unknown,
        }
    }
}

/// The free text a source carries, if any -- a tool name, a path, a mechanism.
fn payload_of(source: &ContextSource) -> Option<&str> {
    match source {
        ContextSource::InstructionFile { path } | ContextSource::FileRead { path } => Some(path),
        ContextSource::ProjectConfig { path } => path.as_deref(),
        ContextSource::HarnessInjection { mechanism } => Some(mechanism),
        ContextSource::ToolExecution { tool } => Some(tool),
        _ => None,
    }
}

impl SourcePattern {
    /// Parse `kind` or `kind:substring`, e.g. `tool` or `tool:Bash`.
    pub fn parse(s: &str) -> Result<SourcePattern, FilterParseError> {
        let (kind_text, detail) = match s.split_once(':') {
            Some((k, d)) if !d.trim().is_empty() => (k, Some(d.trim().to_string())),
            Some((k, _)) => (k, None),
            None => (s, None),
        };
        let kind = SourceKind::parse(kind_text).ok_or_else(|| FilterParseError {
            field: "source",
            value: kind_text.trim().to_string(),
            allowed: SourceKind::ALL.iter().map(|k| k.slug()).collect(),
        })?;
        Ok(SourcePattern { kind, detail })
    }

    pub fn matches(&self, source: &ContextSource) -> bool {
        if SourceKind::of(source) != self.kind {
            return false;
        }
        match &self.detail {
            None => true,
            Some(needle) => payload_of(source)
                .is_some_and(|p| p.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())),
        }
    }
}

impl fmt::Display for SourcePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.detail {
            Some(d) => write!(f, "{}:{d}", self.kind.slug()),
            None => f.write_str(self.kind.slug()),
        }
    }
}

/// A value the user typed that names nothing.
///
/// Carries the permitted values so the CLI can print them rather than making
/// the user guess at spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterParseError {
    pub field: &'static str,
    pub value: String,
    pub allowed: Vec<&'static str>,
}

impl fmt::Display for FilterParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown --{} '{}'; expected one of: {}",
            self.field,
            self.value,
            self.allowed.join(", ")
        )
    }
}

impl std::error::Error for FilterParseError {}

/// Which context items to show. Every field is a conjunction; `None` is "any".
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ItemFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourcePattern>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<ContextCategory>,
    /// Show only items *at least* this trustworthy.
    ///
    /// Directional on purpose. `Confidence` orders weakest-last, so asking for
    /// `derived` admits observed items too: someone excluding guesses wants
    /// everything better than the bar, not exactly the bar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<Confidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_tokens: Option<u32>,
}

impl ItemFilter {
    /// Matches everything. The unfiltered views are this case, not a separate
    /// code path -- one implementation of the denominator, always.
    pub const ALL: ItemFilter = ItemFilter {
        source: None,
        category: None,
        min_confidence: None,
        min_tokens: None,
    };

    pub fn is_active(&self) -> bool {
        *self != ItemFilter::ALL
    }

    pub fn matches(&self, item: &ContextItem) -> bool {
        if let Some(pattern) = &self.source {
            if !pattern.matches(&item.source) {
                return false;
            }
        }
        if let Some(category) = self.category {
            if item.category != category {
                return false;
            }
        }
        if let Some(min) = self.min_confidence {
            // Weakest-last ordering: "at least as trustworthy as" is `<=`.
            if item.confidence() > min {
                return false;
            }
        }
        if let Some(min) = self.min_tokens {
            if item.tokens.tokens() < min {
                return false;
            }
        }
        true
    }
}

impl fmt::Display for ItemFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if let Some(s) = &self.source {
            parts.push(format!("source={s}"));
        }
        if let Some(c) = self.category {
            parts.push(format!("category={}", c.slug()));
        }
        if let Some(c) = self.min_confidence {
            parts.push(format!("confidence={c}"));
        }
        if let Some(t) = self.min_tokens {
            parts.push(format!("min-tokens={t}"));
        }
        if parts.is_empty() {
            f.write_str("none")
        } else {
            f.write_str(&parts.join(", "))
        }
    }
}

/// Part of a snapshot, still measured against the whole of it.
///
/// Borrows rather than owns: see the module docs for why that is the point
/// rather than an optimisation.
pub struct FilteredView<'a> {
    snapshot: &'a ContextSnapshot,
    filter: &'a ItemFilter,
    matched: Vec<&'a ContextItem>,
    residual_included: bool,
}

impl<'a> FilteredView<'a> {
    pub(super) fn new(snapshot: &'a ContextSnapshot, filter: &'a ItemFilter) -> Self {
        let matched: Vec<&ContextItem> = snapshot
            .items()
            .iter()
            .filter(|i| filter.matches(i))
            .collect();
        Self {
            residual_included: residual_survives(snapshot, filter),
            snapshot,
            filter,
            matched,
        }
    }

    pub fn snapshot(&self) -> &'a ContextSnapshot {
        self.snapshot
    }
    pub fn filter(&self) -> &ItemFilter {
        self.filter
    }
    pub fn matched_items(&self) -> usize {
        self.matched.len()
    }
    pub fn total_items(&self) -> usize {
        self.snapshot.items().len()
    }
    /// Whether the unattributed remainder is part of this view.
    pub fn residual_included(&self) -> bool {
        self.residual_included
    }

    /// Tokens covered by this view, including the residual when it survives.
    pub fn matched_tokens(&self) -> u32 {
        let items: u64 = self.matched.iter().map(|i| i.tokens.tokens() as u64).sum();
        let residual = if self.residual_included {
            self.snapshot.residual() as u64
        } else {
            0
        };
        (items + residual).min(u32::MAX as u64) as u32
    }

    /// **The turn's total, always -- never the subset's.**
    pub fn total(&self) -> TokenCount {
        self.snapshot.total()
    }

    /// What fraction of the turn this view covers.
    pub fn share_of_total(&self) -> f32 {
        self.matched_tokens() as f32 / self.denominator()
    }

    fn denominator(&self) -> f32 {
        self.snapshot.total().tokens().max(1) as f32
    }

    /// Composition by category, largest first.
    pub fn by_category(&self) -> Vec<CategoryBreakdown> {
        let mut acc: BTreeMap<ContextCategory, (u32, usize, Confidence)> = BTreeMap::new();

        for item in &self.matched {
            let entry = acc
                .entry(item.category)
                .or_insert((0, 0, Confidence::Observed));
            entry.0 = entry.0.saturating_add(item.tokens.tokens());
            entry.1 += 1;
            entry.2 = entry.2.weakest(item.confidence());
        }

        if self.residual_included && self.snapshot.residual() > 0 {
            acc.insert(
                ContextCategory::Unattributed,
                (self.snapshot.residual(), 1, Confidence::Derived),
            );
        }

        let total = self.denominator();
        let mut rows: Vec<CategoryBreakdown> = acc
            .into_iter()
            .map(|(category, (tokens, item_count, confidence))| CategoryBreakdown {
                category,
                tokens,
                share: tokens as f32 / total,
                item_count,
                confidence,
            })
            .collect();

        rows.sort_by(|a, b| b.tokens.cmp(&a.tokens).then(a.category.cmp(&b.category)));
        rows
    }

    /// The `limit` biggest matching items.
    ///
    /// Filtering happens before the limit, so `--min-tokens 5000 --limit 15`
    /// yields fifteen matches rather than whatever survives the top fifteen.
    pub fn largest_contributors(&self, limit: usize) -> Vec<Contributor> {
        let total = self.denominator();
        let mut items = self.matched.clone();
        items.sort_by(|a, b| b.tokens.tokens().cmp(&a.tokens.tokens()));
        items
            .into_iter()
            .take(limit)
            .map(|i| Contributor {
                id: i.id.clone(),
                label: i.label.clone(),
                category: i.category,
                source: i.source.clone(),
                tokens: i.tokens.tokens(),
                share: i.tokens.tokens() as f32 / total,
                confidence: i.confidence(),
            })
            .collect()
    }

    /// Distinct categories present in the *unfiltered* snapshot, with sizes.
    ///
    /// For the empty-result case: telling someone their filter matched nothing
    /// is half an answer, and the other half is what they could have asked for.
    pub fn available_categories(&self) -> Vec<(ContextCategory, u32)> {
        let mut acc: BTreeMap<ContextCategory, u32> = BTreeMap::new();
        for item in self.snapshot.items() {
            *acc.entry(item.category).or_insert(0) += item.tokens.tokens();
        }
        let mut rows: Vec<_> = acc.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        rows
    }

    /// Distinct source kinds present in the unfiltered snapshot, with sizes.
    pub fn available_sources(&self) -> Vec<(SourceKind, u32)> {
        let mut acc: BTreeMap<&'static str, (SourceKind, u32)> = BTreeMap::new();
        for item in self.snapshot.items() {
            let kind = SourceKind::of(&item.source);
            let entry = acc.entry(kind.slug()).or_insert((kind, 0));
            entry.1 += item.tokens.tokens();
        }
        let mut rows: Vec<_> = acc.into_values().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        rows
    }

    /// Distinct confidences present in the unfiltered snapshot.
    ///
    /// Worth surfacing because per-item sizes are currently `Estimated` for both
    /// agents, so `--confidence observed` legitimately matches nothing and the
    /// user deserves to be told that rather than left suspecting a broken flag.
    pub fn available_confidences(&self) -> Vec<Confidence> {
        let mut seen: Vec<Confidence> = Vec::new();
        for item in self.snapshot.items() {
            let c = item.confidence();
            if !seen.contains(&c) {
                seen.push(c);
            }
        }
        seen.sort();
        seen
    }

    /// The header shared by every serialised view.
    fn header(&self) -> ViewHeader {
        ViewHeader {
            session_id: self.snapshot.session_id().clone(),
            agent: self.snapshot.agent(),
            turn: self.snapshot.turn(),
            model: self.snapshot.model().map(str::to_string),
            total: self.snapshot.total(),
            filter: self.filter.is_active().then(|| self.filter.clone()),
            matched_tokens: self.matched_tokens(),
            matched_items: self.matched_items(),
            total_items: self.total_items(),
            share_of_total: self.share_of_total(),
            residual: self.snapshot.residual(),
            residual_included: self.residual_included,
        }
    }

    pub fn composition_report(&self) -> CompositionReport {
        CompositionReport {
            header: self.header(),
            categories: self.by_category(),
        }
    }

    pub fn contributor_report(&self, limit: usize) -> ContributorReport {
        ContributorReport {
            header: self.header(),
            items: self.largest_contributors(limit),
        }
    }
}

/// Whether the unattributed remainder belongs in a filtered view.
///
/// The residual is not a [`ContextItem`]: it has no source, no line in any file,
/// and exists only because the totals demand it. So a filter *by provenance*
/// cannot honestly admit it -- there is nothing to match against. The rule is
/// therefore: it appears unfiltered, or when named directly by category, and
/// otherwise it is out. Including it anyway would inflate every filtered figure
/// by the one quantity that belongs to no filter.
fn residual_survives(snapshot: &ContextSnapshot, filter: &ItemFilter) -> bool {
    if !filter.is_active() {
        return true;
    }
    if filter.category != Some(ContextCategory::Unattributed) {
        return false;
    }
    if filter.source.is_some() {
        return false;
    }
    if filter
        .min_confidence
        .is_some_and(|min| Confidence::Derived > min)
    {
        return false;
    }
    filter
        .min_tokens
        .is_none_or(|min| snapshot.residual() >= min)
}

/// Facts every filtered view reports about itself, whatever it is showing.
#[derive(Debug, Clone, Serialize)]
pub struct ViewHeader {
    pub session_id: SessionId,
    pub agent: AgentKind,
    pub turn: TurnNumber,
    pub model: Option<String>,
    /// The turn's total. Shares are fractions of this, filtered or not.
    pub total: TokenCount,
    /// `None` when nothing was filtered.
    pub filter: Option<ItemFilter>,
    pub matched_tokens: u32,
    pub matched_items: usize,
    pub total_items: usize,
    pub share_of_total: f32,
    pub residual: u32,
    pub residual_included: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompositionReport {
    #[serde(flatten)]
    pub header: ViewHeader,
    pub categories: Vec<CategoryBreakdown>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContributorReport {
    #[serde(flatten)]
    pub header: ViewHeader,
    pub items: Vec<Contributor>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::identity::{ContextItemId, FileId};
    use crate::model::provenance::{Provenance, SourceRef};
    use crate::model::session::AgentKind;

    fn item(label: &str, category: ContextCategory, source: ContextSource, tokens: u32) -> ContextItem {
        ContextItem {
            id: ContextItemId::new(label),
            category,
            label: label.into(),
            source,
            tokens: TokenCount::calibrated(tokens, tokens),
            first_seen_turn: None,
            provenance: Provenance::observed(SourceRef::new(FileId(0), 0, 0, 1)),
            preview: None,
        }
    }

    /// 1,000 tokens: 600 tool output, 200 file, 100 user, plus a 100 residual.
    fn snapshot() -> ContextSnapshot {
        ContextSnapshot::assemble(
            SessionId::new("s1").unwrap(),
            AgentKind::ClaudeCode,
            TurnNumber::FIRST,
            None,
            vec![
                item(
                    "npm test output",
                    ContextCategory::ToolOutputs,
                    ContextSource::ToolExecution { tool: "Bash".into() },
                    600,
                ),
                item(
                    "schema.ts",
                    ContextCategory::FileContents,
                    ContextSource::FileRead {
                        path: "src/schema.ts".into(),
                    },
                    200,
                ),
                item(
                    "prompt",
                    ContextCategory::CurrentPrompt,
                    ContextSource::UserPrompt,
                    100,
                ),
            ],
            TokenCount::observed(1000),
            100,
            None,
            None,
        )
        .unwrap()
    }

    fn filtered(filter: ItemFilter) -> (ContextSnapshot, ItemFilter) {
        (snapshot(), filter)
    }

    #[test]
    fn a_filtered_view_covers_less_than_the_whole_turn() {
        // The clause this feature turns on: filtered rows still state what share
        // of the *whole* they represent. The unfiltered snapshot's shares sum to
        // 1.0; a filtered view's must sum to less, against the same denominator.
        let (snap, f) = filtered(ItemFilter {
            category: Some(ContextCategory::ToolOutputs),
            ..ItemFilter::ALL
        });
        let view = snap.filtered(&f);

        let sum: f32 = view.by_category().iter().map(|r| r.share).sum();
        assert!((sum - 0.6).abs() < 1e-5, "shares must stay shares of the turn, got {sum}");
        assert_eq!(view.matched_tokens(), 600);
        assert_eq!(view.total().tokens(), 1000, "the denominator is the turn, not the subset");
        assert!((view.share_of_total() - 0.6).abs() < 1e-5);
    }

    #[test]
    fn the_unfiltered_view_is_the_same_code_path_and_still_sums_to_one() {
        let snap = snapshot();
        let view = snap.filtered(&ItemFilter::ALL);
        let sum: f32 = view.by_category().iter().map(|r| r.share).sum();
        assert!((sum - 1.0).abs() < 1e-5, "unfiltered shares must sum to 1, got {sum}");
        assert_eq!(view.matched_tokens(), 1000);
    }

    #[test]
    fn the_residual_is_excluded_from_a_provenance_filter() {
        // It has no source and no line in any file, so nothing about it can
        // match a provenance query. Including it would inflate the figure.
        let (snap, f) = filtered(ItemFilter {
            source: Some(SourcePattern {
                kind: SourceKind::Tool,
                detail: None,
            }),
            ..ItemFilter::ALL
        });
        let view = snap.filtered(&f);
        assert!(!view.residual_included());
        assert_eq!(view.matched_tokens(), 600, "the 100-token residual must not be counted");
    }

    #[test]
    fn the_residual_can_be_asked_for_by_name() {
        let (snap, f) = filtered(ItemFilter {
            category: Some(ContextCategory::Unattributed),
            ..ItemFilter::ALL
        });
        let view = snap.filtered(&f);
        assert!(view.residual_included());
        assert_eq!(view.matched_tokens(), 100);
        assert_eq!(view.by_category().len(), 1);
    }

    #[test]
    fn source_patterns_match_the_payload_not_just_the_variant() {
        let snap = snapshot();

        let bash = ItemFilter {
            source: Some(SourcePattern::parse("tool:bash").unwrap()),
            ..ItemFilter::ALL
        };
        assert_eq!(snap.filtered(&bash).matched_items(), 1);

        let other = ItemFilter {
            source: Some(SourcePattern::parse("tool:Grep").unwrap()),
            ..ItemFilter::ALL
        };
        assert_eq!(snap.filtered(&other).matched_items(), 0);

        let by_path = ItemFilter {
            source: Some(SourcePattern::parse("file:schema").unwrap()),
            ..ItemFilter::ALL
        };
        assert_eq!(snap.filtered(&by_path).matched_items(), 1);
    }

    #[test]
    fn min_tokens_filters_before_the_limit_is_applied() {
        // The bug this guards: filtering after `take(limit)` silently drops
        // matches that were ranked below the cut.
        let snap = snapshot();
        let f = ItemFilter {
            min_tokens: Some(150),
            ..ItemFilter::ALL
        };
        let view = snap.filtered(&f);
        assert_eq!(view.largest_contributors(10).len(), 2);
        assert_eq!(view.largest_contributors(1).len(), 1);
    }

    #[test]
    fn confidence_is_a_floor_not_an_exact_match() {
        let snap = snapshot();
        // Items here are calibrated, so estimated. Asking for "estimated or
        // better" admits them; asking for "observed only" does not.
        let loose = ItemFilter {
            min_confidence: Some(Confidence::Estimated),
            ..ItemFilter::ALL
        };
        assert_eq!(snap.filtered(&loose).matched_items(), 3);

        let strict = ItemFilter {
            min_confidence: Some(Confidence::Observed),
            ..ItemFilter::ALL
        };
        assert_eq!(snap.filtered(&strict).matched_items(), 0);
    }

    #[test]
    fn an_empty_result_can_still_say_what_was_available() {
        let snap = snapshot();
        let f = ItemFilter {
            source: Some(SourcePattern::parse("instruction-file").unwrap()),
            ..ItemFilter::ALL
        };
        let view = snap.filtered(&f);
        assert_eq!(view.matched_items(), 0);
        assert_eq!(view.share_of_total(), 0.0);

        let sources: Vec<&str> = view
            .available_sources()
            .into_iter()
            .map(|(k, _)| k.slug())
            .collect();
        assert_eq!(sources, vec!["tool", "file", "user"]);
        assert_eq!(view.available_confidences(), vec![Confidence::Estimated]);
    }

    #[test]
    fn unparseable_values_name_what_was_allowed() {
        let err = SourcePattern::parse("tolo:Bash").expect_err("must reject a misspelling");
        assert_eq!(err.field, "source");
        assert!(err.to_string().contains("compaction-summary"));
    }

    #[test]
    fn a_filter_that_narrows_nothing_is_not_active() {
        assert!(!ItemFilter::ALL.is_active());
        assert!(ItemFilter {
            min_tokens: Some(1),
            ..ItemFilter::ALL
        }
        .is_active());
    }
}
