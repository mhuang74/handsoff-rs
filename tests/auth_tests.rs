use handsoff::auth;

#[test]
fn test_hash_keycodes_format() {
    let keycodes = [0u32, 12, 15, 37];
    let hash = auth::hash_keycodes(&keycodes);
    assert_eq!(hash.len(), 64); // SHA-256 hex is 64 chars
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_verify_keycodes_correct() {
    let keycodes = [11u32, 2, 8, 46];
    let hash = auth::hash_keycodes(&keycodes);
    assert!(auth::verify_keycodes(&keycodes, &hash));
}

#[test]
fn test_verify_keycodes_incorrect() {
    let keycodes = [11u32, 2, 8, 46];
    let hash = auth::hash_keycodes(&keycodes);
    assert!(!auth::verify_keycodes(&[11, 2, 8, 45], &hash));
}

#[test]
fn test_hash_deterministic() {
    let keycodes = [5u32, 17, 4];
    let hash1 = auth::hash_keycodes(&keycodes);
    let hash2 = auth::hash_keycodes(&keycodes);
    assert_eq!(hash1, hash2);
}

#[test]
fn test_hash_different_inputs() {
    let hash1 = auth::hash_keycodes(&[1, 2, 3, 4]);
    let hash2 = auth::hash_keycodes(&[1, 2, 3, 5]);
    assert_ne!(hash1, hash2);
}

#[test]
fn test_order_matters() {
    // A keycode sequence is ordered: [a][s][d][f] != [f][d][s][a]
    let hash1 = auth::hash_keycodes(&[0, 1, 2, 3]);
    let hash2 = auth::hash_keycodes(&[3, 2, 1, 0]);
    assert_ne!(hash1, hash2);
}

#[test]
fn test_layout_independence_is_explicit() {
    // The same raw keycodes always produce the same hash, regardless of what
    // characters those keys produce on a given layout — that is the whole
    // point of keycode-v1 (spec §3 / L-1).
    let keycodes = [12u32, 15, 0, 37];
    let hash = auth::hash_keycodes(&keycodes);
    // Same sequence re-hashed (simulating a different layout producing
    // different chars for the same physical keys) still verifies.
    assert!(auth::verify_keycodes(&[12, 15, 0, 37], &hash));
    // The AZERTY character mapping of keycode 12 ('q' vs 'a') never enters
    // the hash: there is no character input at all.
}

#[test]
fn test_empty_sequence() {
    let hash = auth::hash_keycodes(&[]);
    assert!(auth::verify_keycodes(&[], &hash));
    assert!(!auth::verify_keycodes(&[1], &hash));
}

#[test]
fn test_long_sequence() {
    let keycodes: Vec<u32> = (0..1000).map(|i| (i % 60) as u32).collect();
    let hash = auth::hash_keycodes(&keycodes);
    assert!(auth::verify_keycodes(&keycodes, &hash));
}

#[test]
fn test_length_mismatch_fails() {
    let hash = auth::hash_keycodes(&[1, 2, 3, 4]);
    assert!(!auth::verify_keycodes(&[1, 2, 3], &hash));
    assert!(!auth::verify_keycodes(&[1, 2, 3, 4, 4], &hash));
}

#[test]
fn test_malformed_hash_fails() {
    assert!(!auth::verify_keycodes(&[1, 2, 3, 4], "garbage"));
    assert!(!auth::verify_keycodes(&[1, 2, 3, 4], ""));
}
