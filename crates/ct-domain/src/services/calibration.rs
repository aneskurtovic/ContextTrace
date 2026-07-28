//! Token calibration: reconciling what we guessed with what the agent measured.
//!
//! This is the domain service that turns a pile of adapter estimates into a
//! [`ContextSnapshot`] whose parts sum to a number the agent itself reported.

use crate::model::context::{ContextItem, ContextSnapshot, SnapshotError};
use crate::model::identity::{SessionId, TurnNumber};
use crate::model::session::AgentKind;
use crate::model::tokens::TokenCount;
use crate::ports::ReconstructedContext;
use std::fmt;

/// Reconciles per-item token estimates against an observed per-turn total.
///
/// # Why this exists
///
/// For Claude Code we know a turn's prompt size *exactly* (from `usage`) but
/// cannot count any individual item exactly (Anthropic ships no local
/// tokenizer). Reporting unreconciled estimates would produce a composition
/// view whose percentages silently disagree with the headline number -- which
/// is worse than useless, because it looks authoritative.
///
/// # The rules
///
/// 1. **Measured counts are never rescaled.** Items counted with a real
///    tokenizer, or reported by the agent, keep their values. Only heuristic
///    estimates flex.
/// 2. **Estimates are scaled to fit the space measurements leave.** If they
///    overflow it, they shrink proportionally and are relabelled
///    [`TokenCount::Calibrated`].
/// 3. **Whatever is left over is named, not hidden.** The remainder becomes the
///    residual -- in practice the agent's system prompt and tool JSON schemas,
///    which no session log contains. Smearing it across the visible categories
///    would flatter the breakdown; naming it tells the truth.
pub struct TokenCalibrator;

#[derive(Debug)]
pub enum CalibrationError {
    /// The assembled snapshot failed its own balance invariant. A bug in this
    /// service, surfaced rather than swallowed.
    Unbalanced(SnapshotError),
}

impl fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalibrationError::Unbalanced(e) => write!(f, "calibration produced an unbalanced snapshot: {e}"),
        }
    }
}

impl std::error::Error for CalibrationError {}

impl TokenCalibrator {
    /// Balance `reconstructed` into a snapshot for `turn`.
    pub fn calibrate(
        reconstructed: ReconstructedContext,
        session_id: SessionId,
        agent: AgentKind,
        turn: TurnNumber,
    ) -> Result<ContextSnapshot, CalibrationError> {
        let ReconstructedContext {
            mut items,
            observed_total,
            context_window,
            model,
            preceding_compaction,
        } = reconstructed;

        let total = match observed_total {
            Some(total) => {
                Self::fit_items_to(&mut items, total.tokens());
                total
            }
            None => {
                // The agent told us nothing about this turn's size, so the only
                // available total is the sum of our own guesses. Typed as an
                // estimate so no caller mistakes it for a measurement.
                let sum: u64 = items.iter().map(|i| i.tokens.tokens() as u64).sum();
                TokenCount::estimated(sum.min(u32::MAX as u64) as u32)
            }
        };

        let attributed: u64 = items.iter().map(|i| i.tokens.tokens() as u64).sum();
        // Cannot underflow: `fit_items_to` guarantees the sum never exceeds the
        // total, and in the estimated branch the total *is* the sum.
        let residual = (total.tokens() as u64).saturating_sub(attributed) as u32;

        ContextSnapshot::assemble(
            session_id,
            agent,
            turn,
            model,
            items,
            total,
            residual,
            context_window,
            preceding_compaction,
        )
        .map_err(CalibrationError::Unbalanced)
    }

    /// Shrink items so their sum fits within `total`, preferring to shrink
    /// guesses over measurements.
    fn fit_items_to(items: &mut [ContextItem], total: u32) {
        let measured: u64 = items
            .iter()
            .filter(|i| i.tokens.is_trustworthy())
            .map(|i| i.tokens.tokens() as u64)
            .sum();
        let estimated: u64 = items
            .iter()
            .filter(|i| !i.tokens.is_trustworthy())
            .map(|i| i.tokens.tokens() as u64)
            .sum();

        if measured > total as u64 {
            // Measured counts alone exceed what the agent says it sent. Our
            // reconstruction has over-included -- e.g. it swept in items the
            // model never actually saw. Nothing here is reliable any more, so
            // scale everything and mark it all as calibrated rather than quietly
            // presenting measurements we have just disproved.
            let all: u64 = measured + estimated;
            Self::scale(items, total as u64, all, true);
            return;
        }

        let room = total as u64 - measured;
        if estimated > room {
            Self::scale(items, room, estimated, false);
        }
        // When estimates fit, they are left alone; the slack becomes residual.
    }

