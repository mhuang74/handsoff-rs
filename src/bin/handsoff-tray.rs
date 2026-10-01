// HandsOff Tray App - macOS menu bar application for input blocking
// This binary provides a native macOS tray icon with dropdown menu

use anyhow::{Context, Result};
use clap::Parser;
use handsoff::constants::{
    NOTIFICATION_ERROR_TIMEOUT_MS,
    NOTIFICATION_TIMEOUT_MS, POLL_INTERVAL_DISABLED_SECS, POLL_INTERVAL_ENABLED_MS,
};
use handsoff::{config, config_file::Config, preferences::menu_state, setup, wizard, HandsOffCore};
use log::{error, info, warn};
use std::cell::RefCell;
use std::rc::Rc;
use tao::event_loop::ControlFlow;
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

    // Create the ONE event loop for the whole process. tao's macOS CFRunLoop
    // observers (registered in EventLoop::new, never removed) hold a
    // Weak<PanicInfo> that panics once the owning EventLoop drops, so the
    // loop must live for the entire process: the wizard runs on this same
    // loop via run_return, then the tray continues on it.
    let mut event_loop =
        tao::event_loop::EventLoopBuilder::<wizard::WizardEvent>::with_user_event().build();

    // Load configuration — first run WITHOUT a config, or a config that
    // fails strict validation (keycode-v1 hash, ≥4 keys, distinct hotkeys,
    // timeout bounds — ADR 0002), launches the in-app Setup Wizard instead
    // of pointing users at the terminal. The wizard runs `run_return` on the
    // loop above and returns when the flow completes; the tray's `run` below
    // is the second (supported) re-entry on the same loop.
    let cfg = match Config::load().and_then(|_| setup::validate_config_strict()) {
        Ok(()) => Config::load().expect("config validated above"),
        Err(e) => {
            info!("Launching Setup Wizard: {}", e);
            match wizard::run_wizard(&mut event_loop) {
                Ok(outcome) => {
                    wizard::wizard_outcome_to_config(&outcome)
                        .context("Failed to save wizard configuration")?;
                    Config::load().context("Wizard saved config failed to load")?
                }
                Err(e) => {
                    error!("Setup wizard failed: {}", e);
                    show_alert(
                        "HandsOff - Setup Required",
                        &format!(
                            "HandsOff needs a passphrase before it can protect input.\n\n{}",
                            e
                        ),
                    );
                    std::process::exit(1);
                }
            }
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
        "Failed to parse lock hotkey from config file. Restart the app to re-run the Setup Wizard."
    })?;
    let talk_key = cfg.get_talk_key_code().with_context(|| {
        "Failed to parse talk hotkey from config file. Restart the app to re-run the Setup Wizard."
    })?;

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

    // (Event loop already created above — the single process-wide loop the
    // wizard also ran on.)

    // Build tray menu
    // Note: When locked, mouse clicks are blocked, so menu is inaccessible
    // Lock menu item only works when unlocked; unlock requires typing passphrase
    let lock_item = MenuItem::new("Lock Input", true, None);
    let disable_item = MenuItem::new("Disable", true, None);
    let reenable_item = MenuItem::new("Reenable", true, None);
    let separator = PredefinedMenuItem::separator();
    let preferences_item = MenuItem::new("Preferences…", true, None);
    let change_passphrase_item = MenuItem::new("Change Passphrase…", true, None);
    let reset_item = MenuItem::new("Reset…", true, None);

    let menu = Menu::new();
    menu.append(&lock_item)
        .context("Failed to add lock menu item")?;
    menu.append(&disable_item)
        .context("Failed to add disable menu item")?;
    menu.append(&reenable_item)
        .context("Failed to add reenable menu item")?;
    menu.append(&separator).context("Failed to add separator")?;
    menu.append(&preferences_item)
        .context("Failed to add preferences menu item")?;
    menu.append(&change_passphrase_item)
        .context("Failed to add change passphrase menu item")?;
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
    let reenable_id = reenable_item.id().clone();
    let preferences_id = preferences_item.id().clone();
    let change_passphrase_id = change_passphrase_item.id().clone();
    let reset_id = reset_item.id().clone();

    // Track state for tooltip updates and permission state
    let was_locked = false;
    let was_disabled = false;
    let last_tooltip = String::new();
    let has_permissions = true; // Assume true at start (already verified at startup)

    let pending_action: Rc<RefCell<Option<SessionAction>>> = Rc::new(RefCell::new(None));
    let pending_in_loop = pending_action.clone();

    // Tray sessions run for the life of the process: each session returns
    // when a window-flow action was requested; main services it, then
    // re-enters the session (the action handlers themselves end their window
    // flows via app.stop and fall back through to here).
    let tracked: &mut TrackedState = &mut (was_locked, was_disabled, last_tooltip, has_permissions);
    loop {
        let action = run_session(
            &mut event_loop,
            core.clone(),
            (
                lock_id.clone(),
                disable_id.clone(),
                reenable_id.clone(),
                preferences_id.clone(),
                change_passphrase_id.clone(),
                reset_id.clone(),
            ),
            (
                lock_item.clone(),
                disable_item.clone(),
                reenable_item.clone(),
                preferences_item.clone(),
                change_passphrase_item.clone(),
                reset_item.clone(),
            ),
            &tray,
            tracked,
            pending_in_loop.clone(),
        );

        match action {
            SessionAction::Preferences => handle_preferences(&mut event_loop, core.clone()),
            SessionAction::ChangePassphrase => handle_change_passphrase(&mut event_loop, core.clone()),
            SessionAction::Reset => handle_reset(&mut event_loop),
        }
    }
}

