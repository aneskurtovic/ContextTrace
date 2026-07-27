//! Deriving a session's characters-per-token ratio from its own observed usage.
//!
//! # Why a constant is not good enough
//!
//! Anthropic ships no local tokenizer, so per-item counts start life as
//! `chars / ratio`. A hardcoded ratio is a guess about content we can see, and
//! it is wrong by different amounts in different sessions: a session of English
//! design discussion and a session of Windows paths and minified JSON do not
//! tokenize alike, and the local corpus spreads across roughly 2 to 7 characters
//! per token depending on which it is.
//!
//! # Why the ratio matters even though calibration rescales everything
//!
//! [`TokenCalibrator`](super::TokenCalibrator) scales estimates to meet the
//! turn's exactly-observed total, so a ratio that is uniformly wrong *cancels
//! out of the proportions*. It does not cancel out of the **residual**. The
//! residual is the context the agent never logged -- its system prompt and tool
//! JSON schemas -- and it is computed as "observed total minus what we could
//! account for". Get the ratio wrong and that subtraction returns noise: too
//! generous a ratio and the residual collapses to zero, implying a complete
//! inventory that does not exist.
//!
//! So the ratio's real job is to make the unlogged remainder a *measured*
//! quantity. That is worth deriving properly.
//!
//! # How it is derived
//!
//! Each turn gives an equation: `tokens ≈ overhead + chars / ratio`, where
//! `overhead` is the unlogged constant. Two properties of real sessions defeat
//! the obvious least-squares fit:
//!
//! - The constant dominates early turns, when logged content is small.
//! - A rewind shortens the ancestor chain without the reported total falling,
//!   which drags a line fit into physically impossible territory. Fitting the
//!   corpus this way yielded negative overheads and ratios below one character
//!   per token.
//!
//! Differencing consecutive turns removes the constant entirely, leaving
//! `Δtokens ≈ Δchars / ratio`. Taking the **median** of those per-pair ratios
//! rather than a mean keeps one anomalous pair from moving the answer. Only then
//! is the overhead recovered, from the levels, using the ratio just found. The
//! order matters: deriving the overhead first would make the ratio depend on it
//! and the two would chase each other.

/// One turn's measured pair: how much logged content was on its context chain,
/// and how many prompt tokens the agent reported for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnSample {
    /// Characters of reconstructed content in the context at this turn.
    pub chars: u64,
    /// The agent's observed prompt size for this turn.
    pub tokens: u32,
    /// How many events the chain held. A drop signals a rewind.
    pub depth: u32,
}

/// A characters-per-token ratio measured from one session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DerivedRatio {
    pub chars_per_token: f32,
    /// How many consecutive-turn pairs survived filtering.
    pub pairs_used: u32,
    /// Tokens present in every prompt but absent from the log: the system
    /// prompt and tool schemas.
    ///
    /// `None` when the measurement came out negative, which means
    /// reconstruction is over-counting and no honest figure can be given. It is
    /// reported as unknown rather than clamped to zero, because clamping would
    /// present a broken measurement as "nothing hidden".
    pub unlogged_overhead: Option<u32>,
    /// Spread of the per-pair ratios, as `p75 / p25`. Near 1.0 means the
    /// session tokenizes consistently and the ratio is dependable; a large
    /// value means it does not and the derived figures deserve less weight.
    pub dispersion: f32,
}

/// Growth below this is too small to divide by without amplifying noise.
const MIN_CHAR_GROWTH: u64 = 500;
const MIN_TOKEN_GROWTH: u32 = 50;

/// Fewer pairs than this and the median is not meaningfully a median.
const MIN_PAIRS: usize = 5;

/// Ratios outside this range are not something a tokenizer produces; a pair
/// yielding one is measuring a rewind or a cache anomaly, not text.
const PLAUSIBLE: std::ops::RangeInclusive<f32> = 0.5..=20.0;

