// HandsOff Tray App - macOS menu bar application for input blocking
// This binary provides a native macOS tray icon with dropdown menu

use handsoff::{config, HandsOffCore};
use anyhow::{Context, Result};
use log::{error, info, warn};
use std::env;
use std::sync::{Arc, Mutex};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconBuilder;
use tao::event_loop::{ControlFlow, EventLoopBuilder};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    info!("Starting HandsOff Tray App v{}", VERSION);

    // Check accessibility permissions
    if !handsoff::input_blocking::check_accessibility_permissions() {
        error!("Accessibility permissions not granted");
        error!("Please grant accessibility permissions to HandsOff in System Preferences > Security & Privacy > Privacy > Accessibility");

        // Show native alert
        show_alert(
            "Accessibility Permissions Required",
            "HandsOff requires Accessibility permissions.\n\nPlease go to:\nSystem Preferences > Security & Privacy > Privacy > Accessibility\n\nand grant permissions to HandsOff."
        );

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
            show_alert(
                "Configuration Error",
                "HANDS_OFF_SECRET_PHRASE is set but empty.\n\nPlease set a valid passphrase using:\nexport HANDS_OFF_SECRET_PHRASE='your-passphrase'"
            );
            std::process::exit(1);
        }
        Err(_) => {
            error!("HANDS_OFF_SECRET_PHRASE environment variable is not set");
            show_alert(
                "Configuration Error",
                "HANDS_OFF_SECRET_PHRASE environment variable is not set.\n\nPlease set your passphrase using:\nexport HANDS_OFF_SECRET_PHRASE='your-passphrase'"
            );
            std::process::exit(1);
        }
    };

    // Create HandsOffCore instance
    let mut core = HandsOffCore::new(&passphrase).context("Failed to initialize HandsOff")?;

    // Configure auto-unlock timeout (from environment)
    let auto_unlock_timeout = config::parse_auto_unlock_timeout();
    core.set_auto_unlock_timeout(auto_unlock_timeout);

    // Configure auto-lock timeout (from environment)
    let auto_lock_timeout = config::parse_auto_lock_timeout();
    core.set_auto_lock_timeout(auto_lock_timeout);

    // Start core components
    core.start_event_tap().context("Failed to start event tap")?;
    core.start_hotkeys().context("Failed to start hotkeys")?;
    core.start_background_threads().context("Failed to start background threads")?;

    info!("HandsOff core components started");

    // CRITICAL: Start CFRunLoop in a background thread
    // The event tap requires CFRunLoop to be running to intercept events
    // Without this, the event tap callback is never invoked and input blocking doesn't work
    std::thread::spawn(|| {
        info!("Starting CFRunLoop in background thread (required for event tap)");
        use core_foundation::runloop::CFRunLoop;
        CFRunLoop::run_current();
    });

    // Wrap core in Arc<Mutex> for event loop
    let core = Arc::new(Mutex::new(core));

    // Create event loop for tray app
    let event_loop = EventLoopBuilder::new().build();

    // Build tray menu
    // Note: When locked, mouse clicks are blocked, so menu is inaccessible
    // Lock menu item only works when unlocked; unlock requires typing passphrase
    let lock_item = MenuItem::new("Lock Input", true, None);
    let separator = PredefinedMenuItem::separator();
    let about_item = MenuItem::new("About", true, None);
    let help_item = MenuItem::new("Help", true, None);
    let reset_item = MenuItem::new("Reset", true, None);

    let menu = Menu::new();
    menu.append(&lock_item).context("Failed to add lock menu item")?;
    menu.append(&separator).context("Failed to add separator")?;
    menu.append(&help_item).context("Failed to add help menu item")?;
    menu.append(&about_item).context("Failed to add about menu item")?;
    menu.append(&reset_item).context("Failed to add reset menu item")?;

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
    let about_id = about_item.id().clone();
    let help_id = help_item.id().clone();
    let reset_id = reset_item.id().clone();

    // Store passphrase for reset functionality
    let passphrase_for_reset = passphrase.clone();

    // Track state for tooltip updates and permission state
    let mut was_locked = false;
    let mut last_tooltip = String::new();
    let mut has_permissions = true; // Assume true at start (already verified at startup)

    // Run event loop with periodic updates
    event_loop.run(move |_event, _, control_flow| {
        // Wait for 500ms or until an event occurs
        *control_flow = ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(500)
        );

        // Handle menu events
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            let event_id = event.id;

            if event_id == lock_id {
                handle_lock_toggle(core.clone());
            } else if event_id == about_id {
                show_about();
            } else if event_id == help_id {
                show_help();
            } else if event_id == reset_id {
                info!("Reset menu item clicked, resetting app state");
                handle_reset(core.clone(), &passphrase_for_reset);
            }
        }

        // Periodically check permissions and update menu state
        let core_lock = core.lock().unwrap();
        let is_locked = core_lock.is_locked();
        let current_permissions = core_lock.has_accessibility_permissions();

        // Update Lock menu item enabled state based on permissions
        // Only enable Lock when we have permissions AND are not already locked
        let should_enable_lock = current_permissions && !is_locked;
        lock_item.set_enabled(should_enable_lock);

        // Track permission state changes for logging
        if has_permissions != current_permissions {
            if current_permissions {
                info!("Tray: Accessibility permissions detected, Lock menu enabled");
            } else {
                warn!("Tray: Accessibility permissions lost, Lock menu disabled");
            }
            has_permissions = current_permissions;
        }

        // Update icon when lock state changes
        if is_locked != was_locked {
            was_locked = is_locked;

            let icon = if is_locked {
                create_icon_locked()
            } else {
                create_icon_unlocked()
            };
            if let Err(e) = tray.set_icon(Some(icon)) {
                error!("Failed to update tray icon: {}", e);
            }

            // Show notification on state change
            #[cfg(target_os = "macos")]
            {
                let _ = notify_rust::Notification::new()
                    .summary("HandsOff")
                    .body(if is_locked {
                        "Input locked - Type passphrase to unlock"
                    } else {
                        "Input unlocked"
                    })
                    .timeout(notify_rust::Timeout::Milliseconds(3000))
                    .show();
            }
        }

        // Always update tooltip (to show live countdown and permission status)
        let tooltip = build_tooltip(&core_lock, is_locked, current_permissions);
        if tooltip != last_tooltip {
            if let Err(e) = tray.set_tooltip(Some(&tooltip)) {
                error!("Failed to update tray tooltip: {}", e);
            }
            last_tooltip = tooltip;
        }
    });
}

