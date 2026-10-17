//! The library commands (stage 6): `presets`, `snapshots`, `settings fav`, `prefs`, `trash`,
//! and `status --history`'s data. The library lives in the data folder, so these run in
//! this process (`ukiyo_core::api::call::library_*`). Only reading the camera (a snapshot, a
//! preset of current values) and applying (a plan) go through `serve`.

use crate::conn;
use crate::out::{line, table, Fail, Out};
use crate::writes::{self, WriteFlags};
use indexmap::IndexMap;
use serde_json::json;
use std::time::Duration;
use ukiyo_core::api::{call, Request, Response};
use ukiyo_core::catalog::Catalog;
use ukiyo_core::lab::Snapshot;
use ukiyo_core::library::history::HistoryView;
use ukiyo_core::library::prefs::{self, Prefs, PrefsPatch};
use ukiyo_core::library::{self, Context, Hit, Id, Kind, Read, SaveInput, SaveKind, SearchInput, SearchKind};
use ukiyo_core::paths;
use ukiyo_core::read::Need;

const ACTOR: &str = "cli";

fn data() -> std::path::PathBuf {
    paths::data_dir()
}

pub(crate) fn when(ms: f64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64).map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

fn id_of(kind: Kind, s: &str) -> Result<String, Fail> {
    match Id::of_kind(kind, s)? {
        Id::Item(k, slug) => Ok(library::item_id(k, &slug)),
        _ => unreachable!("of_kind returns an item"),
    }
}

/// The camera now, through `serve` (spawned when none runs), in tether mode.
fn camera(out: Out) -> Result<Snapshot, Fail> {
    let c = conn::client(out, None);
    let s = conn::settled(&c, Some(Need::Tether));
    c.close();
    s
}

fn hits(out: Out, kind: SearchKind, query: &[String], empty: &str) -> Result<(), Fail> {
    let q = query.join(" ");
    let h = call::library_search(&data(), &SearchInput { kind, query: (!q.trim().is_empty()).then_some(q), limit: Some(1000) });
    if out.json {
        out.data(&h);
        return Ok(());
    }
    if h.items.is_empty() {
        line(empty);
    }
    let rows: Vec<Vec<String>> = h.items.iter().map(|i: &Hit| vec![i.id.clone(), i.summary.clone(), i.at.map(when).unwrap_or_default()]).collect();
    table(&rows).iter().for_each(|l| line(l));
    Ok(())
}

// ---- presets ----

pub fn presets_ls(out: Out, query: &[String]) -> Result<i32, Fail> {
    hits(out, SearchKind::Preset, query, "No presets. Save one: ukiyo presets save NAME SETTING...").map(|_| 0)
}

pub fn presets_show(out: Out, id: &str) -> Result<i32, Fail> {
    let Read::Preset(p) = call::library_read(&data(), &id_of(Kind::Preset, id)?, None)? else { unreachable!() };
    if out.json {
        out.data(&p);
        return Ok(0);
    }
    line(&format!("{} ({}), {} settings, saved {}{}", p.name, p.id, p.set.len(), when(p.updated_at), p.model.as_deref().map(|m| format!(" from an {m}")).unwrap_or_default()));
    let rows: Vec<Vec<String>> = p.set.iter().map(|(k, v)| vec![format!("  {k}"), v.clone()]).collect();
    table(&rows).iter().for_each(|l| line(l));
    if let Some(n) = &p.notes {
        line(&format!("Notes: {n}"));
    }
    Ok(0)
}

pub struct PresetSave {
    pub name: String,
    /// Settings whose current camera values the preset takes.
    pub settings: Vec<String>,
    /// NAME=VALUE pairs given outright.
    pub set: Vec<String>,
    pub from_snapshot: Option<String>,
    pub notes: Option<String>,
}