/// Derive the ratio from consecutive turns of one session.
///
/// Returns `None` when the session does not contain enough usable growth --
/// which is a normal outcome for short sessions, not an error.
pub fn derive(samples: &[TurnSample]) -> Option<DerivedRatio> {
    let mut ratios: Vec<f32> = Vec::new();

    for pair in samples.windows(2) {
        let (before, after) = (pair[0], pair[1]);
        // A shorter chain means the conversation was rewound or compacted, so
        // the two turns are not the same prompt plus an increment.
        if after.depth < before.depth {
            continue;
        }
        let Some(char_growth) = after.chars.checked_sub(before.chars) else {
            continue;
        };
        let Some(token_growth) = after.tokens.checked_sub(before.tokens) else {
            continue;
        };
        if char_growth < MIN_CHAR_GROWTH || token_growth < MIN_TOKEN_GROWTH {
            continue;
        }
        let ratio = char_growth as f32 / token_growth as f32;
        if PLAUSIBLE.contains(&ratio) {
            ratios.push(ratio);
        }
    }

    if ratios.len() < MIN_PAIRS {
        return None;
    }
    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let chars_per_token = median(&ratios);
    if chars_per_token <= 0.0 {
        return None;
    }
    let (p25, p75) = (percentile(&ratios, 0.25), percentile(&ratios, 0.75));
    let dispersion = if p25 > 0.0 { p75 / p25 } else { f32::INFINITY };

    // Only now, with the ratio settled, recover the constant from the levels.
    let mut overheads: Vec<f32> = samples
        .iter()
        .map(|s| s.tokens as f32 - s.chars as f32 / chars_per_token)
        .collect();
    overheads.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let overhead = median(&overheads);

    Some(DerivedRatio {
        chars_per_token,
        pairs_used: ratios.len().min(u32::MAX as usize) as u32,
        unlogged_overhead: (overhead > 0.0).then_some(overhead as u32),
        dispersion,
    })
}

/// Median of a sorted slice.
fn median(sorted: &[f32]) -> f32 {
    match sorted.len() {
        0 => 0.0,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0,
    }
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f32 * p).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a session that grows at a known ratio on top of a known constant.
    fn synthetic(ratio: f32, overhead: u32, turns: usize) -> Vec<TurnSample> {
        let mut chars = 0u64;
        (0..turns)
            .map(|i| {
                chars += 4_000 + (i as u64 % 3) * 1_500;
                TurnSample {
                    chars,
                    tokens: overhead + (chars as f32 / ratio) as u32,
                    depth: i as u32 + 1,
                }
            })
            .collect()
    }

    #[test]
    fn recovers_a_known_ratio_and_constant() {
        let d = derive(&synthetic(3.5, 40_000, 30)).expect("30 growing turns are enough");
        assert!(
            (d.chars_per_token - 3.5).abs() < 0.05,
            "got {}",
            d.chars_per_token
        );
        let overhead = d.unlogged_overhead.expect("a positive constant is measurable");
        assert!(
            overhead.abs_diff(40_000) < 500,
            "got {overhead}, expected about 40,000"
        );
    }

    #[test]
    fn a_dense_session_and_a_prose_session_do_not_get_the_same_answer() {
        let code = derive(&synthetic(2.1, 30_000, 30)).unwrap();
        let prose = derive(&synthetic(4.2, 30_000, 30)).unwrap();
        assert!(
            code.chars_per_token < prose.chars_per_token - 1.0,
            "the point of deriving per session is that these differ: {} vs {}",
            code.chars_per_token,
            prose.chars_per_token
        );
    }

    #[test]
    fn a_rewind_does_not_corrupt_the_ratio() {
        // A rewind shortens the chain while the reported total keeps climbing.
        // Fitting through that pair is what produced sub-1.0 ratios on the real
        // corpus, which no tokenizer can do.
        let mut samples = synthetic(3.5, 20_000, 30);
        samples[15] = TurnSample {
            chars: 1_000,
            tokens: samples[15].tokens,
            depth: 1,
        };
        let d = derive(&samples).expect("the rest of the session is still usable");
        assert!(
            (d.chars_per_token - 3.5).abs() < 0.2,
            "the rewound pair leaked in: {}",
            d.chars_per_token
        );
    }

    #[test]
    fn over_counting_reports_unknown_rather_than_zero_overhead() {
        // Reconstruction that finds more content than the prompt held gives a
        // negative constant. Saying "0 hidden tokens" would be a confident lie.
        let mut samples = synthetic(3.5, 0, 30);
        for s in &mut samples {
            s.tokens = s.tokens.saturating_sub(20_000);
        }
        let d = derive(&samples).expect("growth is still measurable");
        assert_eq!(d.unlogged_overhead, None);
    }

    #[test]
    fn too_few_turns_is_declined_rather_than_guessed() {
        assert!(derive(&[]).is_none());
        assert!(derive(&synthetic(3.5, 1_000, 4)).is_none());
    }

    #[test]
    fn a_session_that_never_grows_yields_nothing() {
        let flat: Vec<TurnSample> = (0..20)
            .map(|i| TurnSample {
                chars: 5_000,
                tokens: 40_000,
                depth: i,
            })
            .collect();
        assert!(derive(&flat).is_none());
    }

    #[test]
    fn dispersion_reports_how_consistent_the_session_was() {
        let steady = derive(&synthetic(3.5, 10_000, 30)).unwrap();
        assert!(
            steady.dispersion < 1.3,
            "a uniform session should look consistent, got {}",
            steady.dispersion
        );
    }
}
