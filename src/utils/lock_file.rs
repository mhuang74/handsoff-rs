//! Single-instance flock (issue #37 N1).
//!
//! The tray holds an exclusive `flock(2)` on a lock file for its whole
//! lifetime; the OS releases it when the process dies, so no stale lockfile
//! exists. The self-relaunch successor (`--skip-instance-lock`) previously
//! skipped the guard forever, leaving a window where a second launch could
//! stack a duplicate instance (the 2026-10-01 lockout class). The successor
//! now retries acquisition for a bounded grace period and ends up holding
//! the lock like any other instance — the flag only suppresses the duplicate
//! alert while the dying parent still holds the lock.

use anyhow::{Context, Result};
use std::path::Path;
use std::time::{Duration, Instant};

/// Retry window for the relaunch successor: the parent exits within
/// milliseconds of spawning the child, so the lock frees almost immediately;
/// ~5 s is generous headroom, after which the process is treated as a plain
/// duplicate and must exit.
pub const ACQUIRE_GRACE: Duration = Duration::from_secs(5);

/// Interval between `flock(LOCK_EX | LOCK_NB)` attempts during the grace
/// window (100 ms — 50 attempts inside the 5 s grace).
pub const RETRY_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(unix)]
const LOCK_EX: i32 = 0x0002; // exclusive lock
#[cfg(unix)]
const LOCK_NB: i32 = 0x0004; // don't block when locking

/// Whether the acquire attempt ended holding the lock.
#[cfg(unix)]
impl AcquireOutcome {
    pub fn is_acquired(&self) -> bool {
        matches!(self, AcquireOutcome::Acquired(_))
    }
}

/// Outcome of a bounded acquire attempt.
#[derive(Debug)]
pub enum AcquireOutcome {
    /// This process now holds the exclusive lock for its lifetime (keep the
    /// File alive — dropping it releases the lock).
    Acquired(std::fs::File),
    /// The lock is still held by another process after the grace period.
    StillHeld,
}

/// Attempt to acquire an exclusive flock on `path`, retrying every
/// [`RETRY_INTERVAL`] until `deadline` passes.
///
/// With `grace: None` (plain launch), a single attempt is made: a held lock
/// means another instance is genuinely running. With a grace window (the
/// relaunch successor), the loop tolerates the parent's still-held lock for
/// up to the grace period.
///
/// On success the returned `File` must be kept alive (or leaked) for the
/// process lifetime — dropping it releases the flock.
#[cfg(unix)]
pub fn try_acquire_flock(path: &Path, grace: Option<Duration>) -> Result<AcquireOutcome> {
    use std::os::unix::io::AsRawFd;

    #[link(name = "c")]
    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create lock directory {}", parent.display()))?;
        }
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("Failed to open lock file {}", path.display()))?;

    let deadline = grace.map(|g| Instant::now() + g);
    loop {
        if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
            return Ok(AcquireOutcome::Acquired(file));
        }
        match deadline {
            Some(deadline) if Instant::now() < deadline => {
                std::thread::sleep(RETRY_INTERVAL);
            }
            _ => return Ok(AcquireOutcome::StillHeld),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_lock_path() -> std::path::PathBuf {
        static SEQ: std::sync::LazyLock<parking_lot::Mutex<u32>> =
            std::sync::LazyLock::new(|| parking_lot::Mutex::new(0));
        let mut seq = SEQ.lock();
        *seq += 1;
        std::env::temp_dir().join(format!(
            "handsoff-test-lock-{}-{}.lock",
            std::process::id(),
            *seq
        ))
    }

    #[test]
    fn test_acquire_fresh_lock_succeeds() {
        let path = temp_lock_path();
        let file = match try_acquire_flock(&path, None).expect("fresh lock must acquire") {
            AcquireOutcome::Acquired(f) => f,
            AcquireOutcome::StillHeld => panic!("fresh lock must not be held"),
        };
        // The held File must keep the lock: re-acquiring in-process must fail
        // (same process, second open — flock is per-open-file-description).
        let second = try_acquire_flock(&path, None).expect("second attempt runs");
        assert!(
            !second.is_acquired(),
            "held flock must exclude a second acquisition"
        );
        drop(file);
        drop(second);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_successor_acquires_after_release_within_grace() {
        // N1 scenario: the parent holds the lock when the successor starts;
        // the successor must acquire once the parent's lock frees.
        let path = temp_lock_path();
        let parent = match try_acquire_flock(&path, None).expect("parent lock must acquire") {
            AcquireOutcome::Acquired(f) => f,
            AcquireOutcome::StillHeld => panic!("fresh lock must not be held"),
        };

        // Grace window long enough for the release below to happen between
        // retries, short enough to keep the suite fast.
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            drop(parent); // parent "exits": OS releases the flock
        });

        let outcome =
            try_acquire_flock(&path, Some(Duration::from_secs(5))).expect("grace attempt must run");
        assert!(
            outcome.is_acquired(),
            "successor must acquire the lock once the parent releases it"
        );
        handle.join().unwrap();

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_grace_expires_when_lock_still_held() {
        let path = temp_lock_path();
        let _holder = match try_acquire_flock(&path, None).expect("holder lock must acquire") {
            AcquireOutcome::Acquired(f) => f,
            AcquireOutcome::StillHeld => panic!("fresh lock must not be held"),
        };

        let started = Instant::now();
        let outcome = try_acquire_flock(&path, Some(Duration::from_millis(300)))
            .expect("grace attempt must run");
        assert!(!outcome.is_acquired());
        assert!(
            started.elapsed() >= Duration::from_millis(300),
            "grace must be honored before giving up"
        );

        let _ = std::fs::remove_file(&path);
    }
}
