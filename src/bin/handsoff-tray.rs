// HandsOff Tray App - macOS menu bar application for input blocking
// This binary provides a native macOS tray icon with dropdown menu

use anyhow::{Context, Result};
use clap::Parser;
use handsoff::constants::{
    NOTIFICATION_ERROR_TIMEOUT_MS, NOTIFICATION_TIMEOUT_MS, POLL_INTERVAL_DISABLED_SECS,
    POLL_INTERVAL_ENABLED_MS, TOOLTIP_UPDATE_INTERVAL_MS,
};
use handsoff::{config, config_file::Config, setup, HandsOffCore};
use log::{error, info, warn};
use std::cell::RefCell;
use std::rc::Rc;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconBuilder;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_HASH: &str = env!("GIT_COMMIT_HASH");

/// HandsOff Tray App arguments
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "macOS menu bar app to block unsolicited input"
)]
struct Args {
    /// Run interactive setup to configure passphrase and timeouts
    #[arg(long)]
    setup: bool,
}

/// Run interactive setup: capture passphrase keycodes, prompt for options
fn run_setup() -> Result<()> {
    let outcome = setup::run_interactive_setup(&mut |s| println!("{}", s))?;

    // Create and save config
    let (auto_unlock_backoff, auto_unlock_base) = match outcome.auto_unlock {
        config::AutoUnlockConfig::Disabled => (false, 0),
        config::AutoUnlockConfig::Backoff { base_interval_secs } => {
            (true, base_interval_secs.get())
        }
    };
    let config = Config::new(
        &outcome.keycodes,
        outcome.auto_lock,
        auto_unlock_backoff,
        auto_unlock_base,
        outcome.lock_key,
        outcome.talk_key,
    )
    .context("Failed to create configuration")?;

    config.save().context("Failed to save configuration")?;

    println!(
        "\nConfiguration saved to: {}",
        Config::config_path().display()
    );
    println!("Setup complete!");
    println!("\nThe tray app will use this configuration at next startup.");

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

    info!("Starting HandsOff Tray App v{}", VERSION);

    // Check accessibility permissions (but don't exit - let app run and show status in tooltip)
    let initial_permissions = handsoff::input_blocking::check_accessibility_permissions();
    if !initial_permissions {
        warn!("Accessibility permissions not granted");
        warn!("App will start but input blocking will not work until permissions are granted");
        info!("Please grant accessibility permissions in System Settings > Privacy & Security > Accessibility");
    } else {
        info!("Accessibility permissions verified");
    }

    // Load configuration — first run WITHOUT a config must NOT silently
    // create a default passphrase (L-5/S-3). Direct the user to setup and exit.
    let cfg = match Config::load() {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Configuration not available: {}", e);
            show_alert(
                "HandsOff - Setup Required",
                "HandsOff needs a passphrase before it can protect input.\n\n\
                 Run the one-time setup from Terminal:\n\
                 ~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup\n\n\
                 (Setup captures your passphrase as physical key presses.)",
            );
            std::process::exit(1);
        }
    };

    // Create HandsOffCore instance from the stored passphrase hash
    let mut core = HandsOffCore::new(cfg.passphrase_hash.clone().unwrap());

    // Configure auto-unlock backoff (precedence: env var > config file mode + base > default enabled)
    core.set_auto_unlock_config(config::resolve_auto_unlock(
        Some(cfg.auto_unlock_backoff_enabled()),
        cfg.auto_unlock_base_interval,
    ));

    // Configure auto-lock timeout (precedence: env var > config file)
    let auto_lock_timeout = config::parse_auto_lock_timeout().or(Some(cfg.auto_lock_timeout));
    core.set_auto_lock_timeout(auto_lock_timeout);

    // Configure hotkeys from config file only (tray app does not support env var overrides)
    let lock_key = cfg.get_lock_key_code().with_context(|| {
        "Failed to parse lock hotkey from config file. Run setup: ~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup"
    })?;
    let talk_key = cfg.get_talk_key_code().with_context(|| {
        "Failed to parse talk hotkey from config file. Run setup: ~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup"
    })?;

    // Validate that configured hotkeys are different
    if lock_key == talk_key {
        error!("Lock and Talk hotkeys cannot be the same: {:?}", lock_key);
        show_alert(
            "HandsOff - Configuration Error",
            &format!(
                "Lock and Talk hotkeys cannot be the same.\n\nBoth are set to: {:?}\n\nThis is likely because the config file was manually edited.\n\nPlease run setup to reconfigure:\n~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup",
                lock_key
            ),
        );
        std::process::exit(1);
    }

    core.set_hotkey_config(lock_key, talk_key);

    // Start core components only if we have accessibility permissions
    if initial_permissions {
        core.start_event_tap()
            .context("Failed to start input blocking")?;
        core.start_hotkeys().context("Failed to start hotkeys")?;
        info!("HandsOff core components started");
    } else {
        info!("Skipping event tap and hotkeys start - waiting for accessibility permissions");
    }

    // Always start background threads (includes permission monitoring)
    core.start_background_threads()
        .context("Failed to start background threads")?;

    // NOTE: CFRunLoop thread is now managed by HandsOffCore
    // It starts when event tap is created and stops when event tap is destroyed
    // This eliminates the zombie CFRunLoop connection that caused WindowServer issues

    // Wrap core in Rc<RefCell> for event loop (single-threaded)
    let core = Rc::new(RefCell::new(core));

    // Create event loop for tray app
    let event_loop = EventLoopBuilder::new().build();

    // Build tray menu
    // Note: When locked, mouse clicks are blocked, so menu is inaccessible
    // Lock menu item only works when unlocked; unlock requires typing passphrase
    let lock_item = MenuItem::new("Lock Input", true, None);
    let disable_item = MenuItem::new("Disable", true, None);
    let separator = PredefinedMenuItem::separator();
    let reset_item = MenuItem::new("Reset", true, None);

    let menu = Menu::new();
    menu.append(&lock_item)
        .context("Failed to add lock menu item")?;
    menu.append(&disable_item)
        .context("Failed to add disable menu item")?;
    menu.append(&separator).context("Failed to add separator")?;
    menu.append(&reset_item)
        .context("Failed to add reset menu item")?;

    // Create tray icon
    let icon = create_icon_unlocked();
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("HandsOff - Input Blocker")
        .with_icon(icon)
        .build()
        .context("Failed to create tray icon")?;

    info!("Tray icon created, running event loop");

    // Clone IDs for event handling
    let lock_id = lock_item.id().clone();
    let disable_id = disable_item.id().clone();
    let reset_id = reset_item.id().clone();

    // Track state for tooltip updates and permission state
    let mut was_locked = false;
    let mut was_disabled = false;
    let mut last_tooltip = String::new();
    let mut last_tooltip_update = std::time::Instant::now();
    let mut has_permissions = true; // Assume true at start (already verified at startup)

    // Run event loop with periodic updates
    event_loop.run(move |_event, _, control_flow| {
        // Adjust polling interval based on disabled state
        // When disabled: minimal WindowServer interaction
        // When enabled: responsive UI updates
        let poll_interval = {
            let core_borrow = core.borrow();
            if core_borrow.state.is_disabled() {
                std::time::Duration::from_secs(POLL_INTERVAL_DISABLED_SECS)
            } else {
                std::time::Duration::from_millis(POLL_INTERVAL_ENABLED_MS)
            }
        };

        *control_flow = ControlFlow::WaitUntil(
            std::time::Instant::now() + poll_interval
        );

        // Handle menu events
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            let event_id = event.id;

            if event_id == lock_id {
                handle_lock_toggle(core.clone());
            } else if event_id == disable_id {
                info!("Disable menu item clicked");
                handle_disable(core.clone());
            } else if event_id == reset_id {
                info!("Reset menu item clicked, resetting app state");
                handle_reset(core.clone());
            }
        }

        // Check if event tap should be stopped (due to permission loss)
        {
            let mut core_borrow = core.borrow_mut();
            if core_borrow.state.should_stop_event_tap_and_clear() {
                warn!("Tray: Stopping input blocking due to permission loss");
                core_borrow.stop_event_tap();
                info!("Tray: Input blocking stopped - normal input restored");
            }
        }

        // Check if existing event tap should be re-enabled (post sleep/wake timeout recovery).
        // This reuses the same CGEventTapRef — no new WindowServer connection is created,
        // which prevents zombie Mach port accumulation across sleep/wake cycles.
        {
            let mut core_borrow = core.borrow_mut();
            if core_borrow.state.should_reenable_event_tap_and_clear() {
                info!("Tray: Re-enabling existing event tap after sleep/wake timeout");
                if let Err(e) = core_borrow.reenable_event_tap() {
                    warn!("Tray: Failed to re-enable event tap: {} — will attempt full restart", e);
                    // reenable_event_tap already falls back to restart internally,
                    // but log the failure so it's visible in telemetry
                }
            }
        }

        // Check if event tap should be started (permission restored)
        {
            let mut core_borrow = core.borrow_mut();
            if core_borrow.state.should_start_event_tap_and_clear() {
                info!("Tray: Restarting input blocking - permissions restored");
                match core_borrow.restart_event_tap() {
                    Ok(()) => {
                        info!("Tray: Input blocking restarted successfully");

                        #[cfg(target_os = "macos")]
                        {
                            let _ = notify_rust::Notification::new()
                                .summary("HandsOff - Input Blocking Restarted")
                                .body("Input blocking restarted successfully.\nHandsOff is now active.")
                                .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS))
                                .show();
                        }
                    }
                    Err(e) => {
                        warn!("Tray: Failed to restart input blocking: {}", e);

                        #[cfg(target_os = "macos")]
                        {
                            let _ = notify_rust::Notification::new()
                                .summary("HandsOff - Restart Failed")
                                .body(&format!(
                                    "Failed to restart input blocking: {}\n\nUse Reset menu to try again.",
                                    e
                                ))
                                .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_ERROR_TIMEOUT_MS))
                                .show();
                        }
                    }
                }
            }
        }

        // Periodically check permissions and update menu state
        let core_borrow = core.borrow();
        let is_locked = core_borrow.is_locked();
        let is_disabled = core_borrow.state.is_disabled();
        let current_permissions = core_borrow.has_accessibility_permissions();

        // Update Lock menu item enabled state based on permissions and disabled state
        // Only enable Lock when we have permissions AND are not already locked AND not disabled
        let should_enable_lock = current_permissions && !is_locked && !is_disabled;
        lock_item.set_enabled(should_enable_lock);

        // Update Disable menu item enabled state
        // Only enable Disable when we have permissions AND are not locked AND not already disabled
        let should_enable_disable = current_permissions && !is_locked && !is_disabled;
        disable_item.set_enabled(should_enable_disable);

        // Track permission state changes for logging
        let permission_changed = has_permissions != current_permissions;
        if permission_changed {
            if current_permissions {
                info!("Tray: Accessibility permissions detected, Lock menu enabled");
            } else {
                warn!("Tray: Accessibility permissions lost, Lock menu disabled");
            }
            has_permissions = current_permissions;
        }

        // Update icon when lock state or disabled state changes
        let state_transition = is_locked != was_locked || is_disabled != was_disabled;
        if state_transition {
            was_locked = is_locked;
            was_disabled = is_disabled;

            let icon = if is_disabled {
                create_icon_disabled()
            } else if is_locked {
                create_icon_locked()
            } else {
                create_icon_unlocked()
            };
            if let Err(e) = tray.set_icon(Some(icon)) {
                error!("Failed to update tray icon: {}", e);
            }

            // Show notification on state change (but not for disabled, handled elsewhere).
            // V10: unlock is silent — no notification when input is restored.
            #[cfg(target_os = "macos")]
            {
                if !is_disabled && is_locked {
                    let _ = notify_rust::Notification::new()
                        .summary("HandsOff")
                        .body("Input locked - Type passphrase to unlock")
                        .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS))
                        .show();
                }
            }
        }

        // Rebuild tooltip on state transitions or every TOOLTIP_UPDATE_INTERVAL_MS
        // (the countdown itself does not need per-second repaint; transitions repaint now)
        let cadence_elapsed = last_tooltip_update
            .elapsed()
            >= std::time::Duration::from_millis(TOOLTIP_UPDATE_INTERVAL_MS);
        if state_transition || permission_changed || cadence_elapsed {
            let tooltip = build_tooltip(&core_borrow, is_locked, is_disabled, current_permissions);
            if tooltip != last_tooltip {
                if let Err(e) = tray.set_tooltip(Some(&tooltip)) {
                    error!("Failed to update tray tooltip: {}", e);
                }
                last_tooltip = tooltip;
            }
            last_tooltip_update = std::time::Instant::now();
        }
    });
}

