//! Passphrase encryption and decryption using AES-256-GCM
//!
//! This module provides functions to encrypt and decrypt the secret passphrase
//! using AES-256-GCM authenticated encryption with a statically derived key.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm,
};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sha2::{Digest, Sha256};

/// Static seed for key derivation (consistent across all builds/versions)
///
/// This ensures that config files remain portable across different versions
/// and builds of the application.
const KEY_SEED: &str = "com.handsoff.inputlock.config.encryption.v1";

/// Derive 32-byte AES-256 key from static seed
///
/// Uses SHA-256 to generate a deterministic key from the constant seed.
/// This ensures the same key is used across all builds and versions.
fn derive_key() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(KEY_SEED.as_bytes());
    hasher.finalize().into()
}

/// Encrypt plaintext passphrase using AES-256-GCM
///
/// # Arguments
///
/// * `plaintext` - The passphrase to encrypt
///
/// # Returns
///
/// Base64-encoded string containing: nonce (12 bytes) || ciphertext || auth tag
///
/// # Errors
///
/// Returns an error if random number generation or encryption fails.
pub fn encrypt_passphrase(plaintext: &str) -> Result<String> {
    let key = derive_key();
    let cipher = Aes256Gcm::new(&key.into());

    // Generate random 12-byte nonce
    let mut nonce_bytes = [0u8; 12];
    getrandom::getrandom(&mut nonce_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to generate random nonce: {:?}", e))?;
    let nonce = &nonce_bytes.into();

    // Encrypt (ciphertext includes authentication tag)
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    // Concatenate: nonce || ciphertext (ciphertext includes auth tag)
    let mut result = Vec::new();
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    // Return base64-encoded
    Ok(BASE64.encode(&result))
}

/// Decrypt encrypted passphrase using AES-256-GCM
///
/// # Arguments
///
/// * `encrypted` - Base64-encoded encrypted data (nonce || ciphertext || auth tag)
///
/// # Returns
///
/// The decrypted plaintext passphrase
///
/// # Errors
///
/// Returns an error if:
/// - Base64 decoding fails
/// - Data is too short (< 12 bytes)
/// - Decryption fails (wrong key, corrupted data, or failed authentication)
pub fn decrypt_passphrase(encrypted: &str) -> Result<String> {
    // Decode base64
    let data = BASE64
        .decode(encrypted)
        .context("Failed to decode base64")?;

    if data.len() < 12 {
        anyhow::bail!("Invalid encrypted data: too short");
    }

    // Extract nonce (first 12 bytes) and ciphertext (rest)
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce_array: [u8; 12] = nonce_bytes.try_into()
        .context("Invalid nonce length")?;
    let nonce = &nonce_array.into();

    // Decrypt
    let key = derive_key();
    let cipher = Aes256Gcm::new(&key.into());
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

    // Convert to string
    String::from_utf8(plaintext)
        .context("Invalid UTF-8 in decrypted data")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let original = "my_secret_passphrase_123";
        let encrypted = encrypt_passphrase(original).expect("Encryption failed");
        let decrypted = decrypt_passphrase(&encrypted).expect("Decryption failed");
        assert_eq!(original, decrypted);
    }

    #[test]
    fn test_different_nonces() {
        let plaintext = "same_passphrase";
        let encrypted1 = encrypt_passphrase(plaintext).expect("Encryption 1 failed");
        let encrypted2 = encrypt_passphrase(plaintext).expect("Encryption 2 failed");

        // Same plaintext should produce different ciphertexts (due to random nonces)
        assert_ne!(encrypted1, encrypted2);

        // But both should decrypt to the same plaintext
        let decrypted1 = decrypt_passphrase(&encrypted1).expect("Decryption 1 failed");
        let decrypted2 = decrypt_passphrase(&encrypted2).expect("Decryption 2 failed");
        assert_eq!(plaintext, decrypted1);
        assert_eq!(plaintext, decrypted2);
    }

    #[test]
    fn test_tampered_ciphertext() {
        let original = "secret";
        let mut encrypted = encrypt_passphrase(original).expect("Encryption failed");

        // Tamper with the encrypted data (flip a bit in the middle)
        let mut bytes = BASE64.decode(&encrypted).unwrap();
        if bytes.len() > 15 {
            bytes[15] ^= 0x01; // Flip one bit
            encrypted = BASE64.encode(&bytes);

            // Decryption should fail due to authentication failure
            let result = decrypt_passphrase(&encrypted);
            assert!(result.is_err(), "Tampered data should fail authentication");
        }
    }

    #[test]
    fn test_static_key_consistency() {
        // Key derivation should be deterministic
        let key1 = derive_key();
        let key2 = derive_key();
        assert_eq!(key1, key2, "Key derivation must be deterministic");

        // Verify key is exactly 32 bytes for AES-256
        assert_eq!(key1.len(), 32, "Key must be 32 bytes for AES-256");
    }

    #[test]
    fn test_invalid_base64() {
        let result = decrypt_passphrase("not-valid-base64!!!");
        assert!(result.is_err(), "Invalid base64 should fail");
    }

    #[test]
    fn test_too_short_data() {
        // Create valid base64 but with data < 12 bytes
        let short_data = BASE64.encode([1u8, 2, 3, 4, 5]);
        let result = decrypt_passphrase(&short_data);
        assert!(result.is_err(), "Data < 12 bytes should fail");
    }

    #[test]
    fn test_empty_passphrase() {
        let empty = "";
        let encrypted = encrypt_passphrase(empty).expect("Should encrypt empty string");
        let decrypted = decrypt_passphrase(&encrypted).expect("Should decrypt empty string");
        assert_eq!(empty, decrypted);
    }

    #[test]
    fn test_unicode_passphrase() {
        let unicode = "🔒 Secure パスワード 密码 🔐";
        let encrypted = encrypt_passphrase(unicode).expect("Should encrypt unicode");
        let decrypted = decrypt_passphrase(&encrypted).expect("Should decrypt unicode");
        assert_eq!(unicode, decrypted);
    }
}