    /// Scale item counts by `numerator / denominator`.
    ///
    /// `include_measured` selects whether trustworthy counts are touched.
    fn scale(items: &mut [ContextItem], numerator: u64, denominator: u64, include_measured: bool) {
        if denominator == 0 {
            return;
        }
        for item in items.iter_mut() {
            if !include_measured && item.tokens.is_trustworthy() {
                continue;
            }
            let raw = item.tokens.tokens();
            let scaled = ((raw as u64 * numerator) / denominator).min(u32::MAX as u64) as u32;
            item.tokens = TokenCount::calibrated(scaled, raw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::context::{ContextCategory, ContextSource};
    use crate::model::identity::{ContextItemId, FileId};
    use crate::model::provenance::{Provenance, SourceRef};

    fn item(label: &str, tokens: TokenCount) -> ContextItem {
        ContextItem {
            id: ContextItemId::new(label),
            category: ContextCategory::ToolOutputs,
            label: label.into(),
            source: ContextSource::Unknown,
            tokens,
            first_seen_turn: None,
            provenance: Provenance::observed(SourceRef::new(FileId(0), 0, 0, 1)),
            preview: None,
            content_fingerprint: None,
        }
    }

    fn calibrate(items: Vec<ContextItem>, observed: Option<u32>) -> ContextSnapshot {
        TokenCalibrator::calibrate(
            ReconstructedContext {
                items,
                observed_total: observed.map(TokenCount::observed),
                context_window: None,
                model: None,
                preceding_compaction: None,
            },
            SessionId::new("s").unwrap(),
            AgentKind::ClaudeCode,
            TurnNumber::FIRST,
        )
        .expect("calibration must always balance")
    }

    #[test]
    fn slack_between_estimates_and_observed_total_becomes_residual() {
        // 300 estimated against an observed 1000: the missing 700 is the hidden
        // system prompt and tool schemas, and must be shown as such.
        let snap = calibrate(
            vec![
                item("a", TokenCount::estimated(200)),
                item("b", TokenCount::estimated(100)),
            ],
            Some(1000),
        );
        assert_eq!(snap.residual(), 700);
        assert_eq!(snap.items()[0].tokens.tokens(), 200, "estimates that fit are not rescaled");
    }

    #[test]
    fn overflowing_estimates_are_scaled_down_and_relabelled() {
        let snap = calibrate(
            vec![
                item("a", TokenCount::estimated(1500)),
                item("b", TokenCount::estimated(500)),
            ],
            Some(1000),
        );
        // Proportions preserved: 3:1 in, 3:1 out.
        assert_eq!(snap.items()[0].tokens.tokens(), 750);
        assert_eq!(snap.items()[1].tokens.tokens(), 250);
        assert!(matches!(
            snap.items()[0].tokens,
            TokenCount::Calibrated { raw_estimate: 1500, .. }
        ));
        assert_eq!(snap.residual(), 0);
    }

    #[test]
    fn measured_counts_survive_while_estimates_absorb_the_squeeze() {
        // 600 measured + 800 estimated against 1000: measurements keep their
        // value, estimates compress into the remaining 400.
        let snap = calibrate(
            vec![
                item("exact", TokenCount::exact(600)),
                item("guess", TokenCount::estimated(800)),
            ],
            Some(1000),
        );
        assert_eq!(snap.items()[0].tokens.tokens(), 600);
        assert!(snap.items()[0].tokens.is_trustworthy());
        assert_eq!(snap.items()[1].tokens.tokens(), 400);
        assert_eq!(snap.residual(), 0);
    }

    #[test]
    fn measurements_exceeding_the_observed_total_scale_everything() {
        // Over-inclusion: our "exact" counts claim more than the agent sent.
        let snap = calibrate(
            vec![
                item("exact-a", TokenCount::exact(800)),
                item("exact-b", TokenCount::exact(400)),
            ],
            Some(600),
        );
        assert!(
            !snap.items()[0].tokens.is_trustworthy(),
            "disproved measurements must be downgraded, not presented as fact"
        );
        assert!(snap.items().iter().map(|i| i.tokens.tokens()).sum::<u32>() <= 600);
    }

    #[test]
    fn integer_rounding_is_absorbed_by_the_residual() {
        // 3 items of 1 scaled into 2 tokens: integer division loses a token,
        // and the residual must pick it up so the snapshot still balances.
        let snap = calibrate(
            vec![
                item("a", TokenCount::estimated(1)),
                item("b", TokenCount::estimated(1)),
                item("c", TokenCount::estimated(1)),
            ],
            Some(2),
        );
        let sum: u32 = snap.items().iter().map(|i| i.tokens.tokens()).sum();
        assert_eq!(sum + snap.residual(), 2);
    }

    #[test]
    fn without_an_observed_total_the_total_is_typed_as_an_estimate() {
        let snap = calibrate(vec![item("a", TokenCount::estimated(120))], None);
        assert_eq!(snap.total().tokens(), 120);
        assert!(!snap.total().is_trustworthy());
        assert_eq!(snap.residual(), 0);
    }

    #[test]
    fn empty_context_still_balances() {
        let snap = calibrate(vec![], Some(5000));
        assert_eq!(snap.residual(), 5000);
        assert!(snap.items().is_empty());
    }
}
