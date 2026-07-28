//! Fixed-size identities for model-visible content.
//!
//! Session lines can hold megabytes of tool output. Keeping a second copy just
//! to compare it later would defeat the lazy-content design, so adapters hash
//! content while the parsed JSON value is already in memory and retain only
//! the digest.

use ct_domain::ContentFingerprint;
use serde_json::Value;

/// Fingerprint one text payload exactly.
pub(crate) fn text(value: &str) -> ContentFingerprint {
    bytes(value.as_bytes())
}

/// Fingerprint a JSON value in the representation the parser received.
///
/// `serde_json`'s `preserve_order` feature is enabled workspace-wide, so object
/// key order survives parsing. This is intentionally exact matching rather
/// than canonical JSON equivalence: near-duplicate normalisation belongs to a
/// separate detector.
pub(crate) fn value(value: &Value) -> ContentFingerprint {
    match value {
        // Transport wrappers should not stop the same plain-text content from
        // matching across a message, attachment and tool result.
        Value::String(text) => self::text(text),
        other => bytes(other.to_string().as_bytes()),
    }
}

fn bytes(value: &[u8]) -> ContentFingerprint {
    ContentFingerprint::new(sha256(value))
}

// Kept here instead of adding a hashing crate because this repository targets
// a deliberately minimal Windows GNU toolchain. The common Rust crypto stack
// brings build scripts that require MinGW libraries absent from that setup.
// This is the standard FIPS 180-4 compression function, used only for content
// identity (never authentication).
fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];

    fn compress(state: &mut [u32; 8], block: &[u8], k: &[u32; 64]) {
        let mut w = [0u32; 64];
        for (i, bytes) in block.chunks_exact(4).take(16).enumerate() {
            w[i] = u32::from_be_bytes(bytes.try_into().expect("four-byte chunk"));
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut chunks = input.chunks_exact(64);
    for block in &mut chunks {
        compress(&mut state, block, &K);
    }

    let remainder = chunks.remainder();
    let mut tail = [0u8; 128];
    tail[..remainder.len()].copy_from_slice(remainder);
    tail[remainder.len()] = 0x80;
    let padded_len = if remainder.len() < 56 { 64 } else { 128 };
    let bit_len = (input.len() as u64).wrapping_mul(8).to_be_bytes();
    tail[padded_len - 8..padded_len].copy_from_slice(&bit_len);
    for block in tail[..padded_len].chunks_exact(64) {
        compress(&mut state, block, &K);
    }

    let mut digest = [0u8; 32];
    for (bytes, word) in digest.chunks_exact_mut(4).zip(state) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    digest
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
