//! Published list prices and the port that resolves their recorded history.

use crate::TokenUsage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// USD per million tokens, expressed in microdollars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRate {
    pub input_per_million: u64,
    pub cache_read_per_million: u64,
    pub cache_write_per_million: u64,
    pub output_per_million: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PriceQuote {
    pub rate: ModelRate,
    pub version: String,
    pub source: String,
    pub warning: String,
}

pub trait PricingProvider: Send + Sync {
    /// `None` requests current prices. A timestamp requests the upstream
    /// revision recorded at or before that instant, never today's substitute.
    fn quote(
        &self,
        model: &str,
        at: Option<DateTime<Utc>>,
        usage: TokenUsage,
    ) -> Result<PriceQuote, String>;
}

/// Application tests and embedders do not implicitly perform network I/O.
pub struct UnavailablePricing;

impl PricingProvider for UnavailablePricing {
    fn quote(
        &self,
        _model: &str,
        _at: Option<DateTime<Utc>>,
        _usage: TokenUsage,
    ) -> Result<PriceQuote, String> {
        Err("automatic pricing provider is unavailable".into())
    }
}
