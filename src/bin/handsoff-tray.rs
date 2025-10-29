// HandsOff Tray App - macOS menu bar application for input blocking
// This binary provides a native macOS tray icon with dropdown menu

use handsoff::{config, HandsOffCore};
use anyhow::{Context, Result};
use log::{error, info};
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

    // Wrap core in Arc<Mutex> for event loop
    let core = Arc::new(Mutex::new(core));

    // Create event loop for tray app
    let event_loop = EventLoopBuilder::new().build();

    // Build tray menu
    // Note: When locked, mouse clicks are blocked, so menu is inaccessible
    // Lock menu item only works when unlocked; unlock requires typing passphrase
    let lock_item = MenuItem::new("Lock Input", true, None);
    let separator = PredefinedMenuItem::separator();
    let version_item = MenuItem::new(format!("Version {}", VERSION), true, None);
    let help_item = MenuItem::new("Help", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let menu = Menu::new();
    menu.append(&lock_item).context("Failed to add lock menu item")?;
    menu.append(&separator).context("Failed to add separator")?;
    menu.append(&version_item).context("Failed to add version menu item")?;
    menu.append(&help_item).context("Failed to add help menu item")?;
    menu.append(&quit_item).context("Failed to add quit menu item")?;

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
    let version_id = version_item.id().clone();
    let help_id = help_item.id().clone();
    let quit_id = quit_item.id().clone();

    // Track state for tooltip updates
    let mut was_locked = false;
    let mut last_tooltip = String::new();

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
            } else if event_id == version_id {
                show_version();
            } else if event_id == help_id {
                show_help();
            } else if event_id == quit_id {
                info!("Quit menu item clicked, exiting");
                *control_flow = ControlFlow::Exit;
                return;
            }
        }

        // Periodically update icon and tooltip based on lock state
        let core_lock = core.lock().unwrap();
        let is_locked = core_lock.is_locked();

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

        // Always update tooltip (to show live countdown)
        let tooltip = build_tooltip(&core_lock, is_locked);
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
        info!("Lock menu clicked while already locked (shouldn't happen - mouse blocked)");
        show_alert(
            "Already Locked",
            "Input is already locked.\n\nTo unlock, type your passphrase on the keyboard.\n\n(The menu is inaccessible when locked because mouse clicks are blocked.)"
        );
    } else {
        // Lock immediately
        if let Err(e) = core.lock() {
            error!("Error locking: {}", e);
            show_alert("Error", &format!("Failed to lock: {}", e));
        } else {
            info!("Input locked via menu");
        }
    }
}

/// Show version information
fn show_version() {
    info!("Version menu item clicked");
    show_alert(
        "HandsOff Version",
        &format!("HandsOff Tray App\nVersion {}\n\nA macOS utility to block unsolicited input.", VERSION)
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
        • Version: Show version information\n\
        • Help: Show this help\n\
        • Quit: Exit the application\n\n\
        Locking:\n\
        • Click 'Lock Input' menu item, OR\n\
        • Press Ctrl+Cmd+Shift+L hotkey\n\n\
        Unlocking:\n\
        • Type your passphrase on the keyboard\n\
        • (Menu is NOT clickable when locked - mouse blocked)\n\
        • Wait 5 seconds between attempts if you mistype\n\n\
        Hotkeys:\n\
        • Ctrl+Cmd+Shift+L: Lock input\n\
        • Ctrl+Cmd+Shift+T (hold): Talk mode (allow spacebar)\n\n\
        Configuration:\n\
        Set HANDS_OFF_SECRET_PHRASE environment variable before launching.\n\n\
        Permissions:\n\
        Requires Accessibility permission in System Preferences."
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

/// Build tooltip text based on lock state
fn build_tooltip(core: &HandsOffCore, is_locked: bool) -> String {
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

/// Create unlocked icon (green circle with unlock symbol)
fn create_icon_unlocked() -> tray_icon::Icon {
    // Use simple text icon for now (Unicode lock symbol)
    // In production, you'd want to load PNG/ICNS files
    create_text_icon("🔓")
}

/// Create locked icon (red circle with lock symbol)
fn create_icon_locked() -> tray_icon::Icon {
    create_text_icon("🔒")
}

/// Create an icon from text (emoji)
/// This is a simple implementation. For production, load PNG/ICNS files.
fn create_text_icon(text: &str) -> tray_icon::Icon {
    // Create a simple 32x32 RGBA icon with the emoji
    // This is a placeholder - ideally load from assets/
    let size = 32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    // For now, create a simple colored square
    // Green for unlocked, red for locked
    let color = if text == "🔓" {
        [0, 255, 0, 255] // Green
    } else {
        [255, 0, 0, 255] // Red
    };

    for i in 0..(size * size) as usize {
        rgba[i * 4] = color[0];
        rgba[i * 4 + 1] = color[1];
        rgba[i * 4 + 2] = color[2];
        rgba[i * 4 + 3] = color[3];
    }

    tray_icon::Icon::from_rgba(rgba, size, size).expect("Failed to create icon")
}
