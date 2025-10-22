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
    /// Auto-lock timeout in seconds (default: 180 = 3 minutes)
    pub auto_lock_timeout: u64,
    /// Input buffer reset timeout in seconds (default: 5)
    pub buffer_reset_timeout: u64,
    /// Whether the Talk hotkey is currently pressed (for passthrough)
    pub talk_key_pressed: bool,
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
                auto_lock_timeout: 180,
                buffer_reset_timeout: 5,
                talk_key_pressed: false,
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
        self.inner.lock().is_locked = locked;
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

    pub fn set_talk_key_pressed(&self, pressed: bool) {
        self.inner.lock().talk_key_pressed = pressed;
    }

    pub fn is_talk_key_pressed(&self) -> bool {
        self.inner.lock().talk_key_pressed
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
