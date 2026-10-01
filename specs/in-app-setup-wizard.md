# Simplified Setup for Non-Technical Users

## Problem Statement

Non-technical users cannot complete HandsOff setup. Today, setup requires opening a
terminal, running `handsoff --setup`, and granting Accessibility permission to the
*terminal app* (not HandsOff) because the terminal is the process that creates the
CGEventTap. While the user struggles, the tray app shows warning dialogs and
notifications telling them to go run a terminal command — an interrupting gauntlet
only technical users survive. Distribution assumes the .pkg pipeline, which installs
to `~/Applications` and whose postinstall only prints instructions.

## Solution

Replace the terminal setup gauntlet with a first-run **Setup Wizard**: a native
window inside the tray app that guides the user through granting Accessibility
permission to HandsOff itself, capturing a passphrase, choosing hotkeys and
timeouts, and registering a login item — no terminal involved. Distribution moves
to an unsigned DMG (`HandsOff.app`) plus a separate unchanged CLI artifact for
power users; the .pkg pipeline is deleted. Full lifecycle is covered: a
Preferences window for editing hotkeys/timeouts and changing the passphrase
without wiping anything, a guarded double-confirmed **Reset** that wipes config
and re-runs the wizard (the recovery path for a forgotten passphrase), the old
escape hatch renamed **Reenable** (unguarded, by design), and an interim
"Check for Updates" tray action that opens the latest GitHub release in the
browser until a paid signing account allows a real auto-updater (Sparkle).
Gatekeeper friction (unsigned app) and per-update Accessibility re-grants
(CDHash change) are absorbed by the wizard itself.

Terminology follows the project glossary (`CONTEXT.md`): Passphrase, Setup
Wizard, Lock, Disable, Reenable, Reset.

## User Stories

### First-run setup

1. As a non-technical user, I want to download a single `HandsOff.app` in a DMG,
   so that installing the app feels like any other Mac app.
2. As a non-technical user, I want the Setup Wizard to explain the Gatekeeper
   "right-click → Open" step with clear instructions, so that an unsigned-app
   warning doesn't make me think the app is broken.
3. As a non-technical user, I want the Setup Wizard to open automatically the
   first time I launch the app without a valid config, so that I never need a
   terminal to get started.
4. As a non-technical user, I want the wizard to explain why Accessibility
   permission is needed before asking for it, so that I understand what I'm
   approving.
5. As a non-technical user, I want a single "Grant Accessibility" button that
   opens the exact System Settings pane, so that I don't have to hunt through
   System Settings.
6. As a non-technical user, I want the wizard to wait and confirm when
   permission is granted, so that I know the step is done.
7. As a non-technical user, I want the wizard to detect when the permission was
   granted to the wrong app (misattribution) and tell me what to fix, so that I
   don't grant permission to the wrong thing.
8. As a user updating the app, I want the wizard's permission step to also work
   as a re-grant screen when an update invalidated my Accessibility grant, so
   that an update doesn't leave me with a broken app and no guidance.
9. As a non-technical user, I want to capture my Passphrase by typing it once
   and confirming it, with the same silent keypress behavior as today, so that
   no one shoulder-surfing can learn it.
10. As a non-technical user, I want to pick my Lock and Talk hotkeys and my
    auto-lock timeout and backoff on a single form, so that setup is one
    coherent flow.
11. As a non-technical user, I want the wizard to offer a "launch at login"
    checkbox backed by a modern login item, so that protection resumes after
    reboot without me remembering anything.
12. As a non-technical user, I want setup to be possible entirely inside the
    app, so that I never see a terminal emulator.

### Settings editing

13. As a user, I want a Preferences window to change hotkeys, auto-lock timeout,
    and backoff without re-entering my Passphrase, so that small tweaks don't
    feel like reinstalling.
14. As a user, I want a "Change Passphrase" action in Preferences that captures
    a new Passphrase while keeping all other settings, so that I can rotate my
    Passphrase deliberately without a full wipe.
15. As a user, I want Change Passphrase to require confirming the new Passphrase
    twice, so that a typo doesn't lock me out.

