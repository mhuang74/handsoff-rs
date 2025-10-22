use handsoff::auth;

#[test]
fn test_hash_passphrase() {
    let passphrase = "test123";
    let hash = auth::hash_passphrase(passphrase);
    assert_eq!(hash.len(), 64); // SHA-256 hex is 64 chars
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_verify_passphrase_correct() {
    let passphrase = "correct_password";
    let hash = auth::hash_passphrase(passphrase);
    assert!(auth::verify_passphrase(passphrase, &hash));
}

#[test]
fn test_verify_passphrase_incorrect() {
    let passphrase = "correct_password";
    let hash = auth::hash_passphrase(passphrase);
    assert!(!auth::verify_passphrase("wrong_password", &hash));
}

#[test]
fn test_hash_deterministic() {
    let passphrase = "same_input";
    let hash1 = auth::hash_passphrase(passphrase);
    let hash2 = auth::hash_passphrase(passphrase);
    assert_eq!(hash1, hash2);
}

#[test]
fn test_hash_different_inputs() {
    let hash1 = auth::hash_passphrase("input1");
    let hash2 = auth::hash_passphrase("input2");
    assert_ne!(hash1, hash2);
}

#[test]
fn test_empty_passphrase() {
    let hash = auth::hash_passphrase("");
    assert_eq!(hash.len(), 64);
    assert!(auth::verify_passphrase("", &hash));
}

#[test]
fn test_unicode_passphrase() {
    let passphrase = "🔒password🔓";
    let hash = auth::hash_passphrase(passphrase);
    assert!(auth::verify_passphrase(passphrase, &hash));
    assert!(!auth::verify_passphrase("password", &hash));
}

#[test]
fn test_long_passphrase() {
    let passphrase = "a".repeat(1000);
    let hash = auth::hash_passphrase(&passphrase);
    assert!(auth::verify_passphrase(&passphrase, &hash));
}

#[test]
fn test_case_sensitivity() {
    let hash = auth::hash_passphrase("Password");
    assert!(auth::verify_passphrase("Password", &hash));
    assert!(!auth::verify_passphrase("password", &hash));
    assert!(!auth::verify_passphrase("PASSWORD", &hash));
}
