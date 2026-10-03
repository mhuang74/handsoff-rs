// Library interface for HandsOff
// This allows tests and other modules to access the crate's functionality

pub mod app_state;
pub mod auth;
pub mod config;
pub mod config_file;
pub mod constants;
pub mod input_blocking;
pub mod preferences;
pub mod setup;
pub mod utils;
pub mod wizard;

use anyhow::{Context, Result};
use app_state::AppState;
use constants::{
    AUTO_LOCK_CHECK_INTERVAL_SECS, AUTO_UNLOCK_CEILING_SECONDS, AUTO_UNLOCK_CHECK_INTERVAL_SECS,
    BUFFER_RESET_CHECK_INTERVAL_MS, CALLBACK_TELEMETRY_INTERVAL_SECS, CFRUNLOOP_POLL_INTERVAL_MS,
    PERMISSION_CHECK_INTERVAL_SECS,
};
use core_graphics::sys::CGEventTapRef;
use input_blocking::event_tap;
use input_blocking::hotkeys::HotkeyManager;
use log::{error, info, warn};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Return current wall-clock time as a human-readable string for correlation with external logs.
fn wall_clock_now() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => {
            let secs = d.as_secs();
            let h = (secs / 3600) % 24;
            let m = (secs / 60) % 60;
            let s = secs % 60;
            format!("{:02}:{:02}:{:02} UTC", h, m, s)
        }
        Err(_) => "unknown".to_string(),
    }
}

// Type alias for CFRunLoopSourceRef (from event_tap.rs)
type CFRunLoopSourceRef = *mut std::ffi::c_void;

/// What [`HandsOffCore::service_tap_lifecycle`] did this pass (issue #37 N3).
#[derive(Debug)]
pub enum TapLifecycleEvent {
    /// No lifecycle flag was set.
    Idle,
    /// The tap was stopped due to permission loss (the tray keeps running
    /// and shows status).
    TapStopped,
    /// The tap was restarted (permissions restored).
    Restarted,
    /// The tap restart was attempted but failed (permissions still missing
    /// or tap creation failed).
    RestartFailed(anyhow::Error),
}

/// Core HandsOff functionality shared by the Tray App binary
pub struct HandsOffCore {
    pub state: Arc<AppState>,
    event_tap: Option<CGEventTapRef>,
    run_loop_source: Option<CFRunLoopSourceRef>,
    hotkey_manager: Option<HotkeyManager>,
    /// Lock hotkey key code (default: Code::KeyL)
    lock_key: global_hotkey::hotkey::Code,
    /// Talk hotkey key code (default: Code::KeyT)
    talk_key: global_hotkey::hotkey::Code,
    /// CFRunLoop thread handle and shutdown channel
    cfrunloop_thread: Option<(JoinHandle<()>, Sender<()>)>,
    /// State pointer passed to event tap (for cleanup)
    event_tap_state_ptr: Option<*mut std::ffi::c_void>,
}

impl HandsOffCore {
    /// Create a new HandsOffCore instance with the given passphrase keycode hash
    pub fn new(passphrase_hash: String) -> Self {
        let state = Arc::new(AppState::new());
        state.set_passphrase_hash(passphrase_hash);

        Self {
            state,
            event_tap: None,
            run_loop_source: None,
            hotkey_manager: None,
            lock_key: global_hotkey::hotkey::Code::KeyL,
            talk_key: global_hotkey::hotkey::Code::KeyT,
            cfrunloop_thread: None,
            event_tap_state_ptr: None,
        }
    }

