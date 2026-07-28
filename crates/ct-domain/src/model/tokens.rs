//! Token counting value objects.
//!
//! This module is where ContextTrace's honesty principle stops being a policy
//! and becomes a compiler-enforced property.

use super::provenance::Confidence;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A token count together with how it was arrived at.
///
/// The variants are not decoration. Presentation code must match on them to
/// extract a number, so it is not possible to render an estimate as though it
/// were a measurement without writing the word "estimate" into the match arm
/// and noticing what you are doing.
///
/// The four-way split reflects a real asymmetry between the supported agents:
/// Codex is GPT-family, so `tiktoken` gives us [`TokenCount::Exact`] per item,
/// while Anthropic ships no local tokenizer, so Claude Code items can only ever
/// be [`TokenCount::Calibrated`] -- even though the per-turn *total* is
/// [`TokenCount::Observed`] from `usage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TokenCount {
    /// The agent reported this number itself.
    Observed { tokens: u32 },
    /// We tokenized the exact text with the model's own tokenizer.
    Exact { tokens: u32 },
    /// A heuristic estimate, rescaled so that the items of a snapshot sum to a
    /// known observed total. Individual values remain approximate; the
    /// proportions they imply are trustworthy.
    Calibrated { tokens: u32, raw_estimate: u32 },
    /// A heuristic estimate with nothing to reconcile it against.
    Estimated { tokens: u32 },
}

impl TokenCount {
    pub fn observed(tokens: u32) -> Self {
        TokenCount::Observed { tokens }
    }

    pub fn exact(tokens: u32) -> Self {
        TokenCount::Exact { tokens }
    }

    pub fn estimated(tokens: u32) -> Self {
        TokenCount::Estimated { tokens }
    }

    pub fn calibrated(tokens: u32, raw_estimate: u32) -> Self {
        TokenCount::Calibrated {
            tokens,
            raw_estimate,
        }
    }

    /// The number itself. Callers that only need arithmetic use this; callers
    /// that *display* the number should also surface [`TokenCount::confidence`].
    pub fn tokens(&self) -> u32 {
        match self {
            TokenCount::Observed { tokens }
            | TokenCount::Exact { tokens }
            | TokenCount::Calibrated { tokens, .. }
            | TokenCount::Estimated { tokens } => *tokens,
        }
    }

    pub fn confidence(&self) -> Confidence {
        match self {
            TokenCount::Observed { .. } => Confidence::Observed,
            TokenCount::Exact { .. } => Confidence::Derived,
            TokenCount::Calibrated { .. } | TokenCount::Estimated { .. } => Confidence::Estimated,
        }
    }

    /// True when this figure can be quoted without qualification.
    pub fn is_trustworthy(&self) -> bool {
        matches!(self, TokenCount::Observed { .. } | TokenCount::Exact { .. })
    }

    /// Short suffix for terminal output, e.g. `61,240 ~`.
    pub fn marker(&self) -> &'static str {
        match self {
            TokenCount::Observed { .. } => "",
            TokenCount::Exact { .. } => "",
            TokenCount::Calibrated { .. } => "~",
            TokenCount::Estimated { .. } => "?",
        }
    }
}

impl fmt::Display for TokenCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.tokens(), self.marker())
    }
}

/// Token usage as the agent reported it for a single model request.
///
/// Every field is optional because every agent reports a different subset, and
/// a field the agent omitted must stay distinguishable from a field it reported
/// as zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Fresh (uncached) input tokens.
    pub input: Option<u32>,
    /// Tokens written into the prompt cache by this request. Anthropic only.
    pub cache_creation: Option<u32>,
    /// Tokens served from the prompt cache. Anthropic reports this; Codex
    /// reports the analogous `cached_input_tokens`.
    pub cache_read: Option<u32>,
    pub output: Option<u32>,
    /// Reasoning tokens, where the agent separates them out. Codex does.
    pub reasoning: Option<u32>,
    /// The model's total context window, when the agent states it.
    pub context_window: Option<u32>,
    /// How many API calls the agent lumped into this one logged response.
    ///
    /// Normally one. Claude Code sometimes makes several calls while producing a
    /// single assistant message and records the *sum* of their cache figures --
    /// see [`TokenUsage::prompt_tokens`]. Kept so diagnostics can say that a
    /// turn's figures came from more than one request rather than hiding it.
    pub api_calls: Option<u32>,
}

