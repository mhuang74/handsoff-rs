# CPU spin fix — review + release plan v1.1

Supersedes the fix/process sections of `cpu-spin-cfrunloop-thread-2026-10.md` (v1.0, unchanged).
v1.0's symptom/evidence/root-cause sections stand; they are re-stated here only where corrected.
Date: 2026-10-05 · Target release: **v0.9.1** · Vehicle: patch release on `main` · Tree: clean at `72260c7`

**Amendments (review-advisory pass):** (1) CHANGELOG is release-owned — the workflow prepends `## [0.9.1]`; no
hand-written pre-tag block (the duplicate `## [0.9.0]` headers were *by design*, not a hazard); measurements and
the re-grant note go in the PR body + post-release prose. (2) `Cargo.toml` version bump added — both
`[package].version` and `[package.metadata.bundle].version`, else the app self-reports 0.9.0. (3) Rollout/re-grant
section added — ADR 0001's CDHash-invalidates-grant masquerades as a regression; smoke must run on a fresh grant.
(4) `constants.rs:88` dangling cross-ref to the deleted constant added to the edit list.

## Verdict on v1.0

**Ship it.** The diagnosis is correct and the fix (delete the thread) is right-sized. Every code claim I
could check against `@72260c7` holds; the spin arithmetic is sound; the blast radius of the deletion is
as small as v1.0 says. Three corrections below (none change the fix), one real measurement-baseline gap,
and the release mechanics v1.0 left unspecified.

## Verification of v1.0's claims (all confirmed against code)

| v1.0 claim | Verified |
|---|---|
| Thread loop body at `lib.rs:311-331`, spins on `run_in_mode` | ✓ exact (`start_cfrunloop_thread` 299–338) |
| Tap source added to caller's (main) run loop, not the thread's | ✓ `event_tap.rs:412` `CFRunLoop::get_current().add_source`, called from main thread |
| `cfrunloop_thread` field at 75, init at 93 | ✓ |
| No test references cfrunloop | ✓ `grep cfrunloop tests/` no matches |
| No code reads the thread's existence | ✓ field is write-only (spawn/stop); nothing branches on `cfrunloop_thread.is_some()` as logic |
| Constant at `constants.rs:59` | ✓; "Recommended range" doc comment is at 54–58 (see line ref below) |
| `is_ok()` shutdown check | ✓ `lib.rs:322` — channel is never closed, only `send(())` |

**Thread-count invariant (rules out the one catastrophic failure mode):** `mpsc::Sender` stays alive in
the struct field for the whole thread lifetime, so `try_recv()` → `Err(Empty)`, never `Err(Disconnected)`
— the spin thread can never escape its shutdown check. The `is_some()` guard (299–302) + every
disable/reenable path routing through `stop_event_tap` (which always stops the thread) means **at most one
spin thread exists**, never N. `restart_event_tap` → `stop_event_tap` → `stop_cfrunloop_thread` →
`start_event_tap` → `start_cfrunloop_thread`. Multiple spinning cores is not possible. (Not tested on-CPU;
static read, high confidence.)

**Sleep/wake edge the sample didn't measure:** the sample was steady-state enabled-idle, so it can't speak to
whether spin threads accumulate across sleep/wake. They don't — the timeout-re-enable path
(`service_tap_lifecycle` → `reenable_event_tap` → `event_tap::reenable_existing_tap`, `event_tap.rs:439`) only
calls `CGEventTapEnable` on the existing handle and never touches `start_cfrunloop_thread`, and the permission-
restore fallback (`restart_event_tap`) always stops the old thread before starting a new one via the
`is_some()` guard. So no spin accumulation on wake either.

## Corrections to v1.0 (do not change the fix)

1. **Stale line refs.** v1.0 cites `start_event_tap` at `lib.rs:363` and `stop_event_tap` at `400`; current
   tree has them at **361** and **378–403** (thread-start call 363–364 inside the former, thread-stop call
   401–402 inside the latter). Cosmetic, but pins must be exact for a fix spec.
