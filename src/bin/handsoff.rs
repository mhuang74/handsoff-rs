// HandsOff CLI - Command-line interface for input blocking utility
// This binary provides a terminal-based interface with argument parsing

use handsoff::{app_state::{AUTO_LOCK_MAX_SECONDS, AUTO_LOCK_MIN_SECONDS, AUTO_UNLOCK_MAX_SECONDS, AUTO_UNLOCK_MIN_SECONDS}, HandsOffCore};
use anyhow::{Context, Result};
use clap::Parser;
use log::{debug, error, info, warn};
use std::env;

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
  HANDS_OFF_AUTO_LOCK        Auto-lock timeout in seconds (20-600, default: 30)
                             Input will lock after this period of contiguous inactivity
                             Example: export HANDS_OFF_AUTO_LOCK=60

  HANDS_OFF_AUTO_UNLOCK      Auto-unlock timeout in seconds (60-900, or 0 to disable)
                             Safety feature: automatically unlocks after this duration
                             to prevent permanent lockouts
                             Example: export HANDS_OFF_AUTO_UNLOCK=300

HOTKEYS:
  Ctrl+Cmd+Shift+L          Lock input (blocks all keyboard/mouse input)
  Ctrl+Cmd+Shift+T          Talk mode (hold to allow spacebar keypress, for unmuting conf calls)

When locked, type your passphrase to unlock (input won't be visible on screen)."
)]
struct Args {
    /// Start with input locked immediately
    #[arg(short, long)]
    locked: bool,

    /// Auto-lock timeout in seconds of contiguous inactivity (20-600, default: 30, overrides HANDS_OFF_AUTO_LOCK)
    /// NOTE: Keep range/default values in sync with AUTO_LOCK_* constants
    #[arg(long)]
    auto_lock: Option<u64>,
}

