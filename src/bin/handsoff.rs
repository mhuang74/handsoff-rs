// HandsOff CLI - Command-line interface for input blocking utility
// This binary provides a terminal-based interface with argument parsing

use handsoff::app_state::{AUTO_LOCK_MAX_SECONDS, AUTO_LOCK_MIN_SECONDS};
use handsoff::{config, config_file::Config, HandsOffCore};
use anyhow::{Context, Result};
use clap::Parser;
use log::{error, info, warn};
use std::io::{self, Write};

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

SETUP:
  Before using HandsOff, run the setup command to configure your passphrase:
    handsoff --setup

  This will prompt you for:
    - Secret passphrase (typing hidden for security)
    - Auto-lock timeout (default: 30 seconds)
    - Auto-unlock timeout (default: 60 seconds)

  Configuration is stored encrypted at:
    ~/Library/Application Support/handsoff/config.toml

HOTKEYS:
  Ctrl+Cmd+Shift+L          Lock input (blocks all keyboard/mouse input)
  Ctrl+Cmd+Shift+T          Talk mode (hold to allow spacebar keypress, for unmuting conf calls)

When locked, type your passphrase to unlock (input won't be visible on screen)."
)]
struct Args {
    /// Start with input locked immediately
    #[arg(short, long)]
    locked: bool,

    /// Auto-lock timeout in seconds of contiguous inactivity (20-600, overrides config file)
    /// NOTE: Keep range/default values in sync with AUTO_LOCK_* constants
    #[arg(long)]
    auto_lock: Option<u64>,

    /// Run interactive setup to configure passphrase and timeouts
    #[arg(long)]
    setup: bool,
}

/// Helper function to prompt for a number with a default value
fn prompt_number(prompt: &str, default: u64) -> Result<u64> {
    print!("{}", prompt);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        Ok(default)
    } else {
        input.parse::<u64>()
            .with_context(|| format!("Invalid number: {}", input))
    }
}

/// Run interactive setup to configure passphrase and timeouts
fn run_setup() -> Result<()> {
    println!("HandsOff Setup");
    println!("==============\n");

    // Prompt for passphrase (non-echoing)
    let passphrase = rpassword::prompt_password("Enter passphrase: ")
        .context("Failed to read passphrase")?;

    if passphrase.is_empty() {
        anyhow::bail!("Error: Passphrase cannot be empty");
    }

    // Confirm passphrase
    let confirm = rpassword::prompt_password("Confirm passphrase: ")
        .context("Failed to read confirmation")?;

    if passphrase != confirm {
        anyhow::bail!("Error: Passphrases do not match");
    }

    // Prompt for timeouts
    let auto_lock = prompt_number(
        "Auto-lock timeout in seconds (default: 30): ",
        30
    )?;

    let auto_unlock = prompt_number(
        "Auto-unlock timeout in seconds (default: 60): ",
        60
    )?;

    // Create and save config
    let config = Config::new(&passphrase, auto_lock, auto_unlock)
        .context("Failed to create configuration")?;

    config.save()
        .context("Failed to save configuration")?;

    println!("\nConfiguration saved to: {}", Config::config_path().display());
    println!("Setup complete!");
    println!("\nYou can now run 'handsoff' to start the application.");

    Ok(())
}

fn main() -> Result<()> {
    // Parse command-line arguments
    let args = Args::parse();

    // Handle setup command
    if args.setup {
        return run_setup();
    }

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

    // Load configuration
    let cfg = match Config::load() {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            error!("\nRun 'handsoff --setup' to configure the application.");
            std::process::exit(1);
        }
    };

    // Decrypt passphrase
    let passphrase = match cfg.get_passphrase() {
        Ok(p) => {
            info!("Configuration loaded from: {}", Config::config_path().display());
            p
        }
        Err(e) => {
            error!("Failed to decrypt passphrase: {}", e);
            error!("Your configuration file may be corrupted.");
            error!("Run 'handsoff --setup' to reconfigure.");
            std::process::exit(1);
        }
    };

    // Create HandsOffCore instance
    let mut core = HandsOffCore::new(&passphrase).context("Failed to initialize HandsOff")?;

    // Configure auto-unlock timeout (from config file, can be overridden by env var)
    let auto_unlock_timeout = config::parse_auto_unlock_timeout()
        .or(Some(cfg.auto_unlock_timeout));
    core.set_auto_unlock_timeout(auto_unlock_timeout);

    // Configure auto-lock timeout (precedence: CLI arg > env var > config file)
    let auto_lock_timeout = match args.auto_lock {
        Some(timeout) if (AUTO_LOCK_MIN_SECONDS..=AUTO_LOCK_MAX_SECONDS).contains(&timeout) => {
            info!("Auto-lock timeout set via --auto-lock argument: {} seconds", timeout);
            Some(timeout)
        }
        Some(timeout) => {
            warn!(
                "Invalid --auto-lock value: {} (must be {}-{} seconds). Using config file or environment variable.",
                timeout, AUTO_LOCK_MIN_SECONDS, AUTO_LOCK_MAX_SECONDS
            );
            config::parse_auto_lock_timeout().or(Some(cfg.auto_lock_timeout))
        }
        None => config::parse_auto_lock_timeout().or(Some(cfg.auto_lock_timeout)),
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

    // Run the event loop on the main thread - this is required for event tap to work!
    info!("Starting event loop (required for event interception)...");
    use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
    use std::time::Duration;

    // Main event loop - polls every 500ms
    loop {
        // Run CFRunLoop for a brief period to process events
        unsafe {
            CFRunLoop::run_in_mode(
                kCFRunLoopDefaultMode,
                Duration::from_millis(500),
                false  // Don't return after single source handled
            );
        }

        // Check if we should exit (permission loss detected by event tap callback)
        if core.state.should_exit_and_clear() {
            warn!("Accessibility permissions lost - exiting");
            eprintln!("\nERROR: Accessibility permissions were revoked.");
            eprintln!("HandsOff cannot function without accessibility permissions.\n");
            eprintln!("To restore:");
            eprintln!("1. Open System Settings > Privacy & Security > Accessibility");
            eprintln!("2. Enable HandsOff in the list");
            eprintln!("3. Restart HandsOff CLI\n");
            eprintln!("Exiting...");

            // Clean shutdown
            core.stop_event_tap();
            break;
        }

        // Check if event tap should be stopped (fallback for permission monitor detection)
        if core.state.should_stop_event_tap_and_clear() {
            warn!("Stopping event tap due to permission loss (detected by monitor)");
            core.stop_event_tap();

            // For CLI, if event tap stops, we should exit
            eprintln!("\nEvent tap stopped due to permission loss. Exiting...");
            break;
        }
    }

    info!("CLI shutdown complete");
    Ok(())
}
