mod app_state;
mod auth;
mod input_blocking;
mod utils;

use anyhow::{Context, Result};
use app_state::AppState;
use clap::Parser;
use input_blocking::event_tap;
use input_blocking::hotkeys::HotkeyManager;
use log::{debug, error, info, warn};
use std::env;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// macOS utility to block unsolicited input from unwanted hands
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "macOS utility to block unsolicited input from unwanted hands",
    long_about = "macOS utility to block accidental or unsolicited input from unwanted hands.

Usecases:
 - safely monitor progress on your laptop from across the room
 - join a conference call with a toddler in your lap
 - prevent your kid from sending out that draft email when you go rummage for snacks

Blocks:
 - keypress
 - mouse/trackpad clicks

ENVIRONMENT VARIABLES (Required):
  HANDS_OFF_SECRET_PHRASE    Passphrase required to unlock input when locked
                             Example: export HANDS_OFF_SECRET_PHRASE='my-secret'

ENVIRONMENT VARIABLES (Optional):
  HANDS_OFF_AUTO_LOCK        Auto-lock timeout in seconds (10-7200)
                             Input will lock after this period of inactivity
                             Example: export HANDS_OFF_AUTO_LOCK=300

  HANDS_OFF_AUTO_UNLOCK      Auto-unlock timeout in seconds (10-3600, or 0 to disable)
                             Safety feature: automatically unlocks after this duration
                             to prevent permanent lockouts
                             Example: export HANDS_OFF_AUTO_UNLOCK=30

HOTKEYS:
  Ctrl+Cmd+Shift+L          Lock input (blocks all keyboard/mouse input)
  Ctrl+Cmd+Shift+T          Talk mode (hold to allow spacebar keypress, for unmuting conf calls)

When locked, type your passphrase to unlock (input won't be visible on screen)."
)]
struct Args {
    /// Start with input locked immediately
    #[arg(short, long)]
    locked: bool,
}


/// Parse the HANDS_OFF_AUTO_UNLOCK environment variable
fn parse_auto_unlock_timeout() -> Option<u64> {
    match env::var("HANDS_OFF_AUTO_UNLOCK") {
        Ok(val) => match val.parse::<u64>() {
            Ok(seconds) if (10..=3600).contains(&seconds) => {
                info!("Auto-unlock safety feature enabled: {} seconds", seconds);
                Some(seconds)
            }
            Ok(0) => {
                info!("Auto-unlock disabled (value: 0)");
                None
            }
            Ok(seconds) => {
                warn!(
                    "Invalid auto-unlock timeout: {} (must be 10-3600 or 0). Feature disabled.",
                    seconds
                );
                None
            }
            Err(e) => {
                warn!(
                    "Failed to parse HANDS_OFF_AUTO_UNLOCK: {}. Feature disabled.",
                    e
                );
                None
            }
        },
        Err(_) => {
            debug!("HANDS_OFF_AUTO_UNLOCK not set. Auto-unlock disabled.");
            None
        }
    }
}

/// Parse the HANDS_OFF_AUTO_LOCK environment variable
fn parse_auto_lock_timeout() -> Option<u64> {
    match env::var("HANDS_OFF_AUTO_LOCK") {
        Ok(val) => match val.parse::<u64>() {
            Ok(seconds) if (10..=7200).contains(&seconds) => {
                info!(
                    "Auto-lock timeout set via environment variable: {} seconds",
                    seconds
                );
                Some(seconds)
            }
            Ok(seconds) => {
                warn!(
                    "Invalid auto-lock timeout: {} (must be 10-7200 seconds). Using default.",
                    seconds
                );
                None
            }
            Err(e) => {
                warn!("Failed to parse HANDS_OFF_AUTO_LOCK: {}. Using default.", e);
                None
            }
        },
        Err(_) => {
            debug!("HANDS_OFF_AUTO_LOCK not set.");
            None
        }
    }
}

