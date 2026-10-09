//! Estimated request costs from the token usage the agent recorded.
//!
//! Prices come from an injected third-party provider. Missing rates stay
//! unpriced; optional contract overrides remain an advanced CLI facility.

pub use ct_domain::pricing::ModelRate;
use ct_domain::pricing::{PriceQuote, PricingProvider, UnavailablePricing};
use ct_domain::{AgentKind, AgentSession, TokenUsage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// One millionth of a US dollar. Integer arithmetic keeps aggregation exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MoneyMicros(pub u64);

impl MoneyMicros {
    pub fn usd(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PricingOverrideRate {
    pub model_prefix: String,
    pub rate: ModelRate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PricingOverrides {
    pub version: String,
    pub source: String,
    pub rates: Vec<PricingOverrideRate>,
}

impl PricingOverrides {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut parsed: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("{}: invalid pricing JSON: {error}", path.display()))?;
        if parsed.version.trim().is_empty() || parsed.source.trim().is_empty() {
            return Err(format!(
                "{}: pricing version and source are required",
                path.display()
            ));
        }
        for rate in &mut parsed.rates {
            rate.model_prefix = rate.model_prefix.trim().to_ascii_lowercase();
            if rate.model_prefix.is_empty() {
                return Err(format!(
                    "{}: model prefixes cannot be empty",
                    path.display()
                ));
            }
        }
        if parsed.rates.is_empty() {
            return Err(format!(
                "{}: at least one pricing rate is required",
                path.display()
            ));
        }
        Ok(parsed)
    }

    fn match_model(&self, model: &str) -> Option<ModelRate> {
        let model = model.to_ascii_lowercase();
        self.rates
            .iter()
            .filter(|candidate| model.starts_with(&candidate.model_prefix))
            .max_by_key(|candidate| candidate.model_prefix.len())
            .map(|candidate| candidate.rate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CostCategory {
    pub name: &'static str,
    pub tokens: u64,
    pub cost: MoneyMicros,
    pub confidence: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CostTurn {
    pub pricing_version: String,
    pub pricing_source: String,
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
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CostReport {
    pub session_id: String,
    pub pricing_version: String,
    pub pricing_source: String,
    pub warning: String,
    pub categories: Vec<CostCategory>,
    pub total: MoneyMicros,
    pub turns: Vec<CostTurn>,
    pub unpriced: Vec<UnpricedTurn>,
    pub forecast: Option<CostForecast>,
    pub cache_usage: CacheUsageReport,
}

/// Recorded token buckets, independent of price availability. These describe
/// the request retained for each turn, not cache lookup attempts or invoices.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsageReport {
    pub fresh_input_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cache_read_share: Option<f64>,
    pub complete_turns: usize,
    pub total_turns: usize,
    pub multi_call_turns: usize,
}

fn cache_usage(session: &AgentSession) -> CacheUsageReport {
    let mut report = CacheUsageReport {
        fresh_input_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        cache_read_share: None,
        complete_turns: 0,
        total_turns: session.turn_count(),
        multi_call_turns: 0,
    };
    let mut complete_input = 0u64;
    let mut complete_reads = 0u64;
    for turn in session.turns() {
        let usage = turn.usage;
        for (total, tokens) in [
            (
                &mut report.fresh_input_tokens,
                usage
                    .input
                    .filter(|_| session.agent() != AgentKind::Codex || usage.cache_read.is_some()),
            ),
            (&mut report.cache_read_tokens, usage.cache_read),
            (&mut report.cache_write_tokens, usage.cache_creation),
        ] {
            if let Some(tokens) = tokens {
                *total = Some(total.unwrap_or(0).saturating_add(u64::from(tokens)));
            }
        }
        if usage.api_calls.is_some_and(|calls| calls > 1) {
            report.multi_call_turns += 1;
        }
        if let (Some(input), Some(read)) = (usage.input, usage.cache_read) {
            if usage.prompt_tokens().is_some()
                && (session.agent() == AgentKind::Codex || usage.cache_creation.is_some())
            {
                report.complete_turns += 1;
                complete_reads += u64::from(read);
                complete_input += u64::from(input)
                    + u64::from(read)
                    + u64::from(usage.cache_creation.unwrap_or(0));
            }
        }
    }
    report.cache_read_share =
        (complete_input > 0).then(|| complete_reads as f64 / complete_input as f64);
    report
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CostForecast {
    pub additional_turns: u32,
    pub average_tokens_per_turn: Vec<CostCategory>,
    pub projected_additional: MoneyMicros,
    pub projected_total: MoneyMicros,
    pub assumptions: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CostScenario {
    pub model_override: Option<String>,
    pub cap_input_tokens: Option<u32>,
    pub cap_output_tokens: Option<u32>,
    pub forecast_turns: Option<u32>,
    pub pricing: Option<PricingOverrides>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CostComparison {
    pub baseline: CostReport,
    pub hypothetical: CostReport,
    pub savings: MoneyMicros,
    pub assumptions: Vec<String>,
}

/// Project the recorded request usage for every turn that has a usable model
/// and a matching published rate. Reasoning is intentionally not a separate bill:
/// providers charge it as output, so adding it again would double-count.
pub fn project(session: &AgentSession) -> CostReport {
    project_scenario(session, &CostScenario::default())
}

pub fn project_with(session: &AgentSession, pricing: Option<&PricingOverrides>) -> CostReport {
    project_scenario(
        session,
        &CostScenario {
            pricing: pricing.cloned(),
            ..CostScenario::default()
        },
    )
}

pub fn compare(session: &AgentSession, scenario: &CostScenario) -> CostComparison {
    compare_with_provider(session, scenario, &UnavailablePricing)
}

pub fn compare_with_provider(
    session: &AgentSession,
    scenario: &CostScenario,
    provider: &dyn PricingProvider,
) -> CostComparison {
    let baseline = project_scenario_with_provider(session, &CostScenario::default(), provider);
    let hypothetical = project_scenario_with_provider(session, scenario, provider);
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

pub fn project_scenario(session: &AgentSession, scenario: &CostScenario) -> CostReport {
    project_scenario_with_provider(session, scenario, &UnavailablePricing)
}

pub fn project_scenario_with_provider(
    session: &AgentSession,
    scenario: &CostScenario,
    provider: &dyn PricingProvider,
) -> CostReport {
    let mut categories = category_totals();
    let mut turns = Vec::new();
    let mut future_turns = Vec::new();
    let mut unpriced = Vec::new();
    let mut versions = BTreeSet::new();
    let mut warnings = BTreeSet::new();
    for turn in session.turns() {
        let model = scenario.model_override.clone().or_else(|| {
            turn.model
                .clone()
                .or_else(|| session.metadata().model.clone())
        });
        let Some(model_name) = model.as_deref() else {
            unpriced.push(UnpricedTurn {
                turn: turn.number.get(),
                model,
                reason: "the log did not record a model".into(),
            });
            continue;
        };
        let mut usage = turn.usage;
        if usage.prompt_tokens().is_none() && usage.output.unwrap_or(0) == 0 {
            unpriced.push(UnpricedTurn {
                turn: turn.number.get(),
                model,
                reason: "the log did not record usable token usage".into(),
            });
            continue;
        }
        let missing_split = usage.input.is_some_and(|input| input > 0)
            && (usage.cache_read.is_none()
                || (session.agent() == AgentKind::ClaudeCode && usage.cache_creation.is_none()));
        if missing_split || usage.api_calls.is_some_and(|calls| calls > 1) {
            unpriced.push(UnpricedTurn {
                turn: turn.number.get(),
                model,
                reason: if missing_split {
                    "the log did not record a complete cache split; uncached pricing would be misleading"
                } else {
                    "multiple API calls were logged together; retained input is the largest request, not billable input across all calls"
                }.into(),
            });
            continue;
        }
        if let Some(cap) = scenario.cap_input_tokens {
            usage.input = Some(usage.input.unwrap_or(0).min(cap));
        }
        if let Some(cap) = scenario.cap_output_tokens {
            usage.output = Some(usage.output.unwrap_or(0).min(cap));
        }
        let quote = |at| -> Result<PriceQuote, String> {
            if let Some(pricing) = &scenario.pricing {
                if let Some(rate) = pricing.match_model(model_name) {
                    return Ok(PriceQuote {
                        rate,
                        version: pricing.version.clone(),
                        source: pricing.source.clone(),
                        warning: "Explicit contract override; verify against your agreement."
                            .into(),
                    });
                }
            }
            provider.quote(model_name, at, usage)
        };
        // A session start is not evidence of when a particular request ran.
        let matched = match quote(turn.timestamp) {
            Ok(matched) => matched,
            Err(reason) => {
                unpriced.push(UnpricedTurn {
                    turn: turn.number.get(),
                    model,
                    reason,
                });
                continue;
            }
        };
        versions.insert(matched.version.clone());
        warnings.insert(matched.warning.clone());
        let priced = usage_cost(usage, matched.rate);
        for category in &priced {
            if let Some(total) = categories
                .iter_mut()
                .find(|total| total.name == category.name)
            {
                total.tokens = total.tokens.saturating_add(category.tokens);
                total.cost.0 = total.cost.0.saturating_add(category.cost.0);
            }
        }
        if scenario.forecast_turns.is_some_and(|count| count > 0) {
            match quote(None) {
                Ok(current) => {
                    warnings.insert(current.warning.replace("Current list-price estimate; the request has no timestamp, so historical pricing cannot be established.", "Forecast uses current list prices."));
                    let categories = usage_cost(usage, current.rate);
                    future_turns.push(CostTurn {
                        turn: turn.number.get(),
                        model: model.clone(),
                        priced: true,
                        total: MoneyMicros(categories.iter().map(|category| category.cost.0).sum()),
                        categories,
                        pricing_version: current.version,
                        pricing_source: current.source,
                    });
                }
                Err(reason) => {
                    warnings.insert(format!("Forecast unavailable: {reason}"));
                }
            }
        }
        turns.push(CostTurn {
            turn: turn.number.get(),
            model,
            priced: true,
            total: MoneyMicros(priced.iter().map(|category| category.cost.0).sum()),
            categories: priced,
            pricing_version: matched.version,
            pricing_source: matched.source,
        });
    }
    let total = MoneyMicros(categories.iter().map(|category| category.cost.0).sum());
    let forecast = scenario
        .forecast_turns
        .filter(|count| *count > 0)
        .filter(|_| unpriced.is_empty() && future_turns.len() == turns.len())
        .and_then(|count| build_forecast(&future_turns, count, total));
    if !unpriced.is_empty() {
        warnings.insert(format!("Partial estimate: {} turn(s) are unpriced and excluded from totals. Forecast is unavailable with incomplete pricing.", unpriced.len()));
    }
    CostReport {
        session_id: session.id().to_string(),
        pricing_version: match versions.len() {
            0 => "unavailable".into(),
            1 => versions.into_iter().next().unwrap(),
            count => format!("{count} pricing revisions"),
        },
        pricing_source: scenario
            .pricing
            .as_ref()
            .map(|p| {
                format!(
                    "{}; automatic third-party rates for unmatched models",
                    p.source
                )
            })
            .unwrap_or_else(|| {
                "LiteLLM public pricing catalog; revision sources are recorded per turn".into()
            }),
        warning: warnings.into_iter().collect::<Vec<_>>().join(" "),
        categories,
        total,
        turns,
        unpriced,
        forecast,
        cache_usage: cache_usage(session),
    }
}

fn build_forecast(
    turns: &[CostTurn],
    additional_turns: u32,
    observed_total: MoneyMicros,
) -> Option<CostForecast> {
    if turns.is_empty() {
        return None;
    }
    let count = turns.len() as u64;
    let mut average = category_totals();
    for turn in turns {
        for category in &turn.categories {
            if let Some(row) = average.iter_mut().find(|row| row.name == category.name) {
                row.tokens = row.tokens.saturating_add(category.tokens);
                row.cost.0 = row.cost.0.saturating_add(category.cost.0);
            }
        }
    }
    for row in &mut average {
        row.tokens /= count;
        row.cost.0 /= count;
        row.confidence = "estimated";
    }
    let per_turn = average.iter().map(|row| row.cost.0).sum::<u64>();
    let projected_additional = MoneyMicros(per_turn.saturating_mul(additional_turns as u64));
    Some(CostForecast {
        additional_turns,
        average_tokens_per_turn: average,
        projected_additional,
        projected_total: MoneyMicros(observed_total.0.saturating_add(projected_additional.0)),
        assumptions: vec![
            "Future turns use the average recorded token usage repriced at current list prices."
                .into(),
            "Future model choice, cache state, and agent behavior are not observed.".into(),
            format!("The forecast covers {additional_turns} additional turn(s)."),
        ],
    })
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
    use ct_domain::{AgentKind, SessionId, SessionMetadata, Turn, TurnNumber};

    struct DatedPrices;
    impl PricingProvider for DatedPrices {
        fn quote(
            &self,
            model: &str,
            at: Option<chrono::DateTime<chrono::Utc>>,
            _usage: TokenUsage,
        ) -> Result<PriceQuote, String> {
            if model == "unknown" {
                return Err("no historical match".into());
            }
            Ok(PriceQuote {
                rate: ModelRate {
                    input_per_million: if at.is_some() { 1_000_000 } else { 2_000_000 },
                    output_per_million: 0,
                    cache_read_per_million: 0,
                    cache_write_per_million: 0,
                },
                version: if at.is_some() {
                    "historical"
                } else {
                    "current"
                }
                .into(),
                source: "test catalog revision".into(),
                warning: "test list-price estimate".into(),
            })
        }
    }

    #[test]
    fn historical_spend_and_current_forecast_use_different_quotes() {
        let session = priced_session("known");
        let report = project_scenario_with_provider(
            &session,
            &CostScenario {
                forecast_turns: Some(3),
                ..Default::default()
            },
            &DatedPrices,
        );
        assert_eq!(report.total.0, 1_000_000);
        assert_eq!(report.turns[0].pricing_version, "historical");
        let forecast = report.forecast.unwrap();
        assert_eq!(forecast.projected_additional.0, 6_000_000);
        assert_eq!(forecast.projected_total.0, 7_000_000);
    }

    #[test]
    fn unavailable_history_disables_forecast_and_missing_time_is_explicitly_current() {
        let report = project_scenario_with_provider(
            &priced_session("unknown"),
            &CostScenario {
                forecast_turns: Some(3),
                ..Default::default()
            },
            &DatedPrices,
        );
        assert_eq!(report.unpriced[0].reason, "no historical match");
        assert!(report.turns.is_empty());
        assert!(report.forecast.is_none());
        let session = priced_session("known");
        let mut turns = session.turns().to_vec();
        turns[0].timestamp = None;
        let untimed = AgentSession::new(
            session.id().clone(),
            session.agent(),
            session.metadata().clone(),
            vec![],
            turns,
            vec![],
        );
        let report =
            project_scenario_with_provider(&untimed, &CostScenario::default(), &DatedPrices);
        assert_eq!(report.total.0, 2_000_000);
        assert_eq!(report.turns[0].pricing_version, "current");
    }

    fn priced_session(model: &str) -> AgentSession {
        AgentSession::new(
            SessionId::new("pricing-test").unwrap(),
            AgentKind::Codex,
            SessionMetadata::default(),
            vec![],
            vec![Turn {
                number: TurnNumber::FIRST,
                model: Some(model.into()),
                timestamp: Some(
                    chrono::DateTime::parse_from_rfc3339("2020-01-02T12:00:00Z")
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                ),
                usage: TokenUsage {
                    input: Some(1_000_000),
                    cache_read: Some(0),
                    ..Default::default()
                },
                event_indices: vec![],
                anchor_index: None,
                response_start_index: None,
            }],
            vec![],
        )
    }

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
        assert!(UnavailablePricing
            .quote("future-model", None, TokenUsage::default())
            .is_err());
    }

    #[test]
    fn cache_statistics_survive_unpriced_models_and_preserve_unknown_buckets() {
        let original = priced_session("unknown");
        let mut turns = original.turns().to_vec();
        turns[0].usage.input = Some(200);
        turns[0].usage.cache_read = Some(800);
        let session = AgentSession::new(
            original.id().clone(),
            original.agent(),
            original.metadata().clone(),
            vec![],
            turns.clone(),
            vec![],
        );
        let report = project(&session);
        assert_eq!(report.cache_usage.fresh_input_tokens, Some(200));
        assert_eq!(report.cache_usage.cache_read_tokens, Some(800));
        assert_eq!(report.cache_usage.cache_write_tokens, None);
        assert_eq!(report.cache_usage.cache_read_share, Some(0.8));
        assert_eq!(report.cache_usage.complete_turns, 1);
        assert!(report.turns.is_empty());
        turns[0].usage.cache_read = None;
        let session = AgentSession::new(
            original.id().clone(),
            original.agent(),
            original.metadata().clone(),
            vec![],
            turns,
            vec![],
        );
        let report = project(&session);
        assert_eq!(report.cache_usage.cache_read_tokens, None);
        assert_eq!(report.cache_usage.fresh_input_tokens, None);
        assert_eq!(report.cache_usage.cache_read_share, None);
        assert!(report.unpriced[0].reason.contains("complete cache split"));
    }

    #[test]
    fn multi_call_context_samples_are_not_priced_as_complete_billing_usage() {
        let original = priced_session("known");
        let mut turns = original.turns().to_vec();
        turns[0].usage.api_calls = Some(2);
        let session = AgentSession::new(
            original.id().clone(),
            original.agent(),
            original.metadata().clone(),
            vec![],
            turns,
            vec![],
        );
        let report = project_scenario_with_provider(
            &session,
            &CostScenario {
                forecast_turns: Some(3),
                ..Default::default()
            },
            &DatedPrices,
        );
        assert!(report.turns.is_empty());
        assert!(report.forecast.is_none());
        assert_eq!(report.cache_usage.multi_call_turns, 1);
        assert!(report.unpriced[0].reason.contains("multiple API calls"));
    }

    #[test]
    fn a_local_rate_and_explicit_horizon_change_only_the_scenario_report() {
        let session = AgentSession::new(
            SessionId::new("cost-test").unwrap(),
            AgentKind::Codex,
            SessionMetadata {
                model: Some("local-model-v2".into()),
                ..Default::default()
            },
            vec![],
            vec![Turn {
                number: TurnNumber::FIRST,
                timestamp: None,
                model: None,
                usage: TokenUsage {
                    input: Some(1_000_000),
                    cache_read: Some(0),
                    output: Some(1_000_000),
                    ..Default::default()
                },
                event_indices: vec![],
                anchor_index: None,
                response_start_index: None,
            }],
            vec![],
        );
        let pricing = PricingOverrides {
            version: "test".into(),
            source: "test contract".into(),
            rates: vec![PricingOverrideRate {
                model_prefix: "local-model".into(),
                rate: ModelRate {
                    input_per_million: 1_000_000,
                    cache_read_per_million: 0,
                    cache_write_per_million: 0,
                    output_per_million: 2_000_000,
                },
            }],
        };
        let report = project_scenario(
            &session,
            &CostScenario {
                pricing: Some(pricing),
                forecast_turns: Some(3),
                ..Default::default()
            },
        );
        assert_eq!(report.pricing_version, "test");
        assert_eq!(report.total.0, 3_000_000);
        assert_eq!(report.forecast.unwrap().additional_turns, 3);
    }
}
