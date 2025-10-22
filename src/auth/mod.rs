pub mod keychain;
pub mod touchid;

use crate::utils;
use anyhow::Result;

/// Verify if a passphrase matches the stored hash
pub fn verify_passphrase(input: &str, stored_hash: &str) -> bool {
    utils::verify_passphrase(input, stored_hash)
}

/// Hash a new passphrase for storage
pub fn hash_passphrase(passphrase: &str) -> String {
    utils::hash_passphrase(passphrase)
}
