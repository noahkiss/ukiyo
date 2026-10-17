//! Read-only views of the Lab's state: the camera status, settings rows with their catalog
//! names and value labels, the menu tree, and why the camera cannot answer yet. The CLI prints
//! them (`ukiyo status`, `ukiyo settings`, `ukiyo camera watch`); the MCP server (stage 5)
//! returns the same shapes (docs/agent-interface.md, sections 3 and 4).

use crate::camera::owner::{Capture, Change, ChangeGroup, ChangeSource, Link, PropRow, Status, UsbMode};
use crate::camera::ptp::{Form, Value};
use crate::catalog::{self, Catalog, Confidence, Entry};
use crate::lab::Snapshot;
use crate::modes;
use crate::serve::wire::Welcome;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Why the camera cannot answer a request now, with what fixes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// `not_connected`, `camera_asleep`, `busy`, `needs_tether` ... (section 5, "Typed errors").
    pub code: &'static str,
    pub message: String,
    pub hint: String,
    /// The command that prints the mode walkthrough, when a mode switch fixes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub walkthrough: Option<String>,
    /// The camera's current USB mode, for a `needs_*` error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<&'static str>,
}

/// What a read needs from the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Connected, in any mode that reads properties.
    Connected,
    /// Connected in USB TETHER SHOOTING FIXED.
    Tether,
}

/// A USB mode as `ukiyo help modes` and the errors name it.
pub fn mode_id(m: UsbMode) -> &'static str {
    match m {
        UsbMode::Tether => "tether",
        UsbMode::RawConv => "rawconv",
        UsbMode::CardReader => "cardreader",
        UsbMode::Unknown => "unknown",
    }
}

/// Whether the camera can answer a read that needs `need`. `rows` is how many properties the
/// Lab has read; a fresh connection has none for a moment.
pub fn readiness(s: &Status, rows: usize, need: Need) -> Result<(), Problem> {
    let plug = || "Plug in the cable and turn the camera on.".to_string();
    let reason = s.reason.as_deref().map(|r| format!(" ({r})")).unwrap_or_default();
    match s.link {
        Link::Connected if rows > 0 => {}
        Link::Connected | Link::Connecting => {
            return Err(Problem { code: "not_connected", message: "The camera is still connecting.".into(), hint: "Try again in a few seconds.".into(), walkthrough: None, mode: None });
        }
        Link::Searching | Link::Waiting => {
            return Err(Problem { code: "not_connected", message: format!("No camera on USB{reason}."), hint: plug(), walkthrough: Some(modes::walkthrough("tether")), mode: None });
        }
        Link::Stopped => {
            return Err(Problem {
                code: "not_connected",
                message: "The Lab's connect switch is off.".into(),
                hint: "Turn it on in the Lab.".into(),
                walkthrough: None,
                mode: None,
            });
        }
        Link::Asleep => {
            return Err(Problem {
                code: "camera_asleep",
                message: "Auto power off took the camera off USB.".into(),
                hint: "Turn the camera on. Keep-awake (on in Settings) resets its timer while Ukiyo is in use.".into(),
                walkthrough: None,
                mode: None,
            });
        }
        Link::Busy => {
            return Err(Problem { code: "busy", message: "The camera is busy.".into(), hint: "Wait; try again in a few seconds.".into(), walkthrough: None, mode: None });
        }
    }
    let mode = s.mode.unwrap_or(UsbMode::Unknown);
    if need == Need::Tether && mode != UsbMode::Tether {
        return Err(Problem {
            code: "needs_tether",
            message: format!("Live settings read only in USB TETHER SHOOTING FIXED mode; the camera is in {}.", mode_name(mode)),
            hint: "Unplug the cable, set SET UP > CONNECTION SETTING > CONNECTION MODE to USB TETHER SHOOTING FIXED, plug back in.".into(),
            walkthrough: Some(modes::walkthrough("tether")),
            mode: Some(mode_id(mode)),
        });
    }
    Ok(())
}

fn mode_name(m: UsbMode) -> &'static str {
    match m {
        UsbMode::Tether => "USB TETHER SHOOTING FIXED",
        UsbMode::RawConv => "USB RAW CONV./BACKUP RESTORE",
        UsbMode::CardReader => "USB CARD READER",
        UsbMode::Unknown => "an unknown mode",
    }
}