    /// Set the hotkey configuration
    ///
    /// # Arguments
    ///
    /// * `lock_key` - The key code for the lock hotkey (e.g., Code::KeyL)
    /// * `talk_key` - The key code for the talk hotkey (e.g., Code::KeyT)
    pub fn set_hotkey_config(
        &mut self,
        lock_key: global_hotkey::hotkey::Code,
        talk_key: global_hotkey::hotkey::Code,
    ) {
        self.lock_key = lock_key;
        self.talk_key = talk_key;

        // Convert to macOS keycodes and store in AppState so event tap can use them
        match utils::keycode::code_to_keycode(lock_key) {
            Some(lock_keycode) => {
                self.state.set_lock_keycode(lock_keycode);
                info!(
                    "Lock hotkey configured: {:?} (macOS keycode: {})",
                    lock_key, lock_keycode
                );
            }
            None => {
                error!(
                    "CRITICAL: Failed to convert lock hotkey {:?} to macOS keycode",
                    lock_key
                );
                error!("Lock hotkey will use default keycode (L). This is likely a bug.");
            }
        }
        match utils::keycode::code_to_keycode(talk_key) {
            Some(talk_keycode) => {
                self.state.set_talk_keycode(talk_keycode);
                info!(
                    "Talk hotkey configured: {:?} (macOS keycode: {})",
                    talk_key, talk_keycode
                );
            }
            None => {
                error!(
                    "CRITICAL: Failed to convert talk hotkey {:?} to macOS keycode",
                    talk_key
                );
                error!("Talk hotkey will use default keycode (T). This is likely a bug.");
            }
        }
    }

    /// Get the lock hotkey as a displayable string (e.g., "L", "M", etc.)
    pub fn get_lock_key_display(&self) -> String {
        Self::key_code_to_string(self.lock_key)
    }

    /// Get the talk hotkey as a displayable string (e.g., "T", "S", etc.)
    pub fn get_talk_key_display(&self) -> String {
        Self::key_code_to_string(self.talk_key)
    }

    /// Convert a Code enum to a displayable string
    fn key_code_to_string(code: global_hotkey::hotkey::Code) -> String {
        format!("{:?}", code).replace("Key", "")
    }

    /// Set the auto-lock timeout in seconds
    pub fn set_auto_lock_timeout(&self, timeout: Option<u64>) {
        if let Some(timeout) = timeout {
            self.state.lock().auto_lock_timeout = timeout;
            info!("Auto-lock timeout set to {} seconds", timeout);
        }
    }

    /// Configure the auto-unlock backoff schedule (called at startup).
    ///
    /// Logs an info line only when the backoff schedule is enabled.
    pub fn set_auto_unlock_config(&self, config: config::AutoUnlockConfig) {
        if let config::AutoUnlockConfig::Backoff { base_interval_secs } = &config {
            info!(
                "Auto-unlock backoff enabled: first window at {}s, doubling up to {}s",
                base_interval_secs.get(),
                AUTO_UNLOCK_CEILING_SECONDS
            );
        }
        self.state.set_auto_unlock_config(config);
    }

    /// Set the initial lock state
    pub fn set_locked(&self, locked: bool) {
        self.state.set_locked(locked);
    }

    /// Currently configured lock hotkey (as set by `set_hotkey_config`).
    pub fn lock_key_code(&self) -> global_hotkey::hotkey::Code {
        self.lock_key
    }

    /// Currently configured talk hotkey (as set by `set_hotkey_config`).
    pub fn talk_key_code(&self) -> global_hotkey::hotkey::Code {
        self.talk_key
    }

    /// Swap hotkeys at runtime (Preferences edit): unregister the old
    /// registrations, update the config, register the new keys.
    ///
    /// Unregister-first is required — the global-hotkey manager rejects a
    /// re-registration of an already-registered combo. If registration of
    /// the NEW keys fails after the old ones were unregistered, we restore
    /// the old keys (the previous config remains authoritative on disk
    /// unless the caller already saved the new one; the tray alerts on
    /// partial failure either way).
    pub fn reregister_hotkeys(
        &mut self,
        lock_key: global_hotkey::hotkey::Code,
        talk_key: global_hotkey::hotkey::Code,
    ) -> Result<()> {
        if let Some(manager) = &mut self.hotkey_manager {
            manager
                .unregister_all()
                .context("Failed to unregister current hotkeys")?;
        }

        self.set_hotkey_config(lock_key, talk_key);

        if let Err(e) = self.start_hotkeys() {
            error!(
                "Hotkey re-registration failed; restoring previous hotkeys: {}",
                e
            );
            // Best-effort restore of whatever was registered before.
            if let Some(manager) = &mut self.hotkey_manager {
                let _ = manager.unregister_all();
            }
            self.set_hotkey_config(self.lock_key, self.talk_key);
            let _ = self.start_hotkeys();
            return Err(e);
        }
        Ok(())
    }

    /// Check if currently locked
    pub fn is_locked(&self) -> bool {
        self.state.is_locked()
    }

