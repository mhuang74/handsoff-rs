use crate::utils;

/// Verify a keycode sequence against the stored hash
pub fn verify_keycodes(keycodes: &[u32], stored_hash: &str) -> bool {
    utils::verify_keycodes(keycodes, stored_hash)
}

/// Hash a keycode sequence for storage
pub fn hash_keycodes(keycodes: &[u32]) -> String {
    utils::hash_keycodes(keycodes)
}
