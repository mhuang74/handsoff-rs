# HandsOff-RS Critical/High Bug Assessment — Second Pass (fix_installer_hang @ ee5196a)

Read-only analysis; no code edits. Companion to `docs/assessment-interception-hang-risk-2026-10.md`
("prior report"), which covered the event-tap hang/stall surface (its H1–H8). This pass audited
everything the prior report did **not**: state machine, auth/buffer logic, config persistence,
hotkey lifecycle, tray menu dispatch, wizard flows, CLI main loop, and the single-instance guard.

Method: three independent read-only audit sweeps (state/auth, config/installer, tray/wizard) plus
manual verification of every HIGH/MEDIUM claim against the source before inclusion. Claims that
contradicted the prior report's verified threading model were downgraded or dropped (see N11).

Findings are NEW; nothing below restates a prior-report item. N4 initially claimed to amend prior
H5; quantification withdrew that claim (H5's "microseconds" assessment stands).

---

## N1 — Single-instance flock is permanently disabled after every self-relaunch [HIGH]

- `src/bin/handsoff-tray.rs:137-141`: when `--skip-instance-lock` is passed, the tray logs and
  **never acquires the flock**. `acquire_single_instance_lock` has exactly one callsite
  (startup). Verified by grep: no re-acquisition anywhere.
- Both relaunch paths spawn the successor with the flag:
  `src/bin/handsoff-tray.rs:1106-1126` (`relaunch_self`), `src/wizard.rs:522-552`
  (`relaunch_after_reset`, TCC-reset path).
- The code comment justifies skipping only for the spawn-overlap window ("parent still holds the
  lock at spawn time and exits immediately after"), but the flag disables the guard for the
  successor's **entire lifetime**.
- **Trigger:** any Reset / wizard-completion / stale-TCC "Reset & Restart" relaunch, followed by a
  second launch (login item, Finder double-click, dock icon). The lock file is free → second
  instance acquires it → two tray instances, two event taps, two hotkey managers, racing AppState
  on the same config.
- **Impact:** exactly the failure class the guard was added for (2026-10-01 incident: 16 stacked
  launches, keyboard lockout) — reintroduced one relaunch later.
- **Fix direction:** successor should retry `flock(LOCK_EX|LOCK_NB)` in a short grace loop instead
  of skipping for its whole life; only the fatal-alert path needs bypassing.

## N2 — Deferred menu dispatch bypasses all gating: Disable+Lock leaves inconsistent state (locked-without-tap) [MEDIUM]

- `src/bin/handsoff-tray.rs:517-531`: `take_deferred_menu_events()` are dispatched
  **unconditionally** at next session start — no `menu_state` check. The session loop disables
  menu items when locked (`preferences.rs:53-72`, `disable_enabled=false` when locked), but
  deferred clicks skip that gate entirely.
- `src/lib.rs:432-448` (`disable()`): stops the tap and unregisters hotkeys but **never touches
  `is_locked`**.
- `handle_disable` (`handsoff-tray.rs:830-852`) has no locked check; `handle_lock_toggle`
  (`handsoff-tray.rs:810-827`) locks unconditionally.
- **Trigger (verified reachable):** with a dialog open (dialogs defer immediate-action clicks,
  `wizard.rs:155-172`), click **Disable**, then **Lock**, then close the dialog. Dispatch runs
  Disable → tap stopped, hotkeys gone; then Lock → `is_locked=true` with no tap and no lock
  hotkey.
- **Impact (corrected after UI verification):** NO misleading-locked UI — both the icon
  (`handsoff-tray.rs:729-733`) and `build_tooltip` (`handsoff-tray.rs:1221-1230`) check
  `is_disabled` FIRST, so the end state renders "STATUS: DISABLED" and input flowing is
  consistent with the display. The residual defect is **latent state inconsistency**:
  `is_locked=true` survives `disable()` with no tap enforcing it; the auto-unlock anchor was
  set by the Lock dispatch; and recovery (Reenable) clears the lock **without authentication**
  (`handsoff-tray.rs:861-866`, documented S-2 escape hatch), silently ending what the state
  machine considers a locked stretch. The "tooltip says LOCKED while input flows" failure is real
  but belongs to N5's dead-tap-while-locked window (where `is_disabled=false`), not here.
- **Fix direction:** gate deferred dispatch on the same `menu_state` rules as live clicks (drop or
  reject Disable/Reenable while locked); make `disable()` either refuse or clear `is_locked` so
  lock state never outlives the tap that enforces it.

## N3 — CLI never re-enables the tap after a macOS timeout: silent loss of protection [HIGH, CLI binary only]

- The tap callback requests re-enable on `DISABLED_BY_TIMEOUT` (sleep/wake, slow callback):
  `src/input_blocking/event_tap.rs:246-261` → `state.request_reenable_event_tap()`.
- The **tray** polls and services this (`handsoff-tray.rs:645-653` → `reenable_event_tap()`).
- The **CLI main loop** (`src/bin/handsoff.rs:225-268`) polls only `should_exit_and_clear()` and
  `should_stop_event_tap_and_clear()` — it never touches `should_reenable_event_tap_and_clear()`.
  Verified by grep: zero CLI references.
- **Trigger:** machine sleeps while the CLI is running and locked → macOS disables the tap on
  wake → flag set, never consumed.
- **Impact:** CLI stays running, `is_locked` stays true, logs claim protection, but every
  keystroke/click flows through. No re-enable, no exit, no user signal. Same silent-failure class
  as prior-report H1's timeout leg, but here it is **permanent** (tray recovers within ~10 s
  debounce; CLI never does).
- **Fix direction:** add the same re-enable poll block to the CLI loop (3 lines; `reenable_event_tap`
  already falls back to full restart).

## N4 — Unbounded passphrase buffer + O(n) verify per keystroke: no-cap growth gap [LOW; severity corrected after quantification]

- `src/input_blocking/mod.rs:113-124`: every locked KeyDown pushes onto `input_buffer` (no length
  cap anywhere — `app_state.rs:47`) and then runs `verify_keycodes` = SHA-256 over the **entire
  buffer** plus a 64-byte String clone, all under the state mutex inside the tap callback.
- Buffer reset requires 3 s of no keys (`BUFFER_RESET_DEFAULT_SECONDS`, `constants.rs:57`);
  holding a key down (macOS auto-repeat ~15–30/s) keeps `last_key_time` fresh forever, so the
  reset thread never fires during a continuous mash.
- **Impact (quantified):** per-keystroke cost is one SHA-256 over 4n bytes. At 30/s auto-repeat,
  one hour gives n≈100k → ~400 KB per hash ≈ sub-millisecond on Apple Silicon (SHA-256 runs at
  multiple GB/s). Reaching even a 1 s callback would need ~1 GB of buffer ≈ 250M keys ≈ months of
  uninterrupted auto-repeat. **The tap-timeout/silent-lockout framing is quantitatively
  unsupported — withdrawn.** Prior H5's "critical section is microseconds" stands for all
  realistic sessions. The genuine residual gap: no cap, so memory and mutex hold time grow
  linearly without bound during a continuous mash — a hygiene/robustness gap, not an availability
  or protection-loss vector.
- **Fix direction (cheap, worth doing while nearby):** skip verify unless `buffer.len()` equals
  the stored passphrase length; cap the buffer (e.g. 2× passphrase length, clear-on-overflow).

## N5 — Lock state is not coupled to tap liveness; Reset / Change Passphrase reachable unauthenticated in dead-tap windows [MEDIUM]

- Security of the locked state rests **solely** on the tap blocking `LeftMouseDown` (comments at
  `handsoff-tray.rs:808-809`). No menu handler checks `is_locked`: `handle_change_passphrase`
  (`handsoff-tray.rs:1007-1022`), `handle_reset` (`handsoff-tray.rs:1031+`),
  `change_passphrase_to` (`preferences.rs:198-246`). `menu_state` marks Change Passphrase and
  Reset **always enabled** (`preferences.rs:66-67`).
- Dead-tap-while-locked windows exist: the re-enable debounce window after `DISABLED_BY_TIMEOUT`
  (up to `REENABLE_DEBOUNCE_SECS = 10` + poll latency, `constants.rs:113-119`), and any tap-death
  path that doesn't clear `is_locked` (tray stop path `handsoff-tray.rs:633-638` stops the tap
  without touching lock state; the permission monitor does unlock, `lib.rs:726-728`, but only on
  the revocation transition it observes).
