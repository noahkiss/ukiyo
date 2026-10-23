//! The read-only commands. Each builds its data from `ukiyo_core::read`, then prints it as
//! text or, with `--json`, as the envelope's `data`.

use crate::conn;
use crate::out::{line, table, Fail, Out};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ukiyo_core::api::Event;
use ukiyo_core::camera::owner::{Link, Status, UsbMode};
use ukiyo_core::catalog::{self, Catalog, Entry};
use ukiyo_core::read::{self, Filter, GroupView, Need, SettingRow, StatusView};
use ukiyo_core::{modes, paths, session};

/// A serde enum value as its wire name: `Link::Connected` is `connected`.
fn name<T: Serialize>(t: &T) -> String {
    serde_json::to_value(t).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn clock(ms: f64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64).map(|t| t.with_timezone(&chrono::Local).format("%H:%M:%S").to_string()).unwrap_or_default()
}

fn parse_codes(codes: &[String]) -> Result<Vec<u16>, Fail> {
    codes.iter().map(|c| read::parse_code(c).ok_or_else(|| Fail::usage(format!("not a property code: {c} (write it as 0xd381)")))).collect()
}

// ---- status ----

pub fn status(out: Out, history: bool) -> Result<(), Fail> {
    let c = conn::client(out, None);
    let snap = conn::settled(&c, None);
    let welcome = c.welcome();
    c.close();
    let snap = snap?;
    let v = read::status_view(&snap, welcome.as_ref());
    let h = history.then(|| crate::library::history_of(snap.status.camera_id.as_deref())).flatten();
    if out.json {
        let mut d = serde_json::to_value(&v).unwrap_or_default();
        if history {
            d["history"] = serde_json::to_value(&h).unwrap_or_default();
        }
        out.data(d);
    } else {
        for l in status_lines(&v) {
            line(&l);
        }
        match (&h, history) {
            (Some(h), _) => crate::library::history_lines(h).iter().for_each(|l| line(l)),
            (None, true) => line("History: none for this camera yet (it starts when a camera connects)."),
            _ => {}
        }
    }
    Ok(())
}

fn owner_text(v: &StatusView) -> String {
    let pid = v.pid.map(|p| format!("pid {p}"));
    let fake = v.fake.then(|| "fake camera".to_string());
    let extra: Vec<String> = pid.into_iter().chain(fake).collect();
    if extra.is_empty() {
        "owner: serve".into()
    } else {
        format!("owner: serve ({})", extra.join(", "))
    }
}

pub(crate) fn status_lines(v: &StatusView) -> Vec<String> {
    if v.link != Link::Connected {
        let reason = v.reason.as_deref().map(|r| format!(" ({r})")).unwrap_or_default();
        let hint = match v.link {
            Link::Asleep => "Turn the camera on. Keep-awake (on in Settings) resets its timer while Ukiyo is in use.",
            Link::Stopped => "The Lab's connect switch is off.",
            Link::Busy => "The camera is busy; try again in a few seconds.",
            _ => "Plug in the cable and turn the camera on. Set its USB mode first: ukiyo help modes",
        };
        return vec![format!("No camera: {}{reason} · {}", name(&v.link), owner_text(v)), hint.into()];
    }
    let mut head = vec![v.model.clone().unwrap_or_else(|| "Camera".into())];
    if let Some(fw) = &v.firmware {
        head.push(format!("firmware {fw}"));
    }
    let state: Vec<String> = [v.mode.map(read::mode_id).map(str::to_string), v.capture.as_ref().map(name), v.screen.as_ref().map(name)].into_iter().flatten().collect();
    if !state.is_empty() {
        head.push(state.join(", "));
    }
    head.push(format!("over {}", v.transport.label()));
    match (&v.battery_percent, &v.battery) {
        (Some(p), _) => head.push(format!("battery {p}%")),
        (None, Some(b)) => head.push(format!("battery {b}")),
        _ => {}
    }
    if let Some(c) = &v.card {
        head.push(format!("card {c} left"));
    }
    if v.keep_awake_active {
        head.push("keeping the camera awake".into());
    } else if v.keep_awake {
        head.push("keep-awake on".into());
    }
    head.push(owner_text(v));
    let mut lines = vec![head.join(" · ")];
    lines.extend(v.battery_note.map(str::to_string));
    lines.extend(v.notices.iter().map(|n| n.text.to_string()));
    let tether = "settings, presets, snapshots, owner strings";
    match v.mode {
        Some(UsbMode::Tether) => {
            lines.push(format!("Works now: {tether}, Lab"));
            lines.push("Needs RAW CONV.: recipe slots (ukiyo help modes rawconv)".into());
            if !v.available.movie_props {
                lines.push("Needs MOVIE: movie settings (ukiyo help modes movie)".into());
            }
        }
        Some(UsbMode::RawConv) => {
            lines.push("Works now: recipe slots, Lab".into());
            lines.push(format!("Needs TETHER: {tether} (ukiyo help modes tether)"));
        }
        _ => {
            lines.push("Works now: nothing; Ukiyo cannot read settings in this mode".into());
            lines.push(format!("Needs TETHER: {tether} (ukiyo help modes tether)"));
            lines.push("Needs RAW CONV.: recipe slots (ukiyo help modes rawconv)".into());
        }
    }
    lines
}