/// Handle lock from menu
/// Note: This only handles locking, not unlocking. When locked, mouse clicks are blocked,
/// so the menu is inaccessible. Users must type their passphrase to unlock (same as CLI).
fn handle_lock_toggle(core: Arc<Mutex<HandsOffCore>>) {
    let core = core.lock().unwrap();

    if core.is_locked() {
        // Menu should not be accessible when locked (mouse clicks blocked)
        // But if somehow clicked (e.g., during race condition), show info
        warn!("Lock menu clicked while already locked (shouldn't happen)");
    }

    // Lock immediately
    if let Err(e) = core.lock() {
        error!("Error locking: {}", e);
        show_alert("Error", &format!("Failed to lock: {}", e));
    } else {
        info!("Input locked via menu");
    }
}

/// Handle reset from menu
/// Resets the app state to default: unlocked with all timers reset
fn handle_reset(core: Arc<Mutex<HandsOffCore>>, passphrase: &str) {
    let core = core.lock().unwrap();

    // Unlock if currently locked (this also resets lock timer)
    if core.is_locked() {
        match core.unlock(passphrase) {
            Ok(true) => {
                info!("App state reset: unlocked successfully");
            }
            Ok(false) => {
                // This shouldn't happen as we're using the stored passphrase
                error!("Failed to unlock during reset: invalid passphrase");
                show_alert("Reset Error", "Failed to unlock. This is unexpected - please check logs.");
                return;
            }
            Err(e) => {
                error!("Error during reset unlock: {}", e);
                show_alert("Reset Error", &format!("Failed to reset: {}", e));
                return;
            }
        }
    }

    // Note: Unlocking automatically resets the lock timer and related state
    // Auto-lock and auto-unlock timers are managed by the core and will reset on next lock
    info!("App state reset complete");

    #[cfg(target_os = "macos")]
    {
        let _ = notify_rust::Notification::new()
            .summary("HandsOff")
            .body("App state reset - Ready to use")
            .timeout(notify_rust::Timeout::Milliseconds(3000))
            .show();
    }
}

/// Show about information
fn show_about() {
    info!("About menu item clicked");
    show_alert(
        "About HandsOff",
        &format!("HandsOff Tray App\nVersion {}\n\nA macOS utility to block unsolicited input.\n\nMichael S. Huang\nhttps://github.com/mhuang74/handsoff-rs", VERSION)
    );
}

/// Show help information
fn show_help() {
    info!("Help menu item clicked");
    show_alert(
        "HandsOff Help",
        "HandsOff Tray App\n\n\
        Menu Items:\n\
        • Lock Input: Lock immediately (menu inaccessible when locked)\n\
        • Help: Show this help\n\
        • About: Show version and project information\n\
        • Reset: Reset app state (unlock and reset all timers)\n\n\
        To Lock:\n\
        • Click 'Lock Input' menu item, OR\n\
        • Press Ctrl+Cmd+Shift+L hotkey\n\n\
        To Unlock:\n\
        • Type your passphrase on the keyboard\n\
        • (Menu is NOT clickable when locked - mouse blocked)\n\
        • Wait 5 seconds between attempts if you mistype\n\n\
        Hotkeys:\n\
        • Ctrl+Cmd+Shift+L: Lock input\n\
        • Ctrl+Cmd+Shift+T (hold): Talk mode (Spacebar passthrough)\n\n\
        Permissions:\n\
        Requires Accessibility permission in System Preferences.\n\n\
        Safety Features:\n\
        • Permission Monitor: Checks every 5 seconds\n\
        • Emergency Unlock: Auto-unlocks if permissions are revoked while locked\n\
        • This prevents lockout if permissions are removed while running"
    );
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

    let _ = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output();
}

/// Build tooltip text based on lock state and permission status
fn build_tooltip(core: &HandsOffCore, is_locked: bool, has_permissions: bool) -> String {
    // Show permission warning if missing
    if !has_permissions {
        return "HandsOff - NO PERMISSIONS\nCannot lock until restored".to_string();
    }

    if !is_locked {
        return "HandsOff - Unlocked".to_string();
    }

    // Locked state - build detailed tooltip
    let mut tooltip = String::new();

    // Show lock duration
    if let Some(elapsed) = core.get_lock_elapsed_secs() {
        tooltip.push_str(&format!("HandsOff - Locked ({})\n", format_duration(elapsed)));
    } else {
        tooltip.push_str("HandsOff - Locked\n");
    }

    // Show auto-unlock countdown if enabled
    if let Some(remaining) = core.get_auto_unlock_remaining_secs() {
        if remaining > 0 {
            tooltip.push_str(&format!("Auto-unlock in {}\n", format_duration(remaining)));
        } else {
            tooltip.push_str("Auto-unlocking...\n");
        }
    }

    // Always show unlock instructions
    tooltip.push_str("Type passphrase to unlock");

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
