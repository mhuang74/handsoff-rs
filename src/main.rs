#[macro_use]
extern crate objc;

mod app_state;
mod auth;
mod input_blocking;
mod ui;
mod utils;

use anyhow::{Context, Result};
use app_state::AppState;
use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicyAccessory};
use cocoa::base::nil;
use cocoa::foundation::NSAutoreleasePool;
use input_blocking::event_tap;
use input_blocking::hotkeys::HotkeyManager;
use log::{error, info};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    info!("Starting HandsOff Input Lock");

    // Check accessibility permissions
    if !input_blocking::check_accessibility_permissions() {
        error!("Accessibility permissions not granted");
        ui::dialogs::show_permissions_dialog();
        std::process::exit(1);
    }

    // Create app state
    let state = Arc::new(AppState::new());

    // Load passphrase hash from keychain
    match auth::keychain::retrieve_passphrase_hash() {
        Ok(Some(hash)) => {
            info!("Loaded passphrase hash from keychain");
            state.set_passphrase_hash(hash);
        }
        Ok(None) => {
            info!("No passphrase set - prompting user");
            if let Some(passphrase) = ui::dialogs::show_set_passphrase_dialog() {
                let hash = auth::hash_passphrase(&passphrase);
                if let Err(e) = auth::keychain::store_passphrase_hash(&hash) {
                    error!("Failed to store passphrase: {}", e);
                } else {
                    state.set_passphrase_hash(hash);
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

    // Create menu bar app
    unsafe {
        let _pool = NSAutoreleasePool::new(nil);

        let app = NSApp();
        app.setActivationPolicy_(NSApplicationActivationPolicyAccessory);

        // Create menu bar
        let _menubar = ui::menubar::MenuBar::new(state.clone());

        info!("HandsOff is running");

        // Run the app
        app.run();
    }

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
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(10));

        if state.should_auto_lock() {
            info!("Auto-lock triggered after inactivity");
            state.set_locked(true);
            ui::menubar::update_menu_bar_icon(true);
            ui::notifications::show_lock_notification();
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