/// Handle lock from menu
/// Note: This only handles locking, not unlocking. When locked, mouse clicks are blocked,
/// so the menu is inaccessible. Users must type their passphrase to unlock (same as CLI).
fn handle_lock_toggle(core: Rc<RefCell<HandsOffCore>>) {
    let core = core.borrow();

    if core.is_locked() {
        // Menu should not be accessible when locked (mouse clicks blocked)
        // But if somehow clicked (e.g., during race condition), show info
        warn!("Lock menu clicked while already locked (shouldn't happen)");
    }

    // Lock immediately
    if let Err(e) = core.lock() {
        error!("Error locking: {}", e);
        show_alert("HandsOff - Error", &format!("Failed to lock: {}", e));
    } else {
        info!("Input locked via menu");
    }
}

/// Handle disable from menu
/// Disables HandsOff by stopping event tap and hotkeys for minimal CPU usage
fn handle_disable(core: Rc<RefCell<HandsOffCore>>) {
    let mut core = core.borrow_mut();

    if let Err(e) = core.disable() {
        error!("Error disabling: {}", e);
        show_alert("HandsOff - Error", &format!("Failed to disable: {}", e));
    } else {
        info!("HandsOff disabled - low system resources mode (input blocking paused)");
        #[cfg(target_os = "macos")]
        {
            let _ = notify_rust::Notification::new()
                .summary("HandsOff")
                .body("Disabled - Low system resources mode\nInput blocking paused. Use Reset to re-enable")
                .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS))
                .show();
        }
    }
}