- **Impact:** during such a window the tray menu is clickable on a "locked" machine → Reset wipes
  the config (passphrase gone, post-relaunch successor boots unlocked) or Change Passphrase
  re-keys the lock — zero authentication.
- **Fix direction:** add an explicit `is_locked` refusal to Reset/Change-Passphrase handlers
  (defense in depth; don't rely on input blocking as the auth boundary).

## N6 — Change Passphrase performs no old-passphrase verification [MEDIUM]

- `src/preferences.rs:198-246`: builds the new hash straight from the freshly captured sequence;
  nothing verifies knowledge of the existing passphrase. The only gate is menu reachability
  (see N5).
- **Impact:** anyone who can reach the menu (any unlocked session — e.g. during an auto-unlock
  window, or a dead-tap window per N5) can silently re-key the lock. Combined with N5, a locked
  machine can be re-keyed to a passphrase the owner doesn't know; the auto-unlock schedule is
  untouched (only real auth resets it, `app_state.rs:234-252`), so the change is also stealthy.
- Note: the project deliberately skips plaintext verification for Reenable (S-2, issue #34), but
  no equivalent rationale is documented for Change Passphrase.
- **Fix direction:** require the current passphrase (typed through the locked-buffer path or a
  capture-verify step) before re-capture; at minimum refuse while `is_locked`.

## N7 — Passphrase hash is unsalted fast SHA-256; write-then-chmod window; warn-only permissive config [MEDIUM]

- `src/utils/mod.rs:24-31` / `config_file.rs` `Config::new`: SHA-256 over raw keycodes, no salt,
  no KDF. Min 4 keys over a ~100-keycode space ≈ 10⁸ combinations — offline brute force of a
  captured hash takes seconds. The stored hash is effectively the passphrase.
- `src/config_file.rs:379-382` (`save`) and `src/preferences.rs:277-295` (`persist_to`):
  `fs::write` creates the file with umask-default mode (typically 0644) and only **then**
  `set_permissions(0600)` — a window where the hash file is world-readable; if chmod fails, the
  0644 file remains.
- `src/config_file.rs:213-228` (`load_from_path`): a permissive-mode config only gets
  `log::warn!`; the app keeps running against a world-readable hash indefinitely.
- **Impact:** another local user (or anything reading during the write window) recovers the
  passphrase offline. Tempered by requiring local file read access; elevated because enforcement
  is warn-only.
- **Fix direction:** set 0600 via `OpenOptions::mode` at creation (no window); a content-valid but
  permissive-mode config is auto-repaired (chmod 0600, log, continue) — do NOT route it to the
  Setup Wizard, which would re-capture and discard the working Passphrase; only a failed chmod
  fails hard with a `chmod 600 <path>` instruction, exiting the process or dialoging — never the
  setup/wipe path. Longer term, a salted KDF (the V5 threat model note in `utils/mod.rs:35-46`
  addresses comparison timing, not offline brute force).

## N8 — Legacy configs (backoff mode, no base interval) permanently break Preferences and Change Passphrase [MEDIUM]

- Load accepts `auto_unlock_mode = "backoff"` with no `auto_unlock_base_interval` (validated only
  `if let Some`, `config_file.rs:354-370`); runtime falls back to the 3600 s default.
- But `preferences.rs:152-154` (`apply_preferences_to`) and `preferences.rs:229-231`
  (`change_passphrase_to`) compute `current.auto_unlock_base_interval.unwrap_or(0)` and
  round-trip through `Config::new`, which **bails on base 0 with backoff enabled**
  (`config_file.rs:121-135`). Verified against source.
- **Impact:** every Preferences save and every Change Passphrase attempt fails on such configs.
  Security-relevant: a user who suspects compromise **cannot rotate the passphrase** without a
  full Reset (which wipes it). Population is legacy configs only, hence MEDIUM.

## N9 — Talk-hotkey transform leaks while unlocked; in-code U-5 claim is false [MEDIUM-LOW]

- `src/input_blocking/mod.rs:44-63`: the talk-hotkey branch runs **before** the `is_locked`
  check, and the tap is live while unlocked (created at startup, stopped only on permission
  loss/disable — `event_tap.rs:262-268` "Always handle keyboard events (for hotkeys even when
  unlocked)"; unlock never stops the tap, verified across tray/CLI/lib call sites).
- The comment "(U-5) When unlocked there is no tap, so no transformation can leak" is factually
  wrong.
- **Impact:** pressing Ctrl+Cmd+Shift+T **while unlocked** rewrites the event to a bare spacebar
  and delivers it to the focused app — spurious spaces, and any other app's use of that
  3-modifier+T combo is silently eaten system-wide, at all times. Also sets `talk_key_pressed`
  state while unlocked.
- **Fix direction:** gate the transform on `state.is_locked()` (the flag update can stay).

## N10 — `reregister_hotkeys` failure path "restores" the NEW keys [LOW-MEDIUM]

- `src/lib.rs:178-202`: `set_hotkey_config(new…)` at line 185 already overwrites
  `self.lock_key/self.talk_key` and the AppState keycodes. The error path calls
  `set_hotkey_config(self.lock_key, self.talk_key)` — i.e. re-applies the new keys, not the old —
  and retries registration of the same failing combo.
- **Impact:** after a failed Preferences hotkey change, both global-hotkey registrations are gone
  while AppState claims the new keycodes. Severity tempered: the tap path (`mod.rs:27-42`)
  intercepts the lock combo independently off `state.get_lock_keycode()`, so locking still works;
  the global-hotkey listener is redundant while the tap lives. In-memory vs on-disk drift and dead
  redundant path remain. Fix: snapshot old `Code`s before the swap.

## N11 — Low findings (verified, listed for completeness)

- **L1** `set_locked(false)` emergency paths (permission monitor, `lib.rs:670-672, 726-728`)
  leave `input_buffer`/`last_key_time`/`talk_key_pressed` stale — hygiene gap vs
  `complete_passphrase_unlock`/`reset_all` which clear them (`app_state.rs:234-313`). Mitigated by
  the 250 ms buffer-reset thread.
- **L2** Verify→unlock race (`mod.rs:118-124`): guard dropped before `complete_passphrase_unlock`;
  a same-instant auto-unlock fire consumes a backoff window before the reset. Harmless today
  (complete resets the counter), fragile invariant.
- **L3** Hotkey listener thread captures hotkey IDs once (`lib.rs:617-640`); after
  `reregister_hotkeys` it listens for stale IDs. Invisible while the tap path works; dead weight.
- **L4** Keycode buffers never zeroed: capture `Vec<u32>` (`setup.rs:366-372, 563-576`), wizard
  change-passphrase `first`/`second` (`wizard.rs:1849+`), `input_buffer.clear()` leaves keycodes
  in the freed allocation. No `zeroize` anywhere. Contradicts the S-1/S-2 secrecy posture; LOW
  (heap-scan required).
- **L5** Auto-lock can fire mid-capture: the capture tap swallows keys before the main tap sees
  them, so `last_input_time` goes stale during a wizard capture and the 180 s auto-lock can
  engage with a dialog open (`setup.rs:497-499`, `lib.rs:540-569`). Recoverable (old passphrase
  still unlocks) but confusing.
- **L6** Reset flow leaves the old core fully armed (tap + hotkeys + old hash in memory) for the
  entire post-wipe wizard (`handsoff-tray.rs:1031-1085`); a crash mid-wizard leaves a running
  instance enforcing a passphrase no longer on disk. Self-heals on next launch.
- **L7** `enable()` partial failure (`lib.rs:455-473`): if `restart_event_tap` fails,
  `set_disabled(false)` never runs — app stuck disabled until another Reenable. The permission
  monitor's restart path recovers eventually.
- **L8** CLI binary has **no** single-instance guard (flock is tray-only): two concurrent
  `handsoff` CLI processes each install a filter tap. Dev-tool surface, hence LOW.
- **L9** Two-tier config writes: `save_to_path` (`config_file.rs:414-421`) and non-unix
  `persist_to` apply no permission enforcement. No exploitable callsite today; latent.
- **Dropped — reported by audit, rejected on verification:** "event-tap state Box freed before
  CFRunLoop-thread join = UAF" (`lib.rs:339-364`). The premise assumed the CFRunLoop thread can be
  inside the tap callback; the prior report's verified threading model shows callbacks and
  teardown are serialized on the main thread, so after `remove_event_tap_from_runloop` returns, no
  callback can be in flight. This is the same thread-identity invariant as prior H2, not a new UAF.

## Areas audited and found clean

- **KeyUp leak while locked:** none — KeyUp explicitly blocked (`mod.rs:80-82`), mask includes it;
  FlagsChanged not in mask so modifier state can't desync the (keycode-only) match.
- **Backoff math:** saturating, shift clamped, ceiling applied (`app_state.rs:198-200`); reset
  conditions match spec §2.3; cannot be bypassed by re-locking; anchor guard can't rewind a fired
  schedule.
- **Env/config precedence:** env > CLI > config > default verified for auto-lock, auto-unlock,
  hotkeys; invalid env values fall back, never crash; lock≠talk enforced at every entry point.
- **Config parsing:** strict validation with re-setup guidance; no reachable panics
  (`.expect()`s all downstream of same-chain validation); no path traversal (fixed names under
  `dirs::config_dir()`).
- **Min-length/hash-shape:** <4-key sequences rejected at construction and capture; empty-sequence
  digest rejected at load; hash compare is XOR-fold over fixed-length hex (documented tradeoff).
- **CLI signal/exit semantics:** no SIGINT handler, but the tap dies with the process — exit can
  never leave input blocked. Ctrl+C during capture is the in-tap abort chord, not a mid-tap
  signal hazard.
- **Wizard window/capture lifecycle (ee5196a):** every exit path orders out the window and tears
  down the capture tap; single-dialog invariant holds; deferred-event consumption consistent.
- **Flock mechanics themselves:** kernel-released on crash, no stale-lockfile, no TOCTOU — the
  defect is N1 (successor never re-acquires), not the lock.
- **Installer surface:** no launchd plists/keepalive/uninstall scripts exist; login item is
  SMAppService via wizard checkbox. Nothing to audit there.
- **launchd restart loops / tap stacking from the permission monitor:** flags consumed once via
  `*_and_clear()`; `restart_event_tap` stops an existing tap first. Clean.

## Priority order

| # | Finding | Severity | Surface |
|---|---------|----------|---------|
| N1 | Flock never re-acquired after self-relaunch → duplicate instances | HIGH | tray |
| N3 | CLI never re-enables tap after timeout → permanent silent unlock | HIGH | CLI |
| N5 | Reset/Change-Passphrase reachable unauthenticated in dead-tap windows; tooltip says LOCKED while input flows | MEDIUM | tray |
| N6 | Change Passphrase requires no old-passphrase knowledge | MEDIUM | tray |
| N7 | Unsalted fast hash + 0644 write window + warn-only perms | MEDIUM | both |
| N8 | Legacy backoff configs break Preferences/Change-Passphrase | MEDIUM | both |
| N2 | Deferred Disable+Lock → latent locked-without-tap state (UI honestly shows DISABLED) | MEDIUM | tray |
| N9 | Talk-hotkey transform leaks while unlocked | MEDIUM-LOW | both |
| N10 | reregister_hotkeys restores the new (failing) keys | LOW-MEDIUM | both |
| N4 | Unbounded buffer growth, no cap (quantified: tap-timeout unreachable) | LOW | both |

Recommended first fixes, in order: **N1** (one-line-class fix, closes the incident recurrence),
**N3** (small CLI poll addition), **N5+N6** (explicit `is_locked`/verification gates on Reset and
Change Passphrase — one seam, closes the unauthenticated re-key/wipe class), then **N2** (gate
deferred dispatch + couple `is_locked` to tap liveness in `disable()`). N4 is a cheap hygiene fix
(length-gated verify + buffer cap) worth bundling with any `input_blocking/mod.rs` change.
