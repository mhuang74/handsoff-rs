# HandsOff Remediation Plan — PR #23 Review Follow-ups

**Status:** Plan only — no code changes yet. This document is the authority for the remediation work once implementation starts.
**Inputs:** Two-axis code review of PR #23 (`harden_design` vs `main`, 2026-09-28): Standards axis (7 findings) and Spec axis (9 findings) against `specs/deep-design-review-v2-2026-09.md`.
**Relationship to other specs:** Does not modify or supersede any existing spec. It implements the open items of `specs/deep-design-review-v2-2026-09.md` (§2.3, §2.7, §3) and corrects deviations found in review. v1 phases (`specs/deep-design-review-2026-09.md`) remain referenced, not duplicated. The scope-creep items landed in PR #23 (C-1, U-4, crypto removal) are **accepted as built**; this plan only notes them for CHANGELOG attribution.

---

## 1. Corrective changes (behavior)

### R1. Backoff counter anchored to stretch-since-last-auth (Spec c-2 — the linchpin, §2.3)

**Problem:** `AppStateInner::lock` (app_state.rs:133-141) re-anchors `stretch_start` on *every* lock event. Intervals become gaps-from-last-lock instead of cumulative-from-stretch-start. After a long open window, an auto-lock re-engagement restarts the clock, drifting the schedule off the §2.1 timeline (`t=60m, 180m, 420m, 900m`) — exactly the "steady 60-min periodic bypass" §2.3 forbids.

**Change:**
- `stretch_start` is set **only** on the transition into a locked stretch: unlocked → locked. Auto-lock re-engagements (locked → locked) leave `stretch_start` untouched.
- `window_index` advances on elapsed `stretch_start` time crossing `base * 2^window_index`, computed from the anchor, not per-lock-event gaps.
- Both are `std::time::Instant` (§2.4 awake-time semantics unchanged).
- Reset to base occurs **only** in `complete_passphrase_unlock`.

**Acceptance:**
- Simulated timeline: lock at t=0, window opens, user types (window extends), re-lock, re-lock again → next window lands at 180m from the *original* lock, not 60m from the last re-lock.
- Existing app_state tests for re-engagement updated to assert cumulative anchoring; new regression test: two consecutive auto-lock re-engagements do NOT reset `stretch_start`.

### R2. Char map removed from the unlock path (Spec c-1, §3)

**Problem:** input_blocking/mod.rs:103 and setup.rs:266 gate passphrase membership on `keycode_to_char(keycode, false).is_none()`. Keys the US char map can't render are silently unrecorded (or rejected), reintroducing the L-1 layout dependence §3 removes.

**Change:**
- Replace the char-map gate with the explicit §3 rejection set: **Escape (kVK_Escape), Backspace/Delete (kVK_Delete), and the configured lock/talk hotkey keycodes** — matched as keycodes directly, no char rendering.
- Any other physical keycode is a valid passphrase member.
- `utils/keycode.rs` `keycode_to_char` remains **only** for hotkey display strings; add a doc-comment saying so and assert no unlock-path caller (grep-guarded).

**Acceptance:** test that F-keys, keypad keys, and non-US-renderable keycodes are accepted as passphrase members; Escape/Backspace/hotkeys rejected in both capture and verification paths.

### R3. Reserved-hotkey resolution unified (Spec a-3 + Standards 1)

**Problem:** CLI `current_hotkey_keycodes` honors `HANDS_OFF_LOCK_HOTKEY`/`HANDS_OFF_TALK_HOTKEY` (handsoff.rs:190-207); the tray copy does not (handsoff-tray.rs:157-173). Tray setup reserves wrong keys under env overrides, violating §3 "Reject … the configured lock/talk hotkey combos".

**Change:** one `current_hotkey_keycodes(config) -> Vec<KeyCode>` (or a shared `SetupEnv` struct carrying both hotkeys) in the lib crate; both binaries call it. Env > config file > default precedence defined once.

**Acceptance:** test (or compile-time single-definition check) that both binaries' reserved sets match under an env override; manual smoke: tray `--setup` with `HANDS_OFF_LOCK_HOTKEY` set reserves the overridden key.

### R4. Setup keeps prompting until ≥ 4 keys (Spec a-2, §3)

**Problem:** Enter sets `done` unconditionally (setup.rs:225-230); `validate_sequence` then errors and `--setup` bails. Spec wants minimum 4 keys, not minimum 4 keys-or-quit.

**Change:** Enter with < 4 keys → inline rejection message, capture continues; only a valid ≥ 4-key sequence confirms. The unreachable `RejectedKey::Escape`/`Backspace` variants (Standards 7) are removed as part of R2's explicit-set rewrite — the callback rejects those keycodes directly.

**Acceptance:** test: Enter at 0, 1, 3 keys does not end capture; capture ends only after ≥ 4 accepted keys + Enter.

### R5. `enabled` field deleted (Standards 2)

**Problem:** `AutoUnlockState.enabled: bool` is only ever `true` when the enclosing `Option` is `Some`.

**Change:** delete the field; `Option<AutoUnlockState>` alone encodes enablement. Update `set_auto_unlock_enabled` / `set_auto_unlock_backoff` signatures accordingly.

**Acceptance:** `cargo clippy` clean; app_state tests compile and pass.

### R6. `AutoUnlockConfig` type replaces the bool+u64 clump (Standards 3+4)