/// Handle reset from menu
/// Resets the app state to default: unlocked with all timers reset
/// If disabled, re-enables the app. Otherwise, restarts the event tap if permissions are available
fn handle_reset(core: Rc<RefCell<HandsOffCore>>) {
    let mut core = core.borrow_mut();

    // Check if disabled - if so, enable instead of just restarting
    let is_disabled = core.state.is_disabled();

    // Unlock if currently locked (user-initiated recovery — S-2: no plaintext
    // verification under keycode passphrases; the operator is past the guard)
    if core.is_locked() {
        core.reset();
        info!("App state reset: unlocked successfully");
    }

    // If disabled, re-enable (which also restarts event tap and hotkeys)
    // Otherwise, just restart event tap
    if is_disabled {
        match core.enable() {
            Ok(()) => {
                info!("HandsOff re-enabled successfully during reset");
                #[cfg(target_os = "macos")]
                {
                    let _ = notify_rust::Notification::new()
                        .summary("HandsOff")
                        .body("App reset complete - Re-enabled and ready to use")
                        .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS))
                        .show();
                }
            }
            Err(e) => {
                warn!("Could not re-enable during reset: {}", e);
                show_alert(
                    "HandsOff - Reset Partial Success",
                    &format!("Timers cleared but could not re-enable:\n{}\n\nPlease check accessibility permissions.", e)
                );
            }
        }
    } else {
        // Attempt to restart event tap (will check permissions internally)
        match core.restart_event_tap() {
            Ok(()) => {
                info!("Input blocking restarted successfully during reset");
                #[cfg(target_os = "macos")]
                {
                    let _ = notify_rust::Notification::new()
                        .summary("HandsOff")
                        .body("Reset complete - Input blocking restarted\nReady to use")
                        .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS))
                        .show();
                }
            }
            Err(e) => {
                warn!("Could not restart input blocking during reset: {}", e);
                show_alert(
                    "HandsOff - Reset Partial Success",
                    &format!("Timers cleared but input blocking could not be restarted:\n{}\n\nPlease check accessibility permissions.", e)
                );
            }
        }
    }

    info!("Finished handling reset");
}

