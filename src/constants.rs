//! Centralized constants for HandsOff application
//!
//! This module contains all configurable numerical values used throughout
//! the application. Each constant includes documentation on its purpose,
//! unit, and recommended value range.

// ============================================================================
// AUTO-LOCK CONFIGURATION
// ============================================================================

/// Minimum auto-lock timeout allowed.
/// Unit: seconds
/// Range: Fixed minimum, do not change without updating UI validation
pub const AUTO_LOCK_MIN_SECONDS: u64 = 20;

/// Maximum auto-lock timeout allowed.
/// Unit: seconds
/// Range: Fixed maximum (10 minutes), do not change without updating UI validation
pub const AUTO_LOCK_MAX_SECONDS: u64 = 600;

/// Default auto-lock timeout when no config exists.
/// Unit: seconds
/// Recommended range: 60-300 (1-5 minutes)
pub const AUTO_LOCK_DEFAULT_SECONDS: u64 = 180;

// ============================================================================
// AUTO-UNLOCK CONFIGURATION (backoff schedule)
// ============================================================================
// Auto-unlock is an exponential-backoff schedule, not a single timeout
// (specs/deep-design-review-v2-2026-09.md §2). The first window opens at
// AUTO_UNLOCK_BASE_SECONDS after the locked stretch begins, then the interval
// doubles each window, never exceeding AUTO_UNLOCK_CEILING_SECONDS.

/// Base interval for the auto-unlock backoff schedule (enabled by default).
/// First window opens this long after lock (awake-time).
/// Unit: seconds
pub const AUTO_UNLOCK_BASE_SECONDS: u64 = 3600;

/// Ceiling on any single auto-unlock interval (24 hours).
/// Unit: seconds
pub const AUTO_UNLOCK_CEILING_SECONDS: u64 = 86400;

// ============================================================================
// INPUT BUFFER CONFIGURATION
// ============================================================================

/// Default buffer reset timeout - clears passphrase buffer after inactivity.
/// Unit: seconds
/// Recommended range: 2-10 (short enough for security, long enough for typing)
pub const BUFFER_RESET_DEFAULT_SECONDS: u64 = 3;

// ============================================================================
// POLLING & THREAD INTERVALS
// ============================================================================

/// Buffer reset thread check interval.
/// Unit: milliseconds
/// Recommended range: 100-500 (must be < BUFFER_RESET_DEFAULT_SECONDS * 1000)
pub const BUFFER_RESET_CHECK_INTERVAL_MS: u64 = 250;

/// Auto-lock state monitoring interval.
/// Unit: seconds
/// Recommended range: 1-10 (balance between responsiveness and CPU usage)
pub const AUTO_LOCK_CHECK_INTERVAL_SECS: u64 = 5;

/// Auto-unlock state monitoring interval.
/// Unit: seconds
/// Recommended range: 5-30 (less critical, can be longer)
pub const AUTO_UNLOCK_CHECK_INTERVAL_SECS: u64 = 10;

/// Accessibility permission check interval.
/// Unit: seconds
/// Recommended range: 10-60 (infrequent check, permission rarely changes)
pub const PERMISSION_CHECK_INTERVAL_SECS: u64 = 15;

/// Tray app polling interval when app is disabled (low-power mode).
/// Unit: seconds
/// Recommended range: 1-10 (minimal activity when disabled)
pub const POLL_INTERVAL_DISABLED_SECS: u64 = 5;

/// Tray app polling interval when app is enabled (active mode).
/// Unit: milliseconds
/// Recommended range: 100-1000 (lower = more responsive, higher = less
/// CPU)
pub const POLL_INTERVAL_ENABLED_MS: u64 = 500;

/// Tooltip rebuild cadence while a countdown is visible.
/// Unit: milliseconds
pub const TOOLTIP_UPDATE_INTERVAL_MS: u64 = 15_000;

/// Threshold for logging slow event tap callbacks.
/// Callbacks exceeding this duration are counted and logged in telemetry summaries.
/// Unit: microseconds
/// Recommended range: 200-1000 (low enough to detect issues, high enough to avoid noise)
pub const CALLBACK_SLOW_THRESHOLD_US: u64 = 500;

/// Interval for logging callback telemetry summaries from the permission monitor thread.
/// Unit: seconds
/// Recommended range: 30-120
pub const CALLBACK_TELEMETRY_INTERVAL_SECS: u64 = 60;

/// Delay between disabling an event tap and releasing its CFMachPortRef.
/// Gives the kernel time to flush in-flight callbacks so WindowServer can release
/// its send right before we drop our receive right, reducing zombie Mach port lifetime.
/// Unit: milliseconds
/// Recommended range: 10-50 (short enough to be imperceptible, long enough to drain)
pub const EVENT_TAP_DRAIN_DELAY_MS: u64 = 20;

/// Re-enable debounce interval to prevent cascading timeouts.
/// When WindowServer is under pressure, rapid timeout events can occur.
/// This cooldown prevents cascading re-enables that would worsen the situation.
/// Unit: seconds
/// Recommended range: 5-15 (long enough to let WindowServer stabilize)
pub const REENABLE_DEBOUNCE_SECS: u64 = 10;

// ============================================================================
// NOTIFICATION TIMEOUTS
// ============================================================================

/// Standard notification display duration.
/// Unit: milliseconds
/// Recommended range: 2000-5000 (long enough to read, short enough to not annoy)
pub const NOTIFICATION_TIMEOUT_MS: u32 = 3000;

/// Error notification display duration (longer for important messages).
/// Unit: milliseconds
/// Recommended range: 4000-10000 (errors need more attention)
pub const NOTIFICATION_ERROR_TIMEOUT_MS: u32 = 5000;

// ============================================================================
// MACOS KEYCODES
// ============================================================================

/// macOS keycode for Backspace/Delete key.
/// Unit: macOS virtual keycode
/// Range: Fixed, do not change (hardware constant)
pub const BACKSPACE_KEYCODE: i64 = 51;

/// Default lock hotkey keycode ('L' key).
/// Unit: macOS virtual keycode
/// Recommended: Any letter key (0-50 range)
pub const DEFAULT_LOCK_KEYCODE: i64 = 37;

/// Default talk/unmute hotkey keycode ('T' key).
/// Unit: macOS virtual keycode
/// Recommended: Any letter key (0-50 range)
pub const DEFAULT_TALK_KEYCODE: i64 = 17;

/// macOS keycode for the Return/Enter key.
/// Unit: macOS virtual keycode
/// Range: Fixed, do not change (hardware constant)
pub const ENTER_KEYCODE: i64 = 36;

/// macOS keycode for the keypad Enter key.
/// Unit: macOS virtual keycode
/// Range: Fixed, do not change (hardware constant)
pub const ENTER_KEYCODE_KEYPAD: i64 = 76;

// ============================================================================
// FILE PERMISSIONS
// ============================================================================

/// Config file permissions (user read/write only for security).
/// Unit: Unix permission bits (octal)
/// Recommended: 0o600 (secure) or 0o644 (readable by others)
pub const CONFIG_FILE_PERMISSIONS: u32 = 0o600;

/// Permission mask to check for group/other access (security check).
/// Unit: Unix permission bits (octal)
/// Range: Fixed, used for security validation
pub const CONFIG_PERMISSION_MASK_GROUP_OTHER: u32 = 0o077;