/// A property code as every surface writes it: `0x` and four lowercase hex digits.
pub fn code_text(code: u16) -> String {
    catalog::key(code)
}

/// Parses a code: `0xd381`, `0xD381` or `d381` (always hex).
pub fn parse_code(s: &str) -> Option<u16> {
    let h = s.trim();
    let h = h.strip_prefix("0x").or_else(|| h.strip_prefix("0X")).unwrap_or(h);
    (!h.is_empty() && h.len() <= 4).then(|| u16::from_str_radix(h, 16).ok()).flatten()
}

/// A value as plain JSON, typed for the window: a number, a string, an array of numbers, or
/// `"0x<hex>"` for bytes of an unknown datatype. The same JSON as [`raw_json`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(untagged)]
pub enum RawValue {
    Int(i64),
    Str(String),
    Array(Vec<i64>),
}

impl RawValue {
    /// As text: the number, the string (`""` when empty), or the array as JSON.
    pub fn text(&self) -> String {
        match self {
            RawValue::Int(n) => n.to_string(),
            RawValue::Str(s) if s.is_empty() => "\"\"".into(),
            RawValue::Str(s) => s.clone(),
            RawValue::Array(a) => serde_json::to_string(a).unwrap_or_default(),
        }
    }
}

pub fn raw_value(v: &Value) -> RawValue {
    match v {
        Value::Int(n) => RawValue::Int(*n),
        Value::Str(s) => RawValue::Str(s.clone()),
        Value::Array(a) => RawValue::Array(a.clone()),
        Value::Raw(h) => RawValue::Str(format!("0x{h}")),
    }
}

/// A value as plain JSON: a number, a string, an array of numbers, or `"0x<hex>"` for bytes
/// of an unknown datatype.
pub fn raw_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Int(n) => json!(n),
        Value::Str(s) => json!(s),
        Value::Array(a) => json!(a),
        Value::Raw(h) => json!(format!("0x{h}")),
    }
}

/// A value as the camera shows it: the catalog's label, or the scaled number with its unit.
/// `None` when the catalog says nothing about the value.
pub fn value_label(e: Option<&Entry>, v: &Value) -> Option<String> {
    let e = e?;
    if let Some(l) = e.values.get(&v.key()) {
        return Some(l.clone());
    }
    match (v, e.scale) {
        (Value::Int(n), Some(scale)) if scale > 0 => {
            let x = *n as f64 / scale as f64;
            let s = if x.fract() == 0.0 { format!("{x}") } else { format!("{x:.2}").trim_end_matches('0').to_string() };
            Some(match e.unit.as_deref() {
                Some("f/") => format!("f/{s}"),
                Some(u) => format!("{s} {u}"),
                None => s,
            })
        }
        _ => None,
    }
}

/// The label, else the raw value as text. `-` for no value.
pub fn display(e: Option<&Entry>, v: Option<&Value>) -> String {
    let Some(v) = v else { return "-".into() };
    value_label(e, v).unwrap_or_else(|| match v {
        Value::Str(s) if s.is_empty() => "\"\"".into(),
        v => v.key(),
    })
}

fn entry(cat: Option<&Catalog>, code: u16) -> Option<&Entry> {
    cat?.properties.get(&catalog::key(code))
}

/// Which features work in the current mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Available {
    pub settings: bool,
    pub slots: bool,
    pub movie_props: bool,
}

/// `ukiyo status` and `camera_status`.
#[derive(Debug, Clone, Serialize)]
pub struct StatusView {
    pub link: Link,
    pub reason: Option<String>,
    pub model: Option<String>,
    pub firmware: Option<String>,
    pub mode: Option<UsbMode>,
    pub capture: Option<Capture>,
    pub screen: Option<crate::camera::owner::Screen>,
    /// BatteryLevel as the catalog labels it (the raw value until the catalog maps it).
    pub battery: Option<String>,
    /// CaptureRemaining, the shots left on the card (raw until the catalog maps it).
    pub card: Option<String>,
    /// Not read yet: D36D carries the lens name with its serial.
    pub lens: Option<String>,
    /// Always `serve`: the one process that owns the camera.
    pub owner: &'static str,
    /// The owner's process id.
    pub pid: Option<u32>,
    pub fake: bool,
    /// The keep-awake setting.
    pub keep_awake: bool,
    /// Keep-awake is resetting the camera's auto power off timer now.
    pub keep_awake_active: bool,
    pub available: Available,
}

