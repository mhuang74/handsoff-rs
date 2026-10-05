# Remove Disable/Reenable tray items; quit on permission loss

## Context

With the dead CFRunLoop spin thread gone (0.9.1), the "minimal CPU mode" rationale for the tray's Disable/Reenable pair is gone; user wants the two items removed for menu simplicity. Verified consequences accepted with the user: tap is always-on when permissions exist; auto-unlock backoff + reboot are the only stuck-Lock recoveries; on runtime permission REVOCATION the app unlocks, notifies, and quits after a 30 s cancelable grace; restart failure uses the same grace-quit. Startup with permissions already missing keeps the app running tapless (wizard/re-grant flow needs a live process) and auto-restarts the tap when granted — that path already exists.

Scope boundary: this is menu-item + disabled-state eradication plus the quit-on-revocation behavior; the sleep/wake tap-timeout recovery APIs (`reenable_event_tap`, `request_reenable_event_tap`, `should_reenable_event_tap_and_clear`, `mark_reenable_completed`, `event_tap::reenable_existing_tap`) are a separate lifecycle and MUST survive untouched.

Build/test constraint (repo AGENTS.md): build on Linux via cargo zigbuild (~/.local/zig, SDK ~/.local/macos-sdk, `SDKROOT`); tests only run on macos-latest CI. Manual smoke requires a release build copied into a locally-built `/Applications/HandsOff.app` (TCC pins the grant to the resolved binary path).

## Approach

Order: (1) core state + lib cleanup, (2) tray menu/handlers/UX, (3) grace-quit machinery, (4) docs, (5) version/changelog. Steps 1–2 keep the build green independently; 3 depends on 1; tests run after each of 1–3.

### 1. Core: delete the disabled state (src/lib.rs, src/app_state.rs, src/preferences.rs, src/constants.rs)

- `src/app_state.rs`: delete field `is_disabled` (~74-75) and its initializer (~101), methods `is_disabled()` (~528-531) and `set_disabled()` (~533-536). Grep confirms the only `set_disabled` writers are `disable()`/`enable()` in lib.rs.
- `src/app_state.rs`: check `clear_lock_state()` callers (`grep -n "clear_lock_state" src/` — currently only `disable()` at lib.rs:407). If it becomes orphaned after step below, delete it (~275-281) and any test referencing it.
- `src/lib.rs`: delete `disable()` (396-424) and `enable()` (425-446). Their only callers are the deleted tray handlers (verified: `handle_disable` → lib.rs:933; `handle_reenable` → lib.rs:971).
- `src/lib.rs`: in the four background worker threads, delete the `if state.is_disabled() { ... }` skip blocks at ~553, ~574, ~615, ~657. In the permission monitor loop delete the `if state.is_disabled() { ... }` skip at ~723-725. Do NOT touch the revoke/restore/missing branches (701-790) except notification copy (step 3).
- `src/lib.rs`: `Drop::drop` calls `stop_event_tap()` (~803) — keep; hotkey unregister in Drop stays.
- `src/preferences.rs`: `MenuState` — delete fields `disable_enabled` and `reenable_enabled` (48-50) and doc comments; `menu_state` signature becomes `pub fn menu_state(is_locked: bool, has_permissions: bool) -> MenuState` with `lock_enabled: has_permissions && !is_locked`; keep `preferences_enabled: true`, `change_passphrase_enabled: !is_locked`, `reset_enabled: !is_locked`.
- `src/constants.rs`: delete `POLL_INTERVAL_DISABLED_SECS` (line 79). `POLL_INTERVAL_ENABLED_MS` stays.

### 2. Tray: remove items, handlers, disabled UX (src/bin/handsoff-tray.rs)

- Delete `disable_item`/`reenable_item` creation (253-255), appends (267-270), id clones (306-307), all tuple threading of `disable_id`/`reenable_id`/`disable_item`/`reenable_item` (~323-372, 532-533, 544-545, 549), and the `_regrant_item`-style unused clone slots that reference them.
- Deferred-dispatch (run_session ~532-580): remove `disable_id`/`reenable_id` from the ids struct and from the gate match (~577-580); the gate whitelist becomes lock_id, check_updates_id, quit_id.
- Deferred/live execution (~600-605, ~646-680): delete the `disable_id`/`reenable_id` branches in both the deferred dispatch and the live `MenuEvent` dispatch; the combined gate condition `event_id == lock_id || event_id == disable_id || event_id == reenable_id` (~646) becomes `event_id == lock_id`, gated on `flags.lock_enabled`.
- Delete `handle_disable` (~930-952) and `handle_reenable` (~955-1016) entirely.
- Poll cadence (~624-633): replace the `is_disabled` branch with a single `Duration::from_millis(POLL_INTERVAL_ENABLED_MS)`.
- `menu_state(...)` call sites (~392, ~415, ~570, ~651, ~796): drop the `is_disabled` argument.
- Icon (820-833): delete `was_disabled` tracking (~330, ~344, ~554) and the `create_icon_disabled()` branch — icon becomes locked/unlocked only.
- Delete `create_icon_disabled()` (~1612-1616) and `assets/tray_disabled.png`.
- `build_tooltip`/`push_status` (~1369-1425): drop the `is_disabled` parameter and the `STATUS: DISABLED` branch (~1407-1410).
- `run_session` tracked-state tuple: remove `was_disabled` everywhere it is threaded.

