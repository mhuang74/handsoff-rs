// HandsOff Tray App - macOS menu bar application for input blocking
// This binary provides a native macOS tray icon with dropdown menu

use handsoff::HandsOffCore;
use anyhow::{Context, Result};
use log::{error, info};
use std::env;
use std::sync::{Arc, Mutex};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};
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
    let auto_unlock_timeout = parse_auto_unlock_timeout();
    core.set_auto_unlock_timeout(auto_unlock_timeout);

    // Configure auto-lock timeout (from environment)
    let auto_lock_timeout = parse_auto_lock_timeout();
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

    // Clone tray for event handling
    let tray_handle = tray.clone();
    let core_for_menu_update = core.clone();

    // Spawn thread to monitor lock state and update menu/icon
    std::thread::spawn(move || {
        let mut was_locked = false;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));

            let is_locked = {
                let core = core_for_menu_update.lock().unwrap();
                core.is_locked()
            };

            if is_locked != was_locked {
                was_locked = is_locked;

                // Update icon
                let icon = if is_locked {
                    create_icon_locked()
                } else {
                    create_icon_unlocked()
                };
                if let Err(e) = tray_handle.set_icon(Some(icon)) {
                    error!("Failed to update tray icon: {}", e);
                }

                // Update menu item text
                let label = if is_locked {
                    "Unlock Input"
                } else {
                    "Lock Input"
                };
                if let Err(e) = lock_item.set_text(label) {
                    error!("Failed to update menu item text: {}", e);
                }

                // Show notification
                #[cfg(target_os = "macos")]
                {
                    let _ = notify_rust::Notification::new()
                        .summary("HandsOff")
                        .body(if is_locked {
                            "Input locked"
                        } else {
                            "Input unlocked"
                        })
                        .timeout(notify_rust::Timeout::Milliseconds(3000))
                        .show();
                }
            }
        }
    });

    // Run event loop
    event_loop.run(move |_event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

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
            }
        }
    });
}

/// Handle lock/unlock toggle from menu
fn handle_lock_toggle(core: Arc<Mutex<HandsOffCore>>) {
    let mut core = core.lock().unwrap();

    if core.is_locked() {
        // Prompt for passphrase to unlock
        if let Some(passphrase) = prompt_passphrase() {
            match core.unlock(&passphrase) {
                Ok(true) => {
                    info!("Input unlocked via menu");
                }
                Ok(false) => {
                    error!("Invalid passphrase");
                    show_alert("Unlock Failed", "Invalid passphrase. Please try again.");
                }
                Err(e) => {
                    error!("Error unlocking: {}", e);
                    show_alert("Error", &format!("Failed to unlock: {}", e));
                }
            }
        }
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
        "HandsOff Tray App\n\nMenu Items:\n\
        • Lock/Unlock: Toggle input blocking\n\
        • Version: Show version information\n\
        • Help: Show this help\n\
        • Quit: Exit the application\n\n\
        Hotkeys:\n\
        • Ctrl+Cmd+Shift+L: Lock/unlock input\n\
        • Ctrl+Cmd+Shift+T (hold): Talk mode (allow spacebar)\n\n\
        Configuration:\n\
        Set HANDS_OFF_SECRET_PHRASE environment variable before launching.\n\n\
        Permissions:\n\
        Requires Accessibility permission in System Preferences."
    );
}

/// Prompt for passphrase using native macOS dialog
fn prompt_passphrase() -> Option<String> {
    use std::process::Command;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(r#"display dialog "Enter passphrase to unlock HandsOff:" default answer "" with hidden answer buttons {"Cancel", "OK"} default button "OK""#)
        .output()
        .ok()?;

    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout);
        // Parse "button returned:OK, text returned:password" format
        result
            .split("text returned:")
            .nth(1)?
            .trim()
            .to_string()
            .into()
    } else {
        None
    }
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

/// Parse the HANDS_OFF_AUTO_UNLOCK environment variable
fn parse_auto_unlock_timeout() -> Option<u64> {
    use handsoff::app_state::{AUTO_UNLOCK_MIN_SECONDS, AUTO_UNLOCK_MAX_SECONDS};
    use log::{debug, warn};

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
    use handsoff::app_state::{AUTO_LOCK_MIN_SECONDS, AUTO_LOCK_MAX_SECONDS};
    use log::{debug, warn};

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