// ---- settings ----

pub struct SettingsQuery {
    pub query: Vec<String>,
    pub menu: Option<String>,
    pub codes: Vec<String>,
    pub favourites: bool,
    pub unnamed: bool,
    pub unconfirmed: bool,
    pub menus: bool,
}

pub fn settings(out: Out, q: SettingsQuery) -> Result<(), Fail> {
    let mut codes = parse_codes(&q.codes)?;
    if q.favourites {
        let favs = ukiyo_core::library::Store::new(&paths::data_dir()).favourites().codes();
        if favs.is_empty() {
            if out.json {
                out.data(json!({ "rows": [], "total": 0, "has_more": false }));
            } else {
                line("No favourites yet. Add one: ukiyo settings fav add NAME");
            }
            return Ok(());
        }
        if codes.is_empty() {
            codes = favs;
        } else {
            codes.retain(|c| favs.contains(c));
            if codes.is_empty() {
                return Err(Fail::new("not_found", "none of those codes is a favourite"));
            }
        }
    }
    // The Lab's property table reads any mode; browsing settings needs tether mode.
    let need = if codes.is_empty() { Need::Tether } else { Need::Connected };
    let c = conn::client(out, None);
    let snap = conn::settled(&c, Some(need));
    c.close();
    let snap = snap?;
    if q.menus {
        let m = read::menus(&snap);
        if out.json {
            out.data(json!({ "menus": m }));
        } else {
            for m in m {
                let last = m.path.rsplit(" > ").next().unwrap_or(&m.path);
                line(&format!("{}{last}  {}", "  ".repeat(m.depth), m.count));
            }
        }
        return Ok(());
    }
    let query = q.query.join(" ");
    let f = Filter { query: (!query.trim().is_empty()).then_some(query), codes, menu: q.menu, unnamed: q.unnamed, unconfirmed: q.unconfirmed, limit: None };
    let rows = read::settings(&snap, &f);
    if out.json {
        out.data(&rows);
        return Ok(());
    }
    if rows.rows.is_empty() {
        line("No settings match.");
        return Ok(());
    }
    let cells: Vec<Vec<String>> = rows
        .rows
        .iter()
        .map(|r| {
            vec![
                r.code.clone(),
                r.name.clone().unwrap_or_else(|| "-".into()),
                value_text(r),
                r.confidence.as_ref().map(name).unwrap_or_else(|| "-".into()),
                r.menu.clone().unwrap_or_default(),
            ]
        })
        .collect();
    for (r, l) in rows.rows.iter().zip(table(&cells)) {
        line(&l);
        if let Some(o) = options_text(r) {
            line(&format!("    {o}"));
        }
    }
    Ok(())
}

fn json_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) if s.is_empty() => "\"\"".into(),
        serde_json::Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

pub(crate) fn value_text(r: &SettingRow) -> String {
    if !r.available {
        return "n/a".into();
    }
    match (&r.value_label, &r.value) {
        (Some(l), _) => l.clone(),
        (None, Some(v)) => json_text(v),
        (None, None) => "-".into(),
    }
}