2. **`stop_event_tap` ref.** Symptom section cites `469-482`; actual is **378–403**. (`disable()` is 470–482 —
   it merely *calls* `stop_event_tap`; v1.0 off-by-one'd the indirection.)
3. **`enable_event_tap` line.** v1.0 cites `event_tap.rs:412` — correct.

### New line-reference table for the edit

| Action | Location (`@72260c7`) |
|---|---|
| Delete `start_cfrunloop_thread` | `lib.rs:299-338` (doc comment `297-298` goes too) |
| Delete `stop_cfrunloop_thread` | `lib.rs:340-358` |
| Remove call + comment in `start_event_tap` | `lib.rs:362-364` |
| Remove call + comment in `stop_event_tap` | `lib.rs:401-402` |
| Remove field + doc comment | `lib.rs:74-75` |
| Remove `None` initializer | `lib.rs:93` |
| Remove constant + doc comment | `constants.rs:54-59` |
| Fix dangling cross-ref: "Recommended range: 100-1000 (same as CFRUNLOOP_POLL_INTERVAL_MS)" | `constants.rs:88` — references the deleted constant; update to a standalone range note in the same pass |
| Bump crate version 0.9.0 → 0.9.1 | `Cargo.toml:3` (`[package].version`) **and** `Cargo.toml:51` (`[package.metadata.bundle].version`) — both populate `CFBundleShortVersionString`/`CFBundleVersion` via `cargo bundle`; no workflow step overrides them. Plus `Cargo.lock` |
| Clean import `CFRUNLOOP_POLL_INTERVAL_MS` | `lib.rs:18-22` (constants use-block) |
| Clean `mpsc`/`Sender` import | `lib.rs:28` — sole mpsc user in `src/`; channel is created only at 305, so the whole line goes |
| Trim `JoinHandle` from thread import | `lib.rs:30` → `use std::thread::{self};` (`self` stays: `thread::spawn` backs buffer-reset/auto-lock/hotkey/auto-unlock/permission-monitor at 622, 641, 681, …) |
| Stale "CFRunLoop thread is now managed by HandsOffCore" comment | `bin/handsoff-tray.rs:244-246` — **v1.0 missed this**; it becomes actively false after the fix and must go |

**Pre-tag CHANGELOG row removed** — the release workflow prepends the `## [0.9.1]` block itself (see "CHANGELOG is
release-owned"), so the edit list carries no CHANGELOG entry before tagging.

**Unused-import fallout (verify at build, don't pre-decide):** `mpsc` and `Sender` (`lib.rs:28`) become unused
once the field/channel go — `stop_cfrunloop_thread` is their only consumer, and `mpsc` appears nowhere else in
`src/`. `JoinHandle` (`lib.rs:30`) is used only in the `cfrunloop_thread` field type and goes too — but
`thread::spawn` backs the other long-lived threads in `lib.rs` (buffer-reset 622, auto-lock 641, hotkey listener
681, auto-unlock 705, permission-monitor 747), so the import becomes `use std::thread::{self}`
minus `JoinHandle`, not deleted outright. The `CFRunLoopRunResult`/`kCFRunLoopDefaultMode`
imports v1.0 worried about are scoped *inside* the thread closure (`lib.rs:309`) and die with it — no global
import to clean. Exact import set to trim is whatever `cargo build --release` + clippy flag; that's the
authority, not this table.

## The one real production hazard in v1.0's framing

**v1.0's risk section undersells the pre-fix failure it fixes and oversells post-fix certainty.**

- It says the "only behavior change: the process no longer burns a core." Also true in reverse: **today the
  process burns a core whenever enabled.** That makes this a genuine high-priority production bug, not polish —
  the fix deserves a dedicated point release, not folding into a feature minor. (Decision taken: v0.9.1.)
- It asserts "input blocking works today with the background loop empty" as proof the main loop can service the
  tap alone. Right inference, and it is also the only behavioral safety net — but note what the `sample` actually
  proves vs. what it doesn't. It shows the **main thread parked in `mach_msg` 99.9%** while the spin thread burns
  a core, which establishes the tap callback does **not** depend on the background loop (the spin thread is busy,
  not servicing the tap, yet input blocking works). What it cannot establish is the counterfactual — that with
  the spin thread *deleted* the main loop wakes as promptly for tap events, because the sampled binary always had
  the spin thread present. The residual risk is not "the tap loses its pump" (the tap never had this pump) but
  subtle wake-on-event behavior on the main loop once the CPU is no longer being kept artificially hot. Judged
  near-impossible, but the on-device functional smoke (step 5) is what closes it: if input is ever **not** blocked
  post-fix, **revert, do not start re-adding pumps**.

## The measurement-baseline gap (the plan's largest hole)

v1.0 asks the user to record "before/after numbers" but the **before-state is already gone** — the fix deletes
the spinning thread, so after the PR there's nothing left to sample the spin from. The 5 s `sample` in v1.0 *is*
the baseline and would be unrecoverable once shipped. Concretely:

- **The pre-fix `sample` output and the Activity Monitor %CPU-enabled-while-idle number must be captured and
  committed to the PR description / this spec before the fix build replaces the running binary.** v1.0 has the
  sample file (`~/tmp/hs-sample.txt`) but never folds its headline numbers into the PR as a citable artifact.
- After-state target is stated (<1% enabled idle) but **no after-state `sample` protocol is pinned** (same 5 s /
  1 ms / same pid-style invocation) — without pinning, before/after aren't comparable.
- **Energy tab is mentioned as a "spot-check."** Make it a recorded before/after number or drop it from the
  protocol; a half-measured energy reading is worse than none for validating the deferred wakeup work.

## Release process gaps in v1.0 (now determined)

v1.0 says "issue first, branch + PR, tag" but leaves the actual mechanics open. Pinned here:

- **Release vehicle: patch v0.9.1 on `main`.** `main` == HEAD == `72260c7` and the working branch
  `minimize_cpu_1005` is **at the same commit** (no divergence, nothing to merge), working tree clean except
  these two spec files. The fix lands as a normal PR onto `main`; the tag then points at that PR's merge
  commit on the default branch. No unreleased v0.10.0 features are queued ahead of it, so nothing unrelated
  ships in the point release.
- **CHANGELOG is release-owned.** `release.yml`'s `changelog` job *prepends* a generated `## [0.9.1]` block on
  release; the repo does **not** add a hand-written `## [0.9.1]` header pre-tag (see "duplicate 0.9.0 headers"
  below). Measurement prose + re-grant note are appended *after* release and carried in the PR body.
- **Tag triggers release.** Per repo memory, tag the CI-verified SHA with `v0.9.1` to fire `release.yml`. The
  tag must point at the merge commit on `main` that CI has already built green.
- **Issue labels.** `ready-for-agent` (v1.0, correct — pure deletion, user-run verification) **plus `bug`** so the
  generated changelog categorizes the fix under `### Fixed` (a `ready-for-agent`-only issue lands in no category).
  Keep the sample summary attached as evidence.

### Pre-existing, explained — duplicate `## [0.9.0]` headers are by design
- **`CHANGELOG.md` has two `## [0.9.0]` blocks (lines 3 and 20). Not a defect.** The `release.yml` `changelog`
  job *prepends* an auto-generated `## [${VERSION}] - ${DATE}` block (from PR titles via
  `mikepenz/release-changelog-builder-action`) above whatever prose the repo already had. Line 3 = the generated
  0.9.0 block; line 20 = the hand-written 0.9.0 prose. Two blocks per version is the established pattern.

- **Consequence: do NOT hand-write a `## [0.9.1]` block.** The workflow prepends its own on release, so a
  hand-written one would duplicate the header. Ownership split:
  - **Generated block owns the `## [0.9.1]` header + the PR-title bullets.** Categorization buckets by label
    (`### Fixed` ← `bug`/`fix`). A `ready-for-agent`-only issue lands in no category; give the issue/PR the
    **`bug` label** so the fix surfaces under `### Fixed`, not uncategorized.
  - **The before/after CPU numbers and the re-grant instruction (below) do not fit a generated one-liner.**
    Carry them in two places the workflow does not overwrite: (a) a **continuous prose section** appended after
    release (the same hand-written-prose-after-generated-block pattern 0.9.0 already uses at line 20), and
    (b) the **PR body** as the pre-tag citable artifact. The generated release body cannot hold them.

- **No CHANGELOG entry before tagging.** Prior "CHANGELOG 0.9.1 entry above line 3" instruction is **rescinded**
  — it collided with the prepending job.

## Rollback (document-only, per decision)

If post-fix on-device smoke shows input blocking regressed (judged near-impossible — the deleted thread servic­ed
nothing — but nonzero):

1. Revert the single PR commit on `main` (`git revert <merge-sha>`) — the diff is a pure deletion, so revert is a
   pure restore and mechanically safe.
2. Delete the `v0.9.1` tag locally + remote before or instead of re-tagging; **do not** re-tag a new SHA as
   `v0.9.1` (release assets are already attached to the tag).
3. If the release has already published, cut `v0.9.2` as the revert rather than mutating `v0.9.1`.

The revert restores the spinning thread verbatim; there is no intermediate degraded state. (Rollback is documented
only — not rehearsed — because a one-commit pure-deletion revert carries no compile risk worth a cycle.)

## Rollout: the re-grant regression-masquerade (ADR 0001)

Every HandsOff update is ad-hoc signed, so the new binary's CDHash differs and **macOS invalidates the existing
Accessibility grant on install** (ADR 0001). Until the user re-grants, the app's permission check fails — and a
CGEventTap without Accessibility **cannot block input**. To a user upgrading to v0.9.1, HandsOff will appear to
"stop blocking" right when the release is supposed to fix a CPU bug. That is the known ADR-0001 consequence, not a
regression this fix introduced — but it *looks* exactly like the failure this PR's smoke test guards against, so
rollout must head it off.

- **Update mechanism matters.** The repo ships a DMG (drag-replace `/Applications/HandsOff.app`); there is no
  notarized auto-updater with a release-notes surface (ADR 0001 defers auto-update). So the actionable re-grant
  instruction lives in the **GitHub release body and the PR body**, not in-app.
- **On launch after update, HandsOff's own stale-grant handling (#34) covers the stuck case**: the permission
  step detects the invalidated grant and surfaces the **Reset Permission & Restart…** escape hatch
  (`tccutil reset Accessibility handsoff-tray.handsoff` + relaunch, re-grant binds to the new build). So the user
  is not dead-ended — but they must *re-grant* for blocking to resume.
- **Release-note / PR-body instruction to ship with v0.9.1:**

  > After installing v0.9.1, macOS revokes the Accessibility permission (every update changes the app signature).
  > Re-enable it in System Settings → Privacy & Security → Accessibility (the app will prompt / offer Reset
  > Permission & Restart on the permission step). Input blocking does not work until re-granted. Fix verified:
  > idle CPU while enabled drops from ~100% (one core) to <1% — see before/after `sample` numbers in this PR.

- **This is also why the functional smoke (step 5) must run *after* a fresh grant on the fix build** — a smoke run
  against a stale-grant build would false-fail "input not blocked" and trigger a spurious revert.

## Determined parameters (user-decided, locking the plan)

- Release: **v0.9.1 patch on main** — branch already sits on `main`'s tip (no divergence); tag points at the fix
  PR's merge commit, no separate branch-to-main merge step.
- Verification: **full user on-device protocol** (v1.0 §Verification), amended below.
- Rollback: **document-only**.

## Amended verification protocol (full, user-run on-device)

Replaces v1.0 §70-78 step 4-5 only; steps 1–3 (build-into-bundle, TCC, functional smoke) unchanged.

4. **Baseline first, before the fix build touches `/Applications`:**
   - `sample handsoff-tray 5 -file ~/tmp/hs-sample-before.txt` while enabled-idle (this pins the pre-fix spin —
     commit the headline numbers into the PR; the live-binary spin is unrecoverable after the upgrade).
   - Activity Monitor %CPU enabled-idle (expect ~100% / one core) and Energy tab — record both.
5. **Fresh grant, then functional smoke.** The fix build invalidates the TCC grant (ADR 0001), so first re-grant
   via `tccutil reset Accessibility handsoff-tray.handsoff` + relaunch + re-grant (v1.0 step 2), then: Lock → input
   blocked → unlock via passphrase → Disable/Reenable still work. This is the pass/fail for behavioral correctness.
   Running it against a stale grant false-fails "input not blocked."
6. **After-state measurement, same protocol as baseline:**
   - `sample handsoff-tray 5 -file ~/tmp/hs-sample-after.txt` enabled-idle — expect **no**
     `start_cfrunloop_thread` thread at top, main thread still parked in `mach_msg`.
   - Activity Monitor %CPU enabled-idle — **target <1%, ~0**; record the actual number.
   - Energy tab — record the number (paired with baseline, so the deferred wakeup-consolidation decision has data).
7. **Record both before/after triples (sample headline, %CPU, Energy) in the PR body** (pre-tag citable artifact);
   prose appended to CHANGELOG **after** release, not a pre-tag `## [0.9.1]` header (workflow would duplicate).

## Acceptance criteria

- [ ] `cargo build --release` green with the thread, field, channel machinery, constant, imports, the
      `bin/handsoff-tray.rs:244-246` stale comment, and the `constants.rs:88` dangling cross-ref removed; clippy
      clean (no unused-import warnings from the trim).
- [ ] **`Cargo.toml` bumped to 0.9.1 in both fields** (`[package].version` line 3, `[package.metadata.bundle].version`
      line 51) + `Cargo.lock` regenerated — so the shipped app self-reports 0.9.1, not 0.9.0.
- [ ] `cargo test` green (CI macos-latest; the deletion touches no tested path, so this guards against accidental
      collateral, not the spin itself).
- [ ] Issue/PR labeled **`bug`** so the generated changelog categorizes it under `### Fixed`.
- [ ] Functional smoke passes (input genuinely blocked while Locked) **against a fresh Accessibility grant on the
      fix build** — the fix's one behavioral net; a stale-grant run false-fails and must not trigger a revert.
- [ ] After-state enabled-idle %CPU <1% and `hs-sample-after.txt` shows no `start_cfrunloop_thread` hot thread.
- [ ] Before/after measurement triples + the ADR-0001 re-grant instruction carried in the **PR body** (pre-tag
      citeable artifact); prose appended to CHANGELOG **after** release (not as a pre-tag `## [0.9.1]` header, which
      the workflow would duplicate).
- [ ] Fix PR merged to `main`, CI green on the merge SHA, `v0.9.1` tagged at that SHA (branch already sits on
      `main`'s tip — no separate branch-to-main merge step).
- [ ] Release body / PR body instructs users to **re-grant Accessibility after install** (ADR 0001 CDHash change).

## Explicitly out of scope (deferred, per v1.0)

Wakeup consolidation — buffer-reset parking (P-3), tray `WaitUntil` → event-driven (C-2), ~8 wakeups/s remaining.
Decide **after** the Energy numbers from step 6 land, not before. Do not bundle into v0.9.1: the point release's
value is a clean before/after CPU attribution, which bundling other wakeup changes would muddy.
