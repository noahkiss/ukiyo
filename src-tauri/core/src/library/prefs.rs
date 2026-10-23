//! Preferences: `settings.json` in the data folder, the one settings file
//! (docs/agent-interface.md, section 8, stage 6). The window, the CLI, the MCP server and
//! `serve` all read and write it through this module.
//!
//! - Writes are read-modify-write under the library lock, then temp-and-rename, so two
//!   processes never lose each other's change.
//! - A key this build does not know is kept as it was, so an older or newer build never
//!   drops another's setting.
//! - A field the file lacks takes its default.

use super::{write_atomic, Store};
use crate::api::{Error, ErrorCode};
use crate::camera::transport::TransportKind;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

pub const SCHEMA: u32 = 1;

/// The window's theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// The sidebar brand marks, by id. The first is the default. The window keeps the images
/// (`app/src/lib/prefs.tsx`); a test checks both lists agree.
pub const BRAND_MARKS: &[&str] = &["still-water", "sun-and-seal", "fuji-in-mist", "clouds", "fuji-reflection", "pine-and-wave"];

/// The app icons, by id. The first is the default. The app applies a choice at its next start.
pub const APP_ICONS: &[&str] = &["brush", "seal", "sun", "aperture"];

/// Everything in `settings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct Prefs {
    pub schema: u32,
    pub theme: Theme,
    pub brand_mark: String,
    pub app_icon: String,
    /// Keep the camera awake while Ukiyo is in use (the owner's D381 toggle).
    pub keep_awake: bool,
    /// Minutes after the last client request or window input that still count as in use.
    pub in_use_minutes: u32,
    /// A write the camera refuses with a menu open over the shooting screen gets one
    /// half-press, which closes the menu, and one retry.
    pub close_menu_to_write: bool,
    /// How `serve` reaches the camera: `usb` or `wifi` (wireless tether).
    /// `UKIYO_TRANSPORT` overrides it for one run.
    pub transport: TransportKind,
    /// Keys this build does not know, kept as they were.
    #[serde(flatten)]
    #[specta(skip)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            schema: SCHEMA,
            theme: Theme::System,
            brand_mark: BRAND_MARKS[0].into(),
            app_icon: APP_ICONS[0].into(),
            keep_awake: true,
            in_use_minutes: 5,
            close_menu_to_write: true,
            transport: TransportKind::Usb,
            other: Default::default(),
        }
    }
}

/// A change to some preferences. `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PrefsPatch {
    #[serde(default)]
    pub theme: Option<Theme>,
    #[serde(default)]
    pub brand_mark: Option<String>,
    #[serde(default)]
    pub app_icon: Option<String>,
    #[serde(default)]
    pub keep_awake: Option<bool>,
    #[serde(default)]
    pub in_use_minutes: Option<u32>,
    #[serde(default)]
    pub close_menu_to_write: Option<bool>,
    #[serde(default)]
    pub transport: Option<TransportKind>,
}

impl PrefsPatch {
    pub fn is_empty(&self) -> bool {
        *self == PrefsPatch::default()
    }

    /// Checks every value before anything is written.
    pub fn check(&self) -> Result<(), Error> {
        let one_of = |what: &str, v: &str, list: &[&str]| {
            if list.contains(&v) {
                Ok(())
            } else {
                Err(Error::new(ErrorCode::NotFound, format!("no {what} {v:?}")).hint(format!("Pick one of: {}.", list.join(", "))))
            }
        };
        if let Some(b) = &self.brand_mark {
            one_of("brand mark", b, BRAND_MARKS)?;
        }
        if let Some(a) = &self.app_icon {
            one_of("app icon", a, APP_ICONS)?;
        }
        if let Some(m) = self.in_use_minutes {
            if !(1..=240).contains(&m) {
                return Err(Error::new(ErrorCode::ValueOutOfRange, format!("in_use_minutes is 1 to 240, not {m}")));
            }
        }
        Ok(())
    }

    fn apply(&self, p: &mut Prefs) {
        if let Some(t) = self.theme {
            p.theme = t;
        }
        if let Some(b) = &self.brand_mark {
            p.brand_mark = b.clone();
        }
        if let Some(a) = &self.app_icon {
            p.app_icon = a.clone();
        }
        if let Some(k) = self.keep_awake {
            p.keep_awake = k;
        }
        if let Some(m) = self.in_use_minutes {
            p.in_use_minutes = m;
        }
        if let Some(c) = self.close_menu_to_write {
            p.close_menu_to_write = c;
        }
        if let Some(t) = self.transport {
            p.transport = t;
        }
    }
}

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