fn main() -> Result<()> {
    // Parse command-line arguments
    let args = Args::parse();

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

    // Set initial lock state based on command-line argument
    if args.locked {
        state.set_locked(true);
        info!("Starting in LOCKED mode (--locked flag)");
    } else {
        info!("Starting in UNLOCKED mode (use --locked to start locked, or press Ctrl+Cmd+Shift+L to lock)");
    }

    // Require passphrase from environment variable
    match env::var("HANDS_OFF_SECRET_PHRASE") {
        Ok(passphrase) if !passphrase.is_empty() => {
            info!("Using passphrase from HANDS_OFF_SECRET_PHRASE environment variable");
            let hash = auth::hash_passphrase(&passphrase);
            state.set_passphrase_hash(hash);
        }
        Ok(_) => {
            error!("HANDS_OFF_SECRET_PHRASE is set but empty");
            error!("Please set a valid passphrase using: export HANDS_OFF_SECRET_PHRASE='your-passphrase'");
            std::process::exit(1);
        }
        Err(_) => {
            error!("HANDS_OFF_SECRET_PHRASE environment variable is not set");
            error!("Please set your passphrase using: export HANDS_OFF_SECRET_PHRASE='your-passphrase'");
            error!("This passphrase will be required to unlock HandsOff when input is locked");
            std::process::exit(1);
        }
    }

    // Load auto-lock timeout from environment variable
    if let Some(timeout) = parse_auto_lock_timeout() {
        state.lock().auto_lock_timeout = timeout;
    } else {
        info!(
            "Using default auto-lock timeout: {} seconds",
            state.lock().auto_lock_timeout
        );
    }

    // Create event tap for input blocking
    let event_tap =
        event_tap::create_event_tap(state.clone()).context("Failed to create event tap")?;
    unsafe {
        event_tap::enable_event_tap(event_tap);
    }

    // Create hotkey manager
    let mut hotkey_manager = HotkeyManager::new().context("Failed to create hotkey manager")?;
    hotkey_manager
        .register_lock_hotkey()
        .context("Failed to register lock hotkey")?;
    hotkey_manager
        .register_talk_hotkey()
        .context("Failed to register talk hotkey")?;

    // Start background threads
    start_buffer_reset_thread(state.clone());
    start_auto_lock_thread(state.clone());
    start_hotkey_listener_thread(state.clone(), hotkey_manager);

    // Start auto-unlock thread if enabled
    if auto_unlock_timeout.is_some() {
        start_auto_unlock_thread(state.clone());
    }

    // Display status and instructions
    info!("HandsOff is running - press Ctrl+C to quit");
    if state.is_locked() {
        info!("STATUS: INPUT IS LOCKED");
        info!("- Type your passphrase to unlock (input won't be visible)");
        info!("- Or press Ctrl+Cmd+Shift+U for Touch ID");
    } else {
        info!("STATUS: INPUT IS UNLOCKED");
        info!("- Press Ctrl+Cmd+Shift+L to lock input");
    }

    // Run the CFRunLoop on the main thread - this is required for event tap to work!
    info!("Starting CFRunLoop (required for event interception)...");
    use core_foundation::runloop::CFRunLoop;
    CFRunLoop::run_current();

    // CFRunLoop::run_current() runs indefinitely, so this is unreachable
    #[allow(unreachable_code)]
    Ok(())
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
    thread::spawn(move || {
        let mut check_count = 0u32;
        loop {
            thread::sleep(Duration::from_secs(10));
            check_count += 1;

            // Log remaining time every 60 seconds (6 checks of 10 seconds each)
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
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(10),
            "Should accept 10 seconds"
        );

        // Test typical value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "30");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(30),
            "Should accept 30 seconds"
        );

        // Test large value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "600");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(600),
            "Should accept 600 seconds"
        );

        // Test maximum valid value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "3600");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(3600),
            "Should accept 3600 seconds"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_disabled() {
        // Test explicit disable with 0
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "0");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should return None for 0"
        );

        // Test not set (should return None, not panic)
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should return None when not set"
        );
    }

    #[test]
    fn test_parse_auto_unlock_invalid_values() {
        // Test too low
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "5");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value below 10"
        );

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "9");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value below 10"
        );

        // Test too high
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "3601");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value above 3600"
        );

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "5000");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value above 3600"
        );

        // Test negative number (will fail to parse)
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "-10");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject negative value"
        );

        // Test non-numeric
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "invalid");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject non-numeric value"
        );

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "30s");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value with units"
        );

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject empty string"
        );

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
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(10),
            "Should accept 10 seconds"
        );

        // Test at maximum boundary
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "3600");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(3600),
            "Should accept 3600 seconds"
        );

        // Test just above maximum
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "3601");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject 3601 seconds"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }
}
