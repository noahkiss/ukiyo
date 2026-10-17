//! Snapshots: the camera's settings at one moment, with the model and firmware, so they can
//! be compared and restored. Restoring is a plan (`plan::build_values`), gated like any
//! other write; nothing here writes to the camera.
//!
//! A snapshot keeps a setting when the camera reads it now, it is writable, and the catalog
//! names it `confirmed` or `likely`. It never keeps a text value (owner strings, the device
//! name: they may name the owner), a counter or the frame number, or a code that moves on
//! its own (`owner::NOISY`).

use super::{now_ms, Error, ErrorCode};
use crate::camera::owner::{Capture, NOISY};
use crate::camera::ptp::Value;
use crate::catalog::{self, Confidence};
use crate::lab::Snapshot;
use crate::read::{self, RawValue};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Catalog names a snapshot never keeps: counters, and the next file number.
pub const NOT_SETTINGS: &[&str] = &["ShotCount", "TotalShotCount", "ShutterExchangeCount", "FrameNumber"];

/// One setting in a snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SavedValue {
    pub code: String,
    pub name: Option<String>,
    pub value: RawValue,
    /// The value's label when it was saved.
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SettingsSnapshot {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub created_at: f64,
    pub model: Option<String>,
    pub firmware: Option<String>,
    /// The USB mode and STILL/MOVIE switch it was taken in: it covers the settings that
    /// read in that state only.
    pub mode: Option<String>,
    pub capture: Option<Capture>,
    pub values: Vec<SavedValue>,
}

impl SettingsSnapshot {
    pub fn summary(&self) -> String {
        let when = chrono::DateTime::from_timestamp_millis(self.created_at as i64).map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
        let cap = self.capture.map(|c| format!(", {c:?}").to_lowercase()).unwrap_or_default();
        format!("{} settings, {} {}{cap}, {when}", self.values.len(), self.model.as_deref().unwrap_or("camera"), self.mode.as_deref().unwrap_or(""))
    }
}

/// One setting that differs between two snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Changed {
    pub code: String,
    pub name: Option<String>,
    pub a: RawValue,
    pub b: RawValue,
    pub a_label: Option<String>,
    pub b_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Diff {
    pub a: String,
    pub b: String,
    /// In `b` only.
    pub added: Vec<SavedValue>,
    /// In `a` only.
    pub removed: Vec<SavedValue>,
    pub changed: Vec<Changed>,
    /// Settings the two hold alike.
    pub same: u32,
}

fn keeps(v: &Value) -> bool {
    matches!(v, Value::Int(_) | Value::Array(_))
}

/// The camera's settings now, under the rules at the top of this module.
pub fn capture(cam: &Snapshot, name: &str) -> Result<SettingsSnapshot, Error> {
    crate::read::readiness(&cam.status, cam.rows.len(), read::Need::Tether)?;
    let cat = cam.catalog.as_ref();
    let mut values = vec![];
    for r in &cam.rows {
        let Some(v) = r.value.as_ref().filter(|v| keeps(v)) else { continue };
        if !r.available || !r.writable || r.datatype == 0xFFFF || NOISY.contains(&r.code) {
            continue;
        }
        let Some(e) = cat.and_then(|c| c.properties.get(&catalog::key(r.code))) else { continue };
        if !matches!(e.confidence, Some(Confidence::Confirmed | Confidence::Likely)) || e.name.as_deref().is_some_and(|n| NOT_SETTINGS.contains(&n)) {
            continue;
        }
        values.push(SavedValue { code: catalog::key(r.code), name: e.name.clone(), value: read::raw_value(v), label: read::value_label(Some(e), v) });
    }
    if values.is_empty() {
        return Err(Error::new(ErrorCode::NotFound, "the camera reports no setting a snapshot keeps").hint("Snapshots keep writable settings the catalog names confirmed or likely."));
    }
    Ok(SettingsSnapshot {
        schema: super::SCHEMA,
        id: String::new(),
        name: name.trim().to_string(),
        created_at: now_ms() as f64,
        model: cam.status.model.clone(),
        firmware: cam.status.firmware.clone(),
        mode: cam.status.mode.map(|m| read::mode_id(m).to_string()),
        capture: cam.status.capture,
        values,
    })
}