fn named_row<'a>(snap: &'a Snapshot, name: &str) -> Option<(&'a PropRow, Option<&'a Entry>)> {
    let cat = snap.catalog.as_ref()?;
    snap.rows.iter().find_map(|r| {
        let e = cat.properties.get(&catalog::key(r.code))?;
        (e.name.as_deref() == Some(name) && r.available && r.value.is_some()).then_some((r, Some(e)))
    })
}

pub fn status_view(snap: &Snapshot, owner: Option<&Welcome>) -> StatusView {
    let s = &snap.status;
    let connected = s.link == Link::Connected;
    let read = |name| named_row(snap, name).map(|(r, e)| display(e, r.value.as_ref()));
    StatusView {
        link: s.link,
        reason: s.reason.clone(),
        model: s.model.clone(),
        firmware: s.firmware.clone(),
        mode: s.mode,
        capture: s.capture,
        screen: s.screen,
        battery: if connected { read("BatteryLevel") } else { None },
        card: if connected { read("CaptureRemaining") } else { None },
        lens: None,
        owner: "serve",
        pid: owner.map(|w| w.pid),
        fake: owner.map_or(snap.fake, |w| w.fake),
        keep_awake: s.keep_awake.enabled,
        keep_awake_active: s.keep_awake.active,
        available: Available {
            settings: connected && s.mode == Some(UsbMode::Tether),
            slots: connected && s.mode == Some(UsbMode::RawConv),
            movie_props: connected && s.mode == Some(UsbMode::Tether) && s.capture == Some(Capture::Movie),
        },
    }
}

/// Which rows `settings` returns. Every set field must match.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Words that must all appear in the code, name, menu path or notes (any case).
    pub query: Option<String>,
    /// Only these codes.
    pub codes: Vec<u16>,
    /// A menu path prefix, any case: `SET UP` or `SET UP > POWER MANAGEMENT`.
    pub menu: Option<String>,
    /// Only codes with no catalog name.
    pub unnamed: bool,
    /// Only codes whose catalog confidence is not `confirmed`.
    pub unconfirmed: bool,
    /// At most this many rows (`None`: all).
    pub limit: Option<usize>,
}

/// One allowed value.
#[derive(Debug, Clone, Serialize)]
pub struct OptionValue {
    pub value: serde_json::Value,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Options {
    Enum(Vec<OptionValue>),
    Range { min: serde_json::Value, max: serde_json::Value, step: serde_json::Value },
}

/// One setting, as `settings_read` returns it.
#[derive(Debug, Clone, Serialize)]
pub struct SettingRow {
    pub code: String,
    pub name: Option<String>,
    pub menu: Option<String>,
    pub value: Option<serde_json::Value>,
    pub value_label: Option<String>,
    /// The allowed values, when the result has 10 rows or fewer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
    pub writable: bool,
    pub confidence: Option<Confidence>,
    /// Readable in the current mode.
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Rows {
    pub rows: Vec<SettingRow>,
    pub total: usize,
    pub has_more: bool,
}

/// `CamelCase` as `camel case`, so `eye detection` finds `FaceEyeDetection`.
fn words(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            out.push(' ');
        }
        out.extend(c.to_lowercase());
    }
    out
}

fn matches(f: &Filter, r: &PropRow, e: Option<&Entry>) -> bool {
    if !f.codes.is_empty() && !f.codes.contains(&r.code) {
        return false;
    }
    let name = e.and_then(|e| e.name.as_deref());
    if f.unnamed && name.is_some() {
        return false;
    }
    if f.unconfirmed && e.and_then(|e| e.confidence) == Some(Confidence::Confirmed) {
        return false;
    }
    let menu = e.and_then(|e| e.menu.as_deref()).unwrap_or("");
    if let Some(m) = &f.menu {
        if !menu.to_lowercase().starts_with(&m.trim().to_lowercase()) {
            return false;
        }
    }
    if let Some(q) = &f.query {
        let hay = format!(
            "{} {} {} {} {}",
            code_text(r.code),
            name.unwrap_or("").to_lowercase(),
            words(name.unwrap_or("")),
            menu.to_lowercase(),
            e.and_then(|e| e.notes.as_deref()).unwrap_or("").to_lowercase()
        );
        if !q.split_whitespace().all(|w| hay.contains(&w.to_lowercase())) {
            return false;
        }
    }
    true
}

