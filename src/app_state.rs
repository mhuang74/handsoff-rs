use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Instant;

// Re-export constants for backward compatibility
use crate::constants::REENABLE_DEBOUNCE_SECS;
pub use crate::constants::{
    AUTO_LOCK_DEFAULT_SECONDS, AUTO_LOCK_MAX_SECONDS, AUTO_LOCK_MIN_SECONDS,
    AUTO_UNLOCK_BASE_SECONDS, AUTO_UNLOCK_CEILING_SECONDS, BUFFER_RESET_DEFAULT_SECONDS,
    DEFAULT_LOCK_KEYCODE, DEFAULT_TALK_KEYCODE,
};

/// Application state shared across modules
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Mutex<AppStateInner>>,
}

/// Auto-unlock backoff schedule state (specs/deep-design-review-v2-2026-09.md §2).
///
/// The counter is keyed to the *locked stretch* (time since the last successful
/// passphrase authentication), not to individual lock events: auto-lock
/// re-engagements inside a stretch do not reset the doubling schedule. Only a
/// successful passphrase unlock resets it (§2.3).
#[derive(Debug)]
pub struct AutoUnlockState {
    /// Base interval in seconds (configurable via config/env override);
    /// windows open at `base * 2^window_index` of awake-time, capped at the
    /// ceiling.
    pub base_interval_secs: u64,
    /// Awake-time instant the current window clock is anchored to
    /// (set only on the unlocked → locked transition of a locked stretch)
    pub stretch_start: Instant,
    /// Index of the next auto-unlock window (0-based; the Nth window opens
    /// `interval(N)` after the current anchor, where interval doubles per N
    /// and is capped at the ceiling). Only a successful passphrase unlock
    /// resets this to 0 (§2.3).
    pub window_index: u32,
}