/// The preferences now; defaults for a missing or unreadable file.
pub fn load(data_dir: &Path) -> Prefs {
    let mut p: Prefs = std::fs::read_to_string(path(data_dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    // An id this build does not know (an asset removed) falls back to the default.
    if !BRAND_MARKS.contains(&p.brand_mark.as_str()) {
        p.brand_mark = BRAND_MARKS[0].into();
    }
    if !APP_ICONS.contains(&p.app_icon.as_str()) {
        p.app_icon = APP_ICONS[0].into();
    }
    p
}

/// Applies a patch under the library lock and saves. Returns the preferences after it.
pub fn update(data_dir: &Path, patch: &PrefsPatch) -> Result<Prefs, Error> {
    patch.check()?;
    let store = Store::new(data_dir);
    let _g = store.lock()?;
    let mut p = load(data_dir);
    patch.apply(&mut p);
    p.schema = SCHEMA;
    let text = serde_json::to_string_pretty(&p).map_err(|e| Error::new(ErrorCode::Failed, e.to_string()))?;
    write_atomic(&path(data_dir), text.as_bytes())?;
    Ok(p)
}

/// Parses one `KEY=VALUE` from `ukiyo prefs set`: `theme=dark`, `brand-mark=clouds`,
/// `app-icon=seal`, `keep-awake=on`, `in-use-minutes=10`, `close-menu-to-write=off`,
/// `transport=wifi`.
pub fn parse_pair(patch: &mut PrefsPatch, key: &str, value: &str) -> Result<(), Error> {
    let v = value.trim();
    let flag = || match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Ok(true),
        "off" | "false" | "no" | "0" => Ok(false),
        _ => Err(Error::new(ErrorCode::NotFound, format!("{key} is on or off, not {v:?}"))),
    };
    match key.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "theme" => {
            patch.theme = Some(serde_json::from_value(serde_json::json!(v.to_ascii_lowercase())).map_err(|_| Error::new(ErrorCode::NotFound, format!("no theme {v:?}")).hint("Pick system, light or dark."))?)
        }
        "brand-mark" => patch.brand_mark = Some(v.to_string()),
        "app-icon" => patch.app_icon = Some(v.to_string()),
        "keep-awake" => patch.keep_awake = Some(flag()?),
        "close-menu-to-write" => patch.close_menu_to_write = Some(flag()?),
        "transport" => patch.transport = Some(TransportKind::parse(v).ok_or_else(|| Error::new(ErrorCode::NotFound, format!("no transport {v:?}")).hint("Pick usb or wifi."))?),
        "in-use-minutes" => patch.in_use_minutes = Some(v.parse().map_err(|_| Error::new(ErrorCode::NotFound, format!("in-use-minutes is a number, not {v:?}")))?),
        k => {
            return Err(Error::new(ErrorCode::NotFound, format!("no preference {k}"))
                .hint("Pick theme, brand-mark, app-icon, keep-awake, in-use-minutes, close-menu-to-write or transport."))
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_patch_and_unknown_keys() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load(d.path()), Prefs::default());
        std::fs::write(path(d.path()), r#"{"keep_awake": false, "from_the_future": [1, 2]}"#).unwrap();
        let p = load(d.path());
        assert!(!p.keep_awake);
        assert_eq!(p.in_use_minutes, 5, "a missing field takes its default");
        let p = update(d.path(), &PrefsPatch { theme: Some(Theme::Dark), brand_mark: Some("clouds".into()), ..Default::default() }).unwrap();
        assert_eq!((p.theme, p.brand_mark.as_str(), p.keep_awake), (Theme::Dark, "clouds", false));
        let on_disk: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path(d.path())).unwrap()).unwrap();
        assert_eq!(on_disk["from_the_future"], serde_json::json!([1, 2]), "unknown keys survive");
        assert_eq!(on_disk["theme"], "dark");
        assert_eq!(on_disk["schema"], 1);
        let e = update(d.path(), &PrefsPatch { app_icon: Some("nope".into()), ..Default::default() }).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);
    }

    #[test]
    fn pairs() {
        let mut p = PrefsPatch::default();
        parse_pair(&mut p, "theme", "Dark").unwrap();
        parse_pair(&mut p, "keep-awake", "off").unwrap();
        parse_pair(&mut p, "in_use_minutes", "10").unwrap();
        parse_pair(&mut p, "transport", "Wi-Fi").unwrap();
        assert_eq!((p.theme, p.keep_awake, p.in_use_minutes, p.transport), (Some(Theme::Dark), Some(false), Some(10), Some(TransportKind::Wifi)));
        assert!(parse_pair(&mut p, "transport", "bluetooth").is_err());
        assert!(parse_pair(&mut p, "colour", "red").is_err());
        assert!(parse_pair(&mut p, "theme", "sepia").is_err());
    }

    /// The window's lists of brand marks and app icons match these.
    #[test]
    fn window_lists_match() {
        let tsx = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/lib/prefs.tsx")).unwrap();
        let ids = |list: &str| -> Vec<String> {
            let start = tsx.find(&format!("export const {list}")).unwrap();
            let body = &tsx[start..start + tsx[start..].find("];").unwrap()];
            regex::Regex::new(r#"\{ id: "([a-z-]+)""#).unwrap().captures_iter(body).map(|c| c[1].to_string()).collect()
        };
        assert_eq!(ids("BRAND_MARKS"), BRAND_MARKS);
        assert_eq!(ids("APP_ICONS"), APP_ICONS);
    }
}
