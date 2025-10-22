use handsoff::app_state::AppState;
use std::thread;
use std::time::Duration;

#[test]
fn test_initial_state() {
    let state = AppState::new();
    assert!(!state.is_locked());
    assert_eq!(state.get_buffer(), "");
    assert!(state.get_passphrase_hash().is_none());
}

#[test]
fn test_lock_unlock() {
    let state = AppState::new();
    state.set_locked(true);
    assert!(state.is_locked());
    state.set_locked(false);
    assert!(!state.is_locked());
}

#[test]
fn test_buffer_operations() {
    let state = AppState::new();
    state.append_to_buffer('a');
    state.append_to_buffer('b');
    state.append_to_buffer('c');
    assert_eq!(state.get_buffer(), "abc");
    state.clear_buffer();
    assert_eq!(state.get_buffer(), "");
}

#[test]
fn test_passphrase_hash() {
    let state = AppState::new();
    let hash = "abc123def456".to_string();
    state.set_passphrase_hash(hash.clone());
    assert_eq!(state.get_passphrase_hash(), Some(hash));
}

#[test]
fn test_buffer_reset_timing() {
    let state = AppState::new();
    state.lock().buffer_reset_timeout = 1; // 1 second for testing

    state.append_to_buffer('x');
    state.update_key_time();

    assert!(!state.should_reset_buffer());

    thread::sleep(Duration::from_millis(1100)); // Slightly over 1 second
    assert!(state.should_reset_buffer());
}

#[test]
fn test_auto_lock_timing() {
    let state = AppState::new();
    state.lock().auto_lock_timeout = 1; // 1 second for testing

    assert!(!state.should_auto_lock()); // Starts unlocked

    thread::sleep(Duration::from_millis(1100));
    assert!(state.should_auto_lock());

    state.update_input_time();
    assert!(!state.should_auto_lock()); // Reset
}

#[test]
fn test_auto_lock_does_not_trigger_when_locked() {
    let state = AppState::new();
    state.lock().auto_lock_timeout = 1;
    state.set_locked(true); // Already locked

    thread::sleep(Duration::from_millis(1100));
    assert!(!state.should_auto_lock()); // Should not auto-lock when already locked
}

#[test]
fn test_talk_key_state() {
    let state = AppState::new();
    assert!(!state.is_talk_key_pressed());

    state.set_talk_key_pressed(true);
    assert!(state.is_talk_key_pressed());

    state.set_talk_key_pressed(false);
    assert!(!state.is_talk_key_pressed());
}

#[test]
fn test_thread_safety_buffer() {
    let state = AppState::new();
    let state_clone = state.clone();

    let handle = thread::spawn(move || {
        for _ in 0..100 {
            state_clone.append_to_buffer('a');
        }
    });

    for _ in 0..100 {
        state.append_to_buffer('b');
    }

    handle.join().unwrap();
    assert_eq!(state.get_buffer().len(), 200);
}

#[test]
fn test_thread_safety_lock_state() {
    let state = AppState::new();
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let state_clone = state.clone();
            thread::spawn(move || {
                for _ in 0..100 {
                    state_clone.set_locked(i % 2 == 0);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    // Should not panic or deadlock
    let _ = state.is_locked();
}

#[test]
fn test_buffer_with_unicode() {
    let state = AppState::new();
    state.append_to_buffer('🔒');
    state.append_to_buffer('a');
    state.append_to_buffer('🔓');
    assert_eq!(state.get_buffer(), "🔒a🔓");
}

#[test]
fn test_multiple_hash_updates() {
    let state = AppState::new();

    state.set_passphrase_hash("hash1".to_string());
    assert_eq!(state.get_passphrase_hash(), Some("hash1".to_string()));

    state.set_passphrase_hash("hash2".to_string());
    assert_eq!(state.get_passphrase_hash(), Some("hash2".to_string()));
}
