//! Compact measurements retained for content diagnostics.
//!
//! Adapters briefly hold model-visible payloads while parsing. This value keeps
//! only what later analyses need -- equality and compressed size -- rather than
//! retaining a second copy of content that can be many megabytes.

use super::identity::ContentFingerprint;

/// Fixed-size facts about one model-visible content payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentMeasurement {
    pub fingerprint: ContentFingerprint,
    pub original_bytes: u32,
    pub compressed_bytes: u32,
}

impl ContentMeasurement {
    pub fn new(
        fingerprint: ContentFingerprint,
        original_bytes: usize,
        compressed_bytes: usize,
    ) -> Self {
        Self {
            fingerprint,
            original_bytes: original_bytes.min(u32::MAX as usize) as u32,
            compressed_bytes: compressed_bytes.min(u32::MAX as usize) as u32,
        }
    }

    /// Compressed bytes divided by original bytes.
    ///
    /// Values near zero are highly repetitive; values near one carry little
    /// redundancy. A compressor's framing overhead can make tiny payloads
    /// exceed one, which is one reason waste detection ignores small blocks.
    pub fn compression_ratio(self) -> f32 {
        self.compressed_bytes as f32 / self.original_bytes.max(1) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_ratio_uses_original_size_as_the_denominator() {
        let measurement = ContentMeasurement::new(ContentFingerprint::new([0; 32]), 10_000, 1_500);
        assert!((measurement.compression_ratio() - 0.15).abs() < f32::EPSILON);
    }

    #[test]
    fn byte_counts_saturate_instead_of_wrapping() {
        let measurement =
            ContentMeasurement::new(ContentFingerprint::new([0; 32]), usize::MAX, usize::MAX);
        assert_eq!(measurement.original_bytes, u32::MAX);
        assert_eq!(measurement.compressed_bytes, u32::MAX);
    }
}
