# ADR 0005: Remove Disable/Reenable menu items; quit immediately on permission loss

Date: 2026-10-05
Status: Accepted
Supersedes: ADR 0003 (partially — the Disable/Reenable and escape-hatch parts; the N2/N5 gating content still applies)
Related: spec `specs/remove-disable-reenable-quit-on-permission-loss-v2-2026-10.md`, ADR 0003, CONTEXT.md glossary

## Context

The "minimal CPU mode" rationale for the tray's Disable/Reenable pair died
with the CFRunLoop busy-spin thread (removed in 0.9.1): idle CPU use no
longer justifies two extra menu items and a whole `is_disabled` state axis.
This ADR records their removal and the replacement behavior for permission
loss.

## Decision

1. **Disable/Reenable and `is_disabled` are deleted.** Tap is always-on
   when permissions exist. There is no paused state.

2. **Stuck-Lock recovery is now auto-unlock backoff (60 min → 24 h) or
   reboot** (user-accepted). The "unguarded anti-lockout escape hatch"
   framing in ADR-0003 §3 is corrected with the qualification established
   in review: Reenable was NEVER reachable during a healthy Locked state
   (the live tap blocks every mouse event type — `handle_mouse_event`
   returns true unconditionally), and in a dead-tap-while-locked window the
   passphrase is equally unusable, so Reenable was only ever the FASTEST
   recovery there, not a bypass of a working lock. Dropping it does not
   remove an escape hatch that ever worked.

3. **Runtime permission revocation quits immediately.** When the monitor
   detects revocation while a live tap is held, the app unlocks (if
   locked), stops the tap, shows ONE final notification explaining why and
   how to fix it ("re-grant Accessibility permission … and relaunch"), then
   exits. This replaces the source plan's 30 s cancelable grace-quit: the
   grace had a real race (a grant landing while the monitor's 15 s poll and
   the tray's 500 ms poll are both mid-flight → quit despite a successful
   re-grant), and a correct fix needed expiry-time restart plus permission
   recheck — judged not worth the machinery. The app does not
   self-relaunch; Launch Agents / login items can do that if the user sets
   one up.

4. **Tap-restart failure also quits.** `RestartFailed` (tap creation failed
   with permissions present) shows the same single-notification pattern
   with failure wording and exits unconditionally — no permission re-check
   gate, which would cancel the quit on the only case the arm exists for.
   Restart failure is one-shot with no retry; accepted.

5. **Startup with permissions missing keeps running tapless** (unchanged
   behavior; the source plan would have broken it): the
   wizard/re-grant flow needs a live process, and the tap auto-restarts
   when the grant lands. The distinction is carried by
   `TapLifecycleEvent::TapStopped { tap_was_running }` — the stop is
   serviced with the fact "was a tap actually enforcing?" captured before
   the stop; `false` = startup-missing (keep running), `true` = runtime
   revocation (notify + quit).

## Consequences

- The tray menu has 8 items (was 10); `MenuState` loses
  `disable_enabled`/`reenable_enabled`; `menu_state(is_locked,
  has_permissions)` is the unchanged single gating authority for what
  remains.
- The stuck-Lock recovery surface shrinks to backoff + reboot; ADR-0003's
  N2/N5 rules (single gating authority, Change Passphrase/Reset refused
  while locked) still apply and are unchanged.
- The pre-exit notification is the entire answer to "the user knows why it
  quit", so both quit arms sleep `NOTIFICATION_EXIT_DELAY_MS` (1 s) between
  `.show()` and `std::process::exit` — `notify-rust` posts to `usernoted`
  asynchronously and an instant exit can drop the notification.
- Accepted residual window: a revoke firing while no tap is held (grant
  and revocation landing inside one monitor interval) reports
  `TapStopped { tap_was_running: false }` and does not quit — the app stays
  running tapless with the NO PERMISSIONS tooltip (the startup-missing
  behavior, not a silent state). A `tap_ever_started` latch would close it
  but adds parallel state that can desync; rejected.
