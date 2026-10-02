# ADR 0003: Gate Reset and Change Passphrase while Locked; single gating authority

Date: 2026-10-02
Status: Accepted
Supersedes: none
Related: issue #37 (findings N2, N5, N6), ADR 0002, CONTEXT.md glossary

## Context

Security of the Locked state rests solely on the event tap blocking mouse
clicks. In any dead-tap-while-locked window (the re-enable debounce after a
macOS tap timeout; tap-stopped paths that did not clear the Lock), the tray
menu is reachable while the tooltip claims LOCKED. Reset and Change
Passphrase were marked always-enabled in `preferences::menu_state`, so
during such a window an unauthenticated person could wipe the configuration
or re-key the Lock.

Separately, the deferred menu-click dispatch (issue #36) executed stale
clicks with no gating at all: a Disable-then-Lock sequence could end in
`is_locked = true` with no tap enforcing it — a locked stretch that Reenable
would later clear without authentication.

## Decision

1. **`preferences::menu_state` is the single gating authority.** Every
   execution path — live clicks, deferred clicks re-validated at dispatch
   time, and the Reset / Change Passphrase handlers — consults the same
   pure function. One rule set, so the paths cannot drift apart again.

2. **Change Passphrase and Reset are refused while Locked.** This narrows
   the glossary's description of Reset ("the recovery path for a forgotten
   Passphrase"): a user who is Locked with a dead tap AND has forgotten
   their Passphrase must now wait for the tap to recover (tray: ~10 s
   debounce) or use the CLI/relaunch. We accept this because the deadlock
   scenario requires two simultaneous failures, while the unauthenticated
   re-key/wipe scenario required only one.

3. **Reenable stays unguarded** (per CONTEXT.md: the deliberate anti-lockout
   escape hatch). It clears a stuck Lock without authentication by design;
   the N5 concern does not apply because Reenable restores input rather than
   destroying protection.

4. **Disable clears an active Lock** (rather than refusing to run while
   Locked). Disable is an authenticated operator action; clearing keeps the
   menu free of dead-end states. The backoff schedule is NOT reset — per
   specs/deep-design-review-v2-2026-09.md §2.3, only a successful Passphrase
   unlock resets it (`AppState::clear_lock_state` vs `reset_all`).

## Consequences

- A dead-tap-while-locked window can no longer produce an unauthenticated
  config wipe or re-key.
- Deferred clicks that would create impossible states are dropped with a log
  line at dispatch time.
- `reset_all` semantics are unchanged for the legitimate Reenable path.