pub(crate) fn options_text(r: &SettingRow) -> Option<String> {
    let v = serde_json::to_value(r.options.as_ref()?).ok()?;
    if let Some(list) = v.as_array() {
        let items: Vec<String> = list
            .iter()
            .map(|o| {
                let raw = json_text(&o["value"]);
                match o["label"].as_str() {
                    Some(l) => format!("{l} ({raw})"),
                    None => raw,
                }
            })
            .collect();
        return Some(format!("options: {}", items.join(", ")));
    }
    Some(format!("range: {} to {}, step {}", json_text(&v["min"]), json_text(&v["max"]), json_text(&v["step"])))
}

// ---- camera watch ----

fn parse_duration(s: &str) -> Result<Duration, Fail> {
    let bad = || Fail::usage(format!("not a duration: {s} (60s, 5m, 1h)"));
    let s = s.trim();
    let (n, per) = match s.chars().last() {
        Some('s') => (&s[..s.len() - 1], 1.0),
        Some('m') => (&s[..s.len() - 1], 60.0),
        Some('h') => (&s[..s.len() - 1], 3600.0),
        _ => (s, 1.0),
    };
    let n: f64 = n.parse().map_err(|_| bad())?;
    if !(n.is_finite() && n >= 0.0) {
        return Err(bad());
    }
    Ok(Duration::from_secs_f64(n * per))
}

/// Blocks SIGINT, SIGTERM and SIGHUP and returns a flag a thread sets when one arrives, so
/// Ctrl-C ends the watch with a clean disconnect. Call before any other thread starts.
fn stop_on_signal() -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    // SAFETY: plain libc calls on a local set, before other threads exist.
    let set = unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::sigaddset(&mut set, s);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    };
    let flag = stop.clone();
    std::thread::spawn(move || {
        let mut sig = 0;
        // SAFETY: `set` is initialised; sigwait only writes `sig`.
        unsafe { libc::sigwait(&set, &mut sig) };
        flag.store(true, Ordering::SeqCst);
    });
    stop
}

type StatusKey = (Link, Option<String>, Option<String>, Option<UsbMode>, Option<String>, Option<String>);

fn status_key(s: &Status) -> StatusKey {
    (s.link, s.reason.clone(), s.model.clone(), s.mode, s.capture.as_ref().map(name), s.screen.as_ref().map(name))
}

fn status_event_text(v: &StatusView) -> String {
    let mut parts = vec![name(&v.link)];
    if let Some(r) = &v.reason {
        parts[0] = format!("{} ({r})", parts[0]);
    }
    if v.link == Link::Connected {
        parts.extend(v.model.clone());
        parts.extend(v.mode.map(read::mode_id).map(str::to_string));
        parts.extend(v.capture.as_ref().map(name));
        parts.extend(v.screen.as_ref().map(name));
    }
    format!("{}  status  {}", clock(chrono::Utc::now().timestamp_millis() as f64), parts.join(", "))
}

pub(crate) fn group_lines(g: &GroupView) -> Vec<String> {
    let label = |l: &Option<String>, v: &Option<read::RawValue>| l.clone().or_else(|| v.as_ref().map(read::RawValue::text)).unwrap_or_else(|| "-".into());
    g.changes
        .iter()
        .map(|c| {
            let who = match &c.name {
                Some(n) => format!("{} {n}", c.code),
                None => c.code.clone(),
            };
            format!("{}  {} #{}  {who}  {} -> {}", clock(g.at), name(&g.source), g.id, label(&c.old_label, &c.old), label(&c.new_label, &c.new))
        })
        .collect()
}

