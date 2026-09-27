# HandsOff Deep Design Review — Decisions v2 (Auto-Unlock & Passphrase)

**Status:** Decisions confirmed with owner 2026-09-28 (grilling session).
**Supersedes:** the auto-unlock (`D2`, `L-4`) and passphrase (`D1`, `L-1`, `L-6`) decisions in `deep-design-review-2026-09.md`. All other findings/phases in that document remain valid and are referenced, not duplicated.
**Implementation status:** design only — no code changes yet; README and tray tooltip updated to describe the target. This document is the authority until implementation lands.

---

## 1. Finalized decisions

| # | Question | Decision |
|---|---|---|
| V1 | Passphrase matching | **Keycode-sequence passphrases** (hash the physical key-code sequence; layout-independent). Reaffirms D1. |
| V2 | Auto-unlock default | Revised from D2's "3600 s fixed scalar" to **enabled-by-default exponential backoff** (see §2). |
| V3 | Security-adjacent findings | Keep as findings, but **downweighted under the chosen threat model** (see §4). |
| V4 | Plan format | Single document. Reaffirms D4. |
| V5 | Threat model | **Child/colleague/screenshare input guard** — casual interference, not a determined local actor. |
| V6 | Passphrase setup capture | **Event tap during `--setup`** (interactive/`--setup` only). |
| V7 | Auto-unlock recurrence | **Periodic with exponential backoff**, not one-shot (see §2). |
| V8 | Auto-unlock re-lock window | **Idle-capped only**, extendable by any input (accepted). |
| V9 | Sleep semantics | **Awake-time** — `Instant` clocks pause during sleep; no auto-lock/auto-unlock iteration counts sleep. |
| V10 | Unlock notification | **Silent** — no unlock notification at the moment input is restored. |
| V11 | Countdown visibility | Tooltip shows the auto-unlock countdown **only when < 5 min away**. |
| V12 | Relaunch/reboot | **Unlock on relaunch** — reboot ends the locked state (accepted bypass). |

---

## 2. Auto-unlock: exponential backoff (replaces D2)

D2's "default 3600 s, max 7200 s" described a single timeout. The confirmed design is a state machine.

### 2.1 Schedule