/// Show native macOS alert dialog
fn show_alert(title: &str, message: &str) {
    use std::process::Command;

    // Escape quotes in message
    let message = message.replace('"', "\\\"");

    let script = format!(
        r#"display dialog "{}" with title "{}" buttons {{"OK"}} default button "OK""#,
        message, title
    );

    let _ = Command::new("osascript").arg("-e").arg(&script).output();
}

/// Build tooltip text based on lock state, disabled state, and permission status
fn build_tooltip(
    core: &HandsOffCore,
    is_locked: bool,
    is_disabled: bool,
    has_permissions: bool,
) -> String {
    let mut tooltip = String::new();

    // Header with version
    tooltip.push_str(&format!("HandsOff v{} ({})\n", VERSION, GIT_HASH));
    tooltip.push_str("A macOS utility to block unsolicited input\n\n");

    // Current status
    if is_disabled {
        tooltip.push_str("STATUS: DISABLED\n");
        tooltip.push_str("Low system resources mode - all features paused\n");
        tooltip.push_str("Use Reset menu to re-enable HandsOff\n\n");
    } else if !has_permissions {
        tooltip.push_str("STATUS: NO PERMISSIONS\n");
        tooltip.push_str("Restore Accessibility Permissions in:\n");
        tooltip.push_str("System Settings > Privacy & Security\n");
        tooltip.push_str("Then use Reset menu to restart\n\n");
    } else if is_locked {
        // Show lock duration
        if let Some(elapsed) = core.get_lock_elapsed_secs() {
            tooltip.push_str(&format!("STATUS: LOCKED ({})\n", format_duration(elapsed)));
        } else {
            tooltip.push_str("STATUS: LOCKED\n");
        }

        // V11: show the auto-unlock countdown ONLY when < 5 min away, so the
        // far-out backoff schedule is not broadcast by the menu bar.
        if let Some(remaining) = core.get_auto_unlock_remaining_secs() {
            if remaining > 0 && remaining < 300 {
                tooltip.push_str(&format!("Auto-unlock in {}\n", format_duration(remaining)));
            }
        }
    } else {
        tooltip.push_str("STATUS: Unlocked\n");

        // Show auto-lock countdown if enabled
        if let Some(remaining) = core.get_auto_lock_remaining_secs() {
            if remaining > 0 {
                tooltip.push_str(&format!("Auto-lock in {}\n", format_duration(remaining)));
            } else {
                tooltip.push_str("Auto-locking...\n");
            }
        }
    }

    tooltip.push_str("\n\n");

    // Menu items
    tooltip.push_str("MENU:\n");
    tooltip.push_str("• Lock Input: Lock immediately\n");
    tooltip.push_str("• Disable: Pause input blocking and reduce system resources\n");
    tooltip.push_str("  (Use Reset to re-enable HandsOff)\n");
    tooltip.push_str("• Reset: Clear all timers and restart input blocking\n\n");

    // Instructions
    let lock_key = core.get_lock_key_display();
    let talk_key = core.get_talk_key_display();

    tooltip.push_str("TO LOCK:\n");
    tooltip.push_str("• Click 'Lock Input' menu, OR\n");
    tooltip.push_str(&format!("• Press Ctrl+Cmd+Shift+{}\n\n", lock_key));

    tooltip.push_str("TO UNLOCK:\n");
    tooltip.push_str("• Type your passphrase on keyboard\n");
    tooltip.push_str("• Press Escape to clear buffer immediately if you mistype\n");
    tooltip.push_str("• Or wait 3 seconds for auto-clear\n\n");

    // Hotkeys
    tooltip.push_str("HOTKEYS:\n");
    tooltip.push_str(&format!("• Ctrl+Cmd+Shift+{}: Lock input\n", lock_key));
    tooltip.push_str(&format!(
        "• Ctrl+Cmd+Shift+{} (hold): Hotkey to Unmute (Spacebar)\n\n",
        talk_key
    ));

    // Setup
    tooltip.push_str("Tip: Run ~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup to set passphrase and timeouts\n\n");

    // Repository info
    tooltip.push_str("Michael S. Huang\n");
    tooltip.push_str("https://github.com/mhuang74/handsoff-rs");

    tooltip
}

/// Format duration in human-readable form (e.g., "2m 30s" or "45s")
fn format_duration(seconds: u64) -> String {
    if seconds >= 60 {
        let mins = seconds / 60;
        let secs = seconds % 60;
        if secs > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}m", mins)
        }
    } else {
        format!("{}s", seconds)
    }
}

/// Create unlocked icon (green circle)
fn create_icon_unlocked() -> tray_icon::Icon {
    let png_data = include_bytes!("../../assets/tray_unlocked.png");
    load_png_icon(png_data)
}

/// Create locked icon (red circle)
fn create_icon_locked() -> tray_icon::Icon {
    let png_data = include_bytes!("../../assets/tray_locked.png");
    load_png_icon(png_data)
}

/// Create disabled icon
fn create_icon_disabled() -> tray_icon::Icon {
    let png_data = include_bytes!("../../assets/tray_disabled.png");
    load_png_icon(png_data)
}

/// Load PNG icon from embedded bytes
fn load_png_icon(png_data: &[u8]) -> tray_icon::Icon {
    use image::ImageReader;
    use std::io::Cursor;

    // Decode PNG to RGBA
    let img = ImageReader::new(Cursor::new(png_data))
        .with_guessed_format()
        .expect("Failed to detect PNG format")
        .decode()
        .expect("Failed to decode PNG icon");

    // Convert to RGBA8
    let rgba_img = img.to_rgba8();
    let (width, height) = rgba_img.dimensions();
    let rgba_data = rgba_img.into_raw();

    tray_icon::Icon::from_rgba(rgba_data, width, height)
        .expect("Failed to create icon from RGBA data")
}
