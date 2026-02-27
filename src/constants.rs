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
pub const AUTO_LOCK_DEFAULT_SECONDS: u64 = 120;

// ============================================================================
// AUTO-UNLOCK CONFIGURATION
// ============================================================================

/// Default auto-unlock timeout (release builds: disabled, debug: 60s for testing).
/// Unit: seconds (0 = disabled)
/// Recommended range: 0 (disabled) or 60-300
#[cfg(not(debug_assertions))]
pub const AUTO_UNLOCK_DEFAULT_SECONDS: u64 = 0;
#[cfg(debug_assertions)]
pub const AUTO_UNLOCK_DEFAULT_SECONDS: u64 = 60;

/// Minimum auto-unlock timeout when enabled.
/// Unit: seconds
/// Range: Fixed minimum, prevents accidental instant unlock
pub const AUTO_UNLOCK_MIN_SECONDS: u64 = 60;

/// Maximum auto-unlock timeout allowed.
/// Unit: seconds
/// Range: Fixed maximum (15 minutes)
pub const AUTO_UNLOCK_MAX_SECONDS: u64 = 900;

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

/// CFRunLoop polling interval for event processing.
/// Unit: milliseconds
/// Recommended range: 100-1000 (lower = more responsive, higher = less CPU)
pub const CFRUNLOOP_POLL_INTERVAL_MS: u64 = 500;

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
/// Recommended range: 100-1000 (same as CFRUNLOOP_POLL_INTERVAL_MS)
pub const POLL_INTERVAL_ENABLED_MS: u64 = 500;

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

// ============================================================================
// CRYPTOGRAPHY
// ============================================================================

/// AES-256-GCM nonce length.
/// Unit: bytes
/// Range: Fixed at 12 bytes (96 bits) per GCM specification
pub const NONCE_LENGTH_BYTES: usize = 12;