impl TokenUsage {
    /// **The single most important function in the domain.**
    ///
    /// The exact size of the prompt that was sent, which is the headline number
    /// ContextTrace exists to explain.
    ///
    /// Cached tokens are still *in the context* -- caching changes what the
    /// request costs, not what the model was shown. So the prompt size is the
    /// sum of fresh input, cache writes and cache reads. Reading only
    /// `input_tokens` is the single easiest way to be badly wrong here: on a
    /// warm cache it can read `2` for a turn carrying 280,000 tokens of
    /// context, which is a real value observed in this machine's corpus.
    ///
    /// # These fields must describe one API call
    ///
    /// The sum is only a prompt size if the three figures came from a single
    /// request. Claude Code sometimes emits an `iterations` array -- several API
    /// calls made while producing one assistant message -- and its *top-level*
    /// `cache_creation_input_tokens` and `cache_read_input_tokens` are the sums
    /// across those calls. Adding them up then yields a number no context window
    /// could hold: one corpus record reports 844,611 "prompt tokens" whose
    /// largest actual call was 429,328.
    ///
    /// So adapters must populate these fields from one call and record how many
    /// calls the log lumped together in [`TokenUsage::api_calls`]. The domain
    /// cannot check this itself, which is exactly why it is written down here.
    ///
    /// Returns `None` when the agent reported no input figures — and also when
    /// it reported them as all-zero, which is not the same claim as it looks.
    ///
    /// **A prompt of zero tokens is not a measurement.** Every model request
    /// carries a prompt; a system prompt alone puts the floor in the thousands.
    /// So an all-zero `usage` object is the agent writing a record without
    /// filling it in, not an observation that nothing was sent.
    ///
    /// Found by charting a session: 27 turns across 3,795 in the local corpus
    /// carry `{input: 0, cache_creation: 0, cache_read: 0, output: 0}`, always
    /// immediately before a turn with a large `cache_creation` and no
    /// `cache_read` — the shape of a cache reset. Treated as a measured zero
    /// they invented a fall and a matching rise of ~288,000 tokens each, which
    /// took three of the five largest reported changes in that session.
    ///
    /// The characters-per-token fit turns out **not** to have been affected,
    /// which was worth checking rather than assuming. Such a turn does enter
    /// [`crate::services::ratio::derive`] as a sample, but both pairs it forms
    /// are already rejected: the pair before it underflows `checked_sub` on
    /// token growth, and the pair after it yields a ratio far below the
    /// plausible range. Its effect on the recovered overhead constant is
    /// absorbed by a median. Measured across the five affected sessions in the
    /// local sample, ratio and overhead are identical with this filter on and
    /// off. So this is a fix to what is *presented per turn*, not to the fit.
    pub fn prompt_tokens(&self) -> Option<u32> {
        match (self.input, self.cache_creation, self.cache_read) {
            (None, None, None) => None,
            (i, c, r) => Some(
                i.unwrap_or(0)
                    .saturating_add(c.unwrap_or(0))
                    .saturating_add(r.unwrap_or(0)),
            )
            .filter(|total| *total > 0),
        }
    }

    /// Fraction of the context window consumed, when both figures are known.
    pub fn context_utilisation(&self) -> Option<f32> {
        let used = self.prompt_tokens()?;
        let window = self.context_window?;
        (window > 0).then(|| used as f32 / window as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_tokens_counts_cached_tokens_as_context() {
        // Modelled on a real warm-cache turn: input_tokens is a rounding error,
        // but the model was shown ~280k tokens.
        let usage = TokenUsage {
            input: Some(2),
            cache_creation: Some(0),
            cache_read: Some(279_943),
            output: Some(1785),
            ..Default::default()
        };
        assert_eq!(usage.prompt_tokens(), Some(279_945));
    }

    #[test]
    fn prompt_tokens_is_none_only_when_nothing_was_reported() {
        assert_eq!(TokenUsage::default().prompt_tokens(), None);
        let only_output = TokenUsage {
            output: Some(10),
            ..Default::default()
        };
        assert_eq!(only_output.prompt_tokens(), None);
        let partial = TokenUsage {
            cache_read: Some(5),
            ..Default::default()
        };
        assert_eq!(partial.prompt_tokens(), Some(5));
    }

    #[test]
    fn an_all_zero_usage_record_is_not_a_prompt_of_zero_tokens() {
        // The real shape, from 27 turns in the local corpus: a usage object
        // written but never filled in, always just before a cache reset. Read
        // as a measured zero it invents a fall and a rise of the session's
        // whole context -- two of the largest "changes" a growth chart can
        // report, neither of which happened.
        let empty = TokenUsage {
            input: Some(0),
            cache_creation: Some(0),
            cache_read: Some(0),
            output: Some(0),
            api_calls: Some(1),
            ..Default::default()
        };
        assert_eq!(
            empty.prompt_tokens(),
            None,
            "every request carries a prompt; a zero here is an unfilled record"
        );
        assert_eq!(empty.context_utilisation(), None);

        // One real token is still a measurement, however implausible.
        let tiny = TokenUsage {
            input: Some(1),
            ..Default::default()
        };
        assert_eq!(tiny.prompt_tokens(), Some(1));
    }

    #[test]
    fn estimates_are_not_trustworthy_but_still_have_a_value() {
        let c = TokenCount::calibrated(61_240, 58_010);
        assert_eq!(c.tokens(), 61_240);
        assert!(!c.is_trustworthy());
        assert_eq!(c.confidence(), Confidence::Estimated);
        assert!(TokenCount::observed(10).is_trustworthy());
    }

    #[test]
    fn utilisation_needs_both_figures() {
        let u = TokenUsage {
            input: Some(50),
            context_window: Some(200),
            ..Default::default()
        };
        assert_eq!(u.context_utilisation(), Some(0.25));
        let no_window = TokenUsage {
            input: Some(50),
            ..Default::default()
        };
        assert_eq!(no_window.context_utilisation(), None);
    }
}
