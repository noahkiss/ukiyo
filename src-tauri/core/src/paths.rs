//! Where ukiyo keeps its files. `serve`, the app and the CLI all resolve the data folder here,
//! so they agree on `owner.lock` and `ukiyo.sock` (docs/agent-interface.md, "One USB owner").
//!
//! The data folder is `~/Library/Application Support/com.nkmkdigital.ukiyo/`, the folder Tauri
//! names after the bundle ID. `UKIYO_DATA_DIR` overrides it (tests, a second dev setup).

use std::path::{Path, PathBuf};

/// The bundle ID; the data folder is named after it.
pub const BUNDLE_ID: &str = "com.nkmkdigital.ukiyo";

/// The data folder name before the bundle ID became [`BUNDLE_ID`].
pub const OLD_DATA_DIR: &str = "app.ukiyo";

/// The data folder: `UKIYO_DATA_DIR`, else `$HOME/Library/Application Support/<BUNDLE_ID>`.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("UKIYO_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    home.join("Library/Application Support").join(BUNDLE_ID)
}

/// The exclusive `flock` the camera owner holds.
pub fn lock_path(data_dir: &Path) -> PathBuf {
    data_dir.join("owner.lock")
}

/// The owner's Unix socket.
pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ukiyo.sock")
}

/// macOS limits a Unix socket path to 103 bytes (`sun_path`).
pub const SOCKET_PATH_MAX: usize = 103;

/// Fails when the data folder's socket path is too long to bind, with the reason: a `serve`
/// started there would fail at once, and its client would wait out its whole connect time.
pub fn check_socket_path(data_dir: &Path) -> std::io::Result<()> {
    let sock = socket_path(data_dir);
    let n = sock.as_os_str().len();
    if n > SOCKET_PATH_MAX {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("the socket path {} is {n} bytes; macOS allows {SOCKET_PATH_MAX}. Set UKIYO_DATA_DIR to a shorter folder", sock.display()),
        ));
    }
    Ok(())
}

/// Where a detached `serve` writes its stderr.
pub fn serve_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("serve.log")
}

/// The bundle's resources folder when `exe` runs from `Ukiyo.app/Contents/MacOS/`.
pub fn bundle_resources(exe: &Path) -> Option<PathBuf> {
    let res = exe.parent()?.parent()?.join("Resources");
    res.is_dir().then_some(res)
}

/// Moves the old data folder (a sibling of `data_dir` named [`OLD_DATA_DIR`]) to `data_dir`,
/// once: only when the old one exists and the new one does not. Returns the old path if it moved.
pub fn migrate_data_dir(data_dir: &Path) -> std::io::Result<Option<PathBuf>> {
    let Some(parent) = data_dir.parent() else { return Ok(None) };
    let old = parent.join(OLD_DATA_DIR);
    if old == data_dir || !old.is_dir() || data_dir.exists() {
        return Ok(None);
    }
    std::fs::rename(&old, data_dir)?;
    Ok(Some(old))
}

/// Moves the old data folder if needed, then creates the data folder (mode 0700).
pub fn prepare_data_dir(data_dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    match migrate_data_dir(data_dir) {
        Ok(Some(old)) => eprintln!("ukiyo: moved {} to {}", old.display(), data_dir.display()),
        Ok(None) => {}
        Err(e) => eprintln!("ukiyo: old data folder not moved: {e}"),
    }
    std::fs::create_dir_all(data_dir)?;
    std::fs::set_permissions(data_dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A data folder whose socket path cannot bind is refused before anything spawns.
    #[test]
    fn a_long_socket_path_is_refused_before_a_spawn() {
        let short = tempfile::tempdir().unwrap();
        assert!(check_socket_path(short.path()).is_ok());
        let long = short.path().join("x".repeat(SOCKET_PATH_MAX));
        let e = check_socket_path(&long).unwrap_err();
        assert!(e.to_string().contains("UKIYO_DATA_DIR"), "{e}");
        let e = crate::serve::spawn_detached(Path::new("/nonexistent/ukiyo"), &long).unwrap_err();
        assert!(e.to_string().contains(&format!("macOS allows {SOCKET_PATH_MAX}")), "{e}");
        assert!(!long.exists(), "nothing was created for a folder that cannot serve");
    }

    #[test]
    fn migrate_data_dir_moves_old_folder_once() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join(OLD_DATA_DIR);
        let new = root.path().join(BUNDLE_ID);

        // Nothing to move.
        assert_eq!(migrate_data_dir(&new).unwrap(), None);

        std::fs::create_dir_all(old.join("sessions")).unwrap();
        std::fs::write(old.join("settings.json"), "{}").unwrap();
        assert_eq!(migrate_data_dir(&new).unwrap(), Some(old.clone()));
        assert!(!old.exists());
        assert_eq!(std::fs::read_to_string(new.join("settings.json")).unwrap(), "{}");
        assert!(new.join("sessions").is_dir());

        // The new folder wins: an old one that reappears stays where it is.
        std::fs::create_dir_all(&old).unwrap();
        assert_eq!(migrate_data_dir(&new).unwrap(), None);
        assert!(old.exists());
    }

    #[test]
    fn prepare_makes_a_private_folder() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let d = root.path().join(BUNDLE_ID);
        prepare_data_dir(&d).unwrap();
        assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(socket_path(&d), d.join("ukiyo.sock"));
        assert_eq!(lock_path(&d), d.join("owner.lock"));
    }
}