pub struct AppStateInner {
    /// Whether input is currently locked
    pub is_locked: bool,
    /// Buffer of physical keycodes typed while locked (canonical passphrase form)
    pub input_buffer: Vec<u32>,
    /// Last time any key was pressed (for buffer reset)
    pub last_key_time: Option<Instant>,
    /// Last time any input occurred (for auto-lock)
    pub last_input_time: Instant,
    /// Current passphrase hash (SHA-256 hex over the keycode sequence)
    pub passphrase_hash: Option<String>,
    /// Auto-lock timeout in seconds (see AUTO_LOCK_DEFAULT_SECONDS)
    pub auto_lock_timeout: u64,
    /// Input buffer reset timeout in seconds (see BUFFER_RESET_DEFAULT_SECONDS)
    pub buffer_reset_timeout: u64,
    /// Whether the Talk hotkey is currently pressed (for passthrough)
    pub talk_key_pressed: bool,
    /// Timestamp when device was locked (for auto-lock elapsed display)
    pub lock_start_time: Option<Instant>,
    /// Auto-unlock backoff schedule state (None = disabled)
    pub auto_unlock: Option<AutoUnlockState>,
    /// Cached accessibility permissions state (updated by background thread)
    pub has_accessibility_permissions: bool,
    /// Flag to signal that event tap should be stopped (set by permission monitor)
    pub should_stop_event_tap: bool,
    /// Flag to signal that event tap should be started (set by permission monitor on restoration)
    pub should_start_event_tap: bool,
    /// Flag to signal that the existing event tap should be re-enabled (set on DISABLED_BY_TIMEOUT)
    /// This is different from should_start_event_tap: re-enable reuses the existing tap handle
    /// rather than destroying and creating a new WindowServer connection.
    pub should_reenable_event_tap: bool,
    /// Timestamp when event tap was last re-enabled (for debouncing)
    pub last_reenable_time: Option<Instant>,
    /// Flag to signal that app should exit (CLI only - set by event tap callback on permission loss)
    pub should_exit: bool,
    /// Whether the app is currently disabled (minimal CPU mode)
    pub is_disabled: bool,
    /// Lock hotkey keycode (macOS keycode, see DEFAULT_LOCK_KEYCODE)
    pub lock_keycode: i64,
    /// Talk hotkey keycode (macOS keycode, see DEFAULT_TALK_KEYCODE)
    pub talk_keycode: i64,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(AppStateInner {
                is_locked: false,
                input_buffer: Vec::new(),
                last_key_time: None,
                last_input_time: Instant::now(),
                passphrase_hash: None,
                auto_lock_timeout: AUTO_LOCK_DEFAULT_SECONDS,
                buffer_reset_timeout: BUFFER_RESET_DEFAULT_SECONDS,
                talk_key_pressed: false,
                lock_start_time: None,
                auto_unlock: None,
                has_accessibility_permissions: false,
                should_stop_event_tap: false,
                should_start_event_tap: false,
                should_reenable_event_tap: false,
                last_reenable_time: None,
                should_exit: false,
                is_disabled: false,
                lock_keycode: DEFAULT_LOCK_KEYCODE,
                talk_keycode: DEFAULT_TALK_KEYCODE,
            })),
        }
    }

    pub fn lock(&self) -> parking_lot::MutexGuard<'_, AppStateInner> {
        self.inner.lock()
    }

    pub fn is_locked(&self) -> bool {
        self.inner.lock().is_locked
    }

    /// Engage the lock.
    ///
    /// Records the lock time; on the unlocked → locked transition of a locked
    /// stretch, starts (or re-anchors) the backoff schedule. Auto-lock
    /// re-engagements (locked → locked) leave the schedule anchor untouched:
    /// the counter is keyed to the locked stretch, not lock events (§2.3).
    pub fn set_locked(&self, locked: bool) {
        let mut state = self.inner.lock();
        let was_locked = state.is_locked;
        state.is_locked = locked;

        if locked {
            // Record when lock was engaged
            state.lock_start_time = Some(Instant::now());

            // Anchor the window clock only on a genuinely fresh locked
            // stretch (window_index == 0: first lock, or a passphrase
            // unlock / Reset that reset the counter). Re-locking inside a
            // stretch keeps the anchor untouched (§2.3 — only a successful
            // passphrase unlock resets the counter). After a fired window
            // (window_index > 0) the anchor set by trigger_auto_unlock
            // stands, so re-locks never move the schedule (§2.3) and each
            // next window lands on the §2.1 cumulative timeline regardless
            // of re-lock timing.
            if let Some(unlock) = &mut state.auto_unlock {
                if !was_locked && unlock.window_index == 0 {
                    unlock.stretch_start = Instant::now();
                    log::debug!(
                        "Locked stretch started; auto-unlock window {} opens after {}s",
                        unlock.window_index,
                        Self::auto_unlock_interval_secs(
                            unlock.base_interval_secs,
                            unlock.window_index
                        ),
                    );
                }
                // locked → locked, or window already fired: leave
                // stretch_start untouched
            }
            log::debug!("Lock engaged at {:?}", state.lock_start_time);
        } else {
            // Clear lock time when manually unlocked
            state.lock_start_time = None;
            log::debug!("Lock disengaged");
        }
    }

    /// Configure auto-unlock (called at startup).
    ///
    /// `AutoUnlockConfig::Disabled` fully disables the schedule;
    /// `AutoUnlockConfig::Backoff` sets the first window's interval
    /// (overridable via config/env). The anchor is a placeholder until the
    /// first unlocked → locked transition.
    pub fn set_auto_unlock_config(&self, config: crate::config::AutoUnlockConfig) {
        let mut state = self.inner.lock();
        state.auto_unlock = match config {
            crate::config::AutoUnlockConfig::Disabled => None,
            crate::config::AutoUnlockConfig::Backoff { base_interval_secs } => {
                Some(AutoUnlockState {
                    base_interval_secs: base_interval_secs.get(),
                    stretch_start: Instant::now(),
                    window_index: 0,
                })
            }
        };
    }

    pub fn auto_unlock_enabled(&self) -> bool {
        self.inner.lock().auto_unlock.is_some()
    }

    /// Interval in seconds of the auto-unlock window at `index`:
    /// `base * 2^index` capped at the ceiling (§2.1). Window 0 is measured
    /// from the locked-stretch anchor (lock time); window N>0 is measured
    /// from window N−1's OPENING (set by `trigger_auto_unlock` at fire
    /// time), so cumulative open times reproduce the §2.1 timeline
    /// t=60m, 180m, 420m, 900m… for base=60m regardless of when auto-lock
    /// re-engages between windows.
    pub fn auto_unlock_interval_secs(base_secs: u64, window_index: u32) -> u64 {
        let doubled =
            base_secs.saturating_mul(1u64 << window_index.min(u32::from(u64::BITS - 1) as u32));
        doubled.min(AUTO_UNLOCK_CEILING_SECONDS)
    }

    /// Check whether an auto-unlock window has opened on schedule (§2).
    ///
    /// Fires when locked, auto-unlock is enabled, and
    /// `stretch_start + interval(window_index) <= now` (awake-time). The fire
    /// is schedule-only: the window is the UNLOCKED period that follows
    /// (§2.2), during which `AUTO_LOCK_DEFAULT_SECONDS` of no input re-locks
    /// the machine — that 120 s bound is realized by the auto-lock thread
    /// once `trigger_auto_unlock` resets `last_input_time`. "An at-keyboard
    /// masher can hold a window open" (§2.2) refers to that unlocked period
    /// — an accepted weakness, NOT a reason to hold the lock. The schedule
    /// counter is NOT reset by the fire (§2.3).
    pub fn should_auto_unlock(&self) -> bool {
        let state = self.inner.lock();

        if !state.is_locked {
            return false;
        }
        let Some(unlock) = state.auto_unlock.as_ref() else {
            return false;
        };

        let interval =
            Self::auto_unlock_interval_secs(unlock.base_interval_secs, unlock.window_index);
        unlock.stretch_start.elapsed().as_secs() >= interval
    }

    /// Successful passphrase unlock: reset the backoff schedule to the base
    /// interval (§2.3 — the linchpin rule) and clear locked state.
    ///
    /// Also resets `last_input_time` so auto-lock does not immediately
    /// re-engage right after unlock.
    pub fn complete_passphrase_unlock(&self) {
        let mut state = self.inner.lock();

        log::info!("Passphrase accepted - input unlocked");

        state.last_input_time = Instant::now();
        state.is_locked = false;
        state.lock_start_time = None;
        state.input_buffer.clear();
        state.last_key_time = None;

        if let Some(unlock) = &mut state.auto_unlock {
            unlock.window_index = 0;
            unlock.stretch_start = Instant::now();
        }
    }

    /// User-initiated Reset: clears locked state and restarts the schedule
    /// from base. This is an intentional recovery action by the operator
    /// (menu access = past the guard), not a passphrase authentication event
    /// — logged as such.
    pub fn reset_all(&self) {
        let mut state = self.inner.lock();

        log::info!("Reset: state cleared, backoff schedule restarted from base");

        state.last_input_time = Instant::now();
        state.is_locked = false;
        state.lock_start_time = None;
        state.input_buffer.clear();
        state.last_key_time = None;

        if let Some(unlock) = &mut state.auto_unlock {
            unlock.window_index = 0;
            unlock.stretch_start = Instant::now();
        }
    }

    /// Trigger auto-unlock (a backoff window fired). Unlocks without
    /// authentication.
    ///
    /// Owns the full fired-window transition: advances the backoff counter
    /// (§2.1/§2.3 — the window is consumed; if the machine re-locks, the next
    /// window is further out) and unlocks WITHOUT resetting the schedule —
    /// only successful passphrase auth resets it (§2.3).
    pub fn trigger_auto_unlock(&self) {
        let mut state = self.inner.lock();

        if state.is_locked {
            let elapsed = state
                .lock_start_time
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);

            log::warn!("AUTO-UNLOCK WINDOW FIRED after {}s awake-time", elapsed);

            // Consume the window: double the interval for the next stretch.
            if let Some(unlock) = &mut state.auto_unlock {
                unlock.window_index = unlock.window_index.saturating_add(1);
                // Anchor the next interval at THIS window's opening (§2.1
                // cumulative timeline): window N>0 opens interval(N) after
                // window N-1 opened, independent of when auto-lock re-engages
                // or how long the window stays open.
                unlock.stretch_start = Instant::now();
                log::info!(
                    "Auto-unlock backoff advanced: next window opens after {}s",
                    Self::auto_unlock_interval_secs(unlock.base_interval_secs, unlock.window_index)
                );
            }

            // Reset last_input_time for fresh auto-lock countdown
            // Note: if don't do this first, auto-lock may kick in right after unlock
            state.last_input_time = Instant::now();

            state.is_locked = false;
            state.lock_start_time = None;
            state.input_buffer.clear();
            state.last_key_time = None;
        }
    }

    pub fn update_input_time(&self) {
        let mut state = self.inner.lock();
        state.last_input_time = Instant::now();
    }

    pub fn update_key_time(&self) {
        let mut state = self.inner.lock();
        state.last_key_time = Some(Instant::now());
    }

    pub fn append_to_buffer(&self, keycode: u32) {
        let mut state = self.inner.lock();
        state.input_buffer.push(keycode);
    }

    pub fn pop_buffer(&self) {
        let mut state = self.inner.lock();
        state.input_buffer.pop();
    }

    pub fn clear_buffer(&self) {
        let mut state = self.inner.lock();
        state.input_buffer.clear();
    }

    /// Snapshot of the current buffer (for length-only logging — S-1: never
    /// log buffer contents).
    pub fn buffer_len(&self) -> usize {
        self.inner.lock().input_buffer.len()
    }

    pub fn set_passphrase_hash(&self, hash: String) {
        self.inner.lock().passphrase_hash = Some(hash);
    }

    pub fn get_passphrase_hash(&self) -> Option<String> {
        self.inner.lock().passphrase_hash.clone()
    }

    pub fn should_reset_buffer(&self) -> bool {
        let state = self.inner.lock();
        if let Some(last_key) = state.last_key_time {
            last_key.elapsed().as_secs() >= state.buffer_reset_timeout
        } else {
            false
        }
    }

    /// Idle seconds = the most recent of ANY input activity. Mouse moves no
    /// longer flow through our tap, so the system-wide CGEventSource idle clock
    /// is authoritative for them; the tap-maintained `last_input_time` covers
    /// everything else (keyboard, clicks, drags, scroll) — and, critically, is
    /// reset by `trigger_auto_unlock` so the post-unlock window stays open for
    /// the full auto-lock timeout. Take the minimum of both clocks: a fresh
    /// value from either source defers the lock. Callers pass the elapsed
    /// `last_input_time` (they already hold the state lock).
    fn current_idle_secs(last_input_elapsed_secs: f64) -> f64 {
        match crate::input_blocking::event_tap::seconds_since_last_input() {
            Some(cg) => cg.min(last_input_elapsed_secs),
            // API unavailable (CI/test environments, query failure): fall back
            // to the tap-maintained clock alone.
            None => last_input_elapsed_secs,
        }
    }

    pub fn should_auto_lock(&self) -> bool {
        let state = self.inner.lock();
        let idle_secs = Self::current_idle_secs(state.last_input_time.elapsed().as_secs() as f64);
        // Only auto-lock if: not locked, timeout exceeded, AND permissions are available
        // This prevents auto-lock from triggering when permissions are lost
        !state.is_locked
            && idle_secs >= state.auto_lock_timeout as f64
            && state.has_accessibility_permissions
    }

    pub fn get_auto_lock_remaining_secs(&self) -> Option<u64> {
        let state = self.inner.lock();
        if state.is_locked {
            return None;
        }
        let idle_secs = Self::current_idle_secs(state.last_input_time.elapsed().as_secs() as f64);
        // Truncate (floor) to match should_auto_lock's boundary: it fires when
        // raw idle_secs >= timeout, so remaining must reach 0 at the same point.
        Some(state.auto_lock_timeout.saturating_sub(idle_secs as u64))
    }

    pub fn set_talk_key_pressed(&self, pressed: bool) {
        self.inner.lock().talk_key_pressed = pressed;
    }

    pub fn is_talk_key_pressed(&self) -> bool {
        self.inner.lock().talk_key_pressed
    }

    /// Get the elapsed time since lock was engaged (in seconds)
    pub fn get_lock_elapsed_secs(&self) -> Option<u64> {
        let state = self.inner.lock();
        state.lock_start_time.map(|t| t.elapsed().as_secs())
    }

    /// Seconds until the current auto-unlock window opens (awake-time).
    /// `None` when unlocked or disabled. Returns `Some(0)` when the window is
    /// already open. The displayed value is the schedule time; the window
    /// itself stays open only while input keeps arriving (§2.2).
    pub fn get_auto_unlock_remaining_secs(&self) -> Option<u64> {
        let state = self.inner.lock();
        let unlock = state.auto_unlock.as_ref()?;
        if !state.is_locked {
            return None;
        }
        let interval =
            Self::auto_unlock_interval_secs(unlock.base_interval_secs, unlock.window_index);
        let elapsed = unlock.stretch_start.elapsed().as_secs();
        Some(interval.saturating_sub(elapsed))
    }

    /// Interval of the currently-pending auto-unlock window in seconds.
    /// `None` when auto-unlock is disabled.
    pub fn get_auto_unlock_interval_secs(&self) -> Option<u64> {
        let state = self.inner.lock();
        state
            .auto_unlock
            .as_ref()
            .map(|u| Self::auto_unlock_interval_secs(u.base_interval_secs, u.window_index))
    }

    /// Get cached accessibility permissions state
    pub fn get_cached_accessibility_permissions(&self) -> bool {
        self.inner.lock().has_accessibility_permissions
    }

    /// Set cached accessibility permissions state (called by permission monitor thread)
    pub fn set_cached_accessibility_permissions(&self, has_permissions: bool) {
        self.inner.lock().has_accessibility_permissions = has_permissions;
    }

    /// Request event tap to be stopped (called by permission monitor when permissions lost)
    pub fn request_stop_event_tap(&self) {
        self.inner.lock().should_stop_event_tap = true;
    }

    /// Check if event tap should be stopped and clear the flag
    pub fn should_stop_event_tap_and_clear(&self) -> bool {
        let mut state = self.inner.lock();
        let should_stop = state.should_stop_event_tap;
        state.should_stop_event_tap = false;
        should_stop
    }

    /// Request event tap to be started (called by permission monitor when permissions restored)
    pub fn request_start_event_tap(&self) {
        self.inner.lock().should_start_event_tap = true;
    }

    /// Check if event tap should be started and clear the flag
    pub fn should_start_event_tap_and_clear(&self) -> bool {
        let mut state = self.inner.lock();
        let should_start = state.should_start_event_tap;
        state.should_start_event_tap = false;
        should_start
    }

    /// Request that the existing event tap be re-enabled without creating a new one.
    /// Called when macOS disables the tap due to callback timeout (e.g. after sleep/wake).
    /// Unlike request_start_event_tap, this reuses the existing CGEventTapRef so no new
    /// WindowServer connection is created — avoiding zombie Mach port accumulation.
    pub fn request_reenable_event_tap(&self) {
        self.inner.lock().should_reenable_event_tap = true;
    }

    /// Check if the existing event tap should be re-enabled and clear the flag.
    /// Includes debouncing: skips re-enable if done within the last REENABLE_DEBOUNCE_SECS.
    pub fn should_reenable_event_tap_and_clear(&self) -> bool {
        let mut state = self.inner.lock();
        if !state.should_reenable_event_tap {
            return false;
        }

        // Debounce: skip if re-enabled in last REENABLE_DEBOUNCE_SECS seconds
        // NOTE: Do NOT clear the flag here - if debounce blocks, keep the flag set
        // so the next check after debounce expires will retry the re-enable.
        // Clearing the flag here caused the tap to stay disabled permanently when
        // macOS rapidly disabled the tap multiple times during sleep/wake.
        if let Some(last) = state.last_reenable_time {
            if last.elapsed().as_secs() < REENABLE_DEBOUNCE_SECS {
                log::info!(
                    "[tap-lifecycle] Skipping re-enable (debounce: {:?} ago, will retry later)",
                    last.elapsed()
                );
                return false;
            }
        }

        state.should_reenable_event_tap = false;
        true
    }

    /// Mark that event tap was just re-enabled (for debouncing)
    pub fn mark_reenable_completed(&self) {
        self.inner.lock().last_reenable_time = Some(Instant::now());
    }

    /// Request that the application exit (CLI only)
    pub fn request_exit(&self) {
        self.inner.lock().should_exit = true;
    }

    /// Check if app should exit and clear the flag
    pub fn should_exit_and_clear(&self) -> bool {
        let mut state = self.inner.lock();
        let should_exit = state.should_exit;
        state.should_exit = false;
        should_exit
    }

    /// Check if the app is currently disabled
    pub fn is_disabled(&self) -> bool {
        self.inner.lock().is_disabled
    }

    /// Set the disabled state
    pub fn set_disabled(&self, disabled: bool) {
        self.inner.lock().is_disabled = disabled;
    }

    /// Set the lock hotkey keycode (macOS keycode)
    pub fn set_lock_keycode(&self, keycode: i64) {
        self.inner.lock().lock_keycode = keycode;
    }

    /// Set the talk hotkey keycode (macOS keycode)
    pub fn set_talk_keycode(&self, keycode: i64) {
        self.inner.lock().talk_keycode = keycode;
    }

    /// Get the lock hotkey keycode (macOS keycode)
    pub fn get_lock_keycode(&self) -> i64 {
        self.inner.lock().lock_keycode
    }

    /// Get the talk hotkey keycode (macOS keycode)
    pub fn get_talk_keycode(&self) -> i64 {
        self.inner.lock().talk_keycode
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn enable_backoff(state: &AppState, base_secs: u64) {
        state.set_auto_unlock_config(crate::config::AutoUnlockConfig::Backoff {
            base_interval_secs: std::num::NonZeroU64::new(base_secs).unwrap(),
        });
    }

    #[test]
    fn test_auto_unlock_disabled_by_default() {
        let state = AppState::new();
        state.set_locked(true);
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !state.should_auto_unlock(),
            "Auto-unlock should be disabled by default"
        );
        assert!(!state.auto_unlock_enabled());
    }

    #[test]
    fn test_interval_schedule_doubles_and_caps() {
        // Base interval windows: 60min, 120min, 240min... capped at 24h
        assert_eq!(AppState::auto_unlock_interval_secs(3600, 0), 3600);
        assert_eq!(AppState::auto_unlock_interval_secs(3600, 1), 7200);
        assert_eq!(AppState::auto_unlock_interval_secs(3600, 2), 14400);
        assert_eq!(AppState::auto_unlock_interval_secs(3600, 3), 28800);
        assert_eq!(AppState::auto_unlock_interval_secs(3600, 4), 57600);
        // 115200 would exceed the 86400 ceiling
        assert_eq!(AppState::auto_unlock_interval_secs(3600, 5), 86400);
        assert_eq!(AppState::auto_unlock_interval_secs(3600, 6), 86400);
        assert_eq!(AppState::auto_unlock_interval_secs(3600, 100), 86400);
    }

    #[test]
    fn test_first_window_opens_at_base() {
        let state = AppState::new();
        enable_backoff(&state, AUTO_UNLOCK_BASE_SECONDS);
        state.set_locked(true);

        // Window not open yet (base = 3600s, we just locked)
        assert!(!state.should_auto_unlock());
        assert_eq!(state.get_auto_unlock_remaining_secs(), Some(3600));
    }

    #[test]
    fn test_relock_does_not_reset_schedule() {
        // §2.3: re-locks inside a stretch must NOT restart the schedule.
        let state = AppState::new();
        enable_backoff(&state, AUTO_UNLOCK_BASE_SECONDS);

        // Simulate a stretch where the first window already fired.
        state.set_locked(true);
        {
            let mut inner = state.lock();
            // Backdate the stretch so window 0 is open
            inner.auto_unlock.as_mut().unwrap().stretch_start =
                Instant::now() - Duration::from_secs(3601);
            inner.last_input_time = Instant::now() - Duration::from_secs(3601);
        }
        assert!(state.should_auto_unlock());

        // Auto-unlock fires (no auth); trigger_auto_unlock consumes the window.
        state.trigger_auto_unlock();
        assert!(!state.is_locked());

        // Re-lock must keep the advanced position, not restart at base.
        // set_locked keeps both the anchor and window_index; only unlock
        // (passphrase auth or Reset) resets the schedule.
        state.set_locked(true);
        assert_eq!(state.get_auto_unlock_interval_secs(), Some(7200));
        assert!(!state.should_auto_unlock());
    }

    #[test]
    fn test_passphrase_unlock_resets_schedule_to_base() {
        // §2.3 linchpin: only successful auth resets the counter.
        let state = AppState::new();
        enable_backoff(&state, AUTO_UNLOCK_BASE_SECONDS);
        state.set_locked(true);
        {
            let mut inner = state.lock();
            let u = inner.auto_unlock.as_mut().unwrap();
            u.stretch_start = Instant::now() - Duration::from_secs(40000);
            u.window_index = 3; // 28800s interval, long past
            inner.last_input_time = Instant::now() - Duration::from_secs(40000);
        }
        assert!(state.should_auto_unlock());

        state.complete_passphrase_unlock();
        assert!(!state.is_locked());
        assert_eq!(state.get_auto_unlock_interval_secs(), Some(3600));
        assert_eq!(state.get_auto_unlock_remaining_secs(), None); // unlocked

        // Re-lock: first window again at base.
        state.set_locked(true);
        assert_eq!(state.get_auto_unlock_interval_secs(), Some(3600));
        assert!(!state.should_auto_unlock());
    }

    #[test]
    fn test_complete_passphrase_unlock_clears_state() {
        let state = AppState::new();
        enable_backoff(&state, AUTO_UNLOCK_BASE_SECONDS);
        state.append_to_buffer(0);
        state.append_to_buffer(12);
        state.set_locked(true);

        state.complete_passphrase_unlock();

        assert!(!state.is_locked());
        assert!(state.buffer_len() == 0, "Buffer should be cleared");
        let inner = state.lock();
        assert!(inner.lock_start_time.is_none());
    }

    #[test]
    fn test_reset_restarts_schedule_from_base() {
        // Reset is a user-intended recovery action (menu access = past the
        // guard): it clears locked state and restarts the schedule from base,
        // without logging a passphrase authentication event.
        let state = AppState::new();
        enable_backoff(&state, AUTO_UNLOCK_BASE_SECONDS);
        state.append_to_buffer(0);
        state.set_locked(true);
        {
            let mut inner = state.lock();
            let u = inner.auto_unlock.as_mut().unwrap();
            u.stretch_start = Instant::now() - Duration::from_secs(40000);
            u.window_index = 3; // 28800s interval, long past
            inner.last_input_time = Instant::now() - Duration::from_secs(40000);
        }
        assert!(state.should_auto_unlock());

        state.reset_all();

        assert!(!state.is_locked());
        assert_eq!(
            state.get_auto_unlock_interval_secs(),
            Some(AUTO_UNLOCK_BASE_SECONDS)
        );
        assert_eq!(state.buffer_len(), 0, "Buffer should be cleared");
        assert_eq!(state.get_auto_unlock_remaining_secs(), None); // unlocked

        // Re-lock: first window again at base.
        state.set_locked(true);
        assert_eq!(
            state.get_auto_unlock_interval_secs(),
            Some(AUTO_UNLOCK_BASE_SECONDS)
        );
        assert!(!state.should_auto_unlock());
    }

    #[test]
    fn test_trigger_auto_unlock_clears_state_and_advances_counter() {
        let state = AppState::new();
        enable_backoff(&state, AUTO_UNLOCK_BASE_SECONDS);
        state.append_to_buffer(0);
        state.set_locked(true);

        state.trigger_auto_unlock();

        assert!(!state.is_locked());
        assert_eq!(state.buffer_len(), 0, "Buffer should be cleared");
        // Window consumed: interval doubled, NOT reset (only passphrase auth resets)
        assert_eq!(state.get_auto_unlock_interval_secs(), Some(7200));
        let inner = state.lock();
        assert!(inner.lock_start_time.is_none());
    }

    #[test]
    fn test_auto_unlock_only_when_locked() {
        let state = AppState::new();
        enable_backoff(&state, AUTO_UNLOCK_BASE_SECONDS);
        {
            let mut inner = state.lock();
            inner.auto_unlock.as_mut().unwrap().stretch_start =
                Instant::now() - Duration::from_secs(7200);
        }

        // Unlocked: no auto-unlock
        assert!(!state.should_auto_unlock());
    }

    #[test]
    fn test_keycode_buffer_operations() {
        let state = AppState::new();
        state.append_to_buffer(0);
        state.append_to_buffer(12);
        assert_eq!(state.buffer_len(), 2);
        state.pop_buffer();
        assert_eq!(state.buffer_len(), 1);
        state.clear_buffer();
        assert_eq!(state.buffer_len(), 0);
    }

    #[test]
    fn test_auto_unlock_disable_clears_schedule() {
        let state = AppState::new();
        enable_backoff(&state, AUTO_UNLOCK_BASE_SECONDS);
        assert!(state.auto_unlock_enabled());
        state.set_auto_unlock_config(crate::config::AutoUnlockConfig::Disabled);
        assert!(!state.auto_unlock_enabled());
        state.set_locked(true);
        assert!(!state.should_auto_unlock());
    }

    #[test]
    fn test_lock_start_time_recorded() {
        let state = AppState::new();
        assert!(state.get_lock_elapsed_secs().is_none());
        state.set_locked(true);
        assert!(state.get_lock_elapsed_secs().is_some());
        state.set_locked(false);
        assert!(state.get_lock_elapsed_secs().is_none());
    }
}