pub fn presets_save(out: Out, a: PresetSave) -> Result<i32, Fail> {
    let sources = [!a.settings.is_empty(), !a.set.is_empty(), a.from_snapshot.is_some()].iter().filter(|x| **x).count();
    if sources != 1 {
        return Err(Fail::usage("give the settings in one way: SETTING... (their values now), --set NAME=VALUE..., or --from-snapshot ID"));
    }
    let mut input = SaveInput { kind: SaveKind::Preset, name: Some(a.name), item: None, from: None };
    let cam;
    let mut ctx = Context::default();
    if let Some(s) = &a.from_snapshot {
        input.from = Some(id_of(Kind::Snapshot, s)?);
    } else if !a.set.is_empty() {
        let mut m = serde_json::Map::new();
        for it in &a.set {
            let (k, v) = it.split_once('=').ok_or_else(|| Fail::usage(format!("not NAME=VALUE: {it}")))?;
            m.insert(k.trim().to_string(), json!(v));
        }
        input.item = Some(json!({ "set": m, "notes": a.notes }));
    } else {
        cam = camera(out)?;
        ctx.camera = Some(&cam);
        input.item = Some(json!({ "settings": a.settings, "notes": a.notes }));
    }
    let saved = call::library_save(&data(), ACTOR, &input, ctx)?;
    if out.json {
        out.data(&saved);
    } else {
        line(&format!("Saved {}{}.", saved.id, if saved.replaced { "; the old version is in the trash" } else { "" }));
        saved.warnings.iter().for_each(|w| line(&format!("Warning: {w}")));
    }
    Ok(0)
}

pub fn presets_apply(out: Out, id: &str, flags: WriteFlags) -> Result<i32, Fail> {
    let id = id_of(Kind::Preset, id)?;
    call::library_read(&data(), &id, None)?;
    writes::plan_from(out, id, flags)
}

pub fn rm(out: Out, kind: Kind, id: &str) -> Result<i32, Fail> {
    let t = call::library_delete(&data(), ACTOR, &id_of(kind, id)?)?;
    if out.json {
        out.data(&t);
    } else {
        line(&format!("Moved {} to the trash for 30 days. Restore it: ukiyo trash restore {}", t.id, t.trash_id.as_deref().unwrap_or("")));
    }
    Ok(0)
}

// ---- snapshots ----

pub fn snapshots_ls(out: Out) -> Result<i32, Fail> {
    hits(out, SearchKind::Snapshot, &[], "No snapshots. Take one: ukiyo snapshots save NAME").map(|_| 0)
}

pub fn snapshots_show(out: Out, id: &str) -> Result<i32, Fail> {
    let Read::Snapshot(s) = call::library_read(&data(), &id_of(Kind::Snapshot, id)?, None)? else { unreachable!() };
    if out.json {
        out.data(&s);
        return Ok(0);
    }
    line(&format!("{} ({}): {}", s.name, s.id, s.summary()));
    let rows: Vec<Vec<String>> = s.values.iter().map(|v| vec![format!("  {}", v.code), v.name.clone().unwrap_or_else(|| "-".into()), v.label.clone().unwrap_or_else(|| v.value.text())]).collect();
    table(&rows).iter().for_each(|l| line(l));
    Ok(0)
}

pub fn snapshots_save(out: Out, name: Option<String>) -> Result<i32, Fail> {
    let cam = camera(out)?;
    let saved = call::library_save(&data(), ACTOR, &SaveInput { kind: SaveKind::Snapshot, name, item: None, from: None }, Context { camera: Some(&cam), catalog: None })?;
    if out.json {
        out.data(&saved);
        return Ok(0);
    }
    let Read::Snapshot(s) = call::library_read(&data(), &saved.id, None)? else { unreachable!() };
    line(&format!("Saved {}: {}{}.", saved.id, s.summary(), if saved.replaced { "; the old one is in the trash" } else { "" }));
    Ok(0)
}

