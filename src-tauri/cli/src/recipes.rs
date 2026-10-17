//! The recipe commands (stage 7): `recipes ls|search|show|import|edit|rm|read|write`. The
//! library and the bundled catalogue are read in this process; reading a slot and writing a
//! recipe to one go through `serve` (USB RAW CONV. mode), a slot write as a plan.

use crate::conn;
use crate::out::{line, table, Fail, Out};
use crate::writes::{self, WriteFlags};
use serde_json::json;
use ukiyo_core::api::{call, Request, Response};
use ukiyo_core::library::{self, Context, Read, SaveInput, SaveKind, SearchInput, SearchKind};
use ukiyo_core::paths;
use ukiyo_core::read::Need;
use ukiyo_core::recipes::slot::{self, SlotView};
use ukiyo_core::recipes::{Note, Parsed, Settings};

const ACTOR: &str = "cli";

fn data() -> std::path::PathBuf {
    paths::data_dir()
}

/// `street`, `recipe:street` or `cat:<id>` as an id.
pub fn recipe_id(s: &str) -> String {
    let s = s.trim();
    if s.starts_with("recipe:") || s.starts_with("cat:") {
        s.to_string()
    } else {
        format!("recipe:{}", library::slug(s))
    }
}

/// A recipe's settings, one per line, as the camera menu names them.
pub fn settings_rows(s: &Settings) -> Vec<Vec<String>> {
    let t = |v: Option<f64>| v.map(|n| if n == 0.0 { "0".to_string() } else { format!("{n:+}") });
    let mut rows: Vec<(&str, Option<String>)> = vec![
        ("Film Simulation", s.film_simulation.clone()),
        ("Dynamic Range", s.dynamic_range.clone()),
        ("D-Range Priority", s.d_range_priority.clone()),
        ("Grain Effect", s.grain_effect.clone().map(|g| match (&s.grain_size, g.as_str()) {
            (Some(z), g) if g != "Off" => format!("{g}, {z}"),
            _ => g,
        })),
        ("Color Chrome Effect", s.color_chrome.clone()),
        ("Color Chrome FX Blue", s.color_chrome_fx_blue.clone()),
        ("Smooth Skin Effect", s.smooth_skin.clone()),
        (
            "White Balance",
            s.white_balance.clone().map(|w| match (w.as_str(), s.color_temp) {
                ("Kelvin", Some(k)) => format!("{k}K"),
                _ => w,
            }),
        ),
    ];
    if s.wb_shift_r.is_some() || s.wb_shift_b.is_some() {
        rows.push(("WB Shift", Some(format!("R {:+}, B {:+}", s.wb_shift_r.unwrap_or(0), s.wb_shift_b.unwrap_or(0)))));
    }
    rows.extend([
        ("Highlight", t(s.highlight_tone)),
        ("Shadow", t(s.shadow_tone)),
        ("Color", t(s.color)),
        ("Sharpness", t(s.sharpness)),
        ("High ISO NR", t(s.high_iso_nr)),
        ("Clarity", t(s.clarity)),
        ("Monochromatic Color WC", t(s.monochromatic_color_wc)),
        ("Monochromatic Color MG", t(s.monochromatic_color_mg)),
    ]);
    rows.into_iter().filter_map(|(k, v)| v.map(|v| vec![format!("  {k}"), v])).collect()
}

fn print_settings(s: &Settings, notes: &[Note]) {
    table(&settings_rows(s)).iter().for_each(|l| line(l));
    for n in notes {
        line(&format!("  {} (not in a slot): {}", n.label, n.value));
    }
}

fn hits(out: Out, kind: SearchKind, query: &[String], sensor: Option<String>, sim: Option<String>, limit: u32, empty: &str) -> Result<i32, Fail> {
    let q = query.join(" ");
    let h = call::library_search(&data(), &SearchInput { kind, query: (!q.trim().is_empty()).then_some(q), limit: Some(limit), sensor, film_sim: sim });
    if out.json {
        out.data(&h);
        return Ok(0);
    }
    if h.items.is_empty() {
        line(empty);
    }
    let rows: Vec<Vec<String>> = h.items.iter().map(|i| vec![i.id.clone(), i.name.clone(), i.summary.clone()]).collect();
    table(&rows).iter().for_each(|l| line(l));
    if h.has_more {
        line(&format!("{} of {}; narrow the search, or --limit N", h.items.len(), h.total));
    }
    Ok(0)
}

pub fn ls(out: Out, query: &[String], sensor: Option<String>, sim: Option<String>) -> Result<i32, Fail> {
    hits(out, SearchKind::Recipe, query, sensor, sim, 1000, "No recipes. Import one: ukiyo recipes import --url URL --save, or ukiyo recipes search")
}