    /// Get the elapsed time since lock was engaged (in seconds)
    pub fn get_lock_elapsed_secs(&self) -> Option<u64> {
        self.state.get_lock_elapsed_secs()
    }

    /// Get remaining time until auto-lock (in seconds)
    pub fn get_auto_lock_remaining_secs(&self) -> Option<u64> {
        self.state.get_auto_lock_remaining_secs()
    }

    /// Seconds until the currently-pending auto-unlock window opens (awake-time)
    pub fn get_auto_unlock_remaining_secs(&self) -> Option<u64> {
        self.state.get_auto_unlock_remaining_secs()
    }

    /// Interval of the currently-pending auto-unlock window in seconds
    pub fn get_auto_unlock_interval_secs(&self) -> Option<u64> {
        self.state.get_auto_unlock_interval_secs()
    }

    /// Check if accessibility permissions are currently granted
    /// Returns cached value updated by background permission monitor thread
    pub fn has_accessibility_permissions(&self) -> bool {
        self.state.get_cached_accessibility_permissions()
    }

    /// Lock input immediately
    ///
    /// # Safety Note
    /// If accessibility permissions are not granted, this will set the locked state
    /// but input blocking will NOT work. The app will think it's locked but events
    /// won't actually be blocked, leading to a broken state where passphrase entry
    /// doesn't work properly. This is why the tray app should check permissions
    /// before allowing lock() to be called.
    ///
    /// The permission monitor thread will detect this condition and perform an
    /// emergency unlock, but it's better to prevent the lock attempt in the first place.
    pub fn lock(&self) -> Result<()> {
        // Check permissions before locking
        if !self.has_accessibility_permissions() {
            warn!("Cannot lock: Accessibility permissions not granted");
            anyhow::bail!("Cannot lock input - accessibility permissions not granted. Please enable permissions in System Settings > Privacy & Security > Accessibility");
        }

        self.state.set_locked(true);
        info!("Input locked");
        Ok(())
    }

    /// Resets app state to unlocked with all timers cleared — the tray Reset
    /// action; not a passphrase authentication event.
    pub fn reset(&self) {
        self.state.reset_all();
    }

    /// Start CFRunLoop in a background thread
    /// Required for event tap to receive events
    fn start_cfrunloop_thread(&mut self) {
        if self.cfrunloop_thread.is_some() {
            warn!("CFRunLoop thread already running");
            return;
        }

        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            info!("CFRunLoop thread started");
            use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRunResult};

            loop {
                // Run the loop for 0.5 seconds, then check for shutdown
                let result = unsafe {
                    CFRunLoop::run_in_mode(
                        kCFRunLoopDefaultMode,
                        Duration::from_millis(CFRUNLOOP_POLL_INTERVAL_MS),
                        false,
                    )
                };

                // Check if shutdown requested
                if shutdown_rx.try_recv().is_ok() {
                    info!("CFRunLoop thread received shutdown signal");
                    break;
                }

                // Log result for debugging (will be removed later if too verbose)
                if result != CFRunLoopRunResult::TimedOut {
                    log::trace!("CFRunLoop run_in_mode returned: {:?}", result);
                }
            }