/// What changed from `a` to `b`, by code.
pub fn diff(a: &SettingsSnapshot, b: &SettingsSnapshot) -> Diff {
    let in_b: IndexMap<&str, &SavedValue> = b.values.iter().map(|v| (v.code.as_str(), v)).collect();
    let in_a: IndexMap<&str, &SavedValue> = a.values.iter().map(|v| (v.code.as_str(), v)).collect();
    let mut d = Diff { a: a.id.clone(), b: b.id.clone(), added: vec![], removed: vec![], changed: vec![], same: 0 };
    for va in &a.values {
        match in_b.get(va.code.as_str()) {
            None => d.removed.push(va.clone()),
            Some(vb) if vb.value == va.value => d.same += 1,
            Some(vb) => d.changed.push(Changed { code: va.code.clone(), name: va.name.clone().or(vb.name.clone()), a: va.value.clone(), b: vb.value.clone(), a_label: va.label.clone(), b_label: vb.label.clone() }),
        }
    }
    d.added = b.values.iter().filter(|v| !in_a.contains_key(v.code.as_str())).cloned().collect();
    d
}

/// The values to write for a restore: code and value, in snapshot order.
pub fn as_values(s: &SettingsSnapshot) -> Vec<(u16, Value)> {
    s.values.iter().filter_map(|v| Some((read::parse_code(&v.code)?, super::value_of(&v.value)))).collect()
}

/// A snapshot's values as a preset's `set`: code to raw value. Codes and raw values, not
/// names and labels: two codes can share a catalog name, and two values a label.
pub fn as_set(s: &SettingsSnapshot) -> IndexMap<String, String> {
    s.values.iter().map(|v| (v.code.clone(), v.value.text())).collect()
}

/// The camera's current values of the named settings, as a preset's `set`.
pub fn current_set(cam: &Snapshot, names: &[String]) -> Result<IndexMap<String, String>, Error> {
    crate::plan::ready(cam)?;
    let cat = cam.catalog.as_ref();
    let mut set = IndexMap::new();
    for n in names {
        let code = crate::plan::resolve_code(cam, n)?;
        let e = cat.and_then(|c| c.properties.get(&catalog::key(code)));
        let row = cam.rows.iter().find(|r| r.code == code && r.available);
        let Some(v) = row.and_then(|r| r.value.as_ref()) else {
            return Err(Error::new(ErrorCode::NotFound, format!("the camera does not read {} now", read::code_text(code))).hint("Some settings read only in another mode or switch position."));
        };
        // The name when it names this code alone, else the code.
        let key = e.and_then(|e| e.name.clone()).filter(|n| crate::plan::resolve_code(cam, n).ok() == Some(code)).unwrap_or_else(|| read::code_text(code));
        let text = read::value_label(e, v).unwrap_or_else(|| match v {
            Value::Str(s) => s.clone(),
            v => v.key(),
        });
        set.insert(key, text);
    }
    Ok(set)
}

/// Why each entry of a preset's `set` would not plan against this camera, if any.
pub fn check_set(cam: &Snapshot, set: &IndexMap<String, String>) -> Vec<String> {
    let cat = cam.catalog.as_ref();
    set.iter()
        .filter_map(|(k, v)| {
            let code = match crate::plan::resolve_code(cam, k) {
                Ok(c) => c,
                Err(e) => return Some(e.message),
            };
            let e = cat.and_then(|c| c.properties.get(&catalog::key(code)));
            let row = cam.rows.iter().find(|r| r.code == code);
            crate::plan::resolve_value(row, e, v, &read::code_text(code)).err().map(|e| e.message)
        })
        .collect()
}