pub fn search(out: Out, query: &[String], sensor: Option<String>, sim: Option<String>, limit: u32) -> Result<i32, Fail> {
    hits(out, SearchKind::Catalogue, query, sensor, sim, limit, "No catalogue recipe matches.")
}

pub fn show(out: Out, id: &str) -> Result<i32, Fail> {
    let r = call::library_read(&data(), &recipe_id(id), None)?;
    if out.json {
        out.data(&r);
        return Ok(0);
    }
    match r {
        Read::Recipe(r) => {
            line(&format!("{} ({}){}", r.name, r.id, r.sensor.as_deref().map(|s| format!(", {s}")).unwrap_or_default()));
            print_settings(&r.settings, &r.notes);
            if let Some(src) = &r.source {
                line(&format!("From: {}{}", src.credit.clone().unwrap_or_default(), src.url.as_deref().map(|u| format!(" {u}")).unwrap_or_default()));
            }
            library::recipes::slot_warnings(&r.settings).iter().for_each(|w| line(&format!("Warning: {w}")));
        }
        Read::Catalogue(c) => {
            line(&format!("{} (cat:{}){}", c.name, c.id, c.sensor.as_deref().map(|s| format!(", {s}")).unwrap_or_default()));
            print_settings(&c.settings, &c.notes);
            line(&format!("From: {} {}", c.credit.author.clone().unwrap_or_else(|| c.credit.name.clone()), c.credit.url));
            line(&format!("Keep it: ukiyo recipes import cat:{} --save", c.id));
        }
        _ => unreachable!("a recipe id reads a recipe"),
    }
    Ok(0)
}

pub struct Import {
    pub text: Option<String>,
    pub url: Option<String>,
    pub cat: Option<String>,
    pub name: Option<String>,
    pub save: bool,
}

fn parsed_lines(p: &Parsed) {
    line(&format!("{}{}", p.name.clone().unwrap_or_else(|| "Recipe".into()), p.sensor.as_deref().map(|s| format!(" ({s})")).unwrap_or_default()));
    print_settings(&p.settings, &p.notes);
    for w in &p.warnings {
        line(&format!("  Warning: {} {}: {}", w.label, w.value, w.message));
    }
    for o in &p.others {
        line(&format!("Also in the post: {}{}", o.name, o.sensor.as_deref().map(|s| format!(" ({s})")).unwrap_or_default()));
    }
}

pub fn import(out: Out, a: Import) -> Result<i32, Fail> {
    let sources = [a.text.is_some(), a.url.is_some(), a.cat.is_some()].iter().filter(|x| **x).count();
    if sources != 1 {
        return Err(Fail::usage("give the recipe one way: --text FILE (or - for stdin), --url URL, or cat:<id>"));
    }
    if let Some(cat) = &a.cat {
        let id = if cat.starts_with("cat:") { cat.clone() } else { format!("cat:{cat}") };
        if !a.save {
            return show(out, &id);
        }
        let saved = call::library_save(&data(), ACTOR, &SaveInput { kind: SaveKind::Recipe, name: a.name, item: None, from: Some(id) }, Context::default())?;
        return saved_lines(out, &saved);
    }
    let text = match a.text.as_deref() {
        Some("-") => {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|e| Fail::new("failed", e.to_string()))?;
            Some(s)
        }
        Some(f) => Some(std::fs::read_to_string(f).map_err(|e| Fail::usage(format!("{f}: {e}")))?),
        None => None,
    };
    if a.url.is_some() {
        out.progress("ukiyo: reading the post ...");
    }
    let p = call::recipe_parse(text.as_deref(), a.url.as_deref())?;
    if !a.save {
        if out.json {
            out.data(&p);
        } else {
            parsed_lines(&p);
            line("Not saved. Keep it: the same command with --save (and --name NAME).");
        }
        return Ok(0);
    }
    let name = a.name.or(p.name.clone()).ok_or_else(|| Fail::usage("the text names no recipe: give --name NAME"))?;
    let source = (p.source_url.is_some() || p.author.is_some()).then(|| json!({ "url": p.source_url, "credit": p.author }));
    let item = json!({ "settings": p.settings, "notes": p.notes, "sensor": p.sensor, "source": source });
    let saved = call::library_save(&data(), ACTOR, &SaveInput { kind: SaveKind::Recipe, name: Some(name), item: Some(item), from: None }, Context::default())?;
    saved_lines(out, &saved)
}

fn saved_lines(out: Out, saved: &library::Saved) -> Result<i32, Fail> {
    if out.json {
        out.data(saved);
    } else {
        line(&format!("Saved {}{}.", saved.id, if saved.replaced { "; the old version is in the trash" } else { "" }));
        saved.warnings.iter().for_each(|w| line(&format!("Warning: {w}")));
    }
    Ok(0)
}

