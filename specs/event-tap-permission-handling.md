# Event Tap Permission Handling - Design Specification

**Status:** Draft
**Created:** 2025-11-06
**Author:** Claude Code Investigation

## Table of Contents

1. [Background & Problem Statement](#background--problem-statement)
2. [Core Concepts](#core-concepts)
3. [Current System Analysis](#current-system-analysis)
4. [The Bug: Root Cause Analysis](#the-bug-root-cause-analysis)
5. [Proposed Solutions](#proposed-solutions)
6. [Recommended Solution](#recommended-solution)
7. [Implementation Details](#implementation-details)
8. [Testing Strategy](#testing-strategy)
9. [Future Considerations](#future-considerations)

---

## Background & Problem Statement

### What Are Accessibility Permissions?

On macOS, applications that need to monitor or modify user input (keyboard/mouse events) require **Accessibility Permissions**. This is a security feature that prevents malicious applications from:
- Keylogging (capturing passwords, sensitive data)
- Injecting fake user input
- Controlling other applications without user consent

HandsOff requires these permissions to:
1. **Monitor** keyboard and mouse events to detect user activity
2. **Block** keyboard and mouse events when in locked state
3. **Capture** passphrase input to unlock the application

Without accessibility permissions, HandsOff cannot function.

### What Happens When Permissions Are Removed?

When a user removes accessibility permissions while the app is running (via System Settings > Privacy & Security > Accessibility):

1. **macOS immediately disables the event tap**
   - The event tap is the mechanism HandsOff uses to intercept events
   - macOS sends a special event type `kCGEventTapDisabledByUserInput` (0xFFFFFFFF) to the callback
   - The event tap stops receiving keyboard/mouse events

2. **The application loses all input monitoring capability**
   - Cannot detect user activity (for auto-lock timing)
   - Cannot block keyboard/mouse input (if locked)
   - Cannot capture passphrase input (to unlock)

3. **System behavior becomes unpredictable**
   - If the app is locked, user input goes through (security violation)
   - If the app tries to unlock, keyboard input isn't captured (user can't unlock)
   - Background threads continue running (wasting resources)

### Which Actions/Loops Must Be Suspended?

When accessibility permissions are lost, the following components must be suspended or stopped:

#### 1. Event Tap (CRITICAL - must stop immediately)
- **Location:** `src/input_blocking/event_tap.rs`
- **Why:** The event tap is non-functional without permissions and becomes a zombie process
- **Impact:** Event tap callback receives no events, cannot block input, cannot capture passphrase
- **Action:** Stop event tap, remove from run loop, free resources

#### 2. Auto-Lock Thread (should suspend)
- **Location:** `src/lib.rs:376-408`
- **Why:** Cannot detect user activity without event tap
- **Impact:** May trigger auto-lock based on stale activity data
- **Action:** Could continue running but won't work correctly; better to suspend

#### 3. Auto-Unlock Thread (should suspend)
- **Location:** `src/lib.rs:450-475`
- **Why:** Cannot detect user activity without event tap
- **Impact:** May trigger auto-unlock based on stale activity data
- **Action:** Could continue running but won't work correctly; better to suspend

#### 4. Hotkey Listener Thread (must stop)
- **Location:** `src/lib.rs:421-448`
- **Why:** Cannot detect hotkey presses without event tap
- **Impact:** Wastes resources polling for events that never arrive
- **Action:** Stop listening for hotkeys

#### 5. Permission Monitor Thread (must continue)
- **Location:** `src/lib.rs:479-573`
- **Why:** Must detect when permissions are restored
- **Impact:** This is the only way to know when to restart
- **Action:** Continue running, detect restoration, trigger restart

#### 6. Locked State (must unlock immediately)
- **Location:** `src/app_state.rs`
- **Why:** User input is passing through to system, so "locked" state is false
- **Impact:** User is effectively locked out (can't interact with anything)
- **Action:** Immediately unlock to restore normal system input

**Summary:** When permissions are lost, essentially all input-related functionality must stop, and the app should enter a "dormant" state waiting for permission restoration.

---

## Core Concepts

### What is CFRunLoop?

**CFRunLoop** is a macOS/iOS Core Foundation mechanism for event processing. It's an event loop that:

1. **Waits for events** from various sources:
   - Timer events
   - Input sources (like event taps)
   - Mach ports
   - Custom sources

2. **Dispatches events** to appropriate handlers:
   - When keyboard/mouse event occurs → calls event tap callback
   - When timer fires → calls timer callback
   - When custom source signals → calls custom handler

3. **Manages run loop modes:**
   - Different modes for different event types
   - Can run indefinitely (`run_current()`) or for specific duration (`run_in_mode()`)

#### CFRunLoop in HandsOff

HandsOff uses CFRunLoop in two places:

**1. CFRunLoop Thread** (`src/lib.rs:147-202`)
```rust
fn start_cfrunloop_thread(&mut self) {
    thread::spawn(move || {
        loop {
            let result = unsafe {
                CFRunLoop::run_in_mode(
                    kCFRunLoopDefaultMode,
                    Duration::from_millis(500),
                    false
                )
            };
            // Check for shutdown signal
            if shutdown_rx.try_recv().is_ok() {
                break;
            }
        }
    });
}
```
This thread runs the CFRunLoop in 500ms intervals, allowing graceful shutdown.

**2. Event Tap Integration** (`src/input_blocking/event_tap.rs:53-91`)
```rust
pub fn create_event_tap(...) -> Result<(CGEventTapRef, *mut CallbackState)> {
    // Create event tap
    let tap = unsafe { CGEventTapCreate(...) };

    // Create run loop source from tap
    let source = unsafe {
        CGEventTapCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
    };

    // Add source to CFRunLoop (enables event delivery)
    unsafe {
        CFRunLoopAddSource(
            CFRunLoop::get_current(),
            source,
            kCFRunLoopCommonModes
        );
    }
}
```

The event tap is added as a **source** to the CFRunLoop. When keyboard/mouse events occur, macOS:
1. Delivers events to the event tap
2. CFRunLoop dispatches to the callback function
3. Callback processes event (block/allow/capture)

**When permissions are removed:** macOS sends `0xFFFFFFFF` event through the same CFRunLoop mechanism, then stops delivering normal events.

---

## Current System Analysis

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                        HandsOff App                          │
│  ┌────────────┐              ┌──────────────────────────┐   │
│  │  CLI App   │              │      Tray App            │   │
│  │            │              │                          │   │
│  │ • Blocks   │              │ • Polling loop (500ms)   │   │
│  │   on       │              │ • Checks flags           │   │
│  │   CFRun    │              │ • Updates UI             │   │
│  │   Loop     │              │                          │   │
│  └─────┬──────┘              └──────────┬───────────────┘   │
│        │                                │                   │
│        └────────────┬───────────────────┘                   │
│                     ▼                                       │
│           ┌─────────────────────┐                           │
│           │   HandsOffCore      │                           │
│           │                     │                           │
│           │ • Event Tap         │                           │
│           │ • AppState          │                           │
│           │ • Background        │                           │
│           │   Threads:          │                           │
│           │   - Auto-lock       │                           │
│           │   - Auto-unlock     │                           │
│           │   - Hotkey          │                           │
│           │   - Permission      │                           │
│           │     Monitor         │                           │
│           │ • CFRunLoop Thread  │                           │
│           └─────────┬───────────┘                           │
│                     │                                       │
│                     ▼                                       │
│           ┌─────────────────────┐                           │
│           │   Event Tap         │                           │
│           │   Callback          │                           │
│           │                     │                           │
│           │ • Process events    │                           │
│           │ • Block/allow       │                           │
│           │ • Capture input     │                           │
│           └─────────────────────┘                           │
└─────────────────────────────────────────────────────────────┘
```

### Event Tap Flow

```
macOS System Events
       │
       ▼
┌──────────────────┐
│   Event Tap      │  (registered with macOS)
│   (CGEventTap)   │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│  CFRunLoop       │  (dispatches events to callback)
│  Dispatch        │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│  Event Tap       │  Match event_type:
│  Callback        │    • KeyDown → process/block
│                  │    • MouseMoved → process/block
└────────┬─────────┘    • ??? → ignore (BUG!)
         │
         ├─── Return NULL (block event)
         └─── Return event (allow event)
```

### Permission Monitor Flow (Current)

```
┌─────────────────────────────────────────────────────────┐
│  Permission Monitor Thread (polls every 15 seconds)    │
└─────────────────────┬───────────────────────────────────┘
                      │
                      ▼
         ┌────────────────────────┐
         │ Check permissions      │
         └────────┬───────────────┘
                  │
         ┌────────┴────────┐
         │                 │
         ▼                 ▼
    ┌────────┐      ┌──────────┐
    │  LOST  │      │ RESTORED │
    └───┬────┘      └─────┬────┘
        │                 │
        ▼                 ▼
┌───────────────┐  ┌──────────────────┐
│ • Unlock if   │  │ • Show notif:    │
│   locked      │  │   "Use Reset     │
│ • Set flag:   │  │    menu"         │
│   should_stop │  │ • No automatic   │
│   _event_tap  │  │   restart        │
└───────┬───────┘  └──────────────────┘
        │
        ▼
┌──────────────────────┐
│  Main Loop (Tray)    │
│  Checks flag every   │
│  500ms and stops     │
│  event tap           │
└──────────────────────┘

Main Loop (CLI) - NEVER CHECKS FLAG (BUG!)
```

### CLI vs Tray App Differences

| Aspect | CLI App | Tray App |
|--------|---------|----------|
| **Main Loop** | `CFRunLoop::run_current()` - blocks forever | Polling loop - wakes every 500ms |
| **Flag Checking** | ❌ Never checks flags | ✅ Checks `should_stop_event_tap` |
| **UI Updates** | None | Menu state, icons |
| **Shutdown** | Ctrl+C signal | Menu quit option |
| **Files** | `src/bin/handsoff.rs` | `src/bin/handsoff-tray.rs` |
| **Lines** | Main loop: 219-222 | Main loop: 216-320 |

**Critical Difference:** CLI blocks indefinitely and cannot respond to permission loss signals.

---

## The Bug: Root Cause Analysis

### Bug Reproduction Steps

1. Start HandsOff CLI with auto-lock and auto-unlock configured
2. Wait for app to be in unlocked state
3. Open System Settings > Privacy & Security > Accessibility
4. Remove HandsOff from the allowed apps list
5. Observe logs show "Tap was removed because of missing permissions"
6. Wait for auto-lock to trigger (~20-30 seconds of inactivity)
7. Wait for auto-unlock to trigger (~60 seconds after lock)
8. **BUG:** Keyboard and mouse remain locked out
9. **BUG:** Typing passphrase has no effect - keyboard input ignored

### What Actually Happens (Timeline)

```
T=0s    User removes accessibility permissions
        ↓
        macOS sends event type 0xFFFFFFFF to callback
        ↓
        Callback doesn't recognize this event type
        ↓
        Falls through to default case: return event
        ↓
        EVENT TAP ENTERS ZOMBIE STATE (disabled but not stopped)

T=0-15s Permission monitor hasn't polled yet
        ↓
        Event tap is disabled (not receiving events)
        ↓
        But app state shows "unlocked" and "running normally"
        ↓
        Background threads still running

T=20s   User stops typing (inactivity begins)
        ↓
        But event tap not receiving events, so activity buffer not updating
        ↓
        Last recorded activity was at T=0s (before permission loss)

T=30s   Auto-lock thread checks: 30s since last activity
        ↓
        Triggers auto-lock
        ↓
        Sets is_locked = true
        ↓
        But event tap is still disabled (zombie state)

T=90s   Auto-unlock thread checks: 60s since lock
        ↓
        Triggers auto-unlock
        ↓
        Sets is_locked = false
        ↓
        But event tap is still disabled (zombie state)

T=90s+  User tries to type (expecting normal input)
        ↓
        But event tap is disabled (no events delivered to callback)
        ↓
        Keyboard input goes to... nowhere? System? Lost?
        ↓
        USER IS EFFECTIVELY LOCKED OUT

T=0-15s Eventually permission monitor polls
        ↓
        Detects permission loss
        ↓
        Sets should_stop_event_tap flag
        ↓
        But CLI never checks this flag (no polling loop)
        ↓
        Tray checks flag and stops event tap (after 15+ seconds)
```

### Root Causes

1. **Event tap callback doesn't handle special event types**
   - `0xFFFFFFFF` (kCGEventTapDisabledByUserInput) not recognized
   - `0xFFFFFFFE` (kCGEventTapDisabledByTimeout) not recognized
   - Callback ignores these events → tap remains in zombie state

2. **Permission monitor has 15-second polling interval**
   - Delay between permission loss and detection
   - During this window, auto-lock/unlock can trigger
   - Creates race condition

3. **CLI app never checks flags**
   - Permission monitor sets `should_stop_event_tap` flag
   - But CLI blocks on `CFRunLoop::run_current()`
   - Never reads the flag → event tap never stopped

4. **State vs Reality mismatch**
   - `is_locked` flag can be false (unlocked)
   - But event tap is disabled (not receiving events)
   - User typing has no effect (disabled tap drops events)

### Why Keyboard Input Is Ignored

```
User types passphrase
       │
       ▼
macOS keyboard event generated
       │
       ▼
Event tap (disabled) - DROPS EVENT
       │
       X  (event never delivered to callback)

Callback never called
       │
       X  (passphrase input never captured)

App never processes input
       │
       X  (app thinks it's unlocked anyway)
```

The disabled event tap is like a black hole - events go in but never come out.

---

## Proposed Solutions

### Solution 1: Auto-Restart on Permission Restoration (Full Automation)

**Philosophy:** Treat permission loss/restoration as symmetric operations. Automatic disable → automatic re-enable.

**Changes Required:**

1. **Event tap callback handles special events**
   - Detect `0xFFFFFFFF` and `0xFFFFFFFE`
   - Immediately set `should_stop_event_tap` flag
   - Eliminates 15-second delay

2. **Add start flag to AppState**
   - `should_start_event_tap` (symmetric to stop flag)
   - `request_start_event_tap()` method
   - `should_start_event_tap_and_clear()` method

3. **Permission monitor triggers restart**
   - On permission restoration, set `should_start_event_tap` flag
   - Update notification: "Restarting automatically..."

4. **CLI app gets polling loop**
   - Replace `CFRunLoop::run_current()` with polling loop
   - Check both stop and start flags
   - Call `stop_event_tap()` or `restart_event_tap()` accordingly

5. **Tray app checks start flag**
   - Add flag check (symmetric to existing stop check)
   - Call `restart_event_tap()` when flag set

**Pros:**
- ✅ Fully automatic - no user intervention required
- ✅ Symmetric architecture (clean design)
- ✅ Immediate response to permission loss (0ms vs 15s)
- ✅ App returns to working state automatically

**Cons:**
- ⚠️ More complex implementation
- ⚠️ Requires CLI architectural change (polling loop)
- ⚠️ Auto-restart might fail if permissions revoked again quickly
- ⚠️ User might be surprised by automatic restart

**Files Changed:**
- `src/input_blocking/event_tap.rs` (callback)
- `src/app_state.rs` (add start flag)
- `src/lib.rs` (permission monitor sets start flag)
- `src/bin/handsoff.rs` (add polling loop - MAJOR CHANGE)
- `src/bin/handsoff-tray.rs` (add start flag check - minor)

---

### Solution 2: Exit CLI on Permission Loss (Simple)

**Philosophy:** Without permissions, CLI cannot function. Exit cleanly rather than entering broken state.

**Changes Required:**

1. **Event tap callback handles special events**
   - Detect `0xFFFFFFFF` and `0xFFFFFFFE`
   - Immediately set `should_stop_event_tap` flag

2. **Add exit flag to AppState**
   - `should_exit: bool`
   - `request_exit()` method
   - `should_exit_and_clear()` method

3. **Permission monitor triggers exit (CLI only)**
   - On permission loss, check if running as CLI
   - If CLI: set `should_exit` flag
   - If Tray: use existing `should_stop_event_tap` flag

4. **CLI app gets minimal polling loop**
   - Replace `CFRunLoop::run_current()` with polling loop
   - Check `should_exit` flag
   - Exit with error message if flag set
   - No need to check start flag (won't restart)

5. **Tray app unchanged**
   - Continues using existing stop logic
   - Shows "Use Reset menu" notification

**Pros:**
- ✅ Simple implementation
- ✅ Clear semantics - CLI exits when it can't work
- ✅ No zombie state (process terminates)
- ✅ Immediate response to permission loss
- ✅ Matches CLI philosophy (run until can't run)

**Cons:**
- ⚠️ User must manually restart CLI after restoring permissions
- ⚠️ Different behavior for CLI vs Tray (but that's okay)
- ⚠️ Still requires CLI architectural change (polling loop)

**Files Changed:**
- `src/input_blocking/event_tap.rs` (callback)
- `src/app_state.rs` (add exit flag)
- `src/lib.rs` (permission monitor sets exit flag for CLI)
- `src/bin/handsoff.rs` (add polling loop, check exit flag - MODERATE CHANGE)

**Exit Message:**
```
ERROR: Accessibility permissions were revoked.
HandsOff cannot function without accessibility permissions.

To restore:
1. Open System Settings > Privacy & Security > Accessibility
2. Enable HandsOff in the list
3. Restart HandsOff CLI

Exiting...
```

---

### Solution 3: Keep Manual Reset (Minimal Changes)

**Philosophy:** Current design is intentional. Just fix the zombie state, keep manual reset flow.

**Changes Required:**

1. **Event tap callback handles special events**
   - Detect `0xFFFFFFFF` and `0xFFFFFFFE`
   - Immediately set `should_stop_event_tap` flag

2. **CLI app gets minimal polling loop**
   - Replace `CFRunLoop::run_current()` with polling loop
   - Check `should_stop_event_tap` flag only
   - Call `stop_event_tap()` when flag set
   - No automatic restart

3. **Tray app unchanged**
   - Already checks stop flag
   - Already shows "Use Reset menu" notification

**Pros:**
- ✅ Minimal changes
- ✅ Preserves current manual reset UX
- ✅ Immediate response to permission loss
- ✅ No change to permission restoration flow

**Cons:**
- ⚠️ User must still manually reset after restoring permissions
- ⚠️ Asymmetric design (auto-stop, manual-start)
- ⚠️ Still requires CLI architectural change (polling loop)

**Files Changed:**
- `src/input_blocking/event_tap.rs` (callback)
- `src/bin/handsoff.rs` (add polling loop - MODERATE CHANGE)

---

### Solution 4: Shared Main Loop (Architectural)

**Philosophy:** Both CLI and Tray should use the same event loop implementation to avoid divergence.

**Changes Required:**

1. **Create shared event loop in HandsOffCore**
   ```rust
   // In src/lib.rs
   pub fn run_event_loop<F>(&mut self, on_tick: F) -> Result<()>
   where
       F: Fn(&HandsOffCore) -> bool  // Returns true to continue
   {
       loop {
           // Run CFRunLoop briefly
           unsafe {
               CFRunLoop::run_in_mode(
                   kCFRunLoopDefaultMode,
                   Duration::from_millis(500),
                   false
               );
           }

           // Check stop flag
           if self.state.should_stop_event_tap_and_clear() {
               self.stop_event_tap();
           }

           // Check start flag (if auto-restart enabled)
           if self.state.should_start_event_tap_and_clear() {
               let _ = self.restart_event_tap();
           }

           // Call app-specific logic
           if !on_tick(self) {
               break;
           }
       }
       Ok(())
   }
   ```

2. **CLI uses shared loop**
   ```rust
   // In src/bin/handsoff.rs
   core.run_event_loop(|_| true)?;  // Just run forever
   ```

3. **Tray uses shared loop**
   ```rust
   // In src/bin/handsoff-tray.rs
   core.run_event_loop(|core| {
       // Tray-specific logic: update menu, handle UI events
       // Return false to exit
   })?;
   ```

4. **Event tap callback handles special events** (same as other solutions)

5. **Add start flag to AppState** (if auto-restart desired)

**Pros:**
- ✅ Eliminates code duplication
- ✅ Ensures both apps have same behavior
- ✅ Easy to add features (just add to shared loop)
- ✅ Centralized flag checking logic

**Cons:**
- ⚠️ Large architectural change
- ⚠️ Requires refactoring both apps
- ⚠️ Tray app's event loop is more complex (UI events)

**Files Changed:**
- `src/lib.rs` (add `run_event_loop` method)
- `src/bin/handsoff.rs` (use shared loop - MAJOR CHANGE)
- `src/bin/handsoff-tray.rs` (use shared loop - MAJOR CHANGE)
- `src/input_blocking/event_tap.rs` (callback)
- `src/app_state.rs` (add start flag if auto-restart desired)

---

## Recommended Solution

### Recommendation: **Solution 2 (Exit CLI) + Solution 1 (Auto-Restart Tray)**

**Rationale:**

1. **CLI and Tray have different use cases:**
   - CLI: Lightweight, runs in foreground, user is actively monitoring
   - Tray: Long-running, background, set-it-and-forget-it

2. **CLI: Exit on permission loss**
   - User is already at terminal, can see exit message
   - Easy to restart after fixing permissions: `handsoff`
   - Matches Unix philosophy: fail fast, fail loudly
   - Prevents zombie process state

3. **Tray: Auto-restart on permission restoration**
   - User might restore permissions hours later
   - Auto-restart provides seamless recovery
   - Matches desktop app expectations
   - User doesn't need to remember to use Reset menu

4. **Implementation complexity is balanced:**
   - Both apps need polling loop anyway (fix CLI architecture)
   - Exit logic simpler than auto-restart
   - Tray auto-restart leverages existing `restart_event_tap()`

### Implementation Priority

**Phase 1: Fix Critical Bug (Both Apps)**
1. Event tap callback handles `0xFFFFFFFF` / `0xFFFFFFFE` events
2. CLI gets polling loop that checks `should_exit` flag
3. Tray already checks `should_stop_event_tap` flag
4. **Result:** Immediate response to permission loss, no zombie state

**Phase 2: Improve UX (Tray Only)**
5. Add `should_start_event_tap` flag to AppState
6. Permission monitor sets flag on restoration
7. Tray polling loop checks flag and calls `restart_event_tap()`
8. **Result:** Tray auto-restarts, CLI exits cleanly

---

## Implementation Details

### Phase 1: Event Tap Callback

**File:** `src/input_blocking/event_tap.rs`
**Location:** Lines 94-200 (event_tap_callback function)

**Current Code:**
```rust
unsafe extern "C" fn event_tap_callback(
    _proxy: CGEventTapRef,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    if user_info.is_null() {
        return event;
    }

    let state = &*(user_info as *const CallbackState);

    match event_type {
        kCGEventKeyDown => { /* ... */ }
        kCGEventKeyUp => { /* ... */ }
        // ... other event types ...
        _ => {
            // BUG: Unknown event types are ignored
            // This includes 0xFFFFFFFF (disabled by user input)
            false
        }
    }

    // Return event or null based on result...
}
```

**Updated Code:**
```rust
unsafe extern "C" fn event_tap_callback(
    proxy: CGEventTapRef,  // Need proxy to re-enable tap
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    // Constants for special event types
    const K_CGEVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFFFFFE;
    const K_CGEVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFFFFFF;

    // Handle event tap disabled events FIRST (before null check)
    if event_type == K_CGEVENT_TAP_DISABLED_BY_TIMEOUT
        || event_type == K_CGEVENT_TAP_DISABLED_BY_USER_INPUT
    {
        let reason = if event_type == K_CGEVENT_TAP_DISABLED_BY_USER_INPUT {
            "user removed accessibility permissions"
        } else {
            "timeout (system was too slow)"
        };

        warn!(
            "Event tap disabled by macOS (0x{:X}): {}",
            event_type,
            reason
        );

        // Try to re-enable the tap (may fail if permissions gone)
        // This is a no-op if permissions were removed, but helps with timeout case
        CGEventTapEnable(proxy, true);

        // Set flag to stop event tap via main loop
        // (only if we have valid user_info)
        if !user_info.is_null() {
            let state = &*(user_info as *const CallbackState);

            // For timeout: try to continue (tap might re-enable)
            // For user input: definitely stop (permissions gone)
            if event_type == K_CGEVENT_TAP_DISABLED_BY_USER_INPUT {
                state.app_state.request_stop_event_tap();

                // Also request exit if CLI app
                #[cfg(feature = "cli-exit-on-permission-loss")]
                state.app_state.request_exit();
            }
        }

        // Return event unmodified (these are system events)
        return event;
    }

    if user_info.is_null() {
        return event;
    }

    let state = &*(user_info as *const CallbackState);

    // ... rest of normal event handling ...
}
```

**Notes:**
- Added constants for special event types
- Check for special events BEFORE null check (might receive these without user_info)
- Attempt to re-enable tap (helps with timeout, no-op for permission loss)
- Set `should_stop_event_tap` flag to trigger graceful shutdown
- Log clear reason for disablement
- Feature flag for CLI exit behavior

### Phase 1: AppState Changes

**File:** `src/app_state.rs`

**Add to AppStateInner struct:**
```rust
pub struct AppStateInner {
    // ... existing fields ...

    /// Flag to signal that event tap should be stopped
    pub should_stop_event_tap: bool,

    /// Flag to signal that app should exit (CLI only)
    pub should_exit: bool,

    // ... other fields ...
}
```

**Add methods:**
```rust
impl AppState {
    // ... existing methods ...

    /// Request that the application exit (CLI only)
    pub fn request_exit(&self) {
        let mut state = self.inner.lock();
        state.should_exit = true;
    }

    /// Check if app should exit, and clear the flag
    pub fn should_exit_and_clear(&self) -> bool {
        let mut state = self.inner.lock();
        let should_exit = state.should_exit;
        state.should_exit = false;
        should_exit
    }
}
```

### Phase 1: CLI Polling Loop

**File:** `src/bin/handsoff.rs`
**Location:** Lines 219-222

**Current Code:**
```rust
info!("Starting CFRunLoop (required for event interception)...");
use core_foundation::runloop::CFRunLoop;
CFRunLoop::run_current();

// CFRunLoop::run_current() runs indefinitely, so this is unreachable
#[allow(unreachable_code)]
Ok(())
```

**Updated Code:**
```rust
info!("Starting event loop (required for event interception)...");

use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
use std::time::Duration;

// Main event loop - polls every 500ms
loop {
    // Run CFRunLoop for a brief period to process events
    unsafe {
        CFRunLoop::run_in_mode(
            kCFRunLoopDefaultMode,
            Duration::from_millis(500),
            false  // Don't return after single source handled
        );
    }

    // Check if we should exit (permission loss or shutdown signal)
    if core.state.should_exit_and_clear() {
        warn!("Accessibility permissions lost - exiting");
        eprintln!("\nERROR: Accessibility permissions were revoked.");
        eprintln!("HandsOff cannot function without accessibility permissions.\n");
        eprintln!("To restore:");
        eprintln!("1. Open System Settings > Privacy & Security > Accessibility");
        eprintln!("2. Enable HandsOff in the list");
        eprintln!("3. Restart HandsOff CLI\n");
        eprintln!("Exiting...");

        // Clean shutdown
        core.stop_event_tap();
        break;
    }

    // Check if event tap should be stopped (fallback)
    if core.state.should_stop_event_tap_and_clear() {
        warn!("Stopping event tap");
        core.stop_event_tap();

        // For CLI, if event tap stops, we should exit
        eprintln!("\nEvent tap stopped. Exiting...");
        break;
    }
}

info!("CLI shutdown complete");
Ok(())
```

**Notes:**
- Replaces blocking `run_current()` with polling loop
- Checks `should_exit` flag (set by callback on permission loss)
- Checks `should_stop_event_tap` flag (fallback, set by permission monitor)
- Provides clear error message to user
- Performs clean shutdown

### Phase 2: Tray Auto-Restart Flag

**File:** `src/app_state.rs`

**Add to AppStateInner struct:**
```rust
pub struct AppStateInner {
    // ... existing fields ...

    /// Flag to signal that event tap should be started
    pub should_start_event_tap: bool,

    // ... other fields ...
}
```

**Add methods:**
```rust
impl AppState {
    // ... existing methods ...

    /// Request that event tap be started (after permission restoration)
    pub fn request_start_event_tap(&self) {
        let mut state = self.inner.lock();
        state.should_start_event_tap = true;
    }

    /// Check if event tap should be started, and clear the flag
    pub fn should_start_event_tap_and_clear(&self) -> bool {
        let mut state = self.inner.lock();
        let should_start = state.should_start_event_tap;
        state.should_start_event_tap = false;
        should_start
    }
}
```

### Phase 2: Permission Monitor Auto-Restart

**File:** `src/lib.rs`
**Location:** Lines 554-565 (permission restoration detection)

**Current Code:**
```rust
// Detect permission restoration
else if !last_permission_state && has_permissions {
    info!("Accessibility permissions have been restored");

    #[cfg(target_os = "macos")]
    {
        let _ = notify_rust::Notification::new()
            .summary("HandsOff - Permissions Restored")
            .body("Accessibility permissions restored.\n\nUse Reset menu to restart event tap.")
            .timeout(notify_rust::Timeout::Milliseconds(5000))
            .show();
    }
}
```

**Updated Code:**
```rust
// Detect permission restoration
else if !last_permission_state && has_permissions {
    info!("Accessibility permissions have been restored");

    // Request automatic restart (Tray app will handle this)
    state.request_start_event_tap();

    #[cfg(target_os = "macos")]
    {
        let _ = notify_rust::Notification::new()
            .summary("HandsOff - Permissions Restored")
            .body("Accessibility permissions restored.\n\nRestarting event tap automatically...")
            .timeout(notify_rust::Timeout::Milliseconds(5000))
            .show();
    }
}
```

### Phase 2: Tray Auto-Restart Check

**File:** `src/bin/handsoff-tray.rs`
**Location:** After line 256 (after stop flag check)

**Add:**
```rust
// Check if event tap should be stopped (due to permission loss)
{
    let mut core_lock = core.lock().unwrap();
    if core_lock.state.should_stop_event_tap_and_clear() {
        warn!("Tray: Stopping event tap due to permission loss");
        core_lock.stop_event_tap();
        info!("Tray: Event tap stopped - normal input restored");
    }
}

// NEW: Check if event tap should be started (permission restored)
{
    let mut core_lock = core.lock().unwrap();
    if core_lock.state.should_start_event_tap_and_clear() {
        info!("Tray: Restarting event tap - permissions restored");
        match core_lock.restart_event_tap() {
            Ok(()) => {
                info!("Tray: Event tap restarted successfully");

                #[cfg(target_os = "macos")]
                {
                    let _ = notify_rust::Notification::new()
                        .summary("HandsOff - Event Tap Restarted")
                        .body("Event tap restarted successfully.\nHandsOff is now active.")
                        .timeout(notify_rust::Timeout::Milliseconds(3000))
                        .show();
                }
            }
            Err(e) => {
                warn!("Tray: Failed to restart event tap: {}", e);

                #[cfg(target_os = "macos")]
                {
                    let _ = notify_rust::Notification::new()
                        .summary("HandsOff - Restart Failed")
                        .body(&format!(
                            "Failed to restart event tap: {}\n\nUse Reset menu to try again.",
                            e
                        ))
                        .timeout(notify_rust::Timeout::Milliseconds(5000))
                        .show();
                }
            }
        }
    }
}
```

---

## Testing Strategy

### Manual Testing Scenarios

#### Test 1: Permission Loss While Unlocked (CLI)
1. Start CLI: `handsoff`
2. Verify event tap is active (logs show "Event tap started")
3. Open System Settings > Privacy & Security > Accessibility
4. Remove HandsOff from allowed apps
5. **Expected:**
   - Logs show "Event tap disabled by macOS (0xFFFFFFFF): user removed accessibility permissions"
   - CLI prints error message about permission loss
   - CLI exits cleanly
   - Normal keyboard/mouse input works immediately

#### Test 2: Permission Loss While Locked (CLI)
1. Start CLI: `handsoff`
2. Lock app (via auto-lock or hotkey)
3. Remove accessibility permissions
4. **Expected:**
   - Logs show "Event tap disabled"
   - Logs show "Unlocked - permissions revoked" (safety unlock)
   - CLI prints error message
   - CLI exits cleanly
   - Normal keyboard/mouse input restored

#### Test 3: Permission Loss While Unlocked (Tray)
1. Start Tray app
2. Remove accessibility permissions
3. **Expected:**
   - Notification: "Permissions Revoked"
   - Event tap stopped
   - Normal input restored
   - App continues running (disabled state)

#### Test 4: Permission Restoration (Tray)
1. Start Tray app
2. Remove permissions (trigger disable)
3. Wait for event tap to stop
4. Re-add permissions in System Settings
5. **Expected (within 15 seconds):**
   - Logs show "Accessibility permissions have been restored"
   - Event tap restarts automatically
   - Notification: "Event tap restarted successfully"
   - App returns to normal operation

#### Test 5: Permission Loss During Auto-Lock Window
1. Start CLI with auto-lock configured (e.g., 30 seconds)
2. Stop typing (start inactivity window)
3. After 15 seconds, remove permissions
4. **Expected:**
   - Permission loss detected immediately
   - CLI exits before auto-lock triggers
   - No zombie state

#### Test 6: Rapid Permission Toggle
1. Start Tray app
2. Remove permissions
3. Immediately re-add permissions (within 1 second)
4. Remove again
5. Re-add again
6. **Expected:**
   - App handles rapid changes gracefully
   - No crashes or deadlocks
   - Event tap state matches permission state

### Automated Testing (Future)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_tap_disabled_event() {
        // Mock callback state
        let state = create_test_callback_state();

        // Simulate 0xFFFFFFFF event
        unsafe {
            let result = event_tap_callback(
                std::ptr::null_mut(),
                0xFFFFFFFF,
                std::ptr::null_mut(),
                &state as *const _ as *mut c_void
            );
        }

        // Verify flag was set
        assert!(state.app_state.should_stop_event_tap_and_clear());
    }

    #[test]
    fn test_permission_monitor_restart_request() {
        // Test that permission restoration sets start flag
    }

    #[test]
    fn test_cli_polling_loop_exit() {
        // Test that CLI exits when should_exit flag set
    }
}
```

### Edge Cases to Verify

1. **Permission loss while passphrase dialog open**
   - Should cancel dialog, exit/stop cleanly

2. **Permission loss while hotkey pressed**
   - Should not deadlock on hotkey handler

3. **Multiple rapid permission changes**
   - Should not enter inconsistent state

4. **Permission loss while buffer reset in progress**
   - Should not crash or corrupt buffer

5. **Event tap disabled by timeout (0xFFFFFFFE)**
   - Should log and attempt to re-enable
   - Should not exit/stop (timeout is temporary)

---

## Future Considerations

### 1. Permission Pre-Check on Startup

Add check before creating event tap:

```rust
pub fn start_event_tap(&mut self) -> Result<()> {
    // Check permissions FIRST
    if !input_blocking::check_accessibility_permissions() {
        anyhow::bail!("Cannot start event tap - accessibility permissions not granted");
    }

    // ... rest of startup ...
}
```

### 2. Graceful Degradation (Tray Only)

Instead of stopping completely, Tray could enter "monitor only" mode:
- Disable locking features
- Keep showing menu/notifications
- Show status: "Accessibility permissions required"

### 3. Permission Request Helper

Add helper command to open System Settings:

```bash
handsoff --request-permissions
# Opens System Settings > Privacy & Security > Accessibility
```

### 4. Watchdog for Event Tap Health

Periodically verify event tap is still receiving events:

```rust
// In permission monitor thread
if event_tap_active {
    let last_event_time = state.last_event_timestamp();
    let now = Instant::now();

    if now.duration_since(last_event_time) > Duration::from_secs(60) {
        // No events for 60 seconds - might be disabled
        warn!("Event tap appears inactive - checking health");
        // Trigger health check...
    }
}
```

### 5. Shared Event Loop (Long-term)

Refactor to use shared event loop implementation (Solution 4) to:
- Eliminate CLI/Tray divergence
- Centralize flag checking logic
- Make testing easier

---

## Conclusion

This bug represents a **critical security and usability issue**:
- Users can become locked out of their system
- App enters zombie state (disabled but not stopped)
- Wastes resources while non-functional

The recommended solution:
- **CLI:** Exit cleanly on permission loss (simple, clear)
- **Tray:** Auto-restart on permission restoration (convenient)
- **Both:** Immediate detection via event tap callback (no 15s delay)

Implementation requires CLI architectural change (polling loop), but this is necessary regardless of which solution is chosen. The polling loop also provides foundation for future improvements (shutdown signals, health checks, etc.).
