// Library interface for HandsOff
// This allows tests and other modules to access the crate's functionality

pub mod app_state;
pub mod auth;
pub mod input_blocking;
pub mod utils;

use anyhow::{Context, Result};
use app_state::AppState;
use core_graphics::sys::CGEventTapRef;
use input_blocking::event_tap;
use input_blocking::hotkeys::HotkeyManager;
use log::{info, warn};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Core HandsOff functionality shared between CLI and Tray App
pub struct HandsOffCore {
    pub state: Arc<AppState>,
    event_tap: Option<CGEventTapRef>,
    hotkey_manager: Option<HotkeyManager>,
}

impl HandsOffCore {
    /// Create a new HandsOffCore instance with the given passphrase hash
    pub fn new(passphrase: &str) -> Result<Self> {
        let state = Arc::new(AppState::new());
        let hash = auth::hash_passphrase(passphrase);
        state.set_passphrase_hash(hash);

        Ok(Self {
            state,
            event_tap: None,
            hotkey_manager: None,
        })
    }

    /// Set the auto-lock timeout in seconds
    pub fn set_auto_lock_timeout(&self, timeout: Option<u64>) {
        if let Some(timeout) = timeout {
            self.state.lock().auto_lock_timeout = timeout;
            info!("Auto-lock timeout set to {} seconds", timeout);
        }
    }

    /// Set the auto-unlock timeout in seconds
    pub fn set_auto_unlock_timeout(&self, timeout: Option<u64>) {
        self.state.set_auto_unlock_timeout(timeout);
        if let Some(timeout) = timeout {
            info!("Auto-unlock timeout set to {} seconds", timeout);
        }
    }

    /// Set the initial lock state
    pub fn set_locked(&self, locked: bool) {
        self.state.set_locked(locked);
    }

    /// Check if currently locked
    pub fn is_locked(&self) -> bool {
        self.state.is_locked()
    }

    /// Get the elapsed time since lock was engaged (in seconds)
    pub fn get_lock_elapsed_secs(&self) -> Option<u64> {
        self.state.get_lock_elapsed_secs()
    }

    /// Get remaining time until auto-unlock (in seconds)
    pub fn get_auto_unlock_remaining_secs(&self) -> Option<u64> {
        self.state.get_auto_unlock_remaining_secs()
    }

    /// Get the configured auto-unlock timeout (in seconds)
    pub fn get_auto_unlock_timeout(&self) -> Option<u64> {
        self.state.get_auto_unlock_timeout()
    }

    /// Lock input immediately
    pub fn lock(&self) -> Result<()> {
        self.state.set_locked(true);
        info!("Input locked");
        Ok(())
    }

    /// Unlock input with passphrase
    pub fn unlock(&self, passphrase: &str) -> Result<bool> {
        let hash = auth::hash_passphrase(passphrase);
        let expected_hash = self.state.get_passphrase_hash();

        if Some(hash) == expected_hash {
            self.state.set_locked(false);
            info!("Input unlocked");
            Ok(true)
        } else {
            warn!("Invalid passphrase attempt");
            Ok(false)
        }
    }

    /// Start the event tap for input blocking
    pub fn start_event_tap(&mut self) -> Result<()> {
        let tap = event_tap::create_event_tap(self.state.clone())
            .context("Failed to create event tap")?;
        unsafe {
            event_tap::enable_event_tap(tap);
        }
        self.event_tap = Some(tap);
        info!("Event tap started");
        Ok(())
    }

    /// Start the hotkey manager
    pub fn start_hotkeys(&mut self) -> Result<()> {
        let mut manager = HotkeyManager::new().context("Failed to create hotkey manager")?;
        manager
            .register_lock_hotkey()
            .context("Failed to register lock hotkey")?;
        manager
            .register_talk_hotkey()
            .context("Failed to register talk hotkey")?;
        self.hotkey_manager = Some(manager);
        info!("Hotkeys registered");
        Ok(())
    }

    /// Start all background threads (buffer reset, auto-lock, hotkey listener, auto-unlock)
    pub fn start_background_threads(&self) -> Result<()> {
        self.start_buffer_reset_thread();
        self.start_auto_lock_thread();

        if let Some(ref manager) = self.hotkey_manager {
            self.start_hotkey_listener_thread(manager);
        }

        // Start auto-unlock thread if timeout is configured
        if self.state.get_auto_unlock_timeout().is_some() {
            self.start_auto_unlock_thread();
        }

        info!("Background threads started");
        Ok(())
    }

    /// Background thread to reset input buffer after timeout
    fn start_buffer_reset_thread(&self) {
        let state = self.state.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(1));

            if state.should_reset_buffer() {
                let buffer = state.get_buffer();
                if !buffer.is_empty() {
                    info!("Resetting input buffer after timeout");
                    state.clear_buffer();
                }
            }
        });
    }

    /// Background thread to enable auto-lock after inactivity
    fn start_auto_lock_thread(&self) {
        let state = self.state.clone();
        thread::spawn(move || {
            let mut check_count = 0u32;
            loop {
                thread::sleep(Duration::from_secs(5));
                check_count += 1;

                // Log remaining time every 30 seconds (6 checks of 5 seconds each)
                if check_count.is_multiple_of(6) {
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

    /// Background thread to trigger auto-unlock after timeout
    fn start_auto_unlock_thread(&self) {
        let state = self.state.clone();
        thread::Builder::new()
            .name("auto-unlock".to_string())
            .spawn(move || {
                info!("Auto-unlock monitoring thread started");

                loop {
                    thread::sleep(Duration::from_secs(10)); // Check every 10 seconds

                    if state.should_auto_unlock() {
                        warn!("Auto-unlock timeout expired - disabling input interception");

                        // Unlock the device
                        state.trigger_auto_unlock();
                        info!("Input unlocked due to auto-unlock timeout");
                    }
                }
            })
            .expect("Failed to spawn auto-unlock thread");
    }
}