/// Tracker state that must survive across tray sessions (icon/tooltip caches
/// and permission logging dedup).
#[allow(clippy::type_complexity)]
type TrackedState = (bool, bool, String, bool);

/// Which menu item requested the session end, if any.
#[allow(clippy::enum_variant_names)]
enum SessionAction {
    Preferences,
    ChangePassphrase,
    Reset,
}

/// One tray session: a `run_return` pass on the process-wide event loop that
/// polls menu events and updates icon/tooltip, ending when a window-flow menu
/// action is clicked (returns that action) or the process is killed.
///
/// Why sessions at all: the wizard/Preferences windows each run their own
/// `run_return` on this same loop (a second tao EventLoop would panic — its
/// CFRunLoop observers hold a Weak<PanicInfo> that dangles on loop drop).
/// Calling `run_return` re-entrantly from INSIDE the tray's callback would
/// nest `[NSApp run]`, which AppKit forbids. So a window-flow menu click
/// records a pending action and ends the session via
/// `NSApplication::stop` — the mechanism tao's own `stop_app_on_panic` uses.
/// ControlFlow::ExitWithCode is NOT usable here: it latches process-wide
/// (tao docs: "once set, cannot be unset"; the macOS impl never resets
/// HANDLER.control_flow between run_return calls), which would make every
/// later session — including the post-Reset wizard — return immediately.
#[allow(clippy::type_complexity)]
fn run_session(
    event_loop: &mut tao::event_loop::EventLoop<wizard::WizardEvent>,
    core: Rc<RefCell<HandsOffCore>>,
    ids: (
        tray_icon::menu::MenuId,
        tray_icon::menu::MenuId,
        tray_icon::menu::MenuId,
        tray_icon::menu::MenuId,
        tray_icon::menu::MenuId,
        tray_icon::menu::MenuId,
    ),
    items: (
        tray_icon::menu::MenuItem,
        tray_icon::menu::MenuItem,
        tray_icon::menu::MenuItem,
        tray_icon::menu::MenuItem,
        tray_icon::menu::MenuItem,
        tray_icon::menu::MenuItem,
    ),
    tray: &tray_icon::TrayIcon,
    tracked: &mut TrackedState,
    pending_action: Rc<RefCell<Option<SessionAction>>>,
) -> SessionAction {
    let (lock_id, disable_id, reenable_id, preferences_id, change_passphrase_id, reset_id) = ids;
    let (
        lock_item,
        disable_item,
        reenable_item,
        preferences_item,
        change_passphrase_item,
        reset_item,
    ) = items;
    let (was_locked, was_disabled, last_tooltip, has_permissions) = tracked;

    // Grabbing the NSApplication handle for session end (see doc comment).
    #[cfg(target_os = "macos")]
    use objc2_app_kit::NSApplication;
    #[cfg(target_os = "macos")]
    let mtm = objc2_foundation::MainThreadMarker::new()
        .expect("tray session must run on main thread");

    use tao::platform::run_return::EventLoopExtRunReturn;

    let pending_in_callback = pending_action.clone();
    event_loop.run_return(move |_event, _, control_flow| {
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
            } else if event_id == reenable_id {
                info!("Reenable menu item clicked, resetting app state");
                handle_reenable(core.clone());
            } else if event_id == preferences_id {
                info!("Preferences menu item clicked");
                *pending_in_callback.borrow_mut() = Some(SessionAction::Preferences);
                #[cfg(target_os = "macos")]
                {
                    // Note: app.stop returns from [NSApp run] on the NEXT
                    // wake; the closure keeps running until then, but the
                    // pending action is already recorded.
                    NSApplication::sharedApplication(mtm).stop(None);
                    return; // control_flow untouched (ExitWithCode latches)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return;
                }
            } else if event_id == change_passphrase_id {
                info!("Change Passphrase menu item clicked");
                *pending_in_callback.borrow_mut() = Some(SessionAction::ChangePassphrase);
                #[cfg(target_os = "macos")]
                {
                    NSApplication::sharedApplication(mtm).stop(None);
                    return;
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return;
                }
            } else if event_id == reset_id {
                info!("Reset menu item clicked");
                *pending_in_callback.borrow_mut() = Some(SessionAction::Reset);
                #[cfg(target_os = "macos")]
                {
                    NSApplication::sharedApplication(mtm).stop(None);
                    return;
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return;
                }
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
                                    "Failed to restart input blocking: {}\n\nUse Reenable menu to try again.",
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

        // Update menu enabled state via the pure helper (unit-tested)
        let menu_flags = menu_state(is_locked, is_disabled, current_permissions);
        lock_item.set_enabled(menu_flags.lock_enabled);
        disable_item.set_enabled(menu_flags.disable_enabled);
        // Reenable and the config-level actions are always enabled by design;
        // set_enabled anyway so a future rule change flows through one place.
        reenable_item.set_enabled(menu_flags.reenable_enabled);
        preferences_item.set_enabled(menu_flags.preferences_enabled);
        change_passphrase_item.set_enabled(menu_flags.change_passphrase_enabled);
        reset_item.set_enabled(menu_flags.reset_enabled);

        // Track permission state changes for logging
        if *has_permissions != current_permissions {
            if current_permissions {
                info!("Tray: Accessibility permissions detected, Lock menu enabled");
            } else {
                warn!("Tray: Accessibility permissions lost, Lock menu disabled");
            }
            *has_permissions = current_permissions;
        }

        // Update icon when lock state or disabled state changes
        if is_locked != *was_locked || is_disabled != *was_disabled {
            *was_locked = is_locked;
            *was_disabled = is_disabled;

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

        // Always update tooltip (to show live countdown and permission status)
        let tooltip = build_tooltip(&core_borrow, is_locked, is_disabled, current_permissions);
        if tooltip != *last_tooltip {
            if let Err(e) = tray.set_tooltip(Some(&tooltip)) {
                error!("Failed to update tray tooltip: {}", e);
            }
            *last_tooltip = tooltip;
        }
    });

    let result = pending_action.borrow_mut().take().unwrap_or_else(|| {
        // run_return ended without a requested action: treat as exit request
        // (never happens on macOS — the tray has no close affordance — but
        // the session must not spin).
        std::process::exit(0);
    });
    result
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

/// Handle Reenable from menu (the old "Reset" — renamed in #27 so Reset can
/// mean the destructive config wipe).
///
/// Unguarded by design (CONTEXT.md): ends a stuck Lock and restarts input
/// capture without changing any configuration. If disabled, re-enables the
/// app. Otherwise, restarts the event tap if permissions are available.
fn handle_reenable(core: Rc<RefCell<HandsOffCore>>) {
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
                info!("HandsOff re-enabled successfully during reenable");
                #[cfg(target_os = "macos")]
                {
                    let _ = notify_rust::Notification::new()
                        .summary("HandsOff")
                        .body("Reenabled - Ready to use")
                        .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS))
                        .show();
                }
            }
            Err(e) => {
                warn!("Could not re-enable during reenable: {}", e);
                show_alert(
                    "HandsOff - Reenable Partial Success",
                    &format!("Timers cleared but could not re-enable:\n{}\n\nPlease check accessibility permissions.", e)
                );
            }
        }
    } else {
        // Attempt to restart event tap (will check permissions internally)
        match core.restart_event_tap() {
            Ok(()) => {
                info!("Input blocking restarted successfully during reenable");
                #[cfg(target_os = "macos")]
                {
                    let _ = notify_rust::Notification::new()
                        .summary("HandsOff")
                        .body("Reenabled - Input blocking restarted\nReady to use")
                        .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS))
                        .show();
                }
            }
            Err(e) => {
                warn!("Could not restart input blocking during reenable: {}", e);
                show_alert(
                    "HandsOff - Reenable Partial Success",
                    &format!("Timers cleared but input blocking could not be restarted:\n{}\n\nPlease check accessibility permissions.", e)
                );
            }
        }
    }

    info!("Finished handling reenable");
}

