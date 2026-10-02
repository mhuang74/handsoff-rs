// HandsOff Tray App - macOS menu bar application for input blocking
// This binary provides a native macOS tray icon with dropdown menu

use anyhow::{Context, Result};
use clap::Parser;
use handsoff::constants::{
    NOTIFICATION_ERROR_TIMEOUT_MS, NOTIFICATION_TIMEOUT_MS, POLL_INTERVAL_DISABLED_SECS,
    POLL_INTERVAL_ENABLED_MS, TOOLTIP_UPDATE_INTERVAL_MS,
};
use handsoff::utils::lock_file;
use handsoff::{config, config_file::Config, preferences::menu_state, setup, wizard, HandsOffCore};
use log::{error, info, warn};
use std::cell::RefCell;
use std::rc::Rc;
use tao::event_loop::ControlFlow;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconBuilder;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_HASH: &str = env!("GIT_COMMIT_HASH");

/// Latest-release page opened by the "Check for Updates…" menu item
/// (issue #29). GitHub redirects this URL to the newest published release;
/// the tray does no networking or version comparison itself (ADR 0001:
/// no Sparkle / self_update — the browser is the update surface).
const RELEASES_URL: &str = "https://github.com/mhuang74/handsoff-rs/releases/latest";

/// Menu label for the update check (exposed for unit tests).
const CHECK_UPDATES_LABEL: &str = "Check for Updates…";