/// `diff A B` compares two snapshots; `diff A` compares one with the camera, as a plan that
/// is made, shown and discarded.
pub fn snapshots_diff(out: Out, a: &str, b: Option<&str>, allow_unconfirmed: bool) -> Result<i32, Fail> {
    let a = id_of(Kind::Snapshot, a)?;
    if let Some(b) = b {
        let Read::Diff(d) = call::library_read(&data(), &a, Some(&id_of(Kind::Snapshot, b)?))? else { unreachable!() };
        if out.json {
            out.data(&d);
            return Ok(0);
        }
        let n = d.changed.len() + d.added.len() + d.removed.len();
        line(&format!("{n} {} between {} and {}; {} alike.", if n == 1 { "setting differs" } else { "settings differ" }, d.a, d.b, d.same));
        let label = |l: &Option<String>, v: &ukiyo_core::read::RawValue| l.clone().unwrap_or_else(|| v.text());
        let mut rows: Vec<Vec<String>> = d.changed.iter().map(|c| vec![format!("  {}", c.code), c.name.clone().unwrap_or_default(), format!("{} -> {}", label(&c.a_label, &c.a), label(&c.b_label, &c.b))]).collect();
        rows.extend(d.removed.iter().map(|v| vec![format!("  {}", v.code), v.name.clone().unwrap_or_default(), format!("{} -> (not in {})", label(&v.label, &v.value), d.b)]));
        rows.extend(d.added.iter().map(|v| vec![format!("  {}", v.code), v.name.clone().unwrap_or_default(), format!("(not in {}) -> {}", d.a, label(&v.label, &v.value))]));
        table(&rows).iter().for_each(|l| line(l));
        return Ok(0);
    }
    call::library_read(&data(), &a, None)?;
    let c = conn::client(out, None);
    let r = (|| {
        conn::settled(&c, None)?;
        match c.call(Request::PlanCreate { set: IndexMap::new(), allow_unconfirmed, from: Some(a.clone()) })? {
            Response::PlanCreate(p) => {
                let _ = c.call(Request::PlanDiscard { plan_id: p.plan_id.clone() });
                Ok(*p)
            }
            r => Err(Fail::new("failed", format!("unexpected reply: {}", serde_json::to_value(r).unwrap_or_default()))),
        }
    })();
    c.close();
    let p = r?;
    if out.json {
        out.data(json!({ "snapshot": a, "plan": p }));
        return Ok(0);
    }
    let n = p.steps.len() + p.blocked.len();
    line(&format!("{n} {} on the camera from {a} (camera -> snapshot):", if n == 1 { "setting differs" } else { "settings differ" }));
    writes::plan_lines(&p).iter().skip(1).for_each(|l| line(l));
    line(&format!("Restore: ukiyo snapshots restore {a}"));
    Ok(0)
}

pub fn snapshots_restore(out: Out, id: &str, flags: WriteFlags) -> Result<i32, Fail> {
    let id = id_of(Kind::Snapshot, id)?;
    call::library_read(&data(), &id, None)?;
    writes::plan_from(out, id, flags)
}

// ---- settings fav ----

/// The catalog for names: the running camera's, else the only catalog there is.
fn catalog() -> Option<Catalog> {
    let c = conn::existing(Duration::from_millis(500));
    let snap = conn::snapshot(&c).ok();
    c.close();
    snap.and_then(|s| s.catalog).or_else(|| crate::cmds::catalog_path(None).ok().and_then(|p| Catalog::load(&p).ok()))
}

