//! Camera history, kept on this Mac: when each camera connected, its shot counters over
//! time, and the lenses it carried. `serve`'s Lab records it; any front end reads it.
//!
//! A camera is known by its id: the SHA-256 of the serial's digest (`ptp::serial_digest`)
//! with a random salt that never leaves this Mac (`history/.salt`), cut to 16 hex digits.
//! The serial itself is never stored, and the id means nothing on another Mac.
//!
//! - Counters are the catalog's `TotalShotCount` and `ShotCount` (both `likely`), read in
//!   tether stills mode only. A connect in another mode records no counters.
//! - `lenses` stays empty: the camera reports the lens name together with its serial (D36D),
//!   and that is not split yet.

use super::{now_ms, write_atomic};
use crate::camera::owner::{Change, Link, PropRow, Status};
use crate::camera::ptp::Value;
use crate::catalog::Catalog;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SCHEMA: u32 = 1;

/// The counters recorded, by catalog name, in the order the Hub prefers them.
pub const COUNTERS: &[&str] = &["TotalShotCount", "ShotCount"];

/// Connects kept per camera.
const MAX_CONNECTS: usize = 2000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Connect {
    /// Wall-clock ms.
    pub at: f64,
    pub firmware: Option<String>,
    pub mode: Option<String>,
    /// Counter name to value, when the connect read them.
    #[serde(default)]
    pub counters: IndexMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct LensSeen {
    pub name: String,
    pub first_seen: f64,
    pub last_seen: f64,
}

/// `history/<camera id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct History {
    pub schema: u32,
    pub camera: String,
    pub model: Option<String>,
    pub first_connected: f64,
    pub last_connected: f64,
    pub connects: Vec<Connect>,
    /// Local date (`2026-10-17`) to the last counter values seen that day.
    #[serde(default)]
    pub days: BTreeMap<String, IndexMap<String, i64>>,
    #[serde(default)]
    pub lenses: Vec<LensSeen>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DayShots {
    pub date: String,
    pub shots: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MonthShots {
    pub month: String,
    pub shots: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ConnectView {
    pub at: f64,
    pub firmware: Option<String>,
    pub mode: Option<String>,
    /// The counter's value at that connect.
    pub shots: Option<i64>,
}

/// One camera's history as the Hub and `status --history` show it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HistoryView {
    pub camera: String,
    pub model: Option<String>,
    pub first_connected: f64,
    pub last_connected: f64,
    /// The counter the shot numbers come from (`TotalShotCount`), when any was read.
    pub counter: Option<String>,
    /// Its latest value.
    pub shots: Option<i64>,
    /// Shots taken since the reading before: on the day of each later reading. The first
    /// reading has no baseline, so it adds no day.
    pub per_day: Vec<DayShots>,
    pub per_month: Vec<MonthShots>,
    /// Newest first.
    pub connects: Vec<ConnectView>,
    pub lenses: Vec<LensSeen>,
}

fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("history")
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// The per-Mac salt, made on first use.
fn salt(data_dir: &Path) -> std::io::Result<String> {
    use std::io::Read as _;
    let p = dir(data_dir).join(".salt");
    if let Ok(s) = std::fs::read_to_string(&p) {
        if s.trim().len() >= 32 {
            return Ok(s.trim().to_string());
        }
    }
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    let s = crate::camera::ptp::hex(&b);
    write_atomic(&p, s.as_bytes())?;
    Ok(s)
}

/// A camera's id from its serial's digest.
pub fn camera_id(data_dir: &Path, digest: &str) -> Option<String> {
    use sha2::Digest as _;
    let salt = salt(data_dir).ok()?;
    let h = sha2::Sha256::digest(format!("{salt}:{digest}").as_bytes());
    Some(crate::camera::ptp::hex(&h[..8]))
}

fn file(data_dir: &Path, id: &str) -> PathBuf {
    dir(data_dir).join(format!("{id}.json"))
}

pub fn load(data_dir: &Path, id: &str) -> Option<History> {
    serde_json::from_str(&std::fs::read_to_string(file(data_dir, id)).ok()?).ok()
}

fn save(data_dir: &Path, h: &History) {
    if let Ok(text) = serde_json::to_string_pretty(h) {
        if let Err(e) = write_atomic(&file(data_dir, &h.camera), text.as_bytes()) {
            eprintln!("ukiyo: history not saved: {e}");
        }
    }
}

/// The counters the rows hold now, by catalog name.
pub fn counters(rows: &[PropRow], cat: Option<&Catalog>) -> IndexMap<String, i64> {
    let mut out = IndexMap::new();
    let Some(cat) = cat else { return out };
    for name in COUNTERS {
        let Some(code) = cat.properties.iter().find(|(_, e)| e.name.as_deref() == Some(name)).and_then(|(k, _)| crate::read::parse_code(k)) else { continue };
        if let Some(Value::Int(n)) = rows.iter().find(|r| r.code == code && r.available).and_then(|r| r.value.as_ref()) {
            out.insert(name.to_string(), *n);
        }
    }
    out
}

/// The counter codes, for the Lab to notice a change.
pub fn counter_codes(cat: Option<&Catalog>) -> Vec<u16> {
    let Some(cat) = cat else { return vec![] };
    cat.properties.iter().filter(|(_, e)| e.name.as_deref().is_some_and(|n| COUNTERS.contains(&n))).filter_map(|(k, _)| crate::read::parse_code(k)).collect()
}

/// Records history from the Lab's events. One per Lab.
#[derive(Debug)]
pub struct Recorder {
    data_dir: PathBuf,
    /// The connected camera's id.
    current: Option<String>,
    /// The connect is recorded; its counters are not read yet.
    needs_counters: bool,
}

impl Recorder {
    pub fn new(data_dir: &Path) -> Recorder {
        Recorder { data_dir: data_dir.to_path_buf(), current: None, needs_counters: false }
    }

    /// Sets the status's `camera_id`, and records a connect when a camera arrives.
    pub fn on_status(&mut self, s: &mut Status) {
        if s.link != Link::Connected {
            if matches!(s.link, Link::Searching | Link::Waiting | Link::Stopped | Link::Asleep) {
                self.current = None;
            }
            s.camera_id = self.current.clone();
            return;
        }
        let Some(digest) = s.serial_digest.clone() else { return };
        if self.current.is_none() {
            let Some(id) = camera_id(&self.data_dir, &digest) else { return };
            let now = now_ms() as f64;
            let mut h = load(&self.data_dir, &id).unwrap_or(History {
                schema: SCHEMA,
                camera: id.clone(),
                model: None,
                first_connected: now,
                last_connected: now,
                connects: vec![],
                days: BTreeMap::new(),
                lenses: vec![],
            });
            h.model = s.model.clone().or(h.model);
            h.last_connected = now;
            h.connects.push(Connect { at: now, firmware: s.firmware.clone(), mode: s.mode.map(|m| crate::read::mode_id(m).to_string()), counters: IndexMap::new() });
            if h.connects.len() > MAX_CONNECTS {
                h.connects.drain(..h.connects.len() - MAX_CONNECTS);
            }
            save(&self.data_dir, &h);
            self.current = Some(id);
            self.needs_counters = true;
        }
        s.camera_id = self.current.clone();
    }

    /// A full set of rows, or a change group: records the counters when they moved.
    pub fn on_rows(&mut self, rows: &[PropRow], cat: Option<&Catalog>, changes: Option<&[Change]>) {
        let Some(id) = self.current.clone() else { return };
        if let Some(ch) = changes {
            let codes = counter_codes(cat);
            if !ch.iter().any(|c| codes.contains(&c.code)) {
                return;
            }
        }
        let now = counters(rows, cat);
        if now.is_empty() {
            return;
        }
        let Some(mut h) = load(&self.data_dir, &id) else { return };
        let day = today();
        let mut changed = h.days.get(&day) != Some(&now);
        if changed {
            h.days.insert(day, now.clone());
        }
        if self.needs_counters {
            if let Some(c) = h.connects.last_mut() {
                c.counters = now;
                changed = true;
            }
            self.needs_counters = false;
        }
        if changed {
            save(&self.data_dir, &h);
        }
    }
}

/// One camera's history as shown.
pub fn view(h: &History) -> HistoryView {
    let counter = h.days.values().next_back().and_then(|last| COUNTERS.iter().find(|c| last.contains_key(**c))).map(|c| c.to_string());
    let mut per_day = vec![];
    let mut per_month: BTreeMap<String, i64> = BTreeMap::new();
    let mut shots = None;
    if let Some(c) = &counter {
        let mut prev: Option<i64> = None;
        for (date, vals) in &h.days {
            let Some(v) = vals.get(c).copied() else { continue };
            if let Some(p) = prev {
                let n = (v - p).max(0);
                per_day.push(DayShots { date: date.clone(), shots: n });
                *per_month.entry(date[..7].to_string()).or_default() += n;
            }
            prev = Some(v);
            shots = Some(v);
        }
    }
    HistoryView {
        camera: h.camera.clone(),
        model: h.model.clone(),
        first_connected: h.first_connected,
        last_connected: h.last_connected,
        shots,
        per_day,
        per_month: per_month.into_iter().map(|(month, shots)| MonthShots { month, shots }).collect(),
        connects: h
            .connects
            .iter()
            .rev()
            .map(|c| ConnectView { at: c.at, firmware: c.firmware.clone(), mode: c.mode.clone(), shots: counter.as_ref().and_then(|k| c.counters.get(k).copied()) })
            .collect(),
        counter,
        lenses: h.lenses.clone(),
    }
}

/// Every camera's history, the most recently connected first.
pub fn views(data_dir: &Path) -> Vec<HistoryView> {
    let mut v: Vec<HistoryView> = std::fs::read_dir(dir(data_dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str::<History>(&std::fs::read_to_string(e.path()).ok()?).ok())
        .map(|h| view(&h))
        .collect();
    v.sort_by(|a, b| b.last_connected.total_cmp(&a.last_connected));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_salted_and_stable() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let x = camera_id(a.path(), "d1").unwrap();
        assert_eq!(x.len(), 16);
        assert_eq!(camera_id(a.path(), "d1").unwrap(), x, "the same Mac, the same id");
        assert_ne!(camera_id(a.path(), "d2").unwrap(), x);
        assert_ne!(camera_id(b.path(), "d1").unwrap(), x, "another salt, another id");
    }

    #[test]
    fn shots_per_day_and_month() {
        let mut h = History { schema: 1, camera: "c".into(), model: Some("X-T5".into()), first_connected: 1.0, last_connected: 2.0, connects: vec![], days: BTreeMap::new(), lenses: vec![] };
        for (d, n) in [("2026-09-30", 100), ("2026-10-01", 130), ("2026-10-05", 175)] {
            h.days.insert(d.into(), [("TotalShotCount".to_string(), n)].into_iter().collect());
        }
        h.connects.push(Connect { at: 1.0, firmware: None, mode: None, counters: [("TotalShotCount".to_string(), 100)].into_iter().collect() });
        let v = view(&h);
        assert_eq!(v.counter.as_deref(), Some("TotalShotCount"));
        assert_eq!(v.shots, Some(175));
        assert_eq!(v.per_day.iter().map(|d| d.shots).collect::<Vec<_>>(), [30, 45]);
        assert_eq!(v.per_month.len(), 1);
        assert_eq!((v.per_month[0].month.as_str(), v.per_month[0].shots), ("2026-10", 75));
        assert_eq!(v.connects[0].shots, Some(100));
    }

    #[test]
    fn recorder_on_a_status_and_rows() {
        let d = tempfile::tempdir().unwrap();
        let mut cat = Catalog::empty("X-T5", "FUJIFILM");
        cat.properties.insert("0xd310".into(), crate::catalog::Entry { name: Some("TotalShotCount".into()), ..Default::default() });
        let mut r = Recorder::new(d.path());
        let mut s = Status { link: Link::Connected, model: Some("X-T5".into()), serial_digest: Some("abc".into()), ..Default::default() };
        r.on_status(&mut s);
        let id = s.camera_id.clone().unwrap();
        let row = |n| PropRow { code: 0xD310, datatype: 6, writable: false, form: crate::camera::ptp::Form::None, value: Some(Value::Int(n)), error: None, available: true, changed_at: None };
        r.on_rows(&[row(42)], Some(&cat), None);
        r.on_rows(&[row(43)], Some(&cat), Some(&[Change { code: 0xD310, old: Some(Value::Int(42)), new: Some(Value::Int(43)) }]));
        let h = load(d.path(), &id).unwrap();
        assert_eq!(h.connects.len(), 1);
        assert_eq!(h.connects[0].counters["TotalShotCount"], 42);
        assert_eq!(h.days[&today()]["TotalShotCount"], 43);
        let text = std::fs::read_to_string(file(d.path(), &id)).unwrap();
        assert!(!text.contains("abc"), "no digest on disk");
        // Unplugged and back: a second connect.
        let mut gone = Status { link: Link::Searching, ..Default::default() };
        r.on_status(&mut gone);
        r.on_status(&mut s);
        assert_eq!(load(d.path(), &id).unwrap().connects.len(), 2);
    }
}