/// `recipes edit ID FIELD=VALUE...`: labels as a post writes them ("Highlight=+1.5",
/// "Grain=Weak, Small"); an empty value clears the setting.
pub fn edit(out: Out, id: &str, pairs: &[String]) -> Result<i32, Fail> {
    let id = recipe_id(id);
    let Read::Recipe(r) = call::library_read(&data(), &id, None)? else {
        return Err(Fail::new("read_only", format!("{id} is in the catalogue; import it with --save to edit a copy")));
    };
    let mut set = serde_json::Map::new();
    for p in pairs {
        let (k, v) = p.split_once('=').ok_or_else(|| Fail::usage(format!("not FIELD=VALUE: {p}")).hint("ukiyo recipes edit street Highlight=+1 \"Grain=Weak, Small\""))?;
        set.insert(k.trim().to_string(), json!(v.trim()));
    }
    let saved = call::library_save(&data(), ACTOR, &SaveInput { kind: SaveKind::Recipe, name: Some(r.name), item: Some(json!({ "set": set })), from: None }, Context::default())?;
    saved_lines(out, &saved)
}

pub fn rm(out: Out, id: &str) -> Result<i32, Fail> {
    let t = call::library_delete(&data(), ACTOR, &recipe_id(id))?;
    if out.json {
        out.data(&t);
    } else {
        line(&format!("Moved {} to the trash for 30 days. Restore it: ukiyo trash restore {}", t.id, t.trash_id.as_deref().unwrap_or("")));
    }
    Ok(0)
}

fn slot_id(s: &str) -> Result<String, Fail> {
    slot::parse_slot(s).map(|n| format!("slot:{}", slot::slot_name(n))).ok_or_else(|| Fail::usage(format!("no slot {s}: pick C1 to C7")))
}

fn slot_lines(v: &SlotView) {
    if let Some(e) = &v.error {
        line(&format!("{}: {e}", v.slot));
        return;
    }
    let same = if v.matches.is_empty() { String::new() } else { format!(" = {}", v.matches.join(", ")) };
    line(&format!("{}  {}{same}", v.slot, if v.name.is_empty() { "(no name)" } else { &v.name }));
    print_settings(&v.settings, &[]);
    v.warnings.iter().for_each(|w| line(&format!("  Warning: {w}")));
}

/// `recipes read [SLOT] [--compare ID]`: one slot, or all seven, from the camera.
pub fn read(out: Out, slot_arg: Option<&str>, compare: Option<&str>) -> Result<i32, Fail> {
    let id = match slot_arg {
        Some(s) => slot_id(s)?,
        None if compare.is_some() => return Err(Fail::usage("--compare compares one slot: give the slot")),
        None => "slots".into(),
    };
    let c = conn::client(out, None);
    let r = (|| {
        conn::settled(&c, Some(Need::RawConv))?;
        match c.call(Request::LibraryRead { id, compare_to: compare.map(recipe_id) })? {
            Response::LibraryRead(r) => Ok(*r),
            r => Err(Fail::new("failed", format!("unexpected reply: {}", serde_json::to_value(r).unwrap_or_default()))),
        }
    })();
    c.close();
    let r = r?;
    if out.json {
        out.data(&r);
        return Ok(0);
    }
    match r {
        Read::Slots(v) => v.iter().for_each(slot_lines),
        Read::RecipeDiff(d) => {
            if d.changed.is_empty() {
                line(&format!("{} holds {} ({} settings alike).", d.a, d.b, d.same));
            } else {
                line(&format!("{} settings differ between {} and {}; {} alike.", d.changed.len(), d.a, d.b, d.same));
                let rows: Vec<Vec<String>> = d.changed.iter().map(|f| vec![format!("  {}", f.field), format!("{} -> {}", f.a, f.b)]).collect();
                table(&rows).iter().for_each(|l| line(l));
            }
        }
        _ => unreachable!("a slot read"),
    }
    Ok(0)
}

/// `recipes write SLOT ID`: a slot plan, shown, then applied after the prompt or `--yes`.
pub fn write(out: Out, slot_arg: &str, id: &str, flags: WriteFlags) -> Result<i32, Fail> {
    let n = slot::parse_slot(slot_arg).ok_or_else(|| Fail::usage(format!("no slot {slot_arg}: pick C1 to C7")))?;
    let id = recipe_id(id);
    call::library_read(&data(), &id, None)?;
    writes::plan_and_decide(out, Request::PlanCreate { set: Default::default(), allow_unconfirmed: flags.allow_unconfirmed, from: Some(id), slot: Some(slot::slot_name(n)) }, flags)
}