### 3. Quit-on-permission-loss (grace machinery)

- `src/constants.rs`: add `pub const EXIT_GRACE_SECS: u64 = 30;`
- `src/app_state.rs`: add to the inner mutex state `exit_deadline: Option<std::time::Instant>` (init `None`) with methods:
  - `pub fn request_exit_grace(&self, grace: std::time::Duration)` — sets `Some(Instant::now() + grace)` (overwrites; repeat RestartFailed restarts the clock).
  - `pub fn cancel_exit(&self)` — sets `None`.
  - `pub fn exit_deadline(&self) -> Option<Instant>` — getter.
- `src/lib.rs` `service_tap_lifecycle` (461-502): in the `TapStopped` arm call `self.state.request_exit_grace(Duration::from_secs(EXIT_GRACE_SECS))` before returning; in the `RestartFailed` arm likewise; in the `Restarted` arm call `self.state.cancel_exit()`. Update the enum doc on `TapLifecycleEvent::TapStopped` (50-60): "the tray keeps running and shows status" is no longer true — the tray quits after the grace unless permission is restored (Restarted cancels).
  - Rationale encoded: the monitor thread cannot reach the tao event loop, and `process::exit(0)` from a non-main thread is already the established Quit mechanism (tray.rs:746-753); but cancelability needs the 500 ms poll, so the deadline lives in AppState and the expiry check lives in the tray poll. The monitor thread itself is NOT given exit authority.
- `src/bin/handsoff-tray.rs` run_session poll (after the `service_tap_lifecycle` block, ~800): add —
  ```rust
  if let Some(deadline) = core_borrow.state.exit_deadline() {
      if std::time::Instant::now() >= deadline {
          info!("Permission grace period elapsed; quitting");
          std::process::exit(0);
      }
  }
  ```
  Cancellation is event-driven, not poll-driven: only a `Restarted` event clears the deadline (see previous bullet). A re-grant during the revoke grace triggers the monitor's restore branch → `Restarted` → cancel; a re-grant during a restart-failure grace is impossible (permissions are present in that case), so it always quits after 30 s — the behavior the user chose.
- Notification/copy updates (grep `Use Reenable` — 5 sites):
  - lib.rs ~708 ("Permissions Missing" — startup-missing path): body → `"Accessibility permissions are missing.\nInput blocking stopped to restore normal keyboard and mouse.\n\nInput blocking resumes automatically once permissions are granted."`
  - lib.rs ~765 ("Permissions Revoked"): body → `"Accessibility permissions were revoked.\nInput blocking stopped - your keyboard and mouse work normally now.\n\nHandsOff will quit in 30 seconds. Re-grant Accessibility permission to cancel."`
  - tray.rs ~782 (RestartFailed): body → `"Failed to restart input blocking: {e}\n\nHandsOff will quit in 30 seconds."`
  - tray.rs ~1415 (`STATUS: DISABLED` branch): deleted with the branch.
  - tray.rs ~1420 (NO PERMISSIONS branch): last line → `"Input blocking resumes automatically once granted\n\n"`.
- Keep the "Fix Accessibility Permission…" menu item and `SessionAction::ReGrantPermission` (tray.rs:260, 718-720, 888): it is the manual recovery inside the grace window and for the stale-CDHash wall; a successful re-grant mid-grace leads to `Restarted`, which cancels the quit.
- Do NOT set any exit deadline on the startup-missing path (monitor "Permissions Missing" branch, lib.rs:695-712): tap was never started, no `TapStopped` fires, so the deadline is naturally absent; verify by reading the monitor loop that this branch does not route through `service_tap_lifecycle`'s `TapStopped` arm after a fresh launch.

### 4. Docs