/// Handle Preferences from menu (#27): open the in-process Preferences window
/// on the shared event loop, apply the edit via the config seam, then sync
/// the running core with the new settings immediately — the tray has no Quit
/// item, so "takes effect on restart" would mean "never".
fn handle_preferences(
    event_loop: &mut tao::event_loop::EventLoop<wizard::WizardEvent>,
    core: Rc<RefCell<HandsOffCore>>,
) {
    match wizard::run_preferences(event_loop) {
        Ok(outcome) => match handsoff::preferences::apply_preferences(&outcome.edit) {
            Ok(cfg) => {
                apply_config_to_core(&core, &cfg);
                info!("Preferences applied");
                #[cfg(target_os = "macos")]
                {
                    let _ = notify_rust::Notification::new()
                        .summary("HandsOff")
                        .body("Preferences saved and applied.")
                        .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS))
                        .show();
                }
            }
            Err(e) => {
                error!("Failed to apply preferences: {}", e);
                show_alert(
                    "HandsOff - Preferences Error",
                    &format!("Could not save preferences:\n{}\n\nNothing was changed.", e),
                );
            }
        },
        Err(e) => info!("Preferences window closed without saving: {}", e),
    }
}

/// Push a freshly saved config onto the running core: auto-lock timeout,
/// auto-unlock backoff, and (when changed) re-registered hotkeys.
///
/// The passphrase hash is NOT synced here — Change Passphrase owns that
/// (via `state.set_passphrase_hash`); Preferences never touches it.
fn apply_config_to_core(core: &Rc<RefCell<HandsOffCore>>, cfg: &Config) {
    let mut core = core.borrow_mut();

    // Auto-lock timeout: env var wins over config (same precedence as startup).
    let auto_lock_timeout = config::parse_auto_lock_timeout().or(Some(cfg.auto_lock_timeout));
    core.set_auto_lock_timeout(auto_lock_timeout);

    // Auto-unlock backoff: env var > config mode + base (same as startup).
    core.set_auto_unlock_config(config::resolve_auto_unlock(
        Some(cfg.auto_unlock_backoff_enabled()),
        cfg.auto_unlock_base_interval,
    ));

    // Hotkeys: re-register only when the configured keys changed — global
    // hotkeys are unregister/register against the OS, so skip when unchanged.
    match (cfg.get_lock_key_code(), cfg.get_talk_key_code()) {
        (Ok(lock_key), Ok(talk_key)) => {
            let changed = lock_key != core.lock_key_code() || talk_key != core.talk_key_code();
            if changed {
                if let Err(e) = core.reregister_hotkeys(lock_key, talk_key) {
                    error!("Failed to re-register hotkeys after Preferences: {}", e);
                    show_alert(
                        "HandsOff - Preferences Partial Success",
                        &format!(
                            "Settings were saved, but the new hotkeys could not be \
                             registered:\n{}\n\nThe previous hotkeys remain active.",
                            e
                        ),
                    );
                    return; // timeout/backoff above were still applied
                }
                info!("Hotkeys re-registered after Preferences");
            }
        }
        (Err(e), _) | (_, Err(e)) => {
            error!("Config hotkey invalid after Preferences save: {}", e);
            // Config was already saved; the strict-validation gate will send
            // the next launch to the wizard. Timeouts/backoff stay applied.
        }
    }
}