fn options(r: &PropRow, e: Option<&Entry>) -> Option<Options> {
    match &r.form {
        Form::None => None,
        Form::Enum { values } => Some(Options::Enum(values.iter().map(|v| OptionValue { value: raw_json(v), label: value_label(e, v) }).collect())),
        Form::Range { min, max, step } => Some(Options::Range { min: raw_json(min), max: raw_json(max), step: raw_json(step) }),
    }
}

/// The settings rows that match `f`, in code order.
pub fn settings(snap: &Snapshot, f: &Filter) -> Rows {
    let cat = snap.catalog.as_ref();
    let hits: Vec<&PropRow> = snap.rows.iter().filter(|r| matches(f, r, entry(cat, r.code))).collect();
    let total = hits.len();
    let shown = f.limit.unwrap_or(usize::MAX).min(total);
    let with_options = shown <= 10;
    let rows = hits[..shown]
        .iter()
        .map(|r| {
            let e = entry(cat, r.code);
            SettingRow {
                code: code_text(r.code),
                name: e.and_then(|e| e.name.clone()),
                menu: e.and_then(|e| e.menu.clone()),
                value: r.value.as_ref().map(raw_json),
                value_label: r.value.as_ref().and_then(|v| value_label(e, v)),
                options: if with_options { options(r, e) } else { None },
                writable: r.writable,
                confidence: e.and_then(|e| e.confidence),
                available: r.available,
                error: r.error.clone(),
            }
        })
        .collect();
    Rows { rows, total, has_more: shown < total }
}

/// One menu in the tree, with how many of the camera's codes sit under it.
#[derive(Debug, Clone, Serialize)]
pub struct MenuCount {
    pub path: String,
    pub depth: usize,
    pub count: usize,
}

const NO_MENU: &str = "(no menu)";

/// The menu tree of the codes the camera advertises: every path prefix, in path order.
/// Codes with no menu in the catalog are counted under `(no menu)`, last.
pub fn menus(snap: &Snapshot) -> Vec<MenuCount> {
    let cat = snap.catalog.as_ref();
    let mut counts: std::collections::BTreeMap<Vec<String>, usize> = Default::default();
    for r in &snap.rows {
        let menu = entry(cat, r.code).and_then(|e| e.menu.clone()).unwrap_or_else(|| NO_MENU.into());
        let parts: Vec<String> = menu.split('>').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
        for n in 1..=parts.len() {
            *counts.entry(parts[..n].to_vec()).or_default() += 1;
        }
    }
    let mut v: Vec<MenuCount> = counts.into_iter().map(|(p, count)| MenuCount { depth: p.len() - 1, path: p.join(" > "), count }).collect();
    // The unmapped codes last, after the real menus.
    v.sort_by_key(|m| m.path == NO_MENU);
    v
}

/// One change in a group, with its catalog name and labels.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ChangeView {
    pub code: String,
    pub name: Option<String>,
    pub old: Option<RawValue>,
    pub new: Option<RawValue>,
    pub old_label: Option<String>,
    pub new_label: Option<String>,
}

/// A change group, as `camera watch` and `camera_watch` return it.
#[derive(Debug, Clone, Serialize)]
pub struct GroupView {
    pub id: u32,
    /// Wall-clock ms.
    pub at: f64,
    pub source: ChangeSource,
    pub changes: Vec<ChangeView>,
}

pub fn change_view(cat: Option<&Catalog>, c: &Change) -> ChangeView {
    let e = entry(cat, c.code);
    ChangeView {
        code: code_text(c.code),
        name: e.and_then(|e| e.name.clone()),
        old: c.old.as_ref().map(raw_value),
        new: c.new.as_ref().map(raw_value),
        old_label: c.old.as_ref().and_then(|v| value_label(e, v)),
        new_label: c.new.as_ref().and_then(|v| value_label(e, v)),
    }
}