- First auto-unlock window opens at **60 min** after lock (`t=0`), then **+120 min, +240 min, +480 min, …** — the interval doubles each time, **capped at 24 h** (no interval ever exceeds 86400 s).
- Example timeline of windows: `t=60m, 180m, 420m, 900m, …`.
- Default **enabled**. Base interval 60 min = 3600 s (same base as D2's scalar, different semantics).

### 2.2 Window semantics

- A window is `auto_lock_timeout` (default 120 s) of **no input**; any input resets the idle timer and extends it.
- Accepted consequence: an at-keyboard masher can hold a window open indefinitely. The **effective lifetime of the lock against stray input is the base interval (60 min)** — a deliberate property of the chosen schedule, not a bug.
- Constraint: `AUTO_LOCK_DEFAULT_SECONDS` (120 s) remains the "no input" re-lock bound within a window.

### 2.3 Reset rule (the linchpin)

- The backoff counter advances across a locked stretch. Auto-lock re-engagements and windows **do not** reset it.
- **Only a successful passphrase unlock resets the counter to the base interval.**
- Without this rule, every 120 s auto-lock re-engagement would restart the schedule at 60 min and the doubling would never engage (steady 60-min periodic bypass). Implementation must key the counter to "stretch since last real auth", not to lock events.

### 2.4 Sleep

- `lock_start_time` / `last_input_time` are `std::time::Instant` (monotonic; does not advance on macOS during sleep). Treat as **awake-time**: sleep pauses both the auto-lock idle timer and the auto-unlock backoff clock.
- Consequence: the §4 metric in v1 ("auto-unlock ≤ 60 min") becomes "≤ 60 min of **awake time**". A locked-then-slept machine does not unlock on wall-clock.

### 2.5 Relaunch / reboot

- On relaunch (including after reboot) the app starts **unlocked**, matching current behavior. A reboot ends the locked state.
- Accepted: a colleague can power-cycle the machine to remove the guard. This is outside the guard's tier (a determined actor can reboot / power off / carry the machine away regardless).

### 2.6 Presentation

- **Silent**: no notification at restore.
- Tooltip countdown shows **only when < 5 min away**, so the far schedule is not broadcast by the menu bar.

### 2.7 Config schema consequence

- `auto_unlock_timeout: u64` (one scalar, `0` = disabled) is no longer sufficient. Target schema: an `auto_unlock` mode — `disabled` | `backoff`, a persisted counter (position + last lock-start), plus base/ceiling constants — not a single timeout.
- The v1 Phase 1 item 3 edits (default 3600 / `AUTO_UNLOCK_MAX_SECONDS` 7200 / 900→901 bounds tests) are **obsolete as written**; they described a timeout, this is a schedule.

---

## 3. Passphrase: keycode-sequence capture (resolves D1's open mechanism)

- Setup captures the physical **key-code sequence** with a throwaway `CGEventTap` active only during `--setup`.
- Requires the Accessibility permission the app already needs; no new TCC permission (unlike IOKit HID, which would pull in Input Monitoring).
- **Interactive only**: `--setup` over SSH/headless cannot capture physical key-codes and is refused (you cannot "type" a physical sequence remotely anyway).
- Reject **Escape, Backspace, and the configured lock/talk hotkey combos** as passphrase members. Minimum **4 keys**.
- `utils/keycode.rs` char map removed from the unlock path (may remain for hotkey display).

### 3.1 Accepted consequence — keyspace

- 4-key sequences are ~50⁴ ≈ 6.25 M possibilities; the hash lives in a `0600` file readable by the local account — offline-trivial to brute force.
- This is coherent **only** under the V5 threat model (casual interference). S-4 (constant-time compare) is therefore largely ceremonial under this model; kept as one-line hygiene at most.

---

## 4. Threat model & severity reconciliation

Chosen model: **child / colleague / screenshare input guard.** Casual, opportunistic interference. Not a determined local actor with physical access (who can reboot, kill the app, or take the machine).

Downstream consistency:
- Auto-unlock-enabled-by-default is **tolerable** under this model.
- S-1 (passphrase in logs) remains a hygiene fix regardless — apply it.
- S-2 (plaintext retained for Reset) and S-4 (constant-time compare) become **informational**; S-2 still simplifies under keycode sequences (Reset force-unlocks via state, no plaintext re-verification).
- S-3 (default `qwet`) is a real leak under *any* model — fixed as first-run setup enforcement, not just tooltip cleanup.

---

## 5. Accepted residuals (explicit, non-goals of remediation)

1. **Reboot bypass** — relaunch is unlocked (§2.5).
2. **Keyspace weakness** — keycode hashes are offline-brute-forceable (§3.1).
3. **Window extension** — idle-capped windows extend under input (§2.2).
4. **Owner returns during a window** — under tap-on-lock there is no tap while unlocked, so passphrase typing in an open window lands in the focused app (the U-5 class). The reset rule means the schedule only advances while locked; the re-lock behavior after an open window needs definition during Phase-2 implementation (recommend: re-armed state, buffer cleared, no auth required — the owner is already past the guard).

---

## 6. Deltas to `deep-design-review-2026-09.md` (not applied — recorded here)

- **L-4 / D2** → replaced by §2 (backoff state machine). Severity of L-4 drops: after D1 the untypeable-passphrase lockout is gone, so auto-unlock is a convenience/recovery net, not a [H] lockout fix.
- **Phase 1 item 3** → "implement auto-unlock backoff mode + counter persistence + reset-on-auth", replacing the scalar edits.
- **Phase 1 item 1** → append the setup capture mechanism (§3): event tap during `--setup`, interactive only.
- **§4 metric** → "auto-unlock ≤ 60 min" becomes "≤ 60 min of awake time (§2.4)".
- **S-2 / S-4** → downweight to informational (§4).