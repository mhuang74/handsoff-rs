mod app_state;
mod auth;
mod input_blocking;
mod utils;

use anyhow::{Context, Result};
use app_state::AppState;
use input_blocking::event_tap;
use input_blocking::hotkeys::HotkeyManager;
use log::{debug, error, info, warn};
use std::env;
use std::io::{self, Write};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Prompt user for passphrase via command line
fn prompt_for_passphrase() -> Option<String> {
    print!("Enter passphrase to unlock HandsOff: ");
    io::stdout().flush().ok()?;

    let mut passphrase = String::new();
    io::stdin().read_line(&mut passphrase).ok()?;

    let passphrase = passphrase.trim();
    if passphrase.is_empty() {
        error!("Empty passphrase not allowed");
        None
    } else {
        Some(passphrase.to_string())
    }
}

/// Parse the HANDS_OFF_AUTO_UNLOCK environment variable
fn parse_auto_unlock_timeout() -> Option<u64> {
    match env::var("HANDS_OFF_AUTO_UNLOCK") {
        Ok(val) => match val.parse::<u64>() {
            Ok(seconds) if seconds >= 10 && seconds <= 3600 => {
                info!("Auto-unlock safety feature enabled: {} seconds", seconds);
                Some(seconds)
            }
            Ok(seconds) if seconds == 0 => {
                info!("Auto-unlock disabled (value: 0)");
                None
            }
            Ok(seconds) => {
                warn!("Invalid auto-unlock timeout: {} (must be 10-3600 or 0). Feature disabled.", seconds);
                None
            }
            Err(e) => {
                warn!("Failed to parse HANDS_OFF_AUTO_UNLOCK: {}. Feature disabled.", e);
                None
            }
        },
        Err(_) => {
            debug!("HANDS_OFF_AUTO_UNLOCK not set. Auto-unlock disabled.");
            None
        }
    }
}

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    info!("Starting HandsOff Input Lock");

    // Check accessibility permissions
    if !input_blocking::check_accessibility_permissions() {
        error!("Accessibility permissions not granted");
        error!("Please grant accessibility permissions to HandsOff in System Preferences > Security & Privacy > Privacy > Accessibility");
        std::process::exit(1);
    }

    // Create app state
    let state = Arc::new(AppState::new());

    // Parse and configure auto-unlock timeout
    let auto_unlock_timeout = parse_auto_unlock_timeout();
    state.set_auto_unlock_timeout(auto_unlock_timeout);

    // Check for passphrase from environment variable first (bypasses keychain)
    if let Ok(passphrase) = env::var("HANDS_OFF_SECRET_PHRASE") {
        if !passphrase.is_empty() {
            info!("Using passphrase from HANDS_OFF_SECRET_PHRASE environment variable");
            let hash = auth::hash_passphrase(&passphrase);
            state.set_passphrase_hash(hash);
        } else {
            error!("HANDS_OFF_SECRET_PHRASE is set but empty");
            std::process::exit(1);
        }
    } else {
        // Fall back to keychain if env var not set
        match auth::keychain::retrieve_passphrase_hash() {
            Ok(Some(hash)) => {
                info!("Loaded passphrase hash from keychain");
                state.set_passphrase_hash(hash);
            }
            Ok(None) => {
                info!("No passphrase set - prompting user");
                if let Some(passphrase) = prompt_for_passphrase() {
                    let hash = auth::hash_passphrase(&passphrase);
                    if let Err(e) = auth::keychain::store_passphrase_hash(&hash) {
                        error!("Failed to store passphrase: {}", e);
                    } else {
                        state.set_passphrase_hash(hash);
                        info!("Passphrase set successfully");
                    }
                } else {
                    error!("No passphrase set - exiting");
                    std::process::exit(1);
                }
            }
            Err(e) => {
                error!("Failed to retrieve passphrase from keychain: {}", e);
            }
        }
    }

    // Load auto-lock timeout
    if let Ok(Some(timeout)) = auth::keychain::retrieve_auto_lock_timeout() {
        state.lock().auto_lock_timeout = timeout;
        info!("Loaded auto-lock timeout: {} seconds", timeout);
    }

    // Create event tap for input blocking
    let event_tap = event_tap::create_event_tap(state.clone())
        .context("Failed to create event tap")?;
    event_tap::enable_event_tap(event_tap);

    // Create hotkey manager
    let mut hotkey_manager = HotkeyManager::new()
        .context("Failed to create hotkey manager")?;
    hotkey_manager.register_lock_hotkey()
        .context("Failed to register lock hotkey")?;
    hotkey_manager.register_talk_hotkey()
        .context("Failed to register talk hotkey")?;

    // Start background threads
    start_buffer_reset_thread(state.clone());
    start_auto_lock_thread(state.clone());
    start_hotkey_listener_thread(state.clone(), hotkey_manager);

    // Start auto-unlock thread if enabled
    if auto_unlock_timeout.is_some() {
        start_auto_unlock_thread(state.clone());
    }

    info!("HandsOff is running - press Ctrl+C to quit");
    info!("Input interception is active. Type your passphrase to unlock.");

    // Keep the main thread alive
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

/// Background thread to reset input buffer after timeout
fn start_buffer_reset_thread(state: Arc<AppState>) {
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
fn start_auto_lock_thread(state: Arc<AppState>) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(10));

        if state.should_auto_lock() {
            info!("Auto-lock triggered after inactivity - input now locked");
            state.set_locked(true);
        }
    });
}

/// Background thread to listen for hotkey events
fn start_hotkey_listener_thread(state: Arc<AppState>, manager: HotkeyManager) {
    thread::spawn(move || {
        use global_hotkey::GlobalHotKeyEvent;

        let receiver = GlobalHotKeyEvent::receiver();
        loop {
            if let Ok(event) = receiver.recv() {
                input_blocking::hotkeys::handle_hotkey_event(event, &state, &manager);
            }
        }
    });
}

/// Background thread to trigger auto-unlock after timeout
fn start_auto_unlock_thread(state: Arc<AppState>) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auto_unlock_valid_values() {
        // Test minimum valid value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "10");
        assert_eq!(parse_auto_unlock_timeout(), Some(10), "Should accept 10 seconds");

        // Test typical value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "30");
        assert_eq!(parse_auto_unlock_timeout(), Some(30), "Should accept 30 seconds");

        // Test large value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "600");
        assert_eq!(parse_auto_unlock_timeout(), Some(600), "Should accept 600 seconds");

        // Test maximum valid value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "3600");
        assert_eq!(parse_auto_unlock_timeout(), Some(3600), "Should accept 3600 seconds");

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_disabled() {
        // Test explicit disable with 0
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "0");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should return None for 0");

        // Test not set (should return None, not panic)
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should return None when not set");
    }

    #[test]
    fn test_parse_auto_unlock_invalid_values() {
        // Test too low
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "5");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject value below 10");

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "9");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject value below 10");

        // Test too high
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "3601");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject value above 3600");

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "5000");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject value above 3600");

        // Test negative number (will fail to parse)
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "-10");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject negative value");

        // Test non-numeric
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "invalid");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject non-numeric value");

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "30s");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject value with units");

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject empty string");

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_boundary_cases() {
        // Test just below minimum
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "9");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject 9 seconds");

        // Test at minimum boundary
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "10");
        assert_eq!(parse_auto_unlock_timeout(), Some(10), "Should accept 10 seconds");

        // Test at maximum boundary
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "3600");
        assert_eq!(parse_auto_unlock_timeout(), Some(3600), "Should accept 3600 seconds");

        // Test just above maximum
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "3601");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject 3601 seconds");

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }
}