### Recovery and destructive actions

16. As a user who forgot their Passphrase, I want a double-confirmed Reset that
    wipes the configuration and relaunches the Setup Wizard, so that I can
    recover access deliberately and knowingly.
17. As a locked-out user, I want the unguarded Reenable action to stay in the
    menu, so that I can always escape a stuck Lock instantly.
18. As a user, I want Reset and Reenable to be clearly distinct in the tray
    menu, so that I never wipe my config when I only meant to unlock.

### Updates

19. As a user, I want a "Check for Updates" tray action that opens the latest
    GitHub release in my browser, so that I can update without hunting for the
    download page.
20. As a user, I want my configuration and Permissions to survive an update as
    much as macOS allows, so that updating doesn't feel like reinstalling.

### Power users

21. As a power user, I want the CLI binary and its terminal `--setup` flow to
    keep working unchanged as a separate release artifact, so that I can script
    and debug without a GUI.
22. As a power user, I want the same config file format shared by CLI and app,
    so that setups done one way are respected by the other.

### Existing installs

23. As an existing user with a legacy `encrypted_passphrase` config, I accept
    that this format is unsupported and I will be routed to the wizard, so that
    the codebase doesn't carry migration complexity.
24. As an existing user with a current `keycode-v1` config, I want my config to
    load cleanly in the new build, so that updating doesn't force a re-setup.

## Implementation Decisions

- **UI stack**: a single native window using the existing `tao` dependency; no
  new front-end language or toolchain. The wizard and Preferences are windows
  hosted **in-process** by the tray app (no helper binary).
- **Module shape**: setup logic becomes a library — the interactive steps
  (permission check/poll, passphrase capture semantics, config assembly) are
  lifted out of the terminal-specific `run_interactive_setup` into reusable
  functions callable by both the GUI and the existing TUI. The TUI remains
  unchanged.
- **Wizard step order**: ① Gatekeeper/permission explanation + "Grant
  Accessibility" button (opens the Accessibility pane via deep link) →
  ② poll until the authoritative test-tap check passes → ③ Passphrase capture
  (reuse the existing capture semantics: silent tap, ≥4 keys, double entry,
  mismatch loop) → ④ hotkeys + timeouts on one form → ⑤ login-item checkbox.
  Permission-first is forced by the requirement that the passphrase-capture tap
  needs Accessibility granted to the app.
- **First-run trigger**: the tray launches the wizard whenever config is absent
  or fails strict validation on every launch (keycode-v1 hash present, ≥4 keys,
  hotkeys distinct, timeout bounds). This replaces the current `exit(1)`
  "Setup Required" dialog. Fixable-only issues (e.g., duplicate hotkeys) can
  route to Preferences instead of the wizard when that distinction is
  implementable; otherwise the wizard runs.
- **Login item**: registered via `SMAppService` when the wizard checkbox is
  checked. This replaces the .pkg LaunchAgent. Bundle ID stays
  `com.handsoff.inputlock`. Requires macOS 13+ (Ventura); the minimum supported
  system version is raised to 13.0 accordingly (Cargo bundle metadata and
  `LSMinimumSystemVersion`), so no legacy login-item fallback
  (`SMLoginItemSetEnabled`/LaunchAgent) is implemented.
- **Signing/distribution**: unsigned (ad-hoc) builds, DMG artifact. The wizard's
  first screen explains the Gatekeeper right-click → Open dance. Per ADR 0001,
  notarization and Sparkle are deferred until a paid Apple Developer account
  exists.
- **Updater (interim)**: a tray "Check for Updates…" action opens the latest
  GitHub release page in the browser. No Sparkle, no self_update crate.
- **Update consequence handling**: because an unsigned update changes the
  CDHash, TCC invalidates the Accessibility grant on each update; the wizard's
  permission step is reused as the re-grant screen (detect stale grant → walk
  the user through re-checking the box).
- **Reset semantics**: new Reset = double-confirmed dialog → wipe config →
  relaunch into the wizard. The old Reset becomes Reenable — unguarded, in the
  menu always, the deliberate anti-lockout escape hatch.
