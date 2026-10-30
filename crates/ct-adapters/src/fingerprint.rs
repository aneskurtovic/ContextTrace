//! Fixed-size measurements for model-visible content.
//!
//! Session lines can hold megabytes of tool output. Keeping a second copy just
//! to compare it later would defeat the lazy-content design, so adapters hash
//! and compress content while the parsed JSON value is already in memory and
//! retain only the digest and two byte counts.

use ct_domain::{ContentFingerprint, ContentHasher, ContentMeasurement};
use miniz_oxide::deflate::compress_to_vec;
use serde_json::Value;
use sha2::Digest;

/// Measure one text payload exactly.
pub(crate) fn text(value: &str) -> ContentMeasurement {
    bytes(value.as_bytes())
}
/// Measure a JSON value in the representation the parser received.
///
/// `serde_json`'s `preserve_order` feature is enabled workspace-wide, so object
/// key order survives parsing. This is intentionally exact matching rather
/// than canonical JSON equivalence: near-duplicate normalisation belongs to a
/// separate detector.
pub(crate) fn value(value: &Value) -> ContentMeasurement {
    match value {
        // Transport wrappers should not stop the same plain-text content from
        // matching across a message, attachment and tool result.
        Value::String(text) => self::text(text),
        other => bytes(other.to_string().as_bytes()),
    }
}

fn bytes(value: &[u8]) -> ContentMeasurement {
    // A Vec is bounded by the input's DEFLATE representation and dropped
    // immediately. The session keeps only the two byte counts and a digest.
    // Level 6 is the conventional speed/ratio compromise used by gzip.
    let compressed = compress_to_vec(value, 6);
    ContentMeasurement::new(
        ContentFingerprint::new(sha256(value)),
        value.len(),
        compressed.len(),
    )
}

/// The same measurement used while parsing an event, exposed for comparisons
/// against a file that exists outside the session log.
pub fn measure_bytes(value: &[u8]) -> ContentMeasurement {
    bytes(value)
}

/// Adapter-owned implementation of the domain's content-measurement port.
#[derive(Debug, Default, Clone, Copy)]
pub struct Sha256ContentHasher;

impl ContentHasher for Sha256ContentHasher {
    fn measure(&self, bytes: &[u8]) -> ContentMeasurement {
        measure_bytes(bytes)
    }
}

// Content fingerprints and archive checksums share the same streaming hasher.
fn sha256(input: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input);
    hasher.finish()
}

pub(crate) struct Sha256(sha2::Sha256);

impl Sha256 {
    pub(crate) fn new() -> Self {
        Self(sha2::Sha256::new())
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub(crate) fn finish(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

/// Lowercase hex encoding of a digest, as archive manifests store it.
pub(crate) fn hex(digest: [u8; 32]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(64);
    for byte in digest {
        write!(out, "{byte:02x}").expect("writing to a String cannot fail");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn equal_payloads_have_equal_fingerprints() {
        assert_eq!(text("same"), text("same"));
        assert_eq!(
            value(&json!({"content": "same"})),
            value(&json!({"content": "same"}))
        );
    }

    #[test]
    fn exact_matching_does_not_normalise_content() {
        assert_ne!(text("same"), text("same\n"));
        assert_ne!(
            value(&json!({"a": 1, "b": 2})),
            value(&json!({"b": 2, "a": 1}))
        );
    }

    #[test]
    fn repeated_content_has_a_far_smaller_compressed_representation() {
        let measurement = text(&"node_modules/pkg/index.js\n".repeat(2_000));
        assert!(measurement.original_bytes > 40_000);
        assert!(measurement.compression_ratio() < 0.05);
    }

    #[test]
    fn deflate_block_overhead_is_retained_in_the_measurement() {
        let measurement = text("x");
        assert!(measurement.compressed_bytes > measurement.original_bytes);
    }

    #[test]
    fn sha256_matches_the_standard_empty_string_vector() {
        assert_eq!(
            sha256(b""),
            [
                0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
                0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
                0x78, 0x52, 0xb8, 0x55,
            ]
        );
    }

    #[test]
    fn sha256_matches_the_standard_abc_vector() {
        assert_eq!(
            sha256(b"abc"),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }

    #[test]
    fn streaming_the_same_bytes_in_small_chunks_yields_the_same_digest_as_one_shot() {
        // The whole point of the incremental hasher: ingest must be able to
        // feed it a session a few kilobytes at a time and still land on the
        // digest a one-shot hash of the same bytes would produce. The input is
        // deliberately longer than one 64-byte block and not a multiple of the
        // chunk size, so it exercises the buffered remainder path too.
        let input: Vec<u8> = (0u32..500).map(|i| (i % 251) as u8).collect();
        let whole = sha256(&input);

        let mut hasher = Sha256::new();
        for chunk in input.chunks(7) {
            hasher.update(chunk);
        }
        assert_eq!(hasher.finish(), whole);
    }

    #[test]
    fn sha256_matches_padding_boundary_vectors_with_empty_updates() {
        for (length, expected) in [
            (
                55,
                "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
            ),
            (
                56,
                "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
            ),
            (
                63,
                "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
            ),
            (
                65,
                "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0",
            ),
        ] {
            let input = vec![b'a'; length];
            let mut hasher = Sha256::new();
            hasher.update(&[]);
            for chunk in input.chunks(7) {
                hasher.update(chunk);
                hasher.update(&[]);
            }
            assert_eq!(hex(hasher.finish()), expected);
            assert_eq!(hex(sha256(&input)), expected);
        }
    }

    #[test]
    fn sha256_processes_complete_blocks_before_padding() {
        assert_eq!(
            sha256(&[b'a'; 64]),
            [
                0xff, 0xe0, 0x54, 0xfe, 0x7a, 0xe0, 0xcb, 0x6d, 0xc6, 0x5c, 0x3a, 0xf9, 0xb6, 0x1d,
                0x52, 0x09, 0xf4, 0x39, 0x85, 0x1d, 0xb4, 0x3d, 0x0b, 0xa5, 0x99, 0x73, 0x37, 0xdf,
                0x15, 0x46, 0x68, 0xeb,
            ]
        );
    }
}
