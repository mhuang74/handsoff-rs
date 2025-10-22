pub mod keycode;

use ring::digest;

/// Hash a passphrase using SHA-256
pub fn hash_passphrase(passphrase: &str) -> String {
    let hash = digest::digest(&digest::SHA256, passphrase.as_bytes());
    hex::encode(hash.as_ref())
}

/// Verify a passphrase against a stored hash
pub fn verify_passphrase(passphrase: &str, hash: &str) -> bool {
    hash_passphrase(passphrase) == hash
}
