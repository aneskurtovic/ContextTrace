//! Temporal context ghosts: item identity across two turns.
//!
//! This is a membership view first. Token deltas are included only with the
//! same bounded comparability rule as the ordinary turn diff, so a changed
//! estimator cannot turn a measurement caveat into a claimed content change.

use crate::diff::{compare, Comparability, Instrument, Side};
use ct_domain::{Confidence, ContextCategory, ContextSnapshot};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GhostItem {
    pub id: String,
    pub label: String,
    pub category: ContextCategory,
    pub source: String,
    pub left_tokens: Option<u32>,
    pub right_tokens: Option<u32>,
    pub token_delta: Option<i64>,
    pub meaningful_token_delta: bool,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TemporalGhostAvailable {
    pub left_turn: u32,
    pub right_turn: u32,
    pub comparability: Comparability,
    pub gained: Vec<GhostItem>,
    pub retained: Vec<GhostItem>,
    pub removed: Vec<GhostItem>,
    pub assumptions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum TemporalGhost {
    Available(TemporalGhostAvailable),
    Unavailable {
        left_turn: u32,
        right_turn: u32,
        reason: String,
    },
}

pub fn compare_turns(
    left: &ContextSnapshot,
    left_instrument: Instrument,
    right: &ContextSnapshot,
    right_instrument: Instrument,
) -> TemporalGhost {
    let diff = compare(
        Side {
            snapshot: left,
            instrument: left_instrument,
        },
        Side {
            snapshot: right,
            instrument: right_instrument,
        },
    );
    if let Comparability::Incomparable { reason, .. } = &diff.comparability {
        return TemporalGhost::Unavailable {
            left_turn: left.turn().get(),
            right_turn: right.turn().get(),
            reason: format!("temporal ghost refused: {reason}"),
        };
    }

    let mut gained = Vec::new();
    let mut retained = Vec::new();
    let mut removed = Vec::new();

    for left_item in left.items() {
        if let Some(right_item) = right.items().iter().find(|item| item.id == left_item.id) {
            retained.push(ghost_item(
                Some(left_item),
                Some(right_item),
                diff.comparability.skew(),
            ));
        } else {
            removed.push(ghost_item(Some(left_item), None, diff.comparability.skew()));
        }
    }
    for right_item in right.items() {
        if !left.items().iter().any(|item| item.id == right_item.id) {
            gained.push(ghost_item(
                None,
                Some(right_item),
                diff.comparability.skew(),
            ));
        }
    }

    gained.sort_by(|a, b| b.right_tokens.cmp(&a.right_tokens));
    removed.sort_by(|a, b| b.left_tokens.cmp(&a.left_tokens));
    retained.sort_by(|a, b| {
        b.right_tokens
            .or(b.left_tokens)
            .cmp(&a.right_tokens.or(a.left_tokens))
    });

    TemporalGhost::Available(TemporalGhostAvailable {
        left_turn: left.turn().get(),
        right_turn: right.turn().get(),
        comparability: diff.comparability,
        gained,
        retained,
        removed,
        assumptions: vec![
            "Item identity is the adapter's stable context-item id; it is not a text diff.".into(),
            "Token changes are meaningful only outside the stated instrument bound.".into(),
            "Unattributed residual context is not represented as an item.".into(),
        ],
    })
}

fn ghost_item(
    left: Option<&ct_domain::ContextItem>,
    right: Option<&ct_domain::ContextItem>,
    skew: Option<f32>,
) -> GhostItem {
    let anchor = left.or(right).expect("a ghost item has one side");
    let left_tokens = left.map(|item| item.tokens.tokens());
    let right_tokens = right.map(|item| item.tokens.tokens());
    let delta = match (left_tokens, right_tokens) {
        (Some(left), Some(right)) => Some(right as i64 - left as i64),
        _ => None,
    };
    let bound = skew.map(|skew| {
        (left_tokens
            .unwrap_or_default()
            .max(right_tokens.unwrap_or_default()) as f32
            * skew)
            .ceil() as u32
    });
    GhostItem {
        id: anchor.id.to_string(),
        label: right
            .map(|item| item.label.clone())
            .unwrap_or_else(|| anchor.label.clone()),
        category: right.map(|item| item.category).unwrap_or(anchor.category),
        source: right
            .map(|item| item.source.to_string())
            .unwrap_or_else(|| anchor.source.to_string()),
        left_tokens,
        right_tokens,
        token_delta: delta,
        meaningful_token_delta: delta
            .is_some_and(|delta| bound.is_some_and(|bound| delta.unsigned_abs() > bound as u64)),
        confidence: right
            .map(|item| {
                left.map(|left| left.confidence().weakest(item.confidence()))
                    .unwrap_or_else(|| item.confidence())
            })
            .unwrap_or_else(|| left.expect("a ghost item has one side").confidence()),
    }
}