/// Handle Change Passphrase from menu (#27): silent double capture, then save
/// with only the hash changed. Restart is NOT required (the hash is read per
/// unlock attempt from AppState) — but the running core still holds the old
/// hash in memory, so sync it after a successful save.
fn handle_change_passphrase(
    event_loop: &mut tao::event_loop::EventLoop<wizard::WizardEvent>,
    core: Rc<RefCell<HandsOffCore>>,
) {
    match wizard::run_change_passphrase(event_loop) {
        Ok(cfg) => {
            info!("Passphrase changed successfully");
            if let Some(hash) = &cfg.passphrase_hash {
                core.borrow()
                    .state
                    .set_passphrase_hash(hash.clone());
            }
            #[cfg(target_os = "macos")]
            {
                let _ = notify_rust::Notification::new()
                    .summary("HandsOff")
                    .body("Passphrase changed.\nUse the new passphrase to unlock.")
                    .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS))
                    .show();
            }
        }
        Err(e) => {
            info!("Change Passphrase not completed: {}", e);
            show_alert(
                "HandsOff - Change Passphrase",
                &format!("Passphrase not changed:\n{}\n\nYour existing passphrase still works.", e),
            );
        }
    }
}

/// Handle Reset from menu (#27): the destructive recovery path.
///
/// Double-confirmed dialog → wipe config → relaunch into the Setup Wizard.
/// Nothing here re-enters normal operation: after the wizard completes (or is
/// cancelled), the process relaunches itself so the startup path (strict
/// validation → core construction) runs exactly as at first boot.
fn handle_reset(event_loop: &mut tao::event_loop::EventLoop<wizard::WizardEvent>) {
    // Double-confirm (spec #24 story 16): a mis-click must not wipe config.
    if !confirm_reset() {
        info!("Reset cancelled at confirmation");
        return;
    }

    match handsoff::preferences::wipe_config() {
        Ok(wiped) => {
            info!(
                "Reset: config {}",
                if wiped { "wiped" } else { "already absent" }
            );
        }
        Err(e) => {
            error!("Reset failed to wipe config: {}", e);
            show_alert(
                "HandsOff - Reset Failed",
                &format!("Could not remove the configuration:\n{}\n\nNothing was changed.", e),
            );
            return;
        }
    }

    // Relaunch into the wizard. The wizard itself refuses nothing here — the
    // user asked for a fresh setup. On successful completion the process
    // relaunches itself to rebuild the core from the new config; on cancel we
    // exit nonzero so the user can relaunch the app (which reopens the wizard
    // via the strict-validation gate).
    match wizard::run_wizard(event_loop) {
        Ok(outcome) => {
            if let Err(e) = wizard::wizard_outcome_to_config(&outcome) {
                error!("Reset: wizard config save failed: {}", e);
                show_alert(
                    "HandsOff - Reset Failed",
                    &format!("The new setup could not be saved:\n{}\n\nRelaunch HandsOff to try again.", e),
                );
                std::process::exit(1);
            }
            info!("Reset complete: new configuration saved, relaunching");
            if let Err(e) = relaunch_self() {
                error!("Reset: relaunch failed: {}", e);
            }
        }
        Err(e) => {
            info!("Reset: wizard cancelled or failed: {}", e);
            std::process::exit(1);
        }
    }
}