/// Parse the HANDS_OFF_AUTO_UNLOCK environment variable
fn parse_auto_unlock_timeout() -> Option<u64> {
    match env::var("HANDS_OFF_AUTO_UNLOCK") {
        Ok(val) => match val.parse::<u64>() {
            Ok(seconds)
                if (AUTO_UNLOCK_MIN_SECONDS..=AUTO_UNLOCK_MAX_SECONDS).contains(&seconds) =>
            {
                info!("Auto-unlock safety feature enabled: {} seconds", seconds);
                Some(seconds)
            }
            Ok(0) => {
                info!("Auto-unlock disabled (value: 0)");
                None
            }
            Ok(seconds) => {
                warn!(
                    "Invalid auto-unlock timeout: {} (must be {}-{} or 0). Feature disabled.",
                    seconds, AUTO_UNLOCK_MIN_SECONDS, AUTO_UNLOCK_MAX_SECONDS
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
            Ok(seconds) if (AUTO_LOCK_MIN_SECONDS..=AUTO_LOCK_MAX_SECONDS).contains(&seconds) => {
                info!(
                    "Auto-lock timeout set via environment variable: {} seconds",
                    seconds
                );
                Some(seconds)
            }
            Ok(seconds) => {
                warn!(
                    "Invalid auto-lock timeout: {} (must be {}-{} seconds). Using default.",
                    seconds, AUTO_LOCK_MIN_SECONDS, AUTO_LOCK_MAX_SECONDS
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
    if !handsoff::input_blocking::check_accessibility_permissions() {
        error!("Accessibility permissions not granted");
        error!("Please grant accessibility permissions to HandsOff in System Preferences > Security & Privacy > Privacy > Accessibility");
        std::process::exit(1);
    }

    // Get passphrase from environment variable
    let passphrase = match env::var("HANDS_OFF_SECRET_PHRASE") {
        Ok(passphrase) if !passphrase.is_empty() => {
            info!("Using passphrase from HANDS_OFF_SECRET_PHRASE environment variable");
            passphrase
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
    };

    // Create HandsOffCore instance
    let mut core = HandsOffCore::new(&passphrase).context("Failed to initialize HandsOff")?;

    // Configure auto-unlock timeout
    let auto_unlock_timeout = parse_auto_unlock_timeout();
    core.set_auto_unlock_timeout(auto_unlock_timeout);

    // Configure auto-lock timeout (command-line takes precedence over environment)
    let auto_lock_timeout = match args.auto_lock {
        Some(timeout) if (AUTO_LOCK_MIN_SECONDS..=AUTO_LOCK_MAX_SECONDS).contains(&timeout) => {
            info!("Auto-lock timeout set via --auto-lock argument: {} seconds", timeout);
            Some(timeout)
        }
        Some(timeout) => {
            warn!(
                "Invalid --auto-lock value: {} (must be {}-{} seconds). Trying environment variable.",
                timeout, AUTO_LOCK_MIN_SECONDS, AUTO_LOCK_MAX_SECONDS
            );
            parse_auto_lock_timeout()
        }
        None => parse_auto_lock_timeout(),
    };
    core.set_auto_lock_timeout(auto_lock_timeout);

    // Set initial lock state
    if args.locked {
        core.set_locked(true);
        info!("Starting in LOCKED mode (--locked flag)");
    } else {
        info!("Starting in UNLOCKED mode (use --locked to start locked, or press Ctrl+Cmd+Shift+L to lock)");
    }

    // Start core components
    core.start_event_tap().context("Failed to start event tap")?;
    core.start_hotkeys().context("Failed to start hotkeys")?;
    core.start_background_threads().context("Failed to start background threads")?;

    // Display status and instructions
    info!("HandsOff is running - press Ctrl+C to quit");
    if core.is_locked() {
        info!("STATUS: INPUT IS LOCKED");
        info!("- Type your passphrase to unlock (input won't be visible)");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auto_unlock_valid_values() {
        // Test minimum valid value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "60");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(60),
            "Should accept 60 seconds"
        );

        // Test typical value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "300");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(300),
            "Should accept 300 seconds"
        );

        // Test large value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "600");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(600),
            "Should accept 600 seconds"
        );

        // Test maximum valid value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "900");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(900),
            "Should accept 900 seconds"
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
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "30");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value below 60"
        );

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "59");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value below 60"
        );

        // Test too high
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "901");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value above 900"
        );

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "1000");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value above 900"
        );

        // Test negative number (will fail to parse)
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "-60");
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
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "59");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject 59 seconds"
        );

        // Test at minimum boundary
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "60");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(60),
            "Should accept 60 seconds"
        );

        // Test at maximum boundary
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "900");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(900),
            "Should accept 900 seconds"
        );

        // Test just above maximum
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "901");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject 901 seconds"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_lock_valid_values() {
        // Test minimum valid value
        env::set_var("HANDS_OFF_AUTO_LOCK", "20");
        assert_eq!(
            parse_auto_lock_timeout(),
            Some(20),
            "Should accept 20 seconds"
        );

        // Test typical value
        env::set_var("HANDS_OFF_AUTO_LOCK", "60");
        assert_eq!(
            parse_auto_lock_timeout(),
            Some(60),
            "Should accept 60 seconds"
        );

        // Test maximum valid value
        env::set_var("HANDS_OFF_AUTO_LOCK", "600");
        assert_eq!(
            parse_auto_lock_timeout(),
            Some(600),
            "Should accept 600 seconds"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_LOCK");
    }

    #[test]
    fn test_parse_auto_lock_invalid_values() {
        // Test too low
        env::set_var("HANDS_OFF_AUTO_LOCK", "10");
        assert_eq!(
            parse_auto_lock_timeout(),
            None,
            "Should reject value below 20"
        );

        // Test too high
        env::set_var("HANDS_OFF_AUTO_LOCK", "601");
        assert_eq!(
            parse_auto_lock_timeout(),
            None,
            "Should reject value above 600"
        );

        // Test non-numeric
        env::set_var("HANDS_OFF_AUTO_LOCK", "invalid");
        assert_eq!(
            parse_auto_lock_timeout(),
            None,
            "Should reject non-numeric value"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_LOCK");
    }

    #[test]
    fn test_parse_auto_lock_boundary_cases() {
        // Test just below minimum
        env::set_var("HANDS_OFF_AUTO_LOCK", "19");
        assert_eq!(
            parse_auto_lock_timeout(),
            None,
            "Should reject 19 seconds"
        );

        // Test at minimum boundary
        env::set_var("HANDS_OFF_AUTO_LOCK", "20");
        assert_eq!(
            parse_auto_lock_timeout(),
            Some(20),
            "Should accept 20 seconds"
        );

        // Test at maximum boundary
        env::set_var("HANDS_OFF_AUTO_LOCK", "600");
        assert_eq!(
            parse_auto_lock_timeout(),
            Some(600),
            "Should accept 600 seconds"
        );

        // Test just above maximum
        env::set_var("HANDS_OFF_AUTO_LOCK", "601");
        assert_eq!(
            parse_auto_lock_timeout(),
            None,
            "Should reject 601 seconds"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_LOCK");
    }

    #[test]
    fn test_parse_auto_lock_not_set() {
        // Test not set (should return None, not panic)
        env::remove_var("HANDS_OFF_AUTO_LOCK");
        assert_eq!(
            parse_auto_lock_timeout(),
            None,
            "Should return None when not set"
        );
    }
}