/// A change group with names and labels; `codes` keeps only those codes (empty: all).
/// `None` when no change is left.
pub fn group_view(cat: Option<&Catalog>, g: &ChangeGroup, codes: &[u16]) -> Option<GroupView> {
    let changes: Vec<ChangeView> = g.changes.iter().filter(|c| codes.is_empty() || codes.contains(&c.code)).map(|c| change_view(cat, c)).collect();
    (!changes.is_empty()).then_some(GroupView { id: g.id, at: g.at, source: g.source, changes })
}

/// The owner strings `ukiyo owner` reads and sets: its field names and the catalog names they
/// stand for. None is confirmed yet, so setting one needs `--allow-unconfirmed`.
pub const OWNER_FIELDS: &[(&str, &str)] = &[("copyright", "Copyright"), ("author", "CopyrightAuthor"), ("prefix", "FileNamePrefix1"), ("device-name", "DeviceName")];

/// The catalog name for an owner field (`copyright`), any case.
pub fn owner_field(field: &str) -> Option<&'static str> {
    OWNER_FIELDS.iter().find(|(f, _)| f.eq_ignore_ascii_case(field.trim())).map(|(_, n)| *n)
}

/// One owner string as the camera holds it now.
#[derive(Debug, Clone, Serialize)]
pub struct OwnerField {
    pub field: &'static str,
    pub name: &'static str,
    /// `None` when the catalog has no code by that name.
    pub code: Option<String>,
    pub value: Option<String>,
    pub confidence: Option<Confidence>,
    pub available: bool,
}

