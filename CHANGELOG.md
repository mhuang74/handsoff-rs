# Changelog

## [Unreleased]

- feat: tray **Help** menu item — static read-only Help window (menu summaries, lock/unlock instructions, troubleshooting) via `wizard::run_help`; the tooltip now shows the configured Lock/Unmute hotkeys and points to Help for the full guide; notification and tooltip wording fixed to reference **Reenable** instead of the retired Reset item
- fix: protection-state consistency fixes from the 2026-10-02 critical-bug assessment (#37, findings N1–N3, N5–N8) — (N1) the self-relaunch successor (`--skip-instance-lock`) now re-acquires the single-instance `flock` after a bounded ~5 s grace loop instead of running unguarded forever, so a stray second launch always exits as a duplicate; (N3) the CLI main loop services the same shared tap-lifecycle block as the tray (`HandsOffCore::service_tap_lifecycle`), recovering from macOS sleep/wake tap timeouts that previously left the CLI reporting Locked while all input flowed; (N5/N6) `preferences::menu_state` is now the single gating authority — Change Passphrase and Reset are refused while Locked (dead-tap windows can no longer re-key or wipe the config unauthenticated), and every execution path (live clicks, deferred clicks re-validated at dispatch, and the handlers) consults it; (N6) Change Passphrase first requires a fresh capture of the CURRENT Passphrase, verified against the stored hash before the new double-entry capture begins (verification failure aborts with no state change); (N2) Disable now clears an active Lock (a stopped tap enforces nothing — `is_locked` can never outlive the tap, while the backoff schedule is left untouched per §2.3); (N7) `config.toml` is created 0600 atomically (no world-readable write window), a group/other-readable config is auto-repaired to 0600 on load instead of merely warning — and only a FAILED chmod is a hard error with a `chmod 600 <path>` instruction, which never routes to the Setup Wizard; (N8) legacy configs with backoff mode but no stored base interval fall back to the 3600 s default in Preferences and Change Passphrase, so Passphrase rotation no longer requires a full Reset
- fix: Change Passphrase dialog overhaul (issue #36) — the dialog is a raw `NSWindow`, so tao's close events never fired for it: closing it while waiting for the **Capture** button wedged the tray (menu clicks queued invisible duplicate dialogs; recovery required `killall`). The window delegate's `windowShouldClose:` now sets a close flag polled in EVERY flow phase (this also fixes the same wedge class in Preferences and the permission re-grant screen, whose windows were missing the delegate), and the window is hidden (`orderOut`) on every exit path — success, failure, cancel, close — so dead windows never pile up. The dialog now appears immediately after the menu click (dummy AppKit event wakes the loop so `stop:` takes effect now instead of on the next accidental wake — tao's `stop_app_on_panic` technique; applies to all window-flow menu actions). Capture feedback matches user expectations while staying silent (no cleartext, ADR 0002): per-entry dot counts with "Entry 1/2 of 2" prefix, visible 120 s countdown, an explicit "first entry accepted — re-enter to confirm" state, a match/mismatch verdict after Enter, and rejected reserved keys NAMED in the status line ("L is reserved (Lock hotkey)") — the reserved-key list is disclosed up front, built by the same source constants capture enforces (`setup::reserved_key`, advertised == enforced, test-enforced by enumeration). Success/failure swap the content to a reason + OK button; the duplicate system notification/alert in the tray on completion is gone. Repeated menu clicks during an open dialog no longer create duplicate dialogs: the dialog consumes window-flow clicks and comes to front; immediate-action clicks (Lock, Disable, Reenable, Check Updates) are deferred to the next tray session so none are swallowed. Window widened to 560 pt so instructions fit.
- fix: wizard silent keyboard lockout (2026-10-01 incident) — the setup wizard used to start the keyboard-capturing passphrase tap **automatically** the moment Accessibility was granted, swallowing every keydown system-wide for up to 300 s with no visible affordance, and each relaunch (login item or user) repeated the cycle. Capture now starts ONLY on an explicit **Capture Passphrase** button click (wizard and Change Passphrase flows), is bounded to 120 s with a visible countdown (`CaptureEvent::Tick`), and closing the wizard window aborts an in-flight capture within one 100 ms pump slice (`setup::request_capture_abort` via `windowShouldClose:` on the wizard window delegate). A single-instance `flock` guard (`~/Library/Application Support/handsoff/handsoff.l…
- fix: wizard stale-TCC-grant dead end (#34) — after an app update replaced the binary, the permission step could hang forever on "Waiting for Accessibility permission…" because the old TCC row is pinned to the previous ad-hoc CDHash (checkbox reads ON, both permission checks fail). Both the wizard and the permission-only re-grant screen now detect the stuck wait (no grant within 30 s of clicking Grant) and surface a **Reset Permission & Restart…** escape hatch: user-confirmed `tccutil reset Accessibility handsoff-tray.handsoff`, then app relaunch so a fresh grant binds to the current build. The TCC service name is `handsoff-tray.handsoff` (cargo-bundle's `<bin>.<package>` fallback), not the `com.handsoff.inputlock` identifier in `Cargo.toml`/docs …

## [0.8.0] - 2026-10-01


- feat: tray **Check for Updates…** menu item (#29) — opens https://github.com/mhuang74/handsoff-rs/releases/latest in the default browser via `open` (no Sparkle / self_update per ADR 0001); fire-and-forget, never blocks the tray
- feat: Accessibility permission re-grant (#29) — a valid config with a stale grant (e.g. after an unsigned update changed the CDHash) now opens the wizard's permission step only, instead of full passphrase re-setup; new pure `wizard::startup_flow` routing helper (Run / ReGrant / Wizard) and `run_permission_regrant` entry point; config untouched on re-grant; "Fix Accessibility Permission…" tray item reopens the screen manually
- docs: wizard first screen explains the Gatekeeper right-click → Open dance for unsigned apps (#28)
- feat: DMG distribution (#30) — release pipeline builds and attaches a `.dmg` instead of a `.pkg`
- fix: review remediation for DMG cutover
- fix: review fixes for #29 — macOS 13 window activation, non-blocking update-check failure notice, dropped tautological tests
- perf: steady-state CPU reduction — `MouseMoved` removed from the event tap (idle time now read via `CGEventSourceSecondsSinceLastEventType`, `min`-combined with the tap clock for decision and countdown display); tray tooltip rebuilt on state change or every 15 s instead of every 500 ms tick
- fix: floor idle seconds in countdown so display boundary matches fire boundary; route countdown through shared idle source, gate tooltip rebuild
- change: auto-lock default 120 s → 180 s

- feat: tray lifecycle split (#27) — old Reset renamed **Reenable** (unguarded, always in menu: ends a stuck Lock and restarts input capture without changing config); new **Reset…** is double-confirmed (caution dialog) and wipes the config then relaunches the in-app Setup Wizard; **Preferences…** window edits hotkeys, auto-lock timeout, and auto-unlock backoff without passphrase re-entry (empty field = unchanged); **Change Passphrase…** captures a new passphrase via the same silent double-entry path and preserves all other settings
- feat: `preferences` library module — `apply_preferences` (merge + constructor revalidation; invalid edits never touch the file), `change_passphrase` (new hash, all other fields preserved), `wipe_config` (idempotent removal), `menu_state` (pure menu-gating rules, unit-tested); path-taking variants (`*_to_path`) back the config round-trip tests in `tests/lifecycle_tests.rs`
- feat: in-app Setup Wizard — the tray launches a native single-window wizard when the config is absent or fails strict validation (ADR 0002): permission explanation + Grant → Accessibility poll → silent physical-key passphrase capture (double entry, no cleartext) → hotkeys/timeouts form → SMAppService login-item checkbox; no terminal involved; tooltip "Run …--setup" tip removed
- refactor: `setup.rs` library split — `capture_passphrase_headless` (GUI-usable event-tap capture), `validate_config_strict` (tray run-or-wizard gate), `assemble_config`/`assemble_and_save_config` (config assembly seam); TUI `--setup` unchanged
- feat: keycode-sequence passphrases (`keycode-v1`) — setup captures physical key-codes via a temporary event tap (interactive only); hash stored in config; legacy encrypted configs force re-setup (V1, V6)
- feat: auto-unlock exponential backoff — enabled by default, base 60 min, doubling capped at 24 h; only a successful passphrase unlock resets the schedule; awake-time semantics (V2, V7–V9)
- feat: first-run setup enforcement in tray — default `qwet` passphrase auto-creation removed, tooltip hint removed (S-3, L-5)
- fix: passphrase buffer contents no longer written to logs; length only (S-1)
- feat: Reset force-unlocks via state, no plaintext stored (S-2)
- feat: silent unlock — no notification when input is restored (V10)
- feat: tooltip shows auto-unlock countdown only when < 5 min away (V11)
- refactor: single-guard keystroke handler; removed `crypto.rs` (AES-256-GCM) and its dependencies

## [0.6.10] - 2026-09-27

## 📦 Uncategorized

- CI: x86_64 macOS release builds, release job restructure, AI code-review updates
   - PR: #20



## [0.6.9] - 2026-03-28

## 📦 Uncategorized

- fix: prevent tray app showing locked state when input is not blocked
   - PR: #19



## [0.6.8] - 2026-03-09

## 📦 Uncategorized

- fix: eliminate desktop stutter from event tap timeout
   - PR: #16

## [0.6.7] - 2026-03-03

## 📦 Uncategorized

- investigate: add sleep/wake stutter telemetry and fix zombie Mach port accumulation
   - PR: #13

## [0.6.6] - 2026-02-27

## 📦 Uncategorized

- feat: reduce passphrase retry delay and add Escape key
   - PR: #12

## [0.6.5] - 2026-02-20

## 📦 Uncategorized

- fix: release CGEventTapRef to prevent desktop stuttering
   - PR: #11

## [0.6.4] - 2025-11-15

- Set default passphrase to `quit`, so user can skip setup and immediately try out HandsOff

## [0.6.3] - 2025-11-15

## 📦 Uncategorized

- Fix Talk hotkey compatibility with Google Meet/Zoom
   - PR: #10

## [0.6.2] - 2025-11-14

## 📦 Uncategorized

- Feature: support configurable hotkeys
   - PR: #9

## [0.6.1] - 2025-11-13

### Fixed
- Fix auto-unlock timeout=0 causing immediate unlock instead of disabling
- Fix typo

### Changed
- Disable Auto-Unlock by Default for Release Builds

## [0.6.0] - 2025-11-12

### Fixed
- Fix critical permission loss bug that could cause system lockout (#7)

## [0.5.1] - 2025-11-06

### Fixed
- Fix PKG installer postinstall script by including LaunchAgent plist template in app bundle

## [0.5.0] - 2025-11-06

### Added
- Add encrypted passphrase storage with AES-256-GCM (#6)
- Add CLI binary releases and update documentation

### Changed
- Separate developer documentation from end-user README
- Update install help text
- Remove deprecated menu items

## [0.4.0] - 2025-11-05

### Added
- Add GitHub Actions workflow for automated macOS releases
- Add Disable feature for minimal CPU usage
- Add dark mode support to installer HTML
- Convert project to produce both CLI and Tray App (#2)

### Changed
- Change installer to user-level installation (no root required)

### Fixed
- Fix WindowServer stability issues in Disabled mode
- Fix reset after disable (#4)
- Fix GitHub Actions workflow syntax errors

## [0.1.0] - 2025-10-22

> Note: features listed below reflect the 0.1.0 architecture (Keychain storage, Touch ID, 3-minute auto-lock). Storage moved to config.toml with keycode-v1 passphrase hashes and auto-lock default 180 s in later releases; Touch ID was removed.

### Initial Release

#### Features
- Complete input blocking via CGEventTap for keyboard, mouse, and trackpad
- Passphrase-based authentication with SHA-256 hashing
- Touch ID support (macOS 10.12.2+)
- Global hotkeys:
  - Lock: `Ctrl+Cmd+Shift+L`
  - Talk: `Ctrl+Cmd+Shift+T`
  - Touch ID: `Ctrl+Cmd+Shift+U`
- Auto-lock after 3 minutes of inactivity
- 5-second input buffer reset for accidental input
- macOS Keychain integration for secure storage
- Menu bar interface with lock status indicator
- System notifications for lock/unlock events

#### Technical Details
- Built with Rust 2021 edition
- Uses CFMachPortCreateRunLoopSource for event tap run loop integration
- Compatible with macOS 10.11+ (El Capitan and later)
- Supports both Intel (x86_64) and Apple Silicon (arm64) architectures

#### Known Limitations
- Touch ID uses osascript for authentication (future: direct LocalAuthentication framework)
- Menu bar menu items don't yet trigger lock/unlock actions
- Talk hotkey framework exists but doesn't implement spacebar passthrough yet
- No visual lock indicator beyond menu bar icon
- Settings UI for customization not yet implemented

### Fixed
- Linker error with CGEventTapCreateRunLoopSource by using CFMachPortCreateRunLoopSource instead
