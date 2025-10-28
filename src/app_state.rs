use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Instant;

/// Application state shared across modules
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Mutex<AppStateInner>>,
}

pub struct AppStateInner {
    /// Whether input is currently locked
    pub is_locked: bool,
    /// Buffer for passphrase input
    pub input_buffer: String,
    /// Last time any key was pressed (for buffer reset)
    pub last_key_time: Option<Instant>,
    /// Last time any input occurred (for auto-lock)
    pub last_input_time: Instant,
    /// Current passphrase hash (SHA-256, hex-encoded)
    pub passphrase_hash: Option<String>,
    /// Auto-lock timeout in seconds (default: 300 = 5 minutes)
    pub auto_lock_timeout: u64,
    /// Input buffer reset timeout in seconds (default: 5)
    pub buffer_reset_timeout: u64,
    /// Whether the Talk hotkey is currently pressed (for passthrough)
    pub talk_key_pressed: bool,
    /// Timestamp when device was locked (for auto-unlock)
    pub lock_start_time: Option<Instant>,
    /// Auto-unlock timeout in seconds (None = disabled)
    pub auto_unlock_timeout: Option<u64>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(AppStateInner {
                is_locked: false,
                input_buffer: String::new(),
                last_key_time: None,
                last_input_time: Instant::now(),
                passphrase_hash: None,
                auto_lock_timeout: 300,
                buffer_reset_timeout: 5,
                talk_key_pressed: false,
                lock_start_time: None,
                auto_unlock_timeout: None,
            })),
        }
    }

    pub fn lock(&self) -> parking_lot::MutexGuard<'_, AppStateInner> {
        self.inner.lock()
    }

    pub fn is_locked(&self) -> bool {
        self.inner.lock().is_locked
    }

    pub fn set_locked(&self, locked: bool) {
        let mut state = self.inner.lock();
        state.is_locked = locked;

        if locked {
            // Record when lock was engaged
            state.lock_start_time = Some(Instant::now());
            log::debug!("Lock engaged at {:?}", state.lock_start_time);
        } else {
            // Clear lock time when manually unlocked
            state.lock_start_time = None;
            log::debug!("Lock disengaged");
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

    pub fn append_to_buffer(&self, ch: char) {
        let mut state = self.inner.lock();
        state.input_buffer.push(ch);
    }

    pub fn clear_buffer(&self) {
        let mut state = self.inner.lock();
        state.input_buffer.clear();
    }

    pub fn get_buffer(&self) -> String {
        self.inner.lock().input_buffer.clone()
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

    pub fn should_auto_lock(&self) -> bool {
        let state = self.inner.lock();
        !state.is_locked && state.last_input_time.elapsed().as_secs() >= state.auto_lock_timeout
    }

    pub fn get_auto_lock_remaining_secs(&self) -> Option<u64> {
        let state = self.inner.lock();
        if state.is_locked {
            return None;
        }
        let elapsed = state.last_input_time.elapsed().as_secs();
        Some(state.auto_lock_timeout.saturating_sub(elapsed))
    }

    pub fn set_talk_key_pressed(&self, pressed: bool) {
        self.inner.lock().talk_key_pressed = pressed;
    }

    pub fn is_talk_key_pressed(&self) -> bool {
        self.inner.lock().talk_key_pressed
    }

    /// Sets the auto-unlock timeout (called at startup)
    pub fn set_auto_unlock_timeout(&self, timeout_seconds: Option<u64>) {
        let mut state = self.inner.lock();
        state.auto_unlock_timeout = timeout_seconds;
    }

    /// Check if auto-unlock should trigger
    pub fn should_auto_unlock(&self) -> bool {
        let state = self.inner.lock();

        // Must be locked and have timeout configured
        if !state.is_locked || state.auto_unlock_timeout.is_none() {
            return false;
        }

        // Must have recorded lock start time
        let lock_start = match state.lock_start_time {
            Some(time) => time,
            None => return false,
        };

        let timeout = std::time::Duration::from_secs(state.auto_unlock_timeout.unwrap());
        lock_start.elapsed() >= timeout
    }

    /// Trigger auto-unlock (called by background thread)
    pub fn trigger_auto_unlock(&self) {
        let mut state = self.inner.lock();

        if state.is_locked {
            let elapsed = state.lock_start_time
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);

            log::warn!("AUTO-UNLOCK TRIGGERED after {} seconds", elapsed);

            state.is_locked = false;
            state.lock_start_time = None;
            state.input_buffer.clear();
        }
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
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_auto_unlock_disabled_by_default() {
        let state = AppState::new();
        state.set_locked(true);
        thread::sleep(Duration::from_secs(1));
        assert!(!state.should_auto_unlock(), "Auto-unlock should be disabled by default");
    }

    #[test]
    fn test_auto_unlock_timeout_triggers() {
        let state = AppState::new();
        state.set_auto_unlock_timeout(Some(2)); // 2 seconds for testing

        // Lock the device
        state.set_locked(true);

        // Should not trigger immediately
        assert!(!state.should_auto_unlock(), "Should not trigger immediately after lock");

        // Wait for timeout
        thread::sleep(Duration::from_secs(3));

        // Should trigger after timeout
        assert!(state.should_auto_unlock(), "Should trigger after timeout expires");
    }

    #[test]
    fn test_auto_unlock_reset_on_manual_unlock() {
        let state = AppState::new();
        state.set_auto_unlock_timeout(Some(2)); // 2 seconds for testing

        // Lock the device
        state.set_locked(true);
        thread::sleep(Duration::from_millis(500));

        // Manual unlock before timeout
        state.set_locked(false);

        // Wait past the original timeout
        thread::sleep(Duration::from_secs(2));

        // Should not trigger after manual unlock
        assert!(!state.should_auto_unlock(), "Should not trigger after manual unlock");
    }

    #[test]
    fn test_auto_unlock_lock_unlock_lock_cycles() {
        let state = AppState::new();
        state.set_auto_unlock_timeout(Some(1)); // 1 second for testing

        // First lock cycle
        state.set_locked(true);
        thread::sleep(Duration::from_millis(500));
        state.set_locked(false);

        // Second lock cycle (timer should start fresh)
        state.set_locked(true);
        thread::sleep(Duration::from_millis(500));

        // Should not trigger yet (only 500ms into second cycle)
        assert!(!state.should_auto_unlock(), "Should not trigger in middle of second cycle");

        // Wait for second cycle to complete
        thread::sleep(Duration::from_millis(600));

        // Should trigger now
        assert!(state.should_auto_unlock(), "Should trigger after second cycle timeout");
    }

    #[test]
    fn test_trigger_auto_unlock_clears_state() {
        let state = AppState::new();
        state.set_auto_unlock_timeout(Some(1));

        // Add some input to buffer
        state.append_to_buffer('t');
        state.append_to_buffer('e');
        state.append_to_buffer('s');
        state.append_to_buffer('t');

        // Lock the device
        state.set_locked(true);

        // Trigger auto-unlock
        state.trigger_auto_unlock();

        // Verify state is cleared
        assert!(!state.is_locked(), "Should be unlocked after trigger");
        assert_eq!(state.get_buffer(), "", "Buffer should be cleared");

        // Verify lock_start_time is cleared
        let inner = state.lock();
        assert!(inner.lock_start_time.is_none(), "Lock start time should be None");
    }

    #[test]
    fn test_auto_unlock_only_when_locked() {
        let state = AppState::new();
        state.set_auto_unlock_timeout(Some(1));

        // Device is unlocked, wait past timeout
        thread::sleep(Duration::from_secs(2));

        // Should not trigger when device is not locked
        assert!(!state.should_auto_unlock(), "Should not trigger when device is unlocked");
    }

    #[test]
    fn test_auto_unlock_minimum_timeout() {
        let state = AppState::new();
        state.set_auto_unlock_timeout(Some(1)); // 1 second (below 10s minimum in production)

        state.set_locked(true);

        // Should not trigger immediately
        assert!(!state.should_auto_unlock());

        // Wait for timeout
        thread::sleep(Duration::from_millis(1100));

        // Should trigger after 1 second
        assert!(state.should_auto_unlock(), "Should work with minimum timeout");
    }

    #[test]
    fn test_set_auto_unlock_timeout_changes_config() {
        let state = AppState::new();

        // Initially None
        {
            let inner = state.lock();
            assert!(inner.auto_unlock_timeout.is_none());
        }

        // Set to 30 seconds
        state.set_auto_unlock_timeout(Some(30));
        {
            let inner = state.lock();
            assert_eq!(inner.auto_unlock_timeout, Some(30));
        }

        // Set to None (disable)
        state.set_auto_unlock_timeout(None);
        {
            let inner = state.lock();
            assert!(inner.auto_unlock_timeout.is_none());
        }
    }

    #[test]
    fn test_lock_start_time_recorded() {
        let state = AppState::new();

        // Initially None
        {
            let inner = state.lock();
            assert!(inner.lock_start_time.is_none(), "Lock start time should be None initially");
        }

        // Lock the device
        state.set_locked(true);

        // Should have recorded start time
        {
            let inner = state.lock();
            assert!(inner.lock_start_time.is_some(), "Lock start time should be recorded");
        }

        // Unlock the device
        state.set_locked(false);

        // Should clear start time
        {
            let inner = state.lock();
            assert!(inner.lock_start_time.is_none(), "Lock start time should be cleared on unlock");
        }
    }
}