pub fn watch(out: Out, codes: &[String], duration: Option<&str>) -> Result<(), Fail> {
    let codes = parse_codes(codes)?;
    let limit = duration.map(parse_duration).transpose()?;
    let stop = stop_on_signal();
    let (on_event, rx) = conn::events();
    let c = conn::client(out, Some(on_event));
    let mut snap = match conn::settled(&c, None) {
        Ok(s) => s,
        Err(f) => {
            c.close();
            return Err(f);
        }
    };
    let mut last = status_key(&snap.status);
    let show_status = |snap: &ukiyo_core::lab::Snapshot| {
        let v = read::status_view(snap, c.welcome().as_ref());
        if out.json {
            line(&json!({ "event": "status", "data": v }).to_string());
        } else {
            line(&status_event_text(&v));
        }
    };
    show_status(&snap);
    let what = if codes.is_empty() { "the change feed".to_string() } else { format!("{} code(s)", codes.len()) };
    out.progress(&format!("ukiyo: watching {what}; Ctrl-C stops."));
    let deadline = limit.map(|d| Instant::now() + d);
    let result = loop {
        if stop.load(Ordering::SeqCst) || deadline.is_some_and(|d| Instant::now() >= d) {
            break Ok(());
        }
        if c.welcome().is_none() {
            break Err(Fail::new("unavailable", "the connection to ukiyo serve dropped").hint("Run the command again; it starts a new ukiyo serve."));
        }
        let wait = deadline.map_or(Duration::from_millis(200), |d| d.saturating_duration_since(Instant::now()).min(Duration::from_millis(200)));
        match rx.recv_timeout(wait) {
            Ok(Event::Changes(g)) => {
                if let Some(v) = read::group_view(snap.catalog.as_ref(), &g, &codes) {
                    if out.json {
                        line(&json!({ "event": "changes", "data": v }).to_string());
                    } else {
                        for l in group_lines(&v) {
                            line(&l);
                        }
                    }
                }
            }
            Ok(Event::Status(s)) => {
                let key = status_key(&s);
                snap.status = s;
                if key != last {
                    last = key;
                    show_status(&snap);
                }
            }
            Ok(Event::Props(rows)) => snap.rows = rows,
            Ok(Event::Catalog(cat)) => snap.catalog = cat,
            Err(_) => {}
        }
    };
    c.close();
    result
}

// ---- catalog show ----

fn catalog_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    v.sort();
    v
}

/// The catalog to show: `--model`, else the connected camera's (asking a running `serve`,
/// never starting one), else the only catalog in the folder.
pub(crate) fn catalog_path(model: Option<&str>) -> Result<PathBuf, Fail> {
    let data = paths::data_dir();
    let resources = std::env::current_exe().ok().and_then(|e| paths::bundle_resources(&e));
    let dir = ukiyo_core::lab::catalog_dir(&data, resources);
    if let Some(m) = model {
        return Ok(Catalog::path(&dir, m));
    }
    let c = conn::existing(Duration::from_secs(2));
    let live = conn::snapshot(&c).ok().and_then(|s| s.catalog_path);
    c.close();
    if let Some(p) = live {
        return Ok(PathBuf::from(p));
    }
    let files = catalog_files(&dir);
    match files.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(Fail::new("not_found", format!("no catalogs in {}", dir.display()))),
        many => {
            let names: Vec<String> = many.iter().filter_map(|p| p.file_stem()).map(|s| s.to_string_lossy().to_uppercase()).collect();
            Err(Fail::usage(format!("which model? No camera is connected; pass --model ({})", names.join(", "))))
        }
    }
}

pub fn catalog_show(out: Out, code: Option<&str>, model: Option<&str>) -> Result<(), Fail> {
    let path = catalog_path(model)?;
    if !path.exists() {
        return Err(Fail::new("not_found", format!("no catalog at {}", path.display())));
    }
    let cat = Catalog::load(&path).map_err(|e| Fail::new("failed", e.to_string()))?;
    let p = path.display().to_string();
    let Some(code) = code else {
        if out.json {
            out.data(json!({ "model": cat.model, "path": p, "firmware": cat.firmware, "properties": cat.properties }));
        } else {
            line(&format!("{} catalog, {} codes: {p}", cat.model, cat.properties.len()));
            let cells: Vec<Vec<String>> = cat
                .properties
                .iter()
                .map(|(k, e)| vec![k.clone(), e.name.clone().unwrap_or_else(|| "-".into()), e.confidence.as_ref().map(name).unwrap_or_else(|| "-".into()), e.menu.clone().unwrap_or_default()])
                .collect();
            for l in table(&cells) {
                line(&l);
            }
        }
        return Ok(());
    };
    let found = match read::parse_code(code).map(catalog::key) {
        Some(k) if cat.properties.contains_key(&k) => Some(k),
        _ => cat.properties.iter().find(|(_, e)| e.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(code))).map(|(k, _)| k.clone()),
    };
    let Some(key) = found else {
        return Err(Fail::new("not_found", format!("{code} is not in the {} catalog", cat.model)).hint("ukiyo catalog show lists every code"));
    };
    let e = &cat.properties[&key];
    if out.json {
        out.data(json!({ "model": cat.model, "path": p, "code": key, "entry": e }));
    } else {
        for l in entry_lines(&key, e) {
            line(&l);
        }
    }
    Ok(())
}

