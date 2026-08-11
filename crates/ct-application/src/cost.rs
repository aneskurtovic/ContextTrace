//! Estimated request costs from the token usage the agent recorded.
//!
//! Prices are deliberately local data, not a claim that ContextTrace can know
//! a user's contract. A model that is absent from the table remains unpriced;
//! zero would be a false measurement.

use ct_domain::{AgentSession, TokenUsage};
use serde::Serialize;

/// One millionth of a US dollar. Integer arithmetic keeps aggregation exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MoneyMicros(pub u64);

impl MoneyMicros {
    pub fn usd(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ModelRate {
    pub input_per_million: u64,
    pub cache_read_per_million: u64,
    pub cache_write_per_million: u64,
    pub output_per_million: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PricingMatch {
    pub model_prefix: &'static str,
    pub rate: ModelRate,
}

/// The embedded price table. Prices are USD per million tokens, stored as
/// microdollars. Update this table deliberately when provider list prices move.
#[derive(Debug, Clone, Copy)]
pub struct PricingCatalog {
    pub version: &'static str,
    pub source: &'static str,
    pub warning: &'static str,
}

impl PricingCatalog {
    pub const BUNDLED: Self = Self {
        version: "2026-08-11",
        source: "bundled local pricing table; provider list prices may differ",
        warning: "This is an estimate based on a local table that can go stale.",
    };

    pub fn match_model(self, model: &str) -> Option<PricingMatch> {
        let model = model.to_ascii_lowercase();
        BUNDLED_RATES
            .iter()
            .filter(|candidate| model.starts_with(candidate.model_prefix))
            .max_by_key(|candidate| candidate.model_prefix.len())
            .copied()
    }
}

// Values are USD/M tokens converted to microdollars. Cache writes use the
// documented five-minute cache rate where the provider publishes one.
const BUNDLED_RATES: &[PricingMatch] = &[
    PricingMatch {
        model_prefix: "gpt-5.6-sol",
        rate: ModelRate {
            input_per_million: 5_000_000,
            cache_read_per_million: 500_000,
            cache_write_per_million: 5_000_000,
            output_per_million: 30_000_000,
        },
    },
    PricingMatch {
        model_prefix: "gpt-5.6-terra",
        rate: ModelRate {
            input_per_million: 2_500_000,
            cache_read_per_million: 250_000,
            cache_write_per_million: 2_500_000,
            output_per_million: 15_000_000,
        },
    },
    PricingMatch {
        model_prefix: "gpt-5.6-luna",
        rate: ModelRate {
            input_per_million: 1_000_000,
            cache_read_per_million: 100_000,
            cache_write_per_million: 1_000_000,
            output_per_million: 6_000_000,
        },
    },
    PricingMatch {
        model_prefix: "gpt-4.1",
        rate: ModelRate {
            input_per_million: 2_000_000,
            cache_read_per_million: 500_000,
            cache_write_per_million: 2_000_000,
            output_per_million: 8_000_000,
        },
    },
    PricingMatch {
        model_prefix: "claude-opus-4-5",
        rate: ModelRate {
            input_per_million: 5_000_000,
            cache_read_per_million: 500_000,
            cache_write_per_million: 6_250_000,
            output_per_million: 25_000_000,
        },
    },
    PricingMatch {
        model_prefix: "claude-sonnet-4",
        rate: ModelRate {
            input_per_million: 3_000_000,
            cache_read_per_million: 300_000,
            cache_write_per_million: 3_750_000,
            output_per_million: 15_000_000,
        },
    },
    PricingMatch {
        model_prefix: "claude-haiku-4-5",
        rate: ModelRate {
            input_per_million: 1_000_000,
            cache_read_per_million: 100_000,
            cache_write_per_million: 1_250_000,
            output_per_million: 5_000_000,
        },
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CostCategory {
    pub name: &'static str,
    pub tokens: u64,
    pub cost: MoneyMicros,
    pub confidence: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CostTurn {
    pub turn: u32,
    pub model: Option<String>,
    pub priced: bool,
    pub categories: Vec<CostCategory>,
    pub total: MoneyMicros,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnpricedTurn {
    pub turn: u32,
    pub model: Option<String>,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CostReport {
    pub session_id: String,
    pub pricing_version: &'static str,
    pub pricing_source: &'static str,
    pub warning: &'static str,
    pub categories: Vec<CostCategory>,
    pub total: MoneyMicros,
    pub turns: Vec<CostTurn>,
    pub unpriced: Vec<UnpricedTurn>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CostScenario {
    pub model_override: Option<String>,
    pub cap_input_tokens: Option<u32>,
    pub cap_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CostComparison {
    pub baseline: CostReport,
    pub hypothetical: CostReport,
    pub savings: MoneyMicros,
    pub assumptions: Vec<String>,
}

/// Project the recorded request usage for every turn that has a usable model
/// and a matching local rate. Reasoning is intentionally not a separate bill:
/// providers charge it as output, so adding it again would double-count.
pub fn project(session: &AgentSession) -> CostReport {
    project_scenario(session, &CostScenario::default())
}

pub fn compare(session: &AgentSession, scenario: &CostScenario) -> CostComparison {
    let baseline = project(session);
    let hypothetical = project_scenario(session, scenario);
    let savings = MoneyMicros(baseline.total.0.saturating_sub(hypothetical.total.0));
    let mut assumptions = vec![
        "This is a token-policy estimate; it does not simulate future agent behavior or cache invalidation.".into(),
        "Cache read/write tokens are held constant while input/output caps are applied.".into(),
    ];
    if let Some(model) = &scenario.model_override {
        assumptions.push(format!(
            "Every priced turn is substituted with model '{model}'."
        ));
    }
    if let Some(tokens) = scenario.cap_input_tokens {
        assumptions.push(format!(
            "Fresh input is capped at {tokens} tokens per turn."
        ));
    }
    if let Some(tokens) = scenario.cap_output_tokens {
        assumptions.push(format!("Output is capped at {tokens} tokens per turn."));
    }
    CostComparison {
        baseline,
        hypothetical,
        savings,
        assumptions,
    }
}

fn project_scenario(session: &AgentSession, scenario: &CostScenario) -> CostReport {
    let catalog = PricingCatalog::BUNDLED;
    let mut categories = category_totals();
    let mut turns = Vec::new();
    let mut unpriced = Vec::new();

    for turn in session.turns() {
        let model = scenario.model_override.clone().or_else(|| {
            turn.model
                .clone()
                .or_else(|| session.metadata().model.clone())
        });
        let Some(model_name) = model.clone() else {
            unpriced.push(UnpricedTurn {
                turn: turn.number.get(),
                model,
                reason: "the log did not record a model",
            });
            continue;
        };
        let Some(pricing) = catalog.match_model(&model_name) else {
            unpriced.push(UnpricedTurn {
                turn: turn.number.get(),
                model,
                reason: "no bundled rate matches this model",
            });
            continue;
        };
        let mut usage = turn.usage;
        if let Some(cap) = scenario.cap_input_tokens {
            usage.input = Some(usage.input.unwrap_or(0).min(cap));
        }
        if let Some(cap) = scenario.cap_output_tokens {
            usage.output = Some(usage.output.unwrap_or(0).min(cap));
        }
        let priced = usage_cost(usage, pricing.rate);
        for category in &priced {
            if let Some(total) = categories
                .iter_mut()
                .find(|total| total.name == category.name)
            {
                total.tokens += category.tokens;
                total.cost.0 += category.cost.0;
            }
        }
        turns.push(CostTurn {
            turn: turn.number.get(),
            model,
            priced: true,
            total: MoneyMicros(priced.iter().map(|category| category.cost.0).sum()),
            categories: priced,
        });
    }

    let total = MoneyMicros(categories.iter().map(|category| category.cost.0).sum());
    CostReport {
        session_id: session.id().to_string(),
        pricing_version: catalog.version,
        pricing_source: catalog.source,
        warning: catalog.warning,
        categories,
        total,
        turns,
        unpriced,
    }
}

fn category_totals() -> Vec<CostCategory> {
    ["input", "cache-read", "cache-write", "output"]
        .into_iter()
        .map(|name| CostCategory {
            name,
            tokens: 0,
            cost: MoneyMicros(0),
            confidence: "estimated",
        })
        .collect()
}

fn usage_cost(usage: TokenUsage, rate: ModelRate) -> Vec<CostCategory> {
    [
        (
            "input",
            usage.input.unwrap_or(0) as u64,
            rate.input_per_million,
        ),
        (
            "cache-read",
            usage.cache_read.unwrap_or(0) as u64,
            rate.cache_read_per_million,
        ),
        (
            "cache-write",
            usage.cache_creation.unwrap_or(0) as u64,
            rate.cache_write_per_million,
        ),
        (
            "output",
            usage.output.unwrap_or(0) as u64,
            rate.output_per_million,
        ),
    ]
    .into_iter()
    .map(|(name, tokens, rate)| CostCategory {
        name,
        tokens,
        cost: MoneyMicros(tokens.saturating_mul(rate) / 1_000_000),
        confidence: "estimated",
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_each_usage_category_without_double_counting_reasoning() {
        let categories = usage_cost(
            TokenUsage {
                input: Some(1_000_000),
                cache_read: Some(2_000_000),
                cache_creation: Some(3_000_000),
                output: Some(4_000_000),
                reasoning: Some(99_000_000),
                ..Default::default()
            },
            ModelRate {
                input_per_million: 2,
                cache_read_per_million: 3,
                cache_write_per_million: 5,
                output_per_million: 7,
            },
        );
        assert_eq!(
            categories
                .iter()
                .map(|category| category.cost.0)
                .sum::<u64>(),
            2 + 6 + 15 + 28
        );
        assert_eq!(
            categories
                .iter()
                .find(|category| category.name == "output")
                .unwrap()
                .tokens,
            4_000_000
        );
    }

    #[test]
    fn unknown_models_are_not_reported_as_free() {
        assert!(PricingCatalog::BUNDLED
            .match_model("future-model")
            .is_none());
    }
}
