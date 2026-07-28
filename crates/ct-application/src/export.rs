//! NDJSON export: the whole session as one record per line.
//!
//! This exists so ContextTrace's analysis is reachable from `jq`, `duckdb` and
//! whatever a user writes themselves, without a database driver in the
//! dependency tree. It replaces the dropped DuckDB export (CT-032) and gets
//! most of its value: `duckdb` reads NDJSON natively, so
//! `SELECT sum(tokens) FROM 'session.ndjson' WHERE type = 'item'` works the same
//! way, and nothing new is linked into a binary whose whole privacy claim rests
//! on being auditable.
//!
//! # The one thing an export can get catastrophically wrong
//!
//! Emitting items only. A consumer's first query is
//! `SELECT sum(tokens) GROUP BY turn`, and if the residual is missing that sum
//! silently disagrees with the prompt size the agent reported -- in the worst
//! local case by half the context. The whole tool exists to stop numbers being
//! read as more complete than they are, so an export that invites the mistake
//! would undo it.
//!
//! So the residual is emitted **as an item row**, in the category the terminal
//! views print it under, and the turn row *also* carries the totals. The naive
//! query is correct, and the two ways of asking cross-check each other.
//!
//! # What is deliberately not here
//!
//! *Content.* Item labels carry file paths, shell commands and search queries,
//! because a size with no name is not analysable. Message and tool-output
//! previews are excluded: an export is a file that leaves the agent's own
//! directory, the brief asks for care exactly there, and redaction is not built
//! yet (CT-025). Structure and sizes are what "analytical value" means here.
//!
//! *Exact counting.* `--exact` is a per-turn opt-in; this sweeps every turn, so
//! the cost would multiply by turn count. Item sizes are the calibrated
//! estimates, and every one of them carries its confidence.

use crate::AppError;
use ct_domain::ports::TokenEstimator;
use ct_domain::{AgentSession, ContextCategory, ContextSnapshot, TokenCount};
use serde::Serialize;

/// The record shape this export promises.
///
/// Bumped only when a consumer's query could break. Emitted on the header line
/// so a script can refuse a file it does not understand instead of
/// misinterpreting one.
pub const SCHEMA_VERSION: u32 = 1;

/// One NDJSON line.
///
/// Externally tagged on `type`, so a consumer filters with
/// `WHERE type = 'item'` and every record is self-describing without reference
/// to its position in the stream.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExportRecord<'a> {
    /// One header line, first.
    Session {
        schema: u32,
        id: &'a str,
        agent: &'a str,
        path: &'a str,
        project: Option<&'a str>,
        model: Option<&'a str>,
        turns: usize,
        events: usize,
        /// Which estimator produced every item size in this file.
        ///
        /// Not decoration. For Claude Code the CLI fits a characters-per-token
        /// ratio to the session itself (CT-014), and the flat default differs
        /// from it enough to move a turn's unattributed remainder by 82% on a
        /// real session. A file whose numbers cannot be reproduced is a file
        /// whose numbers cannot be trusted, so it names its instrument.
        estimator: &'a str,
        /// Fraction of events mapped to domain concepts. A file exported from a
        /// session this build only partly understood must say so, or its sums
        /// will be read as complete.
        fidelity: f32,
    },

    /// One per turn, carrying the figures every item row must add up to.
    Turn {
        turn: u32,
        model: Option<&'a str>,
        /// The prompt size, and how it was arrived at.
        total_tokens: u32,
        total_confidence: &'static str,
        /// Tokens attributed to items, and the remainder that is not.
        accounted_tokens: u32,
        residual_tokens: u32,
        context_window: Option<u32>,
        items: usize,
        /// The factor estimates were scaled by to meet the observed total.
        ///
        /// Present only when scaling happened. Well below 1 means
        /// reconstruction accounted for far more than the prompt held, so the
        /// item rows for this turn are proportions rather than an inventory.
        calibration_scale: Option<f32>,
        /// Whether a compaction preceded this turn.
        after_compaction: bool,
    },

    /// One per context item per turn, plus one residual row per turn.
    Item {
        turn: u32,
        /// `None` for the residual row, which has no line in any file.
        id: Option<&'a str>,
        category: String,
        label: &'a str,
        /// Where it came from, or `None` for the residual.
        source: Option<&'a ct_domain::ContextSource>,
        tokens: u32,
        confidence: &'static str,
        /// The pre-scaling figure, where the count was calibrated.
        raw_estimate: Option<u32>,
        /// The turn this item was first written in, which is not always the
        /// turn it first entered a request.
        first_seen_turn: Option<u32>,
        /// Line in the session file, for joining an export back to its source.
        line_no: Option<u32>,
    },
}