fn entry_lines(key: &str, e: &Entry) -> Vec<String> {
    let mut cells: Vec<Vec<String>> = Vec::new();
    let mut add = |k: &str, v: Option<String>| {
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            cells.push(vec![k.into(), v]);
        }
    };
    add("name", e.name.clone());
    add("menu", e.menu.clone());
    add("type", e.datatype.clone());
    add("unit", e.unit.clone());
    add("scale", e.scale.map(|s| s.to_string()));
    add("confidence", e.confidence.as_ref().map(name));
    add("source", e.source.clone());
    add("modes", Some(e.modes.join(", ")));
    add("notes", e.notes.clone());
    for (i, (k, v)) in e.values.iter().enumerate() {
        cells.push(vec![if i == 0 { "values".into() } else { String::new() }, format!("{k} = {v}")]);
    }
    let mut lines = vec![key.to_string()];
    lines.extend(table(&cells).into_iter().map(|l| format!("  {l}")));
    lines
}

// ---- log show ----

pub fn log_show(out: Out, since: Option<&str>) -> Result<(), Fail> {
    let since = match since {
        Some(s) => Some(session::parse_since(s, chrono::Utc::now().timestamp_millis()).ok_or_else(|| Fail::usage(format!("not a time: {s} (10m, 2h, 2026-10-17, \"2026-10-17 14:30\")")))?),
        None => None,
    };
    let log = session::read(&paths::data_dir(), since).map_err(|e| Fail::new("failed", e.to_string()))?;
    if out.json {
        out.data(&log);
        return Ok(());
    }
    if log.entries.is_empty() {
        line(if since.is_some() { "No session log lines in that time." } else { "No session log yet." });
        return Ok(());
    }
    for e in &log.entries {
        let t = e["t"].as_f64().map(clock).unwrap_or_default();
        line(&format!("{t}  {}  {}", e["kind"].as_str().unwrap_or("?"), entry_summary(e)));
    }
    Ok(())
}

fn hex(v: &serde_json::Value) -> String {
    v.as_u64().map(|c| read::code_text(c as u16)).unwrap_or_else(|| v.to_string())
}

/// A session value (`{"kind":"int","v":2}`) as text.
fn session_value(v: &serde_json::Value) -> String {
    if v.is_null() {
        return "-".into();
    }
    match &v["v"] {
        serde_json::Value::Null => v.to_string(),
        x => json_text(x),
    }
}

fn entry_summary(e: &serde_json::Value) -> String {
    let s = entry_text(e);
    match e["actor"].as_str() {
        Some(a) => format!("[{a}] {s}"),
        None => s,
    }
}

