pub mod keycode;
pub mod lock_file;

use ring::digest;

/// Canonical keycode-sequence passphrase representation.
///
/// A passphrase is the sequence of physical macOS virtual keycodes captured at
/// setup, independent of keyboard layout and modifier state (see
/// specs/deep-design-review-v2-2026-09.md §3). The hash is SHA-256 over the
/// big-endian encoding of each keycode, so it is stable across platforms and
/// independent of any character mapping.
pub const KEYCODE_SEQUENCE_FORMAT: &str = "keycode-v1";

/// Minimum number of physical keys required in a passphrase.
pub const MIN_PASSPHRASE_KEYS: usize = 4;

/// Hash a keycode sequence using SHA-256.
///
/// Keycodes are encoded as big-endian u32 to keep the byte representation
/// deterministic regardless of host endianness.
pub fn hash_keycodes(keycodes: &[u32]) -> String {
    let mut bytes = Vec::with_capacity(keycodes.len() * 4);
    for code in keycodes {
        bytes.extend_from_slice(&code.to_be_bytes());
    }
    let hash = digest::digest(&digest::SHA256, &bytes);
    hex::encode(hash.as_ref())
}

/// Verify a keycode sequence against a stored hash.
///
/// Hash-then-compare (not byte-constant-time, but the stored value is already
/// a digest — comparison of two 64-char hex strings leaks nothing useful under
/// the V5 threat model; see specs/deep-design-review-v2-2026-09.md §3.1).
pub fn verify_keycodes(keycodes: &[u32], stored_hash: &str) -> bool {
    // Constant-time-ish comparison of equal-length hex digests.
    let computed = hash_keycodes(keycodes);
    if computed.len() != stored_hash.len() {
        return false;
    }
    computed
        .bytes()
        .zip(stored_hash.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_keycodes_deterministic() {
        let seq = [0u32, 12, 15, 37];
        assert_eq!(hash_keycodes(&seq), hash_keycodes(&seq));
    }

    #[test]
    fn test_hash_keycodes_differs_by_order() {
        assert_ne!(hash_keycodes(&[0, 12]), hash_keycodes(&[12, 0]));
    }

    #[test]
    fn test_hash_keycodes_differs_by_value() {
        assert_ne!(hash_keycodes(&[0, 12]), hash_keycodes(&[0, 13]));
    }

    #[test]
    fn test_verify_keycodes() {
        let seq = [11u32, 2, 8, 46];
        let hash = hash_keycodes(&seq);
        assert!(verify_keycodes(&seq, &hash));
        assert!(!verify_keycodes(&[11, 2, 8, 45], &hash));
    }

    #[test]
    fn test_verify_keycodes_length_mismatch() {
        let hash = hash_keycodes(&[1, 2, 3, 4]);
        assert!(!verify_keycodes(&[1, 2, 3], &hash));
        assert!(!verify_keycodes(&[1, 2, 3, 4, 5], &hash));
    }

    #[test]
    fn test_verify_keycodes_empty() {
        let hash = hash_keycodes(&[]);
        assert!(verify_keycodes(&[], &hash));
        assert!(!verify_keycodes(&[1], &hash));
    }

    #[test]
    fn test_verify_keycodes_malformed_hash() {
        assert!(!verify_keycodes(&[1, 2, 3, 4], "not-a-hash"));
        assert!(!verify_keycodes(&[1, 2, 3, 4], ""));
    }

    #[test]
    fn test_hash_format_is_hex_sha256() {
        let hash = hash_keycodes(&[0, 1, 2, 3]);
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