**Problem:** `(enabled: bool, base_interval_secs: u64)` travels through `set_auto_unlock_enabled`, `set_auto_unlock_backoff`, `resolve_auto_unlock*`; `parse_auto_unlock_timeout` kept its old name while now parsing a backoff base with a `Some(0)`-disabled sentinel.

**Change:**
- New `AutoUnlockConfig` enum in config.rs: `Disabled` | `Backoff { base_interval_secs: NonZeroU64 }` — the enum kills the `Some(0)` sentinel by construction.
- Rename `parse_auto_unlock_timeout` → `parse_auto_unlock_config` (returns the enum).
- Call sites (`handsoff.rs`, `handsoff-tray.rs` startup wiring) pass the enum through; the duplicated `.is_some()`/`.unwrap_or()` plumbing disappears.

**Acceptance:** `0` in config parses to `Disabled`; both binaries share the type; no `Some(0)` handling remains anywhere.

### R7. Use `AUTO_UNLOCK_MIN_BASE_SECONDS` for the setup bound (Standards 5)

**Change:** both setup flows' `!(60..=AUTO_UNLOCK_CEILING_SECONDS)` literal `60` replaced with `AUTO_UNLOCK_MIN_BASE_SECONDS`.

**Acceptance:** grep shows no bare `60..=` in either binary; bound tests still pass.

---

## 2. Consolidation (structural)

### R8. Shared setup flow extracted into the lib (Standards 1 + 6)

**Problem:** `run_setup`, prompt/validation, and startup wiring are duplicated across `handsoff.rs` and `handsoff-tray.rs` and have already diverged (R3). `setup.rs` also inlines a throwaway CGEventTap FFI block mirroring `event_tap.rs`.

**Change:**
- Move the interactive setup flow (capture loop, validation, prompts) into the lib crate (e.g. `src/setup.rs` grows a `run_interactive_setup(config, io) -> Result<SetupOutcome>`); binaries keep only their thin entry glue (CLI prints to stdout; tray prints via its logger).
- The duplicated auto-unlock startup wiring (`resolve_auto_unlock` + `set_auto_unlock_backoff`) collapses into one lib function both binaries call.
- CGEventTap FFI consolidation (R-4 from the v1 review) is **deferred**, not part of this plan — noted so the inline block in setup.rs does not silently grow a second copy.

**Acceptance:** `git grep run_setup src/bin/` empty; both binaries build and their setup flows behave identically under R2/R3/R4 tests.

---

## 3. Open design questions (owner decision needed before implementation)

### Q1. Persist the backoff counter to disk? (Spec a-1, §2.7)

Spec §2.7: "a persisted counter (position + last lock-start)". PR #23 persists mode + base interval (config_file.rs:43-49) but `window_index`/`stretch_start` are in-memory only. §2.5 says relaunch starts unlocked, which makes the persisted counter unreachable in practice — a relaunch resets the lock state entirely, so a persisted counter has no reader.

**Recommendation:** treat §2.7's "persisted counter" as satisfied by the *config-side* persistence (mode + base) and record the in-memory-only counter as an accepted consequence of §2.5 — amend nothing. If the owner wants literal counter persistence, it only matters for a future "relaunch still locked" mode and should be built then. **Decision needed: accept-as-satisfied (recommended) vs implement file persistence.**

### Q2. Reset (force-unlock) resetting the backoff counter (Spec c-3, §2.3)

lib.rs:216-217 routes Reset through `complete_passphrase_unlock`, which resets the counter — but §2.3 says "Only a successful passphrase unlock resets the counter." Under V5 (menu access ≈ owner) this is harmless, and Reset force-unlocking is S-2's simplification.

**Recommendation:** keep the behavior, but make the code honest: split `complete_passphrase_unlock` into `unlock_state_only` (what Reset calls — no counter reset unless the caller passes an auth flag) vs the passphrase path (resets counter). The distinction costs one boolean parameter and makes §2.3 literal in the code. **Decision needed: split-with-flag (recommended) vs document the second reset path as accepted.**

---

## 4. Non-code items

- **N1.** PR #23 body `Refs #2` points at a merged, unrelated issue ("Convert project to produce both CLI and Tray App"). The real spec is `specs/deep-design-review-v2-2026-09.md`. Future PRs in this series must reference the spec doc or a tracking issue created for it.
- **N2.** Scope creep already merged (C-1 single-guard handler, U-4 README 30s→120s fix, crypto.rs/AES removal) — accepted as built; ensure CHANGELOG mentions AES-256-GCM removal (it does) and that v1's Phase-2 item 4 (C-1) is marked done in the v1 doc's tracking when that doc is next touched (do not edit it now).
- **N3.** Add trailing newlines to `AGENTS.md` and `docs/agents/*.md` (Standards 7, trivial).

---

## 5. Execution order

R3, R5, R6, R7 are mechanical and independent → batch first.
R2 + R4 touch the same files (`setup.rs`, `input_blocking/mod.rs`) → one change-set, after R3 (R3 defines the shared reserved-key source R2 needs).
R8 depends on R2-R4 landing (moves the settled flow into the lib) → last.
R1 is independent of all of the above → anytime; largest test surface, do it alongside the R3 batch.
Q1/Q2 resolved (or defaulted to the recommendations) before R1 and R2 respectively.

## 6. Out of scope

- Implementing anything from v1's remaining phases beyond what PR #23 already landed.
- CGEventTap FFI consolidation (R-4).
- Any change to the accepted residuals (§5 of the v2 spec): reboot bypass, keyspace weakness, window extension, owner-during-window.