/// Open the latest GitHub release page in the default browser. Fire-and-
/// forget shell-out to `open` (same osascript-style delegation the dialogs
/// use): never blocks the tray, never inspects the result. Failure to open
/// a browser surfaces as a NON-BLOCKING notification — a modal osascript
/// alert here would freeze the tray poll loop (run_session callback).
fn handle_check_updates() {
    match std::process::Command::new("open").arg(RELEASES_URL).spawn() {
        Ok(_) => info!("Opened release page in browser: {}", RELEASES_URL),
        Err(e) => {
            error!("Failed to open release page {}: {}", RELEASES_URL, e);
            #[cfg(target_os = "macos")]
            {
                let _ = notify_rust::Notification::new()
                    .summary("HandsOff - Check for Updates")
                    .body(&format!(
                        "Could not open your browser.\nVisit the releases page manually:\n{}",
                        RELEASES_URL
                    ))
                    .timeout(notify_rust::Timeout::Milliseconds(
                        NOTIFICATION_ERROR_TIMEOUT_MS,
                    ))
                    .show();
            }
        }
    }
}

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

    /// Internal: set by `relaunch_self`/`relaunch_after_reset` on the child
    /// process. The parent still holds the single-instance flock when it
    /// spawns the child and only exits afterwards, so without a bypass the
    /// child would see "already running", exit, and leave NOTHING running
    /// after a relaunch. The child therefore re-acquires the lock with a
    /// short grace loop (issue #37 N1) — the flag only suppresses the fatal
    /// duplicate alert during that overlap window; the successor always ends
    /// up holding the lock, so a later launch still exits as a duplicate.
    #[arg(long, hide = true)]
    skip_instance_lock: bool,
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

    // Initialize logger (BEFORE the single-instance guard: a duplicate
    // must be able to log why it is exiting).
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    info!("Starting HandsOff Tray App v{}", VERSION);

    // Single-instance guard: exclusive flock on a lock file for the
    // process lifetime. Without this, a login-item relaunch (or the user)
    // can stack duplicate tray instances — observed as 16 launches in
    // under 2 h during the 2026-10-01 keyboard-lockout incident, each
    // entering the setup wizard. Must be taken BEFORE any AppKit/tray
    // initialization so a duplicate exits before it can show UI.
    // The self-relaunch successor (`--skip-instance-lock`) STILL acquires
    // the lock (issue #37 N1): the flag only bypasses the duplicate alert
    // while the dying parent holds it — a grace loop retries until the
    // parent's lock frees, so exactly one tray instance is ever running.
    acquire_single_instance_lock(args.skip_instance_lock)?;

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
    //
    // Issue #29 adds the third case: config VALID but the Accessibility
    // grant stale/missing (typical after an unsigned update changed the
    // CDHash) routes to the permission-only re-grant screen — never full
    // passphrase re-setup, and the config is not touched.
    let mut permissions = initial_permissions;
    // Issue #37 N7: a config that is valid in content but whose permission
    // repair failed is a hard error, NOT a Setup-Wizard case — re-setup would
    // re-capture and discard the working Passphrase. Surface the `chmod 600`
    // instruction and exit instead.
    match Config::load() {
        Err(e) if Config::is_permission_repair_failure(&e) => {
            error!("Config permission repair failed: {e:#}");
            show_alert(
                "HandsOff - Config Permissions",
                &format!(
                    "Your config file is group/other-readable and could not be repaired.\n\n\
                     {e:#}"
                ),
            );
            std::process::exit(1);
        }
        _ => {}
    }
    let config_valid = Config::load()
        .and_then(|_| setup::validate_config_strict())
        .is_ok();
    let cfg = match wizard::startup_flow(config_valid, permissions) {
        wizard::StartupFlow::Run => Config::load().expect("config validated above"),

        wizard::StartupFlow::ReGrant => {
            info!("Config valid but Accessibility grant stale — opening permission re-grant");
            match wizard::run_permission_regrant(&mut event_loop) {
                Ok(()) => {
                    // Re-check authoritatively: the poll thread inside the
                    // re-grant window already confirmed the full check, but
                    // the tap-start decision below wants the current value.
                    permissions = handsoff::input_blocking::check_accessibility_permissions();
                    Config::load().expect("config valid; only the grant was stale")
                }
                Err(e) => {
                    // User closed the re-grant window: degrade exactly like
                    // a launch without permissions (tray shows status, no
                    // blocking until granted or "Fix …" is used).
                    info!("Permission re-grant closed without completing: {}", e);
                    Config::load().expect("config valid; only the grant was stale")
                }
            }
        }

        wizard::StartupFlow::Wizard => {
            match Config::load().and_then(|_| setup::validate_config_strict()) {
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
    // (re-checked after a possibly completed re-grant flow — issue #29).
    if permissions {
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
    let regrant_item = MenuItem::new("Fix Accessibility Permission…", true, None);
    let check_updates_item = MenuItem::new(CHECK_UPDATES_LABEL, true, None);
    let help_item = MenuItem::new("Help", true, None);

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
    menu.append(&separator)
        .context("Failed to add second separator")?;
    menu.append(&regrant_item)
        .context("Failed to add re-grant menu item")?;
    menu.append(&check_updates_item)
        .context("Failed to add check updates menu item")?;
    menu.append(&help_item)
        .context("Failed to add help menu item")?;

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
    let regrant_id = regrant_item.id().clone();
    let check_updates_id = check_updates_item.id().clone();
    let help_id = help_item.id().clone();

    // Single-dialog invariant (issue #36): while a dialog owns the nested
    // run_return, IT drains the menu channel — clicks on these window-flow
    // items re-front the live dialog instead of queueing duplicate dialogs.
    *wizard::WINDOW_FLOW_MENU_IDS.lock() = vec![
        preferences_id.clone(),
        change_passphrase_id.clone(),
        reset_id.clone(),
        regrant_id.clone(),
        help_id.clone(),
    ];

    // Track state for tooltip updates and permission state. Mutable because
    // the values are threaded through `TrackedState` across tray sessions.
    let was_locked = false;
    let was_disabled = false;
    let last_tooltip = String::new();
    let last_tooltip_update = std::time::Instant::now();
    let has_permissions = permissions; // Re-verified after any re-grant flow (issue #29)

    let pending_action: Rc<RefCell<Option<SessionAction>>> = Rc::new(RefCell::new(None));
    let pending_in_loop = pending_action.clone();

    // Tray sessions run for the life of the process: each session returns
    // when a window-flow action was requested; main services it, then
    // re-enters the session (the action handlers themselves end their window
    // flows via app.stop and fall back through to here).
    let tracked: &mut TrackedState = &mut (
        was_locked,
        was_disabled,
        last_tooltip,
        last_tooltip_update,
        has_permissions,
    );
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
                regrant_id.clone(),
                check_updates_id.clone(),
                help_id.clone(),
            ),
            (
                lock_item.clone(),
                disable_item.clone(),
                reenable_item.clone(),
                preferences_item.clone(),
                change_passphrase_item.clone(),
                reset_item.clone(),
                regrant_item.clone(),
                check_updates_item.clone(),
                help_item.clone(),
            ),
            &tray,
            tracked,
            pending_in_loop.clone(),
        );

        match action {
            SessionAction::Preferences => handle_preferences(&mut event_loop, core.clone()),
            SessionAction::ChangePassphrase => {
                // N5/N6 gate: refused while locked — a dead-tap window must
                // not allow re-keying the Lock. Same menu_state authority as
                // the click gating (issue #37).
                {
                    let core_borrow = core.borrow();
                    let flags = menu_state(
                        core_borrow.is_locked(),
                        core_borrow.state.is_disabled(),
                        core_borrow.has_accessibility_permissions(),
                    );
                    if !flags.change_passphrase_enabled {
                        warn!("Change Passphrase requested while locked — refusing (N5 gate)");
                        show_alert(
                            "HandsOff - Locked",
                            "Change Passphrase is not available while input is locked.\n\
                             Unlock with your Passphrase first.",
                        );
                        continue;
                    }
                }
                handle_change_passphrase(&mut event_loop, core.clone())
            }
            SessionAction::Reset => {
                // N5 gate: refused while locked — a dead-tap window must not
                // allow wiping the configuration. Same menu_state authority
                // as the click gating (issue #37).
                {
                    let core_borrow = core.borrow();
                    let flags = menu_state(
                        core_borrow.is_locked(),
                        core_borrow.state.is_disabled(),
                        core_borrow.has_accessibility_permissions(),
                    );
                    if !flags.reset_enabled {
                        warn!("Reset requested while locked — refusing (N5 gate)");
                        show_alert(
                            "HandsOff - Locked",
                            "Reset is not available while input is locked.\n\
                             Unlock with your Passphrase first.",
                        );
                        continue;
                    }
                }
                handle_reset(&mut event_loop)
            }
            SessionAction::ReGrantPermission => handle_regrant_permission(&mut event_loop),
            SessionAction::Help => handle_help(&mut event_loop),
        }
    }
}

/// Tracker state that must survive across tray sessions (icon/tooltip caches,
/// tooltip rebuild cadence timestamp, and permission logging dedup).
#[allow(clippy::type_complexity)]
type TrackedState = (bool, bool, String, std::time::Instant, bool);

/// Which menu item requested the session end, if any.
#[allow(clippy::enum_variant_names)]
enum SessionAction {
    Preferences,
    ChangePassphrase,
    Reset,
    /// Issue #29 re-grant: config valid, Accessibility grant stale —
    /// permission-only wizard screen, no passphrase re-setup.
    ReGrantPermission,
    Help,
}

/// End the tray session NOW: `-[NSApplication stop:]` alone only takes
/// effect on the next run-loop wake, which made window-flow dialogs take
/// seconds to appear after their menu click (issue #36). Posting a dummy
/// app-defined event forces an immediate wake — the technique tao's own
/// `stop_app_on_panic` uses (SO 48064752).
#[cfg(target_os = "macos")]
fn stop_session_now(mtm: objc2_foundation::MainThreadMarker) {
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
    use objc2_foundation::NSPoint;

    let app = NSApplication::sharedApplication(mtm);
    app.stop(None);
    let dummy = unsafe {
        NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::ApplicationDefined,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            0.0,
            0,
            None,
            0,
            0,
            0,
        )
    };
    if let Some(event) = dummy {
        app.postEvent_atStart(&event, true);
    }
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
        tray_icon::menu::MenuItem,
        tray_icon::menu::MenuItem,
        tray_icon::menu::MenuItem,
    ),
    tray: &tray_icon::TrayIcon,
    tracked: &mut TrackedState,
    pending_action: Rc<RefCell<Option<SessionAction>>>,
) -> SessionAction {
    let (
        lock_id,
        disable_id,
        reenable_id,
        preferences_id,
        change_passphrase_id,
        reset_id,
        regrant_id,
        check_updates_id,
        help_id,
    ) = ids;
    let (
        lock_item,
        disable_item,
        reenable_item,
        preferences_item,
        change_passphrase_item,
        reset_item,
        _regrant_item,
        _check_updates_item,
        _help_item,
    ) = items;
    let (was_locked, was_disabled, last_tooltip, last_tooltip_update, has_permissions) = tracked;

    // Dispatch menu clicks deferred while a dialog owned the loop (issue
    // #36): dialogs consume window-flow clicks themselves (re-fronting the
    // live dialog), so only immediate-action clicks land here. Dispatched
    // before the session loop so the actions take effect immediately.
    //
    // Issue #37 N2: every deferred click is re-validated against the CURRENT
    // menu state before executing — the state may have changed since the
    // click was queued (e.g. a Disable-then-Lock sequence must not end in
    // locked-without-tap). Stale clicks are dropped with a log line; the
    // same gate the live-click path uses decides, so the paths cannot drift.
    for id in wizard::take_deferred_menu_events() {
        let core_borrow = core.borrow();
        let flags = menu_state(
            core_borrow.is_locked(),
            core_borrow.state.is_disabled(),
            core_borrow.has_accessibility_permissions(),
        );
        drop(core_borrow);

        let allowed = if id == lock_id {
            flags.lock_enabled
        } else if id == disable_id {
            flags.disable_enabled
        } else if id == reenable_id {
            flags.reenable_enabled
        } else if id == check_updates_id {
            true // fire-and-forget; no protection state involved
        } else {
            false // window-flow IDs cannot appear here; unknown IDs ignored
        };
        if !allowed {
            warn!(
                "Dropping menu click deferred during a dialog — no longer allowed \
                 by current state (stale click): {:?}",
                id
            );
            continue;
        }

        info!("Dispatching menu click deferred during a dialog: {:?}", id);
        if id == lock_id {
            handle_lock_toggle(core.clone());
        } else if id == disable_id {
            info!("Disable menu item clicked");
            handle_disable(core.clone());
        } else if id == reenable_id {
            info!("Reenable menu item clicked, resetting app state");
            handle_reenable(core.clone());
        } else if id == check_updates_id {
            info!("Check for Updates menu item clicked");
            handle_check_updates();
        }
    }

    // Main-thread marker consumed by stop_session_now (see doc comment):
    #[cfg(target_os = "macos")]
    let mtm =
        objc2_foundation::MainThreadMarker::new().expect("tray session must run on main thread");

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

        // Handle menu events — every immediate-action click passes the SAME
        // menu-state gate as the deferred dispatch (issue #37 N2: one
        // authority decides; the item's set_enabled alone is not a gate).
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            let event_id = event.id;

            if event_id == lock_id || event_id == disable_id || event_id == reenable_id {
                let allowed = {
                    let core_borrow = core.borrow();
                    let flags = menu_state(
                        core_borrow.is_locked(),
                        core_borrow.state.is_disabled(),
                        core_borrow.has_accessibility_permissions(),
                    );
                    if event_id == lock_id {
                        flags.lock_enabled
                    } else if event_id == disable_id {
                        flags.disable_enabled
                    } else {
                        flags.reenable_enabled
                    }
                };
                if !allowed {
                    warn!(
                        "Ignoring menu click not allowed by current state: {:?}",
                        event_id
                    );
                    return;
                }
            }

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
                    // Wake the run loop so `stop` takes effect NOW (see
                    // stop_session_now); the pending action is recorded
                    // before the stop either way.
                    stop_session_now(mtm);
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
                    stop_session_now(mtm);
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
                    stop_session_now(mtm);
                    return;
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return;
                }
            } else if event_id == regrant_id {
                info!("Permission re-grant requested from tray");
                *pending_in_callback.borrow_mut() = Some(SessionAction::ReGrantPermission);
                #[cfg(target_os = "macos")]
                {
                    stop_session_now(mtm);
                    return;
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return;
                }
            } else if event_id == check_updates_id {
                info!("Check for Updates menu item clicked");
                // Fire-and-forget: opens the browser without ending the
                // session or blocking the tray (no window flow involved).
                handle_check_updates();
            } else if event_id == help_id {
                info!("Help menu item clicked");
                *pending_in_callback.borrow_mut() = Some(SessionAction::Help);
                #[cfg(target_os = "macos")]
                {
                    stop_session_now(mtm);
                    return;
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return;
                }
            }
        }

        // Service the shared tap-lifecycle flags (issue #37 N3) — the same
        // block the CLI main loop runs, so both binaries recover from a
        // macOS tap timeout identically. Tray-specific UX (notifications)
        // layers on the returned event.
        {
            let mut core_borrow = core.borrow_mut();
            match core_borrow.service_tap_lifecycle() {
            handsoff::TapLifecycleEvent::TapStopped => {
                info!("Tray: Input blocking stopped - normal input restored");
            }
            handsoff::TapLifecycleEvent::Restarted => {
                #[cfg(target_os = "macos")]
                {
                    let _ = notify_rust::Notification::new()
                        .summary("HandsOff - Input Blocking Restarted")
                        .body("Input blocking restarted successfully.\nHandsOff is now active.")
                        .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS))
                        .show();
                }
            }
            handsoff::TapLifecycleEvent::RestartFailed(e) => {
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
            handsoff::TapLifecycleEvent::Idle => {}
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
        let permission_changed = *has_permissions != current_permissions;
        if permission_changed {
            if current_permissions {
                info!("Tray: Accessibility permissions detected, Lock menu enabled");
            } else {
                warn!("Tray: Accessibility permissions lost, Lock menu disabled");
            }
            *has_permissions = current_permissions;
        }

        // Update icon when lock state or disabled state changes
        let state_transition = is_locked != *was_locked || is_disabled != *was_disabled;
        if state_transition {
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

        // Rebuild tooltip on state transitions or every TOOLTIP_UPDATE_INTERVAL_MS
        // (the countdown itself does not need per-second repaint; transitions repaint now)
        let cadence_elapsed = last_tooltip_update
            .elapsed()
            >= std::time::Duration::from_millis(TOOLTIP_UPDATE_INTERVAL_MS);
        if state_transition || permission_changed || cadence_elapsed {
            let tooltip = build_tooltip(&core_borrow, is_locked, is_disabled, current_permissions);
            if tooltip != *last_tooltip {
                if let Err(e) = tray.set_tooltip(Some(&tooltip)) {
                    error!("Failed to update tray tooltip: {}", e);
                }
                *last_tooltip = tooltip;
            }
            *last_tooltip_update = std::time::Instant::now();
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

/// Handle the permission re-grant flow from the tray (issue #29): open the
/// wizard's permission-only screen on the shared event loop. Used both when
/// startup detected a stale Accessibility grant (config valid, permission
/// missing) and manually via the "Fix Accessibility Permission…" item.
///
/// The config is never read, shown, or written here — the user already has
/// one; only the TCC grant is (re)established. On success the running core
/// resumes normally: the permission monitor's existing `request_start_event_tap`
/// path (or the next poll tick) restarts the event tap once the grant lands,
/// so no relaunch and no config touch is needed.
fn handle_regrant_permission(event_loop: &mut tao::event_loop::EventLoop<wizard::WizardEvent>) {
    match wizard::run_permission_regrant(event_loop) {
        Ok(()) => {
            info!("Accessibility permission re-granted; resuming normal operation");
            #[cfg(target_os = "macos")]
            {
                let _ = notify_rust::Notification::new()
                    .summary("HandsOff")
                    .body(
                        "Accessibility permission restored.\nHandsOff will resume input blocking.",
                    )
                    .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS))
                    .show();
            }
        }
        Err(e) => info!("Permission re-grant not completed: {}", e),
    }
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
                .body("Disabled - Low system resources mode\nInput blocking paused. Use Reenable to re-enable")
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

/// Handle Help from menu: build the help text from the current core state
/// and show it in a read-only native window on the shared event loop. The
/// core borrow ends before the window opens; nothing mutates the core while
/// the Help window is up.
fn handle_help(
    event_loop: &mut tao::event_loop::EventLoop<wizard::WizardEvent>,
) {
    let sections = build_help_text();
    if let Err(e) = wizard::run_help(event_loop, &sections) {
        error!("Help window failed: {}", e);
    }
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
///
/// Cancellation, mismatch, and failure reasons are shown IN the dialog
/// (issue #36): on Err here the user was already informed (or dismissed
/// the window), so only log — no second alert.
fn handle_change_passphrase(
    event_loop: &mut tao::event_loop::EventLoop<wizard::WizardEvent>,
    core: Rc<RefCell<HandsOffCore>>,
) {
    match wizard::run_change_passphrase(event_loop) {
        Ok(cfg) => {
            info!("Passphrase changed successfully");
            if let Some(hash) = &cfg.passphrase_hash {
                core.borrow().state.set_passphrase_hash(hash.clone());
            }
        }
        Err(e) => info!(
            "Change Passphrase not completed (reason shown in dialog): {}",
            e
        ),
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
                &format!(
                    "Could not remove the configuration:\n{}\n\nNothing was changed.",
                    e
                ),
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
                    &format!(
                        "The new setup could not be saved:\n{}\n\nRelaunch HandsOff to try again.",
                        e
                    ),
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
        .args(
            std::env::args()
                .skip(1)
                .filter(|a| a != "--setup")
                // Designated successor: parent exits right after spawning,
                // but still holds the flock — the child skips the fatal
                // duplicate alert and re-acquires the lock during a short
                // grace window (issue #37 N1).
                .chain(["--skip-instance-lock".to_string()]),
        )
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

/// Exclusive single-instance lock via `flock(2)` on
/// `<config dir>/handsoff/handsoff.lock`, held for the process lifetime
/// (the fd is leaked, so the OS releases the lock on exit — a crash or power
/// cycle cannot leave a stale lock). The mechanics live in
/// `utils::lock_file` so the acquire/grace semantics are unit-testable.
///
/// `skip_duplicate_alert` (the `--skip-instance-lock` successor, issue #37
/// N1): still acquires the lock, retrying through a grace window while the
/// dying parent releases it — the flag only suppresses the duplicate alert;
/// the successor always ends up holding the lock. A plain launch makes a
/// single attempt: a held lock means another instance is running (log,
/// alert, exit 0).
fn acquire_single_instance_lock(skip_duplicate_alert: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let path = Config::config_path()
            .parent()
            .expect("config path always has a parent")
            .join("handsoff.lock");
        let grace = if skip_duplicate_alert {
            Some(lock_file::ACQUIRE_GRACE)
        } else {
            None
        };
        match lock_file::try_acquire_flock(&path, grace)? {
            lock_file::AcquireOutcome::Acquired(file) => {
                // Hold the lock for the process lifetime: leak the File so
                // the fd (and with it the flock) is never released.
                std::mem::forget(file);
                info!("Single-instance lock acquired: {}", path.display());
            }
            lock_file::AcquireOutcome::StillHeld => {
                if skip_duplicate_alert {
                    // Grace expired and the lock is STILL held: a full
                    // instance is running after all — exit like a duplicate,
                    // no alert (the flag suppressed it for the overlap only).
                    warn!(
                        "Single-instance lock still held after grace period ({}); \
                         another instance is running; exiting",
                        path.display()
                    );
                } else {
                    error!(
                        "Another HandsOff instance is running (lock held on {}); exiting",
                        path.display()
                    );
                    show_alert(
                        "HandsOff is already running",
                        "Another HandsOff instance is active — check the menu bar. \
                         This duplicate will now exit.",
                    );
                }
                std::process::exit(0);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = skip_duplicate_alert;
        // Tray app is macOS-only; nothing to guard elsewhere.
        Ok(())
    }
    #[cfg(target_os = "macos")]
    Ok(())
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
    push_status(&mut tooltip, core, is_locked, is_disabled, has_permissions);

    // Configured hotkeys (user asked for these on hover; the Help
    // window itself stays static and never shows concrete keys).
    tooltip.push_str(&format!(
        "Lock: Ctrl+Cmd+Shift+{}\n",
        core.get_lock_key_display()
    ));
    tooltip.push_str(&format!(
        "Unmute: press Ctrl+Cmd+Shift+{} (passes Space to apps)\n",
        core.get_talk_key_display()
    ));
    tooltip.push_str("\n");

    // Pointer to the full guide (menu summaries, lock/unlock instructions,
    // hotkeys, troubleshooting live in the Help window)
    tooltip.push_str("Open the Help menu item for the full guide\n");

    tooltip
}

/// Append the contextual status block (DISABLED / NO PERMISSIONS / LOCKED /
/// Unlocked, incl. countdowns) shown in the tray tooltip.
fn push_status(
    text: &mut String,
    core: &HandsOffCore,
    is_locked: bool,
    is_disabled: bool,
    has_permissions: bool,
) {
    if is_disabled {
        text.push_str("STATUS: DISABLED\n");
        text.push_str("Low system resources mode - all features paused\n");
        text.push_str("Use Reenable menu to re-enable HandsOff\n\n");
    } else if !has_permissions {
        text.push_str("STATUS: NO PERMISSIONS\n");
        text.push_str("Restore Accessibility Permissions in:\n");
        text.push_str("System Settings > Privacy & Security\n");
        text.push_str("Then use Reenable menu to restart\n\n");
    } else if is_locked {
        // Show lock duration
        if let Some(elapsed) = core.get_lock_elapsed_secs() {
            text.push_str(&format!("STATUS: LOCKED ({})\n", format_duration(elapsed)));
        } else {
            text.push_str("STATUS: LOCKED\n");
        }

        // V11: show the auto-unlock countdown ONLY when < 5 min away, so the
        // far-out backoff schedule is not broadcast by the menu bar.
        if let Some(remaining) = core.get_auto_unlock_remaining_secs() {
            if remaining > 0 && remaining < 300 {
                text.push_str(&format!("Auto-unlock in {}\n", format_duration(remaining)));
            }
        }
    } else {
        text.push_str("STATUS: Unlocked\n");

        // Show auto-lock countdown if enabled
        if let Some(remaining) = core.get_auto_lock_remaining_secs() {
            if remaining > 0 {
                text.push_str(&format!("Auto-lock in {}\n", format_duration(remaining)));
            } else {
                text.push_str("Auto-locking...\n");
            }
        }
    }

    text.push_str("\n\n");
}

/// Build the static Help window content: plain-English sections with bold
/// headings. Deliberately stateless — no live status and no configured
/// hotkeys (those live in the tray tooltip only), so the window never goes
/// stale.
fn build_help_text() -> Vec<wizard::HelpSection> {
    vec![
        wizard::HelpSection {
            heading: "What is HandsOff?".to_string(),
            body: "\
HandsOff locks your Mac's keyboard and mouse so stray
bumps, pets, or curious hands can't mess with your work.

Your screen stays on, and apps like Zoom keep running.
HandsOff can also lock itself automatically after a while
of no activity — you can change that in Preferences."
                .to_string(),
            table: Vec::new(),
        },
        wizard::HelpSection {
            heading: "Locking and unlocking".to_string(),
            body: "\
Lock: pick \"Lock Input\" from the HandsOff menu in the
menu bar, or use your lock shortcut (hover the menu bar
icon to see it — it depends on your setup).

Unlock: type your passphrase. While locked, mouse clicks
are blocked — even the menu can't be clicked — so there
are only TWO ways back in: your passphrase on the
keyboard, or rebooting your Mac. (The Reenable menu item
can't help — it only turns HandsOff back on after
Disable, and the menu can't be clicked while locked.)

Made a typo? Press Escape to start over."
                .to_string(),
            table: Vec::new(),
        },
        wizard::HelpSection {
            heading: "Choosing your passphrase".to_string(),
            body: "\
Pick something SHORT: 4–6 characters
you can type with one hand. While
locked, your mouse is dead — typing is
all you have, and a short passphrase is
much easier to get right.

And WRITE IT DOWN somewhere safe. If
you forget it, the only way back in is
rebooting your Mac."
                .to_string(),
            table: Vec::new(),
        },
        wizard::HelpSection {
            heading: "Unmuting during video calls".to_string(),
            body: "\
Locked but need to speak? Press your \"talk\" shortcut
(hover the menu bar icon to see it). HandsOff transforms
it into a Space keypress and passes it through to your
apps, so you can unmute in Zoom, Google Meet, and other
call apps."
                .to_string(),
            table: Vec::new(),
        },
        wizard::HelpSection {
            heading: "Menu items".to_string(),
            body: String::new(),
            table: vec![
                ("Lock Input".to_string(), "Locks your Mac right away.".to_string()),
                (
                    "Disable".to_string(),
                    "Pauses blocking to save\nbattery. Use Reenable to\nturn HandsOff back on."
                        .to_string(),
                ),
                (
                    "Reenable".to_string(),
                    "Turns HandsOff back on after\nDisable. Does NOT help with\na stuck lock.".to_string(),
                ),
                (
                    "Preferences…".to_string(),
                    "Change hotkeys and timers.\nNo passphrase needed here.".to_string(),
                ),
                (
                    "Change Passphrase…".to_string(),
                    "Pick a new passphrase.".to_string(),
                ),
                (
                    "Reset…".to_string(),
                    "Wipes everything and runs\nSetup again. Use this if\nyou forgot your passphrase.\nWarning: your old\npassphrase stops working!"
                        .to_string(),
                ),
                (
                    "Fix Accessibility Permission…".to_string(),
                    "Repairs the macOS\npermission HandsOff needs,\nin case an update\nbroke it.".to_string(),
                ),
                (
                    "Check for Updates…".to_string(),
                    "Opens the releases page\nin your browser.".to_string(),
                ),
                ("Help".to_string(), "Shows this window.".to_string()),
            ],
        },
        wizard::HelpSection {
            heading: "If something goes wrong".to_string(),
            body: "\
Not blocking input? HandsOff needs the Accessibility
permission (System Settings > Privacy & Security). Then
try \"Fix Accessibility Permission…\".

Locked and nothing clicks? That's HandsOff working.
Your only ways back in: type your passphrase, or reboot
your Mac. Reenable can't help — it only turns HandsOff
back on after Disable, and the menu is unreachable while
locked.

Forgot your passphrase? Once unlocked, use \"Reset…\" to
start fresh."
                .to_string(),
            table: Vec::new(),
        },
        wizard::HelpSection {
            heading: "About".to_string(),
            body: "\
Created by
Michael S. Huang
michael@michaelhuang.xyz
www.michaelhuang.xyz

https://github.com/mhuang74/handsoff-rs"
                .to_string(),
            table: Vec::new(),
        },
    ]
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

// ---------------------------------------------------------------------------
// Unit tests (issue #29): pure decision logic only. Window/menu plumbing is
// manual-smoke on macOS (no AppKit/event-loop on this platform).
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_startup_flow_routes_all_three_cases() {
        // Config invalid → full wizard, regardless of permissions.
        assert_eq!(
            wizard::startup_flow(false, true),
            wizard::StartupFlow::Wizard
        );
        assert_eq!(
            wizard::startup_flow(false, false),
            wizard::StartupFlow::Wizard
        );

        // Config valid, permissions granted → run normally.
        assert_eq!(wizard::startup_flow(true, true), wizard::StartupFlow::Run);

        // Config valid but Accessibility grant stale (post-update CDHash
        // change) → permission-only re-grant, NOT passphrase re-setup.
        assert_eq!(
            wizard::startup_flow(true, false),
            wizard::StartupFlow::ReGrant
        );
    }
}