            info!("CFRunLoop thread stopped");
        });

        self.cfrunloop_thread = Some((handle, shutdown_tx));
        info!("CFRunLoop thread spawned successfully");
    }

    /// Stop CFRunLoop background thread
    fn stop_cfrunloop_thread(&mut self) {
        if let Some((handle, shutdown_tx)) = self.cfrunloop_thread.take() {
            info!("Stopping CFRunLoop thread");

            // Send shutdown signal
            if let Err(e) = shutdown_tx.send(()) {
                warn!("Failed to send shutdown signal to CFRunLoop thread: {}", e);
            }

            // Wait for thread to finish (with timeout)
            match handle.join() {
                Ok(()) => info!("CFRunLoop thread stopped successfully"),
                Err(e) => warn!("CFRunLoop thread panicked: {:?}", e),
            }
        } else {
            warn!("CFRunLoop thread not running, nothing to stop");
        }
    }

    /// Start the event tap for input blocking
    pub fn start_event_tap(&mut self) -> Result<()> {
        // Start CFRunLoop thread first (required for event tap)
        self.start_cfrunloop_thread();

        info!("[tap-lifecycle] Starting event tap at {}", wall_clock_now());
        let (tap, state_ptr) = event_tap::create_event_tap(self.state.clone())
            .context("Failed to create event tap")?;
        let source = unsafe { event_tap::enable_event_tap(tap) };
        self.event_tap = Some(tap);
        self.run_loop_source = Some(source);
        self.event_tap_state_ptr = Some(state_ptr);
        info!("Event tap started");
        Ok(())
    }

    /// Stop the event tap and remove it from run loop
    /// This should be called when permissions are lost to stop blocking input
    pub fn stop_event_tap(&mut self) {
        if let (Some(tap), Some(source)) = (self.event_tap, self.run_loop_source) {
            warn!("[tap-lifecycle] Stopping event tap at {}", wall_clock_now());
            unsafe {
                event_tap::remove_event_tap_from_runloop(tap, source);
            }
            self.event_tap = None;
            self.run_loop_source = None;
            info!("Event tap stopped - input should now be accessible");
        } else {
            warn!("Attempted to stop event tap but it was not running");
        }

        // Free the state pointer to prevent memory leak
        if let Some(state_ptr) = self.event_tap_state_ptr.take() {
            unsafe {
                let _ = Box::from_raw(state_ptr as *mut Arc<AppState>);
                info!("Event tap state pointer freed");
            }
        }

        // Stop CFRunLoop thread (no longer needed without event tap)
        self.stop_cfrunloop_thread();
    }

    /// Restart the event tap after permissions are restored
    /// Returns Ok if successful, Err if permissions are still missing or creation fails
    pub fn restart_event_tap(&mut self) -> Result<()> {
        // First check if we already have an event tap running
        if self.event_tap.is_some() {
            warn!("Event tap already running, stopping it first");
            self.stop_event_tap();
        }

        // Verify permissions before attempting to create tap
        if !input_blocking::check_accessibility_permissions() {
            anyhow::bail!("Cannot restart event tap - accessibility permissions not granted");
        }

        info!("Restarting event tap");
        self.start_event_tap()?;
        info!("Event tap restarted successfully");
        Ok(())
    }

    /// Re-enable the existing event tap without creating a new WindowServer connection.
    ///
    /// Called after macOS disables the tap due to callback timeout (typically on sleep/wake).
    /// The tap handle is still valid — we just need to call CGEventTapEnable again.
    /// This avoids the zombie Mach port accumulation caused by creating a new tap each wake.
    ///
    /// If no tap is currently held (e.g. it was stopped due to permission loss), falls back
    /// to a full restart so the caller never needs to distinguish the two cases.
    pub fn reenable_event_tap(&mut self) -> Result<()> {
        match self.event_tap {
            Some(tap) => {
                info!(
                    "[tap-lifecycle] Re-enabling existing event tap at {} (reusing WindowServer connection, no new Mach port)",
                    wall_clock_now()
                );
                // NOTE: Removed log_mach_port_count() — lsof subprocess adds 500ms-8s latency during re-enable
                let success = unsafe { event_tap::reenable_existing_tap(tap) };
                if success {
                    self.state.mark_reenable_completed();
                    info!("[tap-lifecycle] Event tap re-enabled successfully");
                    Ok(())
                } else {
                    warn!(
                        "[tap-lifecycle] Re-enable failed (tap still disabled), falling back to full restart at {}",
                        wall_clock_now()
                    );
                    self.restart_event_tap()
                }
            }
            None => {
                warn!(
                    "[tap-lifecycle] Re-enable requested but no tap handle held — falling back to full restart at {}",
                    wall_clock_now()
                );
                self.restart_event_tap()
            }
        }
    }

    /// Disable HandsOff (stops event tap and hotkeys for minimal CPU usage)
    ///
    /// Issue #37 N2: clearing `is_locked` here guarantees a Lock flag can
    /// never outlive the tap that enforces it. A deferred Disable click (or
    /// any disable path) previously left `is_locked=true` with no tap behind
    /// it — a locked stretch the state machine believed was in progress and
    /// that Reenable would later clear WITHOUT authentication.
    pub fn disable(&mut self) -> Result<()> {
        info!("Disabling HandsOff - entering minimal CPU mode");

        // Set disabled flag first (background threads will become inactive)
        self.state.set_disabled(true);

        // A stopped tap enforces nothing: the Lock flag must go with it.
        // Clear ONLY the lock state — do NOT restart the backoff schedule
        // (issue #37: §2.3 says only a successful Passphrase unlock resets
        // it; disable() previously reset it via reset_all(), shifting any
        // remaining auto-unlock windows earlier than the §2.1 timeline).
        self.state.clear_lock_state();

        // Stop event tap
        self.stop_event_tap();

        // Unregister hotkeys
        if let Some(ref mut manager) = self.hotkey_manager {
            manager
                .unregister_all()
                .context("Failed to unregister hotkeys")?;
        }

        // Clear input buffer for clean state
        self.state.clear_buffer();

        info!("HandsOff disabled successfully");
        Ok(())
    }

    /// Enable HandsOff (restarts event tap and hotkeys)
    pub fn enable(&mut self) -> Result<()> {
        info!("Enabling HandsOff - resuming normal operation");

        // Reset last_input_time for fresh auto-lock countdown
        // Note: if don't do this first, auto-lock may kick in right after set_disabled(false)
        self.state.update_input_time();

        // Restart event tap (checks permissions internally)
        self.restart_event_tap()
            .context("Failed to restart event tap")?;

        // Re-register hotkeys
        self.start_hotkeys()?;

        // Clear disabled flag first
        self.state.set_disabled(false);

        info!("HandsOff enabled successfully");
        Ok(())
    }

    /// Service the tap-lifecycle flags — the ONE block both binary main
    /// loops must run each poll (issue #37 N3).
    ///
    /// Consumes the AppState lifecycle flags and performs the corresponding
    /// tap operation:
    /// - `should_stop_event_tap` → `stop_event_tap` (permission loss)
    /// - `should_reenable_event_tap` → `reenable_event_tap` (macOS disabled
    ///   the tap on a sleep/wake timeout; falls back to a full restart)
    /// - `should_start_event_tap` → `restart_event_tap` (permissions restored)
    ///
    /// The tray session loop calls this. Returns what happened so the
    /// caller can layer its own UX (the tray notifies on restart
    /// success/failure).
    pub fn service_tap_lifecycle(&mut self) -> TapLifecycleEvent {
        // Permission loss: stop the tap.
        if self.state.should_stop_event_tap_and_clear() {
            warn!("Stopping input blocking due to permission loss");
            self.stop_event_tap();
            info!("Input blocking stopped - normal input restored");
            return TapLifecycleEvent::TapStopped;
        }

        // Re-enable the existing tap (post sleep/wake timeout recovery).
        // This reuses the same CGEventTapRef — no new WindowServer connection
        // is created, which prevents zombie Mach port accumulation across
        // sleep/wake cycles.
        if self.state.should_reenable_event_tap_and_clear() {
            info!("Re-enabling existing event tap after sleep/wake timeout");
            if let Err(e) = self.reenable_event_tap() {
                warn!(
                    "Failed to re-enable event tap: {} — will attempt full restart",
                    e
                );
                // reenable_event_tap already falls back to restart internally,
                // but log the failure so it's visible in telemetry
            }
        }

        // Permission restored: (re)start the tap.
        if self.state.should_start_event_tap_and_clear() {
            info!("Restarting input blocking - permissions restored");
            return match self.restart_event_tap() {
                Ok(()) => {
                    info!("Input blocking restarted successfully");
                    TapLifecycleEvent::Restarted
                }
                Err(e) => {
                    warn!("Failed to restart input blocking: {}", e);
                    TapLifecycleEvent::RestartFailed(e)
                }
            };
        }

        TapLifecycleEvent::Idle
    }

    /// Start the hotkey manager using configured keys
    pub fn start_hotkeys(&mut self) -> Result<()> {
        if self.hotkey_manager.is_none() {
            let new_mgr = HotkeyManager::new().context("Failed to create hotkey manager")?;
            info!("Instantiated new hotkey manager");
            self.hotkey_manager = Some(new_mgr);
        }

        let manager: &mut HotkeyManager = self.hotkey_manager.as_mut().unwrap();

        manager
            .register_lock_hotkey(self.lock_key)
            .context("Failed to register lock hotkey")?;
        manager
            .register_talk_hotkey(self.talk_key)
            .context("Failed to register talk hotkey")?;

        info!("Hotkeys registered");
        Ok(())
    }

    /// Start all background threads (buffer reset, auto-lock, hotkey listener, auto-unlock, permission monitor)
    pub fn start_background_threads(&self) -> Result<()> {
        self.start_buffer_reset_thread();
        self.start_auto_lock_thread();

        if let Some(ref manager) = self.hotkey_manager {
            self.start_hotkey_listener_thread(manager);
        }

        // Start auto-unlock thread if the backoff schedule is enabled
        if self.state.auto_unlock_enabled() {
            self.start_auto_unlock_thread();
        }

        // Start permission monitoring thread for safety
        self.start_permission_monitor_thread();

        info!("Background threads started");
        Ok(())
    }

    /// Background thread to reset input buffer after timeout
    fn start_buffer_reset_thread(&self) {
        let state = self.state.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(BUFFER_RESET_CHECK_INTERVAL_MS));

            // Skip processing when disabled
            if state.is_disabled() {
                continue;
            }

            if state.should_reset_buffer()
                && state.buffer_len() > 0 {
                    info!("Resetting input buffer after timeout");
                    state.clear_buffer();
                }
        });
    }

    /// Background thread to enable auto-lock after inactivity
    fn start_auto_lock_thread(&self) {
        let state = self.state.clone();
        thread::spawn(move || {
            let mut check_count = 0u32;
            loop {
                thread::sleep(Duration::from_secs(AUTO_LOCK_CHECK_INTERVAL_SECS));

                // Skip processing when disabled
                if state.is_disabled() {
                    continue;
                }

                check_count += 1;

                // Log remaining time every 30 seconds (6 checks of 5 seconds each)
                if check_count % 6 == 0 {
                    if let Some(remaining_secs) = state.get_auto_lock_remaining_secs() {
                        let minutes = remaining_secs / 60;
                        let seconds = remaining_secs % 60;
                        info!(
                            "Auto-lock in {} seconds ({} min {} sec remaining)",
                            remaining_secs, minutes, seconds
                        );
                    }
                }

                if state.should_auto_lock() {
                    info!("Auto-lock triggered after inactivity - input now locked");
                    state.set_locked(true);
                }
            }
        });
    }

    /// Background thread to listen for hotkey events
    fn start_hotkey_listener_thread(&self, manager: &HotkeyManager) {
        let state = self.state.clone();

        // Extract hotkey IDs to avoid needing to clone manager
        let lock_hotkey_id = manager.lock_hotkey.map(|hk| hk.id());
        let talk_hotkey_id = manager.talk_hotkey.map(|hk| hk.id());

        thread::spawn(move || {
            use global_hotkey::GlobalHotKeyEvent;

            let receiver = GlobalHotKeyEvent::receiver();
            loop {
                if let Ok(event) = receiver.recv() {
                    // Skip processing when disabled
                    if state.is_disabled() {
                        continue;
                    }

                    let event_id = event.id;

                    // Check if it's the lock hotkey
                    if lock_hotkey_id.is_some_and(|id| id == event_id) {
                        info!("Lock hotkey triggered");
                        if !state.is_locked() {
                            state.set_locked(true);
                            info!("Input locked via hotkey");
                        }
                    }
                    // Check if it's the talk hotkey
                    else if talk_hotkey_id.is_some_and(|id| id == event_id) {
                        info!("Talk hotkey triggered");
                        // Note: Spacebar passthrough is handled in the event tap
                    }
                }
            }
        });
    }

    /// Background thread to trigger auto-unlock when the backoff window opens.
    ///
    /// Schedule (specs/deep-design-review-v2-2026-09.md §2): windows open at
    /// base×2^n awake-time gaps since the locked stretch began. A fired window
    /// unlocks WITHOUT resetting the backoff counter — only successful
    /// passphrase auth resets it (§2.3). `trigger_auto_unlock` consumes the
    /// window (advances the counter) as part of the transition.
    fn start_auto_unlock_thread(&self) {
        let state = self.state.clone();
        thread::Builder::new()
            .name("auto-unlock".to_string())
            .spawn(move || {
                info!("Auto-unlock backoff monitoring thread started");

                loop {
                    thread::sleep(Duration::from_secs(AUTO_UNLOCK_CHECK_INTERVAL_SECS));

                    // Skip processing when disabled
                    if state.is_disabled() {
                        continue;
                    }

                    if state.should_auto_unlock() {
                        warn!("Auto-unlock window opened - releasing input (unauthenticated)");
                        state.trigger_auto_unlock();
                        info!("Input unlocked due to auto-unlock window");
                    }
                }
            })
            .expect("Failed to spawn auto-unlock thread");
    }

    /// Background thread to monitor accessibility permissions and signal when to stop event tap
    /// CRITICAL SAFETY FEATURE: Prevents user lockout if permissions are revoked while app is running
    fn start_permission_monitor_thread(&self) {
        let state = self.state.clone();

        thread::Builder::new()
            .name("permission-monitor".to_string())
            .spawn(move || {
                info!(
                    "Permission monitoring thread started - will check every {} seconds",
                    PERMISSION_CHECK_INTERVAL_SECS
                );

                // CRITICAL: Check initial permission state rather than assuming true
                // This handles the edge case where permissions are removed before the first check
                let mut last_permission_state = input_blocking::check_accessibility_permissions();

                // Cache the initial permission state
                state.set_cached_accessibility_permissions(last_permission_state);

                // If permissions are already missing, request event tap stop
                if !last_permission_state {
                    warn!("CRITICAL: Accessibility permissions are missing at startup");

                    // Unlock if locked
                    if state.is_locked() {
                        state.set_locked(false);
                        info!("Unlocked - permissions missing");
                    }

                    // Signal to stop event tap
                    state.request_stop_event_tap();

                    #[cfg(target_os = "macos")]
                    {
                        let _ = notify_rust::Notification::new()
                            .summary("HandsOff - Permissions Missing")
                            .body("Accessibility permissions are missing.\nInput blocking stopped to restore normal keyboard and mouse.\n\nUse Reenable menu to restart after granting permissions.")
                            .timeout(notify_rust::Timeout::Milliseconds(10000))
                            .show();
                    }
                }

                // Track elapsed checks for periodic telemetry logging
                let telemetry_checks_per_interval =
                    (CALLBACK_TELEMETRY_INTERVAL_SECS / PERMISSION_CHECK_INTERVAL_SECS).max(1);
                let mut check_counter: u64 = 0;

                loop {
                    thread::sleep(Duration::from_secs(PERMISSION_CHECK_INTERVAL_SECS));

                    // Skip permission checking when disabled (no event tap running)
                    if state.is_disabled() {
                        continue;
                    }

                    check_counter += 1;

                    // Log callback telemetry periodically
                    if check_counter % telemetry_checks_per_interval == 0 {
                        let (count, slow, max_us) =
                            event_tap::reset_callback_telemetry();
                        if count > 0 {
                            info!(
                                "[telemetry] callback stats (last {}s): total={}, slow={}, max_duration={}us",
                                CALLBACK_TELEMETRY_INTERVAL_SECS, count, slow, max_us
                            );
                        }
                    }

                    // Lightweight check: only AXIsProcessTrusted(), no WindowServer interaction.
                    // Avoids the CGEventTapCreate/CFRelease cycle that degrades WindowServer
                    // over hundreds of calls (root cause of "callback was too slow" timeouts).
                    let has_permissions = input_blocking::check_accessibility_permissions_lightweight();

                    // Detect permission loss (transition from true to false)
                    if last_permission_state && !has_permissions {
                        warn!("CRITICAL: Accessibility permissions were revoked while app is running!");

                        // Unlock if currently locked
                        if state.is_locked() {
                            warn!("App is locked - unlocking to restore input");
                            state.set_locked(false);
                            info!("Unlocked - permissions revoked");
                        }

                        // Signal to stop event tap (main thread will handle the actual stop)
                        state.request_stop_event_tap();

                        // Show notification
                        #[cfg(target_os = "macos")]
                        {
                            let _ = notify_rust::Notification::new()
                                .summary("HandsOff - Permissions Revoked")
                                .body("Accessibility permissions were revoked.\nInput blocking stopped - your keyboard and mouse work normally now.\n\nRestore permissions and use Reenable menu to restart.")
                                .timeout(notify_rust::Timeout::Milliseconds(10000))
                                .show();
                        }

                        warn!("Event tap stop requested - main thread will handle cleanup");
                    }
                    // Detect permission restoration
                    else if !last_permission_state && has_permissions {
                        info!("Accessibility permissions have been restored");

                        // Request automatic restart (Tray app will handle this)
                        state.request_start_event_tap();

                        #[cfg(target_os = "macos")]
                        {
                            let _ = notify_rust::Notification::new()
                                .summary("HandsOff - Permissions Restored")
                                .body("Accessibility permissions restored.\n\nRestarting input blocking automatically...")
                                .timeout(notify_rust::Timeout::Milliseconds(5000))
                                .show();
                        }
                    }

                    // Update cached state
                    state.set_cached_accessibility_permissions(has_permissions);
                    last_permission_state = has_permissions;
                }
            })
            .expect("Failed to spawn permission monitor thread");
    }
}