- New `docs/adr/0005-remove-disable-reenable-menu-items.md`: records — Disable/Reenable menu items and the `is_disabled` state removed (menu simplicity post-CPU-fix); tap is always-on when permissions exist; the "unguarded anti-lockout escape hatch" framing in ADR-0003 §3 is corrected: `reenable_enabled` was hardcoded `true` and reachable in dead-tap-while-locked windows, and it is now obsolete because stuck-Lock recovery = auto-unlock backoff (60 min→24 h) or reboot (user-accepted); runtime permission revocation now quits after a 30 s cancelable grace; startup-missing keeps the app running for the wizard/re-grant flow. Mark ADR-0003 superseded-by-0005 in its header, keeping its N2/N5 gating content that still applies (single gating authority, Change Passphrase/Reset refused while locked).
- `CONTEXT.md` glossary: delete the `**Disable**` entry (19-21) and `**Reenable**` entry (23-25) plus the `_Avoid_: reenable` line (29); add a short "Permission loss" glossary term: app quits after 30 s grace; re-granting cancels.

### 5. Version & changelog

- Bump `0.9.1` → `0.10.0` in `Cargo.toml` (grep `0.9.1` — two occurrences: lines 3 and 51).
- `CHANGELOG.md`: new `## [0.10.0]` section: removed Disable/Reenable menu items and minimal-CPU mode; permission revocation now quits after a 30 s cancelable grace; restart failure quits after the same grace; startup with missing permissions unchanged (auto-resume).

## Critical files & anchors

- `src/lib.rs` — `disable()`/`enable()` deletion (396-446); worker-skip blocks (553/574/615/657/723); `service_tap_lifecycle` grace wiring (461-502); monitor notification copy (695-790).
- `src/bin/handsoff-tray.rs` — menu construction/dispatch (253-310, 532-605, 646-680, 740-800); handlers (930-1016); icon/tooltip (820-833, 1369-1425); poll cadence (624-633); new deadline-expiry check (~800).
- `src/preferences.rs` — `MenuState`/`menu_state` (46-68).
- `src/app_state.rs` — `is_disabled` removal; new exit-deadline trio.
- `tests/lifecycle_tests.rs` — gating matrix rewrite (57-151, 537).

## Verification

Linux-side (can run here):
1. `SDKROOT=~/.local/macos-sdk cargo zigbuild --target x86_64-apple-darwin` (repo's standard cross-build; adjust target to the release matrix's triple if the Makefile/CI uses aarch64 — check `.github/workflows` first) — must compile with no `is_disabled`/`disable(`/`enable(` references remaining: `grep -rn "is_disabled\|handle_disable\|handle_reenable\|disable_enabled\|reenable_enabled\|POLL_INTERVAL_DISABLED\|tray_disabled" src/ tests/ assets/` returns only the tap-timeout recovery APIs (`reenable_event_tap`, `request_reenable_event_tap`, `reenable_existing_tap`, `should_reenable_event_tap*`, `mark_reenable_completed`).
2. `grep -c "Use Reenable" -r src/` → 0.
3. New unit tests in `src/lib.rs` test module (macOS-gated, run on CI): `service_tap_lifecycle` with `should_stop_event_tap` set → returns `TapStopped` AND `state.exit_deadline().is_some()`; then `request_start_event_tap` → `Restarted` AND `exit_deadline().is_none()`; `RestartFailed` path (no permissions, CI-friendly) → deadline `is_some()` again.
4. `tests/lifecycle_tests.rs` rewritten for the 2-arg `menu_state`: locked → lock/change_passphrase/reset false, preferences true; unlocked+perms → lock true; no-perms → lock false. Push and watch macos-latest CI (watch by headSha, not --commit).

macOS-side (user, per AGENTS.md GUI limits — cannot be agent-verified):
5. Build release, copy into locally-built `/Applications/HandsOff.app` (TCC path requirement), run with `RUST_LOG=debug`: Lock → input blocked → passphrase unlock works (regression).
6. Revoke Accessibility in System Settings while unlocked: expect unlock-if-locked, "revoked" notification with 30 s wording, tray quits after ~30 s.
7. Repeat 6 but re-grant within 30 s: expect quit canceled and "Input Blocking Restarted" notification.
8. Startup without grant (fresh `tccutil reset Accessibility handsoff-tray.handsoff` + relaunch): expect app stays running, NO PERMISSIONS tooltip, no quit; granting restarts the tap automatically.

## Assumptions & contingencies

- If the monitor loop's "Permissions Missing" branch (lib.rs:695-712) turns out to fire for runtime revocation in some path (not just startup), route the deadline ONLY through `service_tap_lifecycle`'s `TapStopped`/`RestartFailed` arms and verify the startup case sets no deadline; if both paths share the flag, add a `initial_permission_check_done` guard in the monitor so the first observation never arms the exit deadline.
- If `clear_lock_state()` has callers beyond `disable()` discovered by grep, keep it and only delete the `disable()` call site.
- `cargo zigbuild` target triple: read `.github/workflows/release.yml` and mirror its macOS target exactly.
