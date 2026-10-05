use handsoff::app_state::{AppState, AppStateInner};
use handsoff::config::AutoUnlockConfig;
use std::thread;
use std::time::Duration;

fn enable_backoff(state: &AppState, base_secs: u64) {
    state.set_auto_unlock_config(AutoUnlockConfig::Backoff {
        base_interval_secs: std::num::NonZeroU64::new(base_secs).unwrap(),
    });
}

#[test]
fn test_initial_state() {
    let state = AppState::new();
    assert!(!state.is_locked());
    assert_eq!(state.buffer_len(), 0);
    assert!(state.get_passphrase_hash().is_none());
    assert!(!state.auto_unlock_enabled());
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
    state.append_to_buffer(0);
    state.append_to_buffer(12);
    state.append_to_buffer(15);
    assert_eq!(state.buffer_len(), 3);
    state.pop_buffer();
    assert_eq!(state.buffer_len(), 2);
    state.clear_buffer();
    assert_eq!(state.buffer_len(), 0);
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

    state.append_to_buffer(0);
    state.update_key_time();

    assert!(!state.should_reset_buffer());

    thread::sleep(Duration::from_millis(1100)); // Slightly over 1 second
    assert!(state.should_reset_buffer());
}

#[test]
fn test_auto_lock_timing() {
    let state = AppState::new();
    {
        let mut inner = state.lock();
        inner.auto_lock_timeout = 1; // 1 second for testing
        inner.has_accessibility_permissions = true; // allow auto-lock in this test
    }

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
        for i in 0..100 {
            state_clone.append_to_buffer(i as u32);
        }
    });

    for i in 100..200 {
        state.append_to_buffer(i as u32);
    }

    handle.join().unwrap();
    assert_eq!(state.buffer_len(), 200);
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
fn test_multiple_hash_updates() {
    let state = AppState::new();

    state.set_passphrase_hash("hash1".to_string());
    assert_eq!(state.get_passphrase_hash(), Some("hash1".to_string()));

    state.set_passphrase_hash("hash2".to_string());
    assert_eq!(state.get_passphrase_hash(), Some("hash2".to_string()));
}

// ---- Auto-unlock backoff integration (spec §2) ----

#[test]
fn test_backoff_full_stretch_progression() {
    // Simulate the schedule with short intervals by backdating stretch_start.
    let state = AppState::new();
    enable_backoff(&state, 3600);
    state.set_locked(true);

    // Window 0 (base interval) — backdate past 3600s; machine unattended
    // (idle large), so the window fires immediately on schedule.
    {
        let mut inner = state.lock();
        inner.auto_unlock.as_mut().unwrap().stretch_start =
            std::time::Instant::now() - Duration::from_secs(3601);
        inner.last_input_time = std::time::Instant::now() - Duration::from_secs(3601);
    }
    assert!(state.should_auto_unlock(), "Window 0 should be open");

    // Fire window 0: trigger consumes it (advances index) and unlocks
    state.trigger_auto_unlock();
    assert!(!state.is_locked());

    // Re-lock: window 1 interval (7200s) must already be pending — the
    // schedule did NOT reset to base on re-lock (§2.3).
    state.set_locked(true);
    assert_eq!(state.get_auto_unlock_interval_secs(), Some(7200));

    // Backdate most of window 1 — not open yet
    {
        let mut inner = state.lock();
        inner.auto_unlock.as_mut().unwrap().stretch_start =
            std::time::Instant::now() - Duration::from_secs(7199);
    }
    assert!(!state.should_auto_unlock());

    // Backdate fully — open (unattended: idle large)
    {
        let mut inner = state.lock();
        inner.auto_unlock.as_mut().unwrap().stretch_start =
            std::time::Instant::now() - Duration::from_secs(7201);
        inner.last_input_time = std::time::Instant::now() - Duration::from_secs(7201);
    }
    assert!(state.should_auto_unlock());
}