/// Double-confirmed Reset dialog. Returns true only on explicit confirm.
fn confirm_reset() -> bool {
    use std::process::Command;

    let script = r#"display dialog "This wipes your HandsOff configuration — passphrase, hotkeys, and timeouts — and restarts setup from the beginning.\n\nYour current passphrase will STOP working." with title "HandsOff - Reset"\nbuttons {"Cancel", "Reset"} default button "Cancel" with icon caution"#;
    let script = script.replace("\\n", "\n");
    let out = Command::new("osascript").arg("-e").arg(&script).output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("button returned:Reset"),
        Err(_) => {
            warn!("Reset confirmation dialog failed to show; treating as cancel");
            false
        }
    }
}

/// Relaunch the running binary (post-Reset, post-wizard) so startup runs the
/// normal first-boot path against the fresh config.
fn relaunch_self() -> Result<()> {
    let exe = std::env::current_exe().context("Failed to locate running binary")?;
    let err = std::process::Command::new(exe)
        .args(std::env::args().skip(1).filter(|a| a != "--setup"))
        .spawn();
    match err {
        Ok(_) => {
            info!("Relaunched HandsOff after Reset");
            std::process::exit(0);
        }
        Err(e) => {
            error!("Failed to relaunch after Reset: {}", e);
            std::process::exit(1);
        }
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