pub fn fav(out: Out, add: bool, names: &[String]) -> Result<i32, Fail> {
    let d = data();
    if add {
        let cat = catalog();
        let saved = call::library_save(&d, ACTOR, &SaveInput { kind: SaveKind::Favourite, name: None, item: Some(json!(names)), from: None }, Context { camera: None, catalog: cat.as_ref() })?;
        if out.json {
            out.data(&saved);
        } else {
            line(&format!("Favourites: added {}.", saved.id));
        }
        return Ok(0);
    }
    let favs = library::Store::new(&d).favourites();
    let mut removed = vec![];
    for n in names {
        let code = ukiyo_core::read::parse_code(n).or_else(|| favs.items.iter().find(|f| f.name.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(n))).and_then(|f| ukiyo_core::read::parse_code(&f.code)));
        let code = code.ok_or_else(|| Fail::new("not_found", format!("{n} is not a favourite")).hint("ukiyo settings --favourites lists them"))?;
        removed.push(call::library_delete(&d, ACTOR, &format!("favourite:{}", ukiyo_core::catalog::key(code)))?);
    }
    if out.json {
        out.data(json!({ "removed": removed }));
    } else {
        line(&format!("Favourites: removed {}.", removed.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")));
    }
    Ok(0)
}

// ---- prefs ----

fn prefs_lines(p: &Prefs) -> Vec<String> {
    let on = |b: bool| if b { "on" } else { "off" }.to_string();
    let theme = serde_json::to_value(p.theme).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    table(&[
        vec!["theme".into(), theme],
        vec!["brand-mark".into(), p.brand_mark.clone()],
        vec!["app-icon".into(), format!("{} (applied at the next app start)", p.app_icon)],
        vec!["keep-awake".into(), on(p.keep_awake)],
        vec!["in-use-minutes".into(), p.in_use_minutes.to_string()],
        vec!["close-menu-to-write".into(), on(p.close_menu_to_write)],
    ])
}

pub fn prefs_get(out: Out) -> Result<i32, Fail> {
    let p = call::prefs_get(&data());
    if out.json {
        out.data(&p);
    } else {
        prefs_lines(&p).iter().for_each(|l| line(l));
    }
    Ok(0)
}

pub fn prefs_set(out: Out, pairs: &[String]) -> Result<i32, Fail> {
    let mut patch = PrefsPatch::default();
    for it in pairs {
        let (k, v) = it.split_once('=').ok_or_else(|| Fail::usage(format!("not KEY=VALUE: {it}")).hint("ukiyo prefs set theme=dark keep-awake=on"))?;
        prefs::parse_pair(&mut patch, k, v)?;
    }
    patch.check()?;
    // Keep-awake reaches a running owner at once; with none, `serve` reads it at its start.
    if let Some(on) = patch.keep_awake {
        let c = conn::existing(Duration::from_millis(500));
        let _ = c.call(Request::LabKeepAwake { on });
        c.close();
    }
    let p = call::prefs_set(&data(), &patch)?;
    if out.json {
        out.data(&p);
    } else {
        prefs_lines(&p).iter().for_each(|l| line(l));
    }
    Ok(0)
}

// ---- trash ----

pub fn trash_ls(out: Out) -> Result<i32, Fail> {
    hits(out, SearchKind::Trash, &[], "The trash is empty.").map(|_| 0)
}

pub fn trash_restore(out: Out, id: &str) -> Result<i32, Fail> {
    let id = if id.starts_with("trash:") { id.to_string() } else { format!("trash:{id}") };
    let saved = call::library_save(&data(), ACTOR, &SaveInput { kind: SaveKind::Preset, name: None, item: None, from: Some(id) }, Context::default())?;
    if out.json {
        out.data(&saved);
    } else {
        line(&format!("Restored {}{}.", saved.id, if saved.replaced { "; the one it replaced is in the trash" } else { "" }));
    }
    Ok(0)
}

// ---- history ----

/// The history of the camera with this id, if any.
pub fn history_of(camera_id: Option<&str>) -> Option<HistoryView> {
    let id = camera_id?;
    library::history::views(&data()).into_iter().find(|h| h.camera == id)
}

pub fn history_lines(h: &HistoryView) -> Vec<String> {
    let mut v = vec![format!(
        "History of this {}: first connected {}, {} connects{}",
        h.model.as_deref().unwrap_or("camera"),
        when(h.first_connected),
        h.connects.len(),
        h.shots.map(|s| format!(", {s} shots ({})", h.counter.as_deref().unwrap_or(""))).unwrap_or_default()
    )];
    if h.per_month.is_empty() {
        v.push("Shots per month: none counted yet (counters read in tether stills mode)".into());
    } else {
        v.push(format!("Shots per month: {}", h.per_month.iter().map(|m| format!("{} {}", m.month, m.shots)).collect::<Vec<_>>().join(", ")));
    }
    for c in h.connects.iter().take(5) {
        v.push(format!("  {}  {}{}", when(c.at), c.mode.as_deref().unwrap_or("-"), c.shots.map(|s| format!("  {s} shots")).unwrap_or_default()));
    }
    v
}