- **Change Passphrase**: Preferences action; captures a new Passphrase using
  the same silent-capture path as the wizard (confirm twice), then saves the
  config preserving hotkeys/timeouts. Not a wipe.
- **Legacy configs**: `encrypted_passphrase` configs remain rejected (existing
  `load()` behavior); affected users are routed into the wizard. No migration
  tooling.
- **CLI artifact**: same crate, unchanged TUI `--setup`, shipped as a separate
  release artifact.
- **Packaging cutover** (done, #28): the Makefile `pkg` target, the LaunchAgent
  plist template, and the installer scripts are deleted; `INSTALLER-GUIDE.md`
  was replaced by `docs/DMG-GUIDE.md` and `BUILD.md` describes the DMG flow.
  No transitional dual-packaging.
- **Docs**: ADR 0001 (unsigned distribution) and ADR 0002 (in-app wizard
  replaces CLI setup) already record the architectural decisions. Update the
  glossary (`CONTEXT.md`) if any new term emerges during implementation.

## Testing Decisions

- **What makes a good test here**: only external behavior — what a config file
  looks like after a user action, and what the pure validation contract admits
  or rejects. No tests of window wiring, notification firing, or menu-item
  plumbing; the GUI is a thin shell over tested logic.
- **Seams** (user-confirmed):
  1. `ConfigFile::load_from_path` / `save_to_path` round-trips — the primary
     seam. Wizard and Preferences outcomes are verified as `config.toml`
     round-trips: new setup → complete valid config; Change Passphrase →
     new hash, preserved other fields; Reset → config gone; Preferences edit →
     only intended fields changed. Prior art: existing round-trip patterns in
     `tests/`.
  2. Pure helpers in the setup module (`validate_sequence`,
     `is_rejected_keycode`, `is_unlock_blocked_keycode`) — for capture edge
     cases (too few keys, rejected modifiers, backspace/escape behavior
     invariants). Prior art: `tests/auth_tests.rs` style.
- **Strict validation contract**: the "config valid → tray runs; config
  invalid → wizard" decision is itself part of the seam-1 contract and gets
  table-style tests over malformed configs (missing hash, short sequence,
  duplicate hotkeys, out-of-bounds timeouts).
- **GUI/manual**: the actual wizard window, permission polling, and login-item
  registration are verified by a manual smoke script on a real Mac (they cannot
  be meaningfully unit-tested and are the thin shell by design).

## Out of Scope

- macOS versions before 13.0 (Ventura). The minimum system version is raised
  from 10.11 to 13.0 to allow `SMAppService` login items; older systems are
  unsupported, and no legacy login-item fallback is built.

- Sparkle or any signed auto-updater (deferred until a paid Apple Developer
  account; ADR 0001).
- Notarization and Developer ID signing.
- Migration tooling for legacy `encrypted_passphrase` configs.
- Any redesign of the passphrase storage format (keycode-v1 hashing stays; the
  AES-GCM encryption spec is superseded and explicitly not re-litigated).
- Changes to the Lock/Unlock input-capture runtime behavior.
- Homebrew formula / CLI distribution channel mechanics (artifact is produced;
  channel setup is separate).
- App Store distribution.

## Further Notes

- Unsigned distribution means every update re-invalidates the Accessibility
  grant (CDHash changes → TCC treats it as a new app). This is accepted, and
  the wizard's permission step is designed to double as the re-grant screen.
  Buying the Apple Developer account later supersedes ADR 0001 and unlocks
  Sparkle + notarization.
- The tray tooltip's "Tip: Run …--setup" line should be removed once the wizard
  exists — pointing average users at the terminal defeats the purpose.
- Bundle ID `com.handsoff.inputlock` is kept across the cutover so existing TCC
  grants survive as long as the CDHash does; the LaunchAgent label collision
  disappears with the LaunchAgent itself.
- The wizard must handle being launched with no stdin/TCC context (Finder
  launch) gracefully — the old TUI refused non-interactive sessions; the GUI
  wizard has no such constraint.
