//! The app's own updates through tauri-plugin-updater (on QuadCam's pattern): the feed is
//! `latest.json` on the latest GitHub release (`tauri.conf.json`, `plugins.updater`), and every
//! archive must carry a signature from the updater key.
//!
//! The updater is off, and never reaches the network, when:
//!
//! - the build has no updater key: `tauri.conf.json` holds a placeholder, and
//!   `tools/release/release.sh` puts the real public key in at release time
//!   (`UKIYO_UPDATER_PUBKEY`);
//! - the build is a debug build (`pnpm tauri dev`, tests), unless `UKIYO_UPDATER=real`;
//! - `UKIYO_UPDATER=off`.
//!
//! One check runs 30 s after launch; Settings > Connection shows its answer and can check
//! again. Install downloads the update, checks its signature, replaces the bundle and restarts
//! the app. These are window commands only: an agent never updates the app.

use serde::Serialize;
use specta::Type;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;

/// The `pubkey` a build without the updater key carries.
pub const PLACEHOLDER: &str = "UKIYO_UPDATER_PUBKEY_PLACEHOLDER";

/// How long after launch the automatic check waits.
const FIRST_CHECK: Duration = Duration::from_secs(30);

/// What the window shows about updates.
#[derive(Debug, Clone, Default, Serialize, Type)]
pub struct UpdateStatus {
    /// The running version.
    pub current: String,
    /// Whether this build checks for updates.
    pub enabled: bool,
    /// Why it does not, when `enabled` is false.
    pub reason: Option<String>,
    /// A newer version on the feed, when a check found one.
    pub available: Option<String>,
    /// Its release notes.
    pub notes: Option<String>,
    /// The last check's error.
    pub error: Option<String>,
    /// Whether a check has run since launch.
    pub checked: bool,
}

/// The last check's answer.
#[derive(Default)]
pub struct State(Mutex<UpdateStatus>);

/// Why this build does not check, or None when it does.
pub fn disabled_reason(pubkey: &str, debug_build: bool, env: Option<&str>) -> Option<String> {
    if env == Some("off") {
        return Some("UKIYO_UPDATER=off.".into());
    }
    if pubkey.trim().is_empty() || pubkey == PLACEHOLDER {
        return Some("This build has no updater key.".into());
    }
    if debug_build && env != Some("real") {
        return Some("Development builds do not check for updates (UKIYO_UPDATER=real turns it on).".into());
    }
    None
}

fn pubkey<R: Runtime>(app: &AppHandle<R>) -> String {
    app.config().plugins.0.get("updater").and_then(|u| u.get("pubkey")).and_then(|k| k.as_str()).unwrap_or_default().to_string()
}

fn reason<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    disabled_reason(&pubkey(app), cfg!(debug_assertions), std::env::var("UKIYO_UPDATER").ok().as_deref())
}

fn base<R: Runtime>(app: &AppHandle<R>) -> UpdateStatus {
    let reason = reason(app);
    UpdateStatus { current: app.package_info().version.to_string(), enabled: reason.is_none(), reason, ..UpdateStatus::default() }
}

/// Manages the state and, when the build checks, starts the launch check.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    app.manage(State(Mutex::new(base(app))));
    if reason(app).is_some() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        tauri::async_runtime::block_on(check(&app));
    });
}

async fn check<R: Runtime>(app: &AppHandle<R>) -> UpdateStatus {
    let mut s = base(app);
    if s.enabled {
        match app.updater() {
            Ok(u) => match u.check().await {
                Ok(Some(found)) => {
                    s.available = Some(found.version.clone());
                    s.notes = found.body.clone();
                }
                Ok(None) => {}
                Err(e) => s.error = Some(format!("Could not check for updates: {e}")),
            },
            Err(e) => s.error = Some(format!("Could not check for updates: {e}")),
        }
        s.checked = true;
    }
    *app.state::<State>().0.lock().unwrap() = s.clone();
    s
}

/// The last check's answer (no network).
#[tauri::command]
#[specta::specta]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    app.state::<State>().0.lock().unwrap().clone()
}

/// Checks the feed now.
#[tauri::command]
#[specta::specta]
pub async fn update_check(app: AppHandle) -> UpdateStatus {
    check(&app).await
}

/// Downloads the newest update, checks its signature, installs it and restarts the app.
#[tauri::command]
#[specta::specta]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    if let Some(r) = reason(&app) {
        return Err(r);
    }
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| format!("Could not check for updates: {e}"))?;
    let Some(update) = update else {
        return Err("Ukiyo is up to date.".into());
    };
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| format!("The update did not install: {e}"))?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_updater_is_off_without_a_key_in_debug_and_on_request() {
        let key = "a-public-key";
        assert!(disabled_reason(PLACEHOLDER, false, None).is_some(), "no key");
        assert!(disabled_reason("", false, None).is_some(), "empty key");
        assert!(disabled_reason(key, true, None).is_some(), "debug build");
        assert!(disabled_reason(key, true, Some("real")).is_none(), "debug build, asked for");
        assert!(disabled_reason(key, false, Some("off")).is_some(), "turned off");
        assert!(disabled_reason(PLACEHOLDER, true, Some("real")).is_some(), "no key beats real");
        assert!(disabled_reason(key, false, None).is_none(), "a release build with a key");
    }

    /// The feed is the latest GitHub release, and the key is the placeholder or a minisign
    /// public key (base64 of "untrusted comment: ...").
    #[test]
    fn the_config_names_the_feed_and_a_key() {
        let conf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json")).unwrap()).unwrap();
        let u = &conf["plugins"]["updater"];
        assert_eq!(u["endpoints"], serde_json::json!(["https://github.com/noahkiss/ukiyo/releases/latest/download/latest.json"]));
        let key = u["pubkey"].as_str().unwrap();
        assert!(key == PLACEHOLDER || key.starts_with("dW50cnVzdGVkIGNvbW1lbnQ6"), "{key}");
        assert_eq!(u["requireSignedVersion"], true);
    }
}
