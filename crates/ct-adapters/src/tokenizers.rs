//! [`TokenEstimator`] implementations.
//!
//! Two exist because the supported agents differ in what is *knowable*, not
//! merely in what is convenient:
//!
//! - Codex runs GPT-family models, whose tokenizer is public. We can count
//!   exactly, and [`TiktokenEstimator`] returns [`TokenCount::Exact`].
//! - Claude Code runs Anthropic models, which ship no local tokenizer. Nothing
//!   we do here can be exact, so [`HeuristicEstimator`] returns
//!   [`TokenCount::Estimated`] and leans on the calibration step to make the
//!   resulting proportions trustworthy.

use ct_domain::ports::TokenEstimator;
use ct_domain::TokenCount;
use tiktoken_rs::CoreBPE;

/// Exact token counting for GPT-family models via `tiktoken`.
pub struct TiktokenEstimator {
    bpe: CoreBPE,
    name: &'static str,
    /// Characters per token, measured from this encoding, used when we have a
    /// length but not the text.
    chars_per_token: f32,
}

impl TiktokenEstimator {
    /// `o200k_base`: the encoding used by current GPT-family models, and so by
    /// Codex CLI.
    pub fn o200k() -> Result<Self, String> {
        let bpe = tiktoken_rs::o200k_base().map_err(|e| e.to_string())?;
        Ok(Self {
            bpe,
            name: "o200k_base",
            chars_per_token: 3.8,
        })
    }
}

impl TokenEstimator for TiktokenEstimator {
    fn count_text(&self, text: &str) -> TokenCount {
        // `encode_ordinary` ignores special tokens, which is correct here: we
        // are measuring log content, not constructing a prompt, and treating a
        // literal "<|endoftext|>" in a tool output as a control token would be
        // both wrong and a way for session content to influence our counting.
        TokenCount::exact(self.bpe.encode_ordinary(text).len() as u32)
    }

    fn estimate_from_chars(&self, char_len: u32) -> TokenCount {
        // Falls back to estimation deliberately: we have a length, not the
        // text, so an exact count is not available even though the encoding is.
        TokenCount::estimated((char_len as f32 / self.chars_per_token).ceil() as u32)
    }

    fn name(&self) -> &str {
        self.name
    }

    // `chars_per_token` is deliberately left at the port's `None`, despite the
    // field of that name above. The field is a fallback for the one case where
    // the text is unavailable; this instrument's figures are otherwise measured.
    // Publishing 3.8 here would invite a comparison to rescale exact counts by a
    // ratio they were never produced from.
}

/// Character-ratio estimation, for models with no public tokenizer.
///
/// # Why a single ratio is defensible
///
/// It looks crude, and in absolute terms it is. But per-item counts feed into
/// [`TokenCalibrator`](ct_domain::services::TokenCalibrator), which rescales
/// them to an exactly-known per-turn total. A uniform ratio therefore cancels
/// out of the *proportions*, which is what the composition view actually
/// reports. The ratio only matters where it differs between item kinds -- which
/// is why [`HeuristicEstimator::for_code`] exists rather than one global value.
pub struct HeuristicEstimator {
    chars_per_token: f32,
    name: String,
}

impl HeuristicEstimator {
    /// Prose and mixed content. English text runs about 3.6 characters per
    /// token on modern BPE vocabularies.
    pub fn for_prose() -> Self {
        Self::with_ratio(3.6)
    }

    /// Source code, JSON and terminal output, which tokenize more densely
    /// because of punctuation and indentation.
    pub fn for_code() -> Self {
        Self::with_ratio(3.1)
    }

    pub fn with_ratio(chars_per_token: f32) -> Self {
        let ratio = if chars_per_token > 0.0 {
            chars_per_token
        } else {
            3.6
        };
        Self {
            chars_per_token: ratio,
            name: format!("heuristic:chars/{ratio:.1}"),
        }
    }
}

impl Default for HeuristicEstimator {
    fn default() -> Self {
        Self::for_prose()
    }
}

impl TokenEstimator for HeuristicEstimator {
    fn count_text(&self, text: &str) -> TokenCount {
        self.estimate_from_chars(text.chars().count() as u32)
    }

    fn estimate_from_chars(&self, char_len: u32) -> TokenCount {
        TokenCount::estimated((char_len as f32 / self.chars_per_token).ceil() as u32)
    }

    fn name(&self) -> &str {
        &self.name
    }

    /// Every figure this estimator produces is a character count over this
    /// ratio, so two of them differing only here differ by a known factor.
    fn chars_per_token(&self) -> Option<f32> {
        Some(self.chars_per_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::Confidence;

    #[test]
    fn heuristic_counts_are_always_typed_as_estimates() {
        let est = HeuristicEstimator::for_prose();
        let count = est.count_text("hello world, this is some prose");
        assert_eq!(count.confidence(), Confidence::Estimated);
        assert!(!count.is_trustworthy());
    }

    #[test]
    fn code_ratio_yields_more_tokens_than_prose_for_equal_length() {
        let prose = HeuristicEstimator::for_prose().estimate_from_chars(1000);
        let code = HeuristicEstimator::for_code().estimate_from_chars(1000);
        assert!(
            code.tokens() > prose.tokens(),
            "denser content must estimate higher: code={} prose={}",
            code.tokens(),
            prose.tokens()
        );
    }

    #[test]
    fn heuristic_counts_characters_not_bytes() {
        // Ten multi-byte characters must estimate like ten characters, not
        // like the twenty-plus bytes they occupy.
        let est = HeuristicEstimator::with_ratio(1.0);
        assert_eq!(est.count_text("ααααααααατ").tokens(), 10);
    }

    #[test]
    fn zero_and_negative_ratios_fall_back_rather_than_dividing_by_zero() {
        let est = HeuristicEstimator::with_ratio(0.0);
        let count = est.estimate_from_chars(360);
        assert!(count.tokens() > 0 && count.tokens() < 360);
    }

    #[test]
    fn empty_text_costs_nothing() {
        assert_eq!(HeuristicEstimator::for_prose().count_text("").tokens(), 0);
    }

    #[test]
    fn tiktoken_produces_exact_counts_for_known_text() {
        let est = match TiktokenEstimator::o200k() {
            Ok(e) => e,
            Err(e) => panic!("o200k_base must load: {e}"),
        };
        let count = est.count_text("hello world");
        assert!(count.is_trustworthy(), "tiktoken counts are measurements");
        assert_eq!(count.confidence(), Confidence::Derived);
        assert!(count.tokens() >= 2 && count.tokens() <= 4, "got {}", count.tokens());
    }

    #[test]
    fn tiktoken_does_not_treat_log_content_as_control_tokens() {
        let est = TiktokenEstimator::o200k().expect("o200k_base must load");
        // A tool output could contain this literal string; it must be counted
        // as ordinary text rather than interpreted.
        let count = est.count_text("<|endoftext|>");
        assert!(count.tokens() > 1, "special-token text must not collapse to one token");
    }

    #[test]
    fn estimating_from_a_length_is_not_exact_even_with_a_real_tokenizer() {
        let est = TiktokenEstimator::o200k().expect("o200k_base must load");
        let count = est.estimate_from_chars(3800);
        assert!(
            !count.is_trustworthy(),
            "without the text there is nothing to measure, only to guess"
        );
    }
}