/// Name a confidence for export without going through a terminal tag.
///
/// Deliberately not `confidence_tag`'s bracketed form: an export is read by
/// machines, and `[estimated]` would make every consumer strip brackets.
fn confidence_name(c: ct_domain::Confidence) -> &'static str {
    match c {
        ct_domain::Confidence::Observed => "observed",
        ct_domain::Confidence::Derived => "derived",
        ct_domain::Confidence::Estimated => "estimated",
    }
}

fn raw_estimate_of(tokens: TokenCount) -> Option<u32> {
    match tokens {
        TokenCount::Calibrated { raw_estimate, .. } => Some(raw_estimate),
        _ => None,
    }
}

impl super::ContextTrace {
    /// Stream a session as NDJSON records.
    ///
    /// Records are handed to `emit` as they are produced rather than collected,
    /// so memory stays proportional to one turn instead of to the session. The
    /// largest local session would otherwise materialise a few hundred thousand
    /// records before writing a byte.
    ///
    /// The sink stays with the caller for the usual reason: which bytes go
    /// where is presentation, and this crate names no writer.
    ///
    /// A turn that cannot be reconstructed is skipped rather than fatal. One
    /// unreadable turn must not cost the export of the other four hundred, and
    /// the turn rows that *are* present say which those are.
    /// `estimator` must be the same one the terminal views use for this
    /// session, or `ct export` and `ct context` will report different sizes for
    /// the same turn with nothing to explain the difference.
    pub fn export_ndjson(
        &self,
        session: &AgentSession,
        resolved_path: &str,
        binding: usize,
        estimator: &dyn TokenEstimator,
        mut emit: impl FnMut(&ExportRecord<'_>) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let meta = session.metadata();
        emit(&ExportRecord::Session {
            schema: SCHEMA_VERSION,
            id: session.id().as_str(),
            agent: session.agent().label(),
            path: resolved_path,
            project: meta.project.as_deref(),
            model: meta.model.as_deref(),
            turns: session.turn_count(),
            events: session.events().len(),
            estimator: estimator.name(),
            fidelity: session.fidelity(),
        })?;

        for turn in session.turns() {
            let Ok(snapshot) = self.snapshot_with(session, binding, turn.number, estimator) else {
                continue;
            };
            emit_turn(&snapshot, &mut emit)?;
        }

        Ok(())
    }
}

fn emit_turn(
    snapshot: &ContextSnapshot,
    emit: &mut impl FnMut(&ExportRecord<'_>) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let turn = snapshot.turn().get();
    let accounted: u32 = snapshot
        .items()
        .iter()
        .map(|i| i.tokens.tokens())
        .fold(0u32, u32::saturating_add);

    emit(&ExportRecord::Turn {
        turn,
        model: snapshot.model(),
        total_tokens: snapshot.total().tokens(),
        total_confidence: confidence_name(snapshot.total().confidence()),
        accounted_tokens: accounted,
        residual_tokens: snapshot.residual(),
        context_window: snapshot.context_window(),
        items: snapshot.items().len(),
        calibration_scale: snapshot.calibration_scale(),
        after_compaction: snapshot.preceding_compaction().is_some(),
    })?;

    for item in snapshot.items() {
        emit(&ExportRecord::Item {
            turn,
            id: Some(item.id.as_str()),
            category: item.category.slug(),
            label: &item.label,
            source: Some(&item.source),
            tokens: item.tokens.tokens(),
            confidence: confidence_name(item.confidence()),
            raw_estimate: raw_estimate_of(item.tokens),
            first_seen_turn: item.first_seen_turn.map(|t| t.get()),
            line_no: item.provenance.source.map(|s| s.line_no),
        })?;
    }

    // The row that makes `sum(tokens) GROUP BY turn` agree with the prompt the
    // agent reported. Emitted even where it is small, because a consumer cannot
    // tell a residual that is genuinely zero from one that was left out.
    emit(&ExportRecord::Item {
        turn,
        id: None,
        category: ContextCategory::Unattributed.slug(),
        label: ContextCategory::Unattributed.label(),
        source: None,
        tokens: snapshot.residual(),
        // The remainder is a fitted difference, never a measurement, whatever
        // the confidence of the figures either side of it.
        confidence: confidence_name(ct_domain::Confidence::Derived),
        raw_estimate: None,
        first_seen_turn: None,
        line_no: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::model::provenance::{Provenance, SourceRef};
    use ct_domain::{
        AgentKind, ContextItem, ContextItemId, ContextSource, FileId, SessionId, TurnNumber,
    };
    use serde_json::Value;

    fn item(label: &str, tokens: TokenCount) -> ContextItem {
        ContextItem {
            id: ContextItemId::new(label),
            category: ContextCategory::ToolOutputs,
            label: label.into(),
            source: ContextSource::Unknown,
            tokens,
            first_seen_turn: Some(TurnNumber::FIRST),
            provenance: Provenance::observed(SourceRef::new(FileId(0), 0, 0, 7)),
            preview: Some("this must not reach the export".into()),
        }
    }

    /// Collect one turn's records as parsed JSON, exactly as a consumer sees
    /// them: through serialization, not through the enum.
    fn records(items: Vec<ContextItem>, total: u32, residual: u32) -> Vec<Value> {
        let snapshot = ContextSnapshot::assemble(
            SessionId::new("s").unwrap(),
            AgentKind::Codex,
            TurnNumber::FIRST,
            Some("gpt-5".into()),
            items,
            TokenCount::observed(total),
            residual,
            Some(258_400),
            None,
        )
        .expect("the snapshot must balance");

        let mut out = Vec::new();
        emit_turn(&snapshot, &mut |record| {
            out.push(serde_json::to_value(record).unwrap());
            Ok(())
        })
        .unwrap();
        out
    }

    fn items_of(records: &[Value]) -> Vec<&Value> {
        records
            .iter()
            .filter(|r| r["type"] == "item")
            .collect()
    }

    #[test]
    fn item_rows_sum_to_the_turn_total() {
        // The property the whole record shape exists for. A consumer's first
        // query is `sum(tokens) GROUP BY turn`, and if it disagrees with the
        // prompt the agent reported, the export has taught them something
        // false.
        let rows = records(
            vec![
                item("a", TokenCount::estimated(200)),
                item("b", TokenCount::estimated(100)),
            ],
            1000,
            700,
        );

        let summed: u64 = items_of(&rows)
            .iter()
            .map(|r| r["tokens"].as_u64().unwrap())
            .sum();
        assert_eq!(summed, 1000);

        let turn = rows.iter().find(|r| r["type"] == "turn").unwrap();
        assert_eq!(turn["total_tokens"], 1000);
        assert_eq!(turn["accounted_tokens"], 300);
        assert_eq!(turn["residual_tokens"], 700);
    }

    #[test]
    fn the_residual_row_is_emitted_even_when_it_is_zero() {
        // A consumer cannot tell a residual that is genuinely nothing from one
        // that was left out, so it is never left out.
        let rows = records(vec![item("a", TokenCount::estimated(1000))], 1000, 0);
        let residual = items_of(&rows)
            .into_iter()
            .find(|r| r["category"] == "unattributed")
            .expect("the residual row must always be present");
        assert_eq!(residual["tokens"], 0);
        assert!(residual["id"].is_null(), "it has no line in any file");
        assert!(residual["source"].is_null());
    }

    #[test]
    fn every_token_figure_carries_its_confidence() {
        // An export that drops provenance launders a guess into a measurement
        // one `SELECT` later, which is the failure the type system exists to
        // make impossible in this codebase.
        let rows = records(
            vec![
                item("guess", TokenCount::estimated(300)),
                item("scaled", TokenCount::calibrated(200, 900)),
            ],
            1000,
            500,
        );

        for row in items_of(&rows) {
            assert!(
                row["confidence"].is_string(),
                "no row may report a size without saying how it was arrived at: {row}"
            );
        }
        let scaled = items_of(&rows)
            .into_iter()
            .find(|r| r["label"] == "scaled")
            .unwrap();
        assert_eq!(
            scaled["raw_estimate"], 900,
            "the pre-scaling figure must survive into the export"
        );
    }

    #[test]
    fn previews_never_reach_the_export() {
        // An export is a file that leaves the agent's directory, and redaction
        // is not built yet (CT-025). Labels carry paths and commands because a
        // size with no name is not analysable; conversation content does not.
        let rows = records(vec![item("a", TokenCount::estimated(1000))], 1000, 0);
        let text = serde_json::to_string(&rows).unwrap();
        assert!(
            !text.contains("must not reach the export"),
            "no message or tool-output preview may be serialised"
        );
    }
}
