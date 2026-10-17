//! `owner.lock`: an exclusive `flock`. The process that holds it is the camera owner, and only
//! that process may open USB ([`crate::lab::Lab::start`] takes the lock as proof). The kernel
//! drops a `flock` when its process dies, so a crash leaves no stale lock.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The held lock. Dropping it releases it.
#[derive(Debug)]
pub struct OwnerLock {
    file: File,
    path: PathBuf,
}

/// Why the lock was not taken.
#[derive(Debug)]
pub enum LockError {
    /// Another process holds it: another `serve` owns the camera.
    Held,
    Io(std::io::Error),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::Held => f.write_str("another process owns the camera (owner.lock is held by another ukiyo serve or a tools/ptp script)"),
            LockError::Io(e) => write!(f, "owner.lock: {e}"),
        }
    }
}

impl std::error::Error for LockError {}

fn open(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)
}

impl OwnerLock {
    /// Takes the lock, or fails at once with [`LockError::Held`].
    pub fn try_acquire(path: &Path) -> Result<OwnerLock, LockError> {
        let mut file = open(path).map_err(LockError::Io)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(LockError::Held),
            Err(TryLockError::Error(e)) => return Err(LockError::Io(e)),
        }
        // The pid is for people reading the file; the flock is what counts.
        let _ = file.set_len(0);
        let _ = writeln!(file, "{}", std::process::id());
        Ok(OwnerLock { file, path: path.to_path_buf() })
    }

    /// Takes the lock, retrying for up to `wait`. A client's [`is_free`] probe holds the lock
    /// for an instant, so a `serve` that starts at that moment retries instead of giving up.
    pub fn acquire_within(path: &Path, wait: Duration) -> Result<OwnerLock, LockError> {
        let deadline = Instant::now() + wait;
        loop {
            match OwnerLock::try_acquire(path) {
                Err(LockError::Held) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
                r => return r,
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for OwnerLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// Whether no process holds the lock now. Takes and drops it at once; a missing file is free.
pub fn is_free(path: &Path) -> bool {
    match OwnerLock::try_acquire(path) {
        Ok(l) => {
            drop(l);
            true
        }
        Err(LockError::Held) => false,
        // Cannot even open it (folder missing): nobody can hold it.
        Err(LockError::Io(_)) => !path.exists(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_holder_at_a_time() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("owner.lock");
        assert!(is_free(&p));
        let a = OwnerLock::try_acquire(&p).unwrap();
        assert!(matches!(OwnerLock::try_acquire(&p), Err(LockError::Held)));
        assert!(!is_free(&p));
        let t = Instant::now();
        assert!(matches!(OwnerLock::acquire_within(&p, Duration::from_millis(200)), Err(LockError::Held)));
        assert!(t.elapsed() >= Duration::from_millis(200));
        drop(a);
        assert!(is_free(&p));
        let _b = OwnerLock::acquire_within(&p, Duration::from_millis(200)).unwrap();
    }

    #[test]
    fn a_released_lock_is_taken_by_a_waiter() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("owner.lock");
        let a = OwnerLock::try_acquire(&p).unwrap();
        let p2 = p.clone();
        let waiter = std::thread::spawn(move || OwnerLock::acquire_within(&p2, Duration::from_secs(2)).is_ok());
        std::thread::sleep(Duration::from_millis(100));
        drop(a);
        assert!(waiter.join().unwrap());
    }
}