#[test]
fn test_window_fire_reanchors_next_interval_from_fire_time() {
    // §2.1 cumulative timeline: window N>0 opens interval(N) after window
    // N-1 OPENED (trigger_auto_unlock re-anchors at fire time), and a
    // re-lock after the fire must NOT move that anchor. Fails on pre-fix
    // code, where set_locked re-anchored on every unlocked→locked
    // transition (measuring from the re-lock instead).
    let state = AppState::new();
    enable_backoff(&state, 3600);
    state.set_locked(true);

    // Window 0 (base interval) — backdate past 3600s; unattended.
    {
        let mut inner = state.lock();
        inner.auto_unlock.as_mut().unwrap().stretch_start =
            std::time::Instant::now() - Duration::from_secs(3601);
        inner.last_input_time = std::time::Instant::now() - Duration::from_secs(3601);
    }
    assert!(state.should_auto_unlock(), "Window 0 should be open");

    // Fire window 0, then simulate auto-lock re-engagement.
    state.trigger_auto_unlock();

    // Anchor set at fire time (window 0's opening): fresh, not the backdated
    // pre-fire value (pins the trigger_auto_unlock re-anchor; without it the
    // stale backdated anchor would carry through and this test would still
    // pass).
    let anchor = state.lock().auto_unlock.as_ref().unwrap().stretch_start;
    assert!(
        anchor.elapsed() < Duration::from_secs(1),
        "trigger_auto_unlock must re-anchor stretch_start at fire time"
    );

    state.set_locked(true);

    // The fire-time anchor must survive the re-lock: the next window opens
    // interval(1) after window 0 OPENED, not after the re-lock.
    assert_eq!(
        state.lock().auto_unlock.as_ref().unwrap().stretch_start,
        anchor,
        "re-lock after a fired window must not re-anchor the schedule"
    );

    // Window 1 (7200s from window 0's opening): just under — not open.
    {
        let mut inner = state.lock();
        inner.auto_unlock.as_mut().unwrap().stretch_start =
            anchor - Duration::from_secs(7199);
    }
    assert!(!state.should_auto_unlock());

    // Fully backdated past interval(1) from the fire-time anchor — open.
    {
        let mut inner = state.lock();
        inner.auto_unlock.as_mut().unwrap().stretch_start =
            anchor - Duration::from_secs(7201);
        inner.last_input_time = std::time::Instant::now() - Duration::from_secs(7201);
    }
    assert!(state.should_auto_unlock());
}

#[test]
fn test_relock_does_not_reanchor_stretch() {
    // R1: locked → locked re-engagements (e.g. auto-lock re-firing during a
    // window) must NOT re-anchor the schedule — the counter is keyed to the
    // locked stretch, not lock events (§2.3).
    let state = AppState::new();
    enable_backoff(&state, 3600);
    state.set_locked(true);

    // Backdate the stretch so the base window is already open.
    {
        let mut inner = state.lock();
        inner.auto_unlock.as_mut().unwrap().stretch_start =
            std::time::Instant::now() - Duration::from_secs(3601);
    }

    // Two consecutive re-engagements while still locked.
    state.set_locked(true);
    state.set_locked(true);

    // The anchor survived both re-engagements: window 0 is still open.
    assert_eq!(state.get_auto_unlock_remaining_secs(), Some(0));
}

#[test]
fn test_auto_unlock_does_not_fire_when_disabled() {
    let state = AppState::new();
    state.set_locked(true);
    thread::sleep(Duration::from_millis(50));
    assert!(!state.should_auto_unlock());
    assert!(state.get_auto_unlock_remaining_secs().is_none());
}

#[test]
fn test_inner_auto_unlock_state_shape() {
    // AppStateInner must carry the schedule in one struct (§2.7)
    let state = AppState::new();
    enable_backoff(&state, 3600);
    let inner: parking_lot::MutexGuard<AppStateInner> = state.lock();
    let unlock = inner.auto_unlock.as_ref().expect("enabled => Some");
    assert_eq!(unlock.base_interval_secs, 3600);
    assert_eq!(unlock.window_index, 0);
}

#[test]
fn test_window_fires_on_schedule_only() {
    // §2.2: the window fires on SCHEDULE alone — elapsed >= interval —
    // regardless of input while locked. The 120 s idle bound is the re-lock
    // bound within the UNLOCKED window afterwards, handled by the auto-lock
    // thread. (A blocked-key masher must NOT be able to hold the lock by
    // keeping last_input_time fresh — that would be an indefinite lockout.)
    let state = AppState::new();
    enable_backoff(&state, 3600);
    {
        let mut inner = state.lock();
        inner.auto_lock_timeout = 2; // small cap for testing
    }
    state.set_locked(true);
    {
        let mut inner = state.lock();
        inner.auto_unlock.as_mut().unwrap().stretch_start =
            std::time::Instant::now() - Duration::from_secs(3601);
    }
    assert!(state.should_auto_unlock(), "Window fires on schedule even with stale idle");

    // Fresh input while locked (masher) must NOT block the fire.
    state.update_input_time();
    assert!(state.should_auto_unlock(), "Masher input must not hold the lock");

    // Before the interval elapses, no fire even with fresh input.
    let state2 = AppState::new();
    enable_backoff(&state2, 3600);
    state2.set_locked(true);
    assert!(!state2.should_auto_unlock());
    state2.update_input_time();
    assert!(!state2.should_auto_unlock(), "No fire before interval regardless of input");
}


#[test]
fn test_custom_base_interval_used() {
    // The resolved base interval (config/env override) must reach the schedule
    let state = AppState::new();
    enable_backoff(&state, 300);
    state.set_locked(true);
    assert_eq!(state.get_auto_unlock_interval_secs(), Some(300));
}