fn entry_text(e: &serde_json::Value) -> String {
    match e["kind"].as_str().unwrap_or("") {
        "start" => if e["fake"].as_bool() == Some(true) { "session start, fake camera" } else { "session start" }.into(),
        "stop" => if e["clean"].as_bool() == Some(true) { "clean stop" } else { "stop, not clean" }.into(),
        "status" => {
            let s = &e["status"];
            let parts: Vec<String> = ["link", "reason", "model", "mode", "capture", "screen"].iter().filter_map(|k| s[*k].as_str().map(str::to_string)).collect();
            parts.join(", ")
        }
        "changes" => {
            let g = &e["group"];
            let changes: Vec<String> = g["changes"].as_array().into_iter().flatten().map(|c| format!("{} {} -> {}", hex(&c["code"]), session_value(&c["old"]), session_value(&c["new"]))).collect();
            format!("#{} {}: {}", g["id"], g["source"].as_str().unwrap_or("?"), changes.join("; "))
        }
        "write" => {
            let o = &e["outcome"];
            let plan = e["plan"].as_str().map(|p| format!(", plan {p}")).unwrap_or_default();
            format!("{} {} -> {} ({}{plan})", hex(&o["code"]), session_value(&o["before"]), session_value(&o["readback"]), o["result"].as_str().unwrap_or("?"))
        }
        "apply" => match e["refused"].as_str() {
            Some(r) => format!("plan {} refused: {r}", e["plan"].as_str().unwrap_or("?")),
            None => format!("plan {}: {} applied, {} failed", e["plan"].as_str().unwrap_or("?"), e["applied"], e["failed"]),
        },
        "connect" => if e["on"].as_bool() == Some(true) { "connect switch on" } else { "disconnect" }.into(),
        "keep_awake" => if e["on"].as_bool() == Some(true) { "keep-awake on" } else { "keep-awake off" }.into(),
        "export" => e["path"].as_str().unwrap_or("").into(),
        "label" => {
            let p = &e["patch"];
            let name = p["name"].as_str().map(|n| format!(" {n}")).unwrap_or_default();
            format!("{}{name}", hex(&p["code"]))
        }
        "note" => match e["group"].as_u64() {
            Some(g) => format!("#{g}: {}", e["text"].as_str().unwrap_or("")),
            None => e["text"].as_str().unwrap_or("").into(),
        },
        "log" => e["text"].as_str().unwrap_or("").into(),
        _ => {
            let mut o = e.clone();
            if let Some(m) = o.as_object_mut() {
                m.remove("t");
                m.remove("kind");
                m.remove("session");
            }
            o.to_string()
        }
    }
}

// ---- help modes ----

pub fn help_modes(out: Out, mode: Option<&str>) -> Result<(), Fail> {
    let list = match mode {
        Some(m) => vec![modes::find(m).ok_or_else(|| Fail::usage(format!("no mode {m}: pick tether, rawconv or movie")))?],
        None => modes::all(),
    };
    if out.json {
        out.data(json!({ "modes": list }));
        return Ok(());
    }
    for (i, m) in list.iter().enumerate() {
        if i > 0 {
            line("");
        }
        line(&format!("{} ({})", m.name, modes::walkthrough(m.id)));
        line(&format!("  {}", m.gives));
        for (n, s) in m.steps.iter().enumerate() {
            line(&format!("  {}. {s}", n + 1));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("60s").unwrap(), Duration::from_secs(60));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_duration("1.5").unwrap(), Duration::from_millis(1500));
        assert!(parse_duration("soon").is_err());
        assert!(parse_duration("-1s").is_err());
    }

    #[test]
    fn session_summaries() {
        let e = json!({ "kind": "changes", "group": { "id": 3, "source": "camera", "changes": [{ "code": 54145, "old": { "kind": "int", "v": 1 }, "new": { "kind": "int", "v": 3 } }] } });
        assert_eq!(entry_summary(&e), "#3 camera: 0xd381 1 -> 3");
        let e = json!({ "kind": "write", "outcome": { "code": 53283, "ok": true, "result": "OK", "before": { "kind": "int", "v": 1 }, "readback": { "kind": "int", "v": 2 } } });
        assert_eq!(entry_summary(&e), "0xd023 1 -> 2 (OK)");
        assert_eq!(entry_summary(&json!({ "kind": "note", "group": 4, "text": "dial" })), "#4: dial");
        let e = json!({ "kind": "write", "actor": "cli", "plan": "p-1a2b", "outcome": { "code": 54145, "ok": true, "result": "OK", "before": { "kind": "int", "v": 1 }, "readback": { "kind": "int", "v": 3 } } });
        assert_eq!(entry_summary(&e), "[cli] 0xd381 1 -> 3 (OK, plan p-1a2b)");
        assert_eq!(entry_summary(&json!({ "kind": "apply", "actor": "app", "plan": "p-1", "applied": 1, "failed": 0 })), "[app] plan p-1: 1 applied, 0 failed");
    }
}