pub fn owner_view(snap: &Snapshot) -> Vec<OwnerField> {
    let cat = snap.catalog.as_ref();
    OWNER_FIELDS
        .iter()
        .map(|(field, name)| {
            let found = cat.and_then(|c| c.properties.iter().find(|(_, e)| e.name.as_deref() == Some(*name)));
            let code = found.and_then(|(k, _)| parse_code(k));
            let row = code.and_then(|c| snap.rows.iter().find(|r| r.code == c));
            OwnerField {
                field,
                name,
                code: code.map(code_text),
                value: row.filter(|r| r.available).and_then(|r| r.value.as_ref()).map(|v| match v {
                    Value::Str(s) => s.clone(),
                    v => v.key(),
                }),
                confidence: found.and_then(|(_, e)| e.confidence),
                available: row.is_some_and(|r| r.available),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::owner::Screen;

    fn snap() -> Snapshot {
        let mut cat = Catalog::empty("X-T5", "FUJIFILM");
        cat.properties.insert(
            "0xd381".into(),
            Entry {
                name: Some("AutoPowerOff".into()),
                menu: Some("SET UP > POWER MANAGEMENT > AUTO POWER OFF".into()),
                values: [("1".to_string(), "5 MIN".to_string()), ("3".to_string(), "OFF".to_string())].into_iter().collect(),
                confidence: Some(Confidence::Confirmed),
                ..Default::default()
            },
        );
        cat.properties.insert("0x5007".into(), Entry { name: Some("FNumber".into()), unit: Some("f/".into()), scale: Some(100), ..Default::default() });
        cat.properties.insert("0xd242".into(), Entry { name: Some("BatteryLevel".into()), ..Default::default() });
        let row = |code, value, form| PropRow { code, datatype: 4, writable: true, form, value: Some(value), error: None, available: true, changed_at: None };
        Snapshot {
            status: Status { link: Link::Connected, model: Some("X-T5".into()), mode: Some(UsbMode::Tether), capture: Some(Capture::Stills), screen: Some(Screen::Shooting), ..Default::default() },
            rows: vec![
                row(0x5007, Value::Int(560), Form::Range { min: Value::Int(100), max: Value::Int(2200), step: Value::Int(10) }),
                row(0xD242, Value::Int(4), Form::None),
                row(0xD381, Value::Int(1), Form::Enum { values: vec![Value::Int(1), Value::Int(3)] }),
                row(0xD999, Value::Int(0), Form::None),
            ],
            groups: vec![],
            catalog: Some(cat),
            catalog_path: None,
            session_path: None,
            fake: true,
            noisy: vec![],
        }
    }

    #[test]
    fn owner_fields_map_to_catalog_names() {
        assert_eq!(owner_field("Copyright"), Some("Copyright"));
        assert_eq!(owner_field("device-name"), Some("DeviceName"));
        assert_eq!(owner_field("nickname"), None);
        let v = owner_view(&snap());
        assert_eq!(v.len(), OWNER_FIELDS.len());
        assert!(v.iter().all(|f| f.code.is_none() && !f.available), "the test catalog has none of them");
    }

    #[test]
    fn codes_parse_as_hex() {
        assert_eq!(parse_code("0xd381"), Some(0xD381));
        assert_eq!(parse_code("D381"), Some(0xD381));
        assert_eq!(parse_code("0x5007"), Some(0x5007));
        assert_eq!(parse_code("0x12345"), None);
        assert_eq!(parse_code("power"), None);
    }

    #[test]
    fn labels_and_scales() {
        let s = snap();
        let cat = s.catalog.as_ref();
        assert_eq!(value_label(entry(cat, 0x5007), &Value::Int(560)).as_deref(), Some("f/5.6"));
        assert_eq!(display(entry(cat, 0xD381), Some(&Value::Int(3))), "OFF");
        assert_eq!(display(entry(cat, 0xD999), Some(&Value::Int(7))), "7");
        assert_eq!(display(None, None), "-");
    }

    #[test]
    fn settings_filters_and_options() {
        let s = snap();
        let r = settings(&s, &Filter { query: Some("power off".into()), ..Default::default() });
        assert_eq!(r.total, 1);
        assert_eq!(r.rows[0].code, "0xd381");
        assert_eq!(r.rows[0].value_label.as_deref(), Some("5 MIN"));
        let opts = serde_json::to_value(&r.rows[0].options).unwrap();
        assert_eq!(opts, json!([{ "value": 1, "label": "5 MIN" }, { "value": 3, "label": "OFF" }]));
        assert_eq!(settings(&s, &Filter { query: Some("auto power".into()), ..Default::default() }).total, 1, "CamelCase names split");
        assert_eq!(settings(&s, &Filter { menu: Some("set up".into()), ..Default::default() }).total, 1);
        assert_eq!(settings(&s, &Filter { unnamed: true, ..Default::default() }).rows[0].code, "0xd999");
        assert_eq!(settings(&s, &Filter { unconfirmed: true, ..Default::default() }).total, 3);
        let r = settings(&s, &Filter { limit: Some(2), ..Default::default() });
        assert_eq!((r.rows.len(), r.total, r.has_more), (2, 4, true));
        let m = menus(&s);
        assert_eq!(m.last().unwrap().path, "(no menu)");
        assert_eq!(m.iter().find(|m| m.path == "SET UP").unwrap().count, 1);
        assert!(m.iter().any(|m| m.path == "SET UP > POWER MANAGEMENT > AUTO POWER OFF" && m.depth == 2));
    }

    #[test]
    fn status_and_readiness() {
        let mut s = snap();
        let v = status_view(&s, None);
        assert_eq!(v.battery.as_deref(), Some("4"));
        assert!(v.available.settings && !v.available.slots);
        assert_eq!(readiness(&s.status, 4, Need::Tether), Ok(()));
        assert_eq!(readiness(&s.status, 0, Need::Connected).unwrap_err().code, "not_connected");
        s.status.mode = Some(UsbMode::RawConv);
        let p = readiness(&s.status, 4, Need::Tether).unwrap_err();
        assert_eq!((p.code, p.mode), ("needs_tether", Some("rawconv")));
        assert_eq!(p.walkthrough.as_deref(), Some("ukiyo help modes tether"));
        assert_eq!(readiness(&s.status, 4, Need::Connected), Ok(()));
        s.status.link = Link::Asleep;
        assert_eq!(readiness(&s.status, 4, Need::Connected).unwrap_err().code, "camera_asleep");
        s.status.link = Link::Searching;
        assert_eq!(readiness(&s.status, 0, Need::Connected).unwrap_err().code, "not_connected");
    }
}