impl Drop for HandsOffCore {
    fn drop(&mut self) {
        info!("HandsOffCore dropping - cleaning up resources");

        // Stop the event tap to release CGEventTapRef and prevent WindowServer resource leak
        self.stop_event_tap();

        // Unregister hotkeys to clean up global_hotkey resources
        if let Some(ref mut manager) = self.hotkey_manager {
            if let Err(e) = manager.unregister_all() {
                warn!("Failed to unregister hotkeys during drop: {}", e);
            }
        }

        info!("HandsOffCore cleanup complete");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_tap_lifecycle_consumes_stop_flag() {
        // Issue #37 N3: the shared servicing method must consume the stop
        // flag and report TapStopped.
        let mut core = HandsOffCore::new(crate::utils::hash_keycodes(&[0, 12, 15, 37]));
        core.state.request_stop_event_tap();

        match core.service_tap_lifecycle() {
            TapLifecycleEvent::TapStopped => {} // expected
            other => panic!("stop flag must yield TapStopped, got {other:?}"),
        }
        // Flag consumed: a second pass must be idle.
        assert!(matches!(
            core.service_tap_lifecycle(),
            TapLifecycleEvent::Idle
        ));
    }

    #[test]
    fn test_service_tap_lifecycle_consumes_reenable_flag() {
        // Issue #37 N3: the re-enable flag (set on macOS tap timeout) must
        // be consumed by the shared servicing block. reenable_event_tap
        // falls back to a full restart when no tap is held, so this
        // exercises the full fallback path and must NOT return TapStopped.
        let mut core = HandsOffCore::new(crate::utils::hash_keycodes(&[0, 12, 15, 37]));
        core.state.request_reenable_event_tap();

        let event = core.service_tap_lifecycle();
        assert!(
            !matches!(event, TapLifecycleEvent::TapStopped),
            "re-enable must never report TapStopped"
        );
        // Flag consumed: second pass must be idle.
        assert!(matches!(
            core.service_tap_lifecycle(),
            TapLifecycleEvent::Idle
        ));
    }

    #[test]
    fn test_service_tap_lifecycle_consumes_start_flag() {
        // Issue #37 N3: the start flag (permissions restored) must also be
        // consumed. restart_event_tap fails without permissions on CI, so
        // either Restarted or RestartFailed is acceptable — the requirement
        // is that the flag is cleared and the event is one of the two.
        let mut core = HandsOffCore::new(crate::utils::hash_keycodes(&[0, 12, 15, 37]));
        core.state.request_start_event_tap();

        let event = core.service_tap_lifecycle();
        assert!(
            matches!(
                event,
                TapLifecycleEvent::Restarted | TapLifecycleEvent::RestartFailed(_)
            ),
            "start flag must yield a restart event, got {event:?}"
        );
        assert!(matches!(
            core.service_tap_lifecycle(),
            TapLifecycleEvent::Idle
        ));
    }

    #[test]
    fn test_service_tap_lifecycle_stop_takes_priority() {
        // If both stop and re-enable are somehow set, stop must win: the
        // method returns TapStopped and the re-enable flag is left for the
        // next pass (or consumed — but never blocks the stop report).
        let mut core = HandsOffCore::new(crate::utils::hash_keycodes(&[0, 12, 15, 37]));
        core.state.request_stop_event_tap();
        core.state.request_reenable_event_tap();

        assert!(matches!(
            core.service_tap_lifecycle(),
            TapLifecycleEvent::TapStopped
        ));
    }
}
