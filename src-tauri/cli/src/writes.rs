//! The commands that change something: `settings set`, `owner set`, `plan`, `camera
//! connect|disconnect|export` and `log note`. Every camera write is a plan in `serve` first
//! (docs/agent-interface.md, section 5): the command prints the plan, applies it after a
//! y/N prompt or `--yes`, and prints each step's read-back.
//!
//! Each returns its exit code: 0 done, 6 not applied (`--dry-run`, declined, no terminal
//! without `--yes`), 7 applied in part.

use crate::conn;
use crate::out::{line, table, Fail, Out};
use indexmap::IndexMap;
use serde_json::json;
use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::time::Duration;
use ukiyo_core::api::{Request, Response};
use ukiyo_core::plan::{Applied, Plan};
use ukiyo_core::read::{self, Need};
use ukiyo_core::serve::client::Client;

/// `--dry-run`, `--yes` and `--allow-unconfirmed`, on every command that writes.
#[derive(Debug, Clone, Copy, Default, clap::Args)]
pub struct WriteFlags {
    /// Build and print the plan; apply nothing (exit 6).
    #[arg(long)]
    pub dry_run: bool,
    /// Apply without the y/N prompt. Without a terminal, a write needs it.
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Let the plan write codes whose catalog confidence is not `confirmed`.
    #[arg(long)]
    pub allow_unconfirmed: bool,
}

pub const DONE: i32 = 0;
pub const NOT_APPLIED: i32 = 6;
pub const PARTIAL: i32 = 7;

fn unexpected(r: Response) -> Fail {
    Fail::new("failed", format!("unexpected reply: {}", serde_json::to_value(r).unwrap_or_default()))
}

/// `NAME=VALUE` pairs, in order. The value may be empty (a string set to nothing).
fn pairs(items: &[String]) -> Result<IndexMap<String, String>, Fail> {
    let mut m = IndexMap::new();
    for it in items {
        let Some((k, v)) = it.split_once('=') else {
            return Err(Fail::usage(format!("not NAME=VALUE: {it}")).hint("ukiyo settings set \"AUTO POWER OFF=OFF\""));
        };
        if k.trim().is_empty() {
            return Err(Fail::usage(format!("no name before = in {it}")));
        }
        m.insert(k.trim().to_string(), v.to_string());
    }
    if m.is_empty() {
        return Err(Fail::usage("name at least one NAME=VALUE"));
    }
    Ok(m)
}

pub(crate) fn plan_lines(p: &Plan) -> Vec<String> {
    let mut needs: Vec<String> = p.requires.mode.iter().cloned().collect();
    if let Some(c) = &p.requires.capture {
        needs.push(format!("{c:?}").to_lowercase());
    }
    let needs = if needs.is_empty() { String::new() } else { format!(", needs {}", needs.join(" + ")) };
    let mins = (p.expires_in_s as f64 / 60.0).ceil() as u32;
    let slot = match (&p.slot, &p.recipe) {
        (Some(s), Some(r)) => format!(", writes {r} to slot {s}"),
        (Some(s), None) => format!(", slot {s}"),
        _ => String::new(),
    };
    let mut out = vec![format!("Plan {} (expires in {mins} min){needs}{slot}", p.plan_id)];
    let rows: Vec<Vec<String>> = p
        .steps
        .iter()
        .map(|s| {
            let gate = match s.gate.as_str() {
                "ok" => s.confidence.as_ref().map(|c| format!("{c:?}").to_lowercase()).unwrap_or_default(),
                g => format!("{} ({g})", s.confidence.as_ref().map(|c| format!("{c:?}").to_lowercase()).unwrap_or_else(|| "unmapped".into())),
            };
            let purpose = if s.purpose == ukiyo_core::plan::purpose::SET { String::new() } else { s.purpose.clone() };
            vec![format!("  {}", s.name.clone().unwrap_or_else(|| s.code.clone())), format!("{} -> {}", s.before, s.after), gate, purpose]
        })
        .collect();
    out.extend(table(&rows));
    for s in &p.steps {
        let who = s.name.clone().unwrap_or_else(|| s.code.clone());
        if let Some(n) = &s.note {
            out.push(format!("  {who} ({}): {n}", s.purpose));
        }
        for r in &s.resets {
            out.push(format!("  {who} -> {}: the camera also sets {r}", s.after));
        }
    }
    out.extend(p.warnings.iter().map(|w| format!("  Warning: {w}")));
    out.extend(left_lines("Blocked", &p.blocked));
    out.extend(left_lines("Skipped", &p.skipped));
    out
}

/// Blocked or skipped entries: each on a line, or, past 3 of one reason (a snapshot
/// restore), one line per reason with the names.
fn left_lines(what: &str, left: &[ukiyo_core::plan::Left]) -> Vec<String> {
    let mut by: IndexMap<&str, Vec<&ukiyo_core::plan::Left>> = IndexMap::new();
    for l in left {
        by.entry(l.reason.as_str()).or_default().push(l);
    }
    let mut out = vec![];
    for (reason, ls) in by {
        if ls.len() <= 3 {
            out.extend(ls.iter().map(|l| format!("  {what}: {} ({reason})", l.message)));
        } else if reason == ukiyo_core::plan::UNCHANGED {
            out.push(format!("  {what}: {} settings already at their value", ls.len()));
        } else {
            let names: Vec<String> = ls.iter().map(|l| l.name.clone().unwrap_or_else(|| l.code.clone())).collect();
            out.push(format!("  {what}: {} settings ({reason}): {}", ls.len(), names.join(", ")));
        }
    }
    out
}

pub(crate) fn applied_lines(a: &Applied, total: usize) -> Vec<String> {
    let rows: Vec<Vec<String>> = a
        .steps
        .iter()
        .map(|s| {
            let res = match &s.error {
                None => s.result.clone(),
                Some(e) => format!("{} ({})", e.name(), s.result),
            };
            let tries = if s.attempts > 1 { format!(", {} tries", s.attempts) } else { String::new() };
            vec![s.name.clone().unwrap_or_else(|| s.code.clone()), s.wanted.clone(), format!("read back {}", s.readback.clone().unwrap_or_else(|| "nothing".into())), format!("{res}{tries}")]
        })
        .collect();
    let mut out = table(&rows);
    for e in &a.side_effects {
        let label = |l: &Option<String>, v: &Option<read::RawValue>| l.clone().or_else(|| v.as_ref().map(read::RawValue::text)).unwrap_or_else(|| "-".into());
        out.push(format!("Also changed: {} {}  {} -> {}", e.code, e.name.clone().unwrap_or_default(), label(&e.old_label, &e.old), label(&e.new_label, &e.new)));
    }
    out.extend(a.warnings.iter().map(|w| format!("Warning: {w}")));
    out.extend(a.shots.iter().map(|s| format!("Saved {} ({} KB): {}", s.name, s.bytes / 1024, s.path)));
    if a.not_attempted > 0 {
        out.push(format!("{} step(s) not attempted after the failure.", a.not_attempted));
    }
    let undo = a.undo_plan_id.as_deref().map(|u| format!(" Undo: ukiyo plan apply {u}")).unwrap_or_default();
    out.push(format!("Applied {} of {total}.{undo}", a.applied));
    out
}

/// Asks y/N on the terminal. `None`: no terminal to ask on.
fn confirm(out: Out) -> Option<bool> {
    if out.json || !std::io::stdin().is_terminal() {
        return None;
    }
    eprint!("Apply? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).ok()?;
    Some(matches!(answer.trim(), "y" | "Y" | "yes"))
}

/// Shows a plan, then applies it when the flags or the person say so.
fn decide(out: Out, c: &Client, plan: Plan, flags: WriteFlags) -> Result<i32, Fail> {
    let not_applied = |why: &str| -> Result<i32, Fail> {
        if out.json {
            out.data(json!({ "plan": plan, "result": null }));
        } else {
            plan_lines(&plan).iter().for_each(|l| line(l));
            out.progress(&format!("Not applied ({why}). Apply it: ukiyo plan apply {} --yes", plan.plan_id));
        }
        Ok(NOT_APPLIED)
    };
    if flags.dry_run {
        return not_applied("--dry-run");
    }
    if !flags.yes {
        if !out.json {
            plan_lines(&plan).iter().for_each(|l| line(l));
        }
        return match confirm(out) {
            Some(true) => apply(out, c, plan),
            Some(false) => {
                out.progress("Not applied.");
                Ok(NOT_APPLIED)
            }
            None if out.json => not_applied("no --yes"),
            None => {
                out.progress(&format!("Not applied: no terminal to confirm on. Pass --yes, or: ukiyo plan apply {} --yes", plan.plan_id));
                Ok(NOT_APPLIED)
            }
        };
    }
    if !out.json {
        plan_lines(&plan).iter().for_each(|l| line(l));
    }
    apply(out, c, plan)
}

fn apply(out: Out, c: &Client, plan: Plan) -> Result<i32, Fail> {
    let a = match c.call(Request::PlanApply { plan_id: plan.plan_id.clone() })? {
        Response::PlanApply(a) => *a,
        r => return Err(unexpected(r)),
    };
    let code = if a.failed > 0 { PARTIAL } else { DONE };
    if out.json {
        out.data(json!({ "plan": plan, "result": a }));
    } else {
        applied_lines(&a, plan.steps.len()).iter().for_each(|l| line(l));
    }
    Ok(code)
}

/// Plans `set` and carries it through: the common body of `settings set` and `owner set`.
fn set(out: Out, set: IndexMap<String, String>, flags: WriteFlags) -> Result<i32, Fail> {
    plan_and_decide(out, Request::PlanCreate { set, allow_unconfirmed: flags.allow_unconfirmed, from: None, slot: None, capture: false }, flags)
}

/// Plans a library item (`preset:<slug>`, `snapshot:<slug>`) and carries it through:
/// `presets apply` and `snapshots restore`.
pub fn plan_from(out: Out, from: String, flags: WriteFlags) -> Result<i32, Fail> {
    plan_and_decide(out, Request::PlanCreate { set: IndexMap::new(), allow_unconfirmed: flags.allow_unconfirmed, from: Some(from), slot: None, capture: false }, flags)
}

/// `ukiyo camera capture`: a capture plan, shown, then applied on yes.
pub fn camera_capture(out: Out, flags: WriteFlags) -> Result<i32, Fail> {
    plan_and_decide(out, Request::PlanCreate { set: IndexMap::new(), allow_unconfirmed: false, from: None, slot: None, capture: true }, flags)
}

pub(crate) fn plan_and_decide(out: Out, req: Request, flags: WriteFlags) -> Result<i32, Fail> {
    let c = conn::client(out, None);
    let r = (|| {
        conn::settled(&c, None)?;
        let plan = match c.call(req)? {
            Response::PlanCreate(p) => *p,
            r => return Err(unexpected(r)),
        };
        if plan.steps.is_empty() {
            let _ = c.call(Request::PlanDiscard { plan_id: plan.plan_id.clone() });
            if let Some(e) = plan.empty_reason() {
                return Err(e.into());
            }
            if out.json {
                out.data(json!({ "plan": plan, "result": null }));
            } else {
                plan_lines(&plan).iter().skip(1).for_each(|l| line(l));
                line("Nothing to change.");
            }
            return Ok(DONE);
        }
        decide(out, &c, plan, flags)
    })();
    c.close();
    r
}

pub fn settings_set(out: Out, items: &[String], flags: WriteFlags) -> Result<i32, Fail> {
    set(out, pairs(items)?, flags)
}

fn owner_fields() -> String {
    read::OWNER_FIELDS.iter().map(|(f, _)| *f).collect::<Vec<_>>().join(", ")
}

pub fn owner_set(out: Out, items: &[String], flags: WriteFlags) -> Result<i32, Fail> {
    let mut named = IndexMap::new();
    for (k, v) in pairs(items)? {
        let name = read::owner_field(&k).ok_or_else(|| Fail::usage(format!("no owner field {k}: pick {}", owner_fields())))?;
        named.insert(name.to_string(), v);
    }
    set(out, named, flags)
}

pub fn owner_get(out: Out) -> Result<i32, Fail> {
    let c = conn::client(out, None);
    let snap = conn::settled(&c, Some(Need::Tether));
    c.close();
    let fields = read::owner_view(&snap?);
    if out.json {
        out.data(json!({ "fields": fields }));
        return Ok(DONE);
    }
    let rows: Vec<Vec<String>> = fields
        .iter()
        .map(|f| {
            let value = match (&f.value, f.available) {
                (Some(v), true) if v.is_empty() => "(empty)".to_string(),
                (Some(v), true) => v.clone(),
                _ => "n/a".into(),
            };
            vec![f.field.to_string(), value, f.code.clone().unwrap_or_else(|| "-".into()), f.confidence.as_ref().map(|c| format!("{c:?}").to_lowercase()).unwrap_or_else(|| "-".into())]
        })
        .collect();
    table(&rows).iter().for_each(|l| line(l));
    Ok(DONE)
}

// ---- plan ----

fn fetch(c: &Client, plan_id: &str) -> Result<Plan, Fail> {
    match c.call(Request::PlanShow { plan_id: plan_id.into() }).map_err(no_serve)? {
        Response::PlanShow(p) => Ok(*p),
        r => Err(unexpected(r)),
    }
}

/// Plans live in `serve`; with none running there is no plan to find.
fn plan_client() -> std::sync::Arc<Client> {
    conn::existing(Duration::from_secs(2))
}

fn no_serve(e: ukiyo_core::api::Error) -> Fail {
    if e.code == ukiyo_core::api::ErrorCode::Unavailable {
        Fail::new("plan_expired", "no ukiyo serve runs, so no plan exists; plans end when serve stops").hint("Make a new plan.")
    } else {
        e.into()
    }
}

pub fn plan_show(out: Out, plan_id: &str) -> Result<i32, Fail> {
    let c = plan_client();
    let p = fetch(&c, plan_id);
    c.close();
    let p = p?;
    if out.json {
        out.data(&p);
    } else {
        plan_lines(&p).iter().for_each(|l| line(l));
    }
    Ok(DONE)
}

pub fn plan_apply(out: Out, plan_id: &str, flags: WriteFlags) -> Result<i32, Fail> {
    let c = plan_client();
    let r = fetch(&c, plan_id).and_then(|plan| decide(out, &c, plan, flags));
    c.close();
    r
}

pub fn plan_discard(out: Out, plan_id: &str) -> Result<i32, Fail> {
    let c = plan_client();
    let r = c.call(Request::PlanDiscard { plan_id: plan_id.into() }).map_err(no_serve);
    c.close();
    r?;
    if out.json {
        out.data(json!({ "plan_id": plan_id, "discarded": true }));
    } else {
        line(&format!("Discarded {plan_id}."));
    }
    Ok(DONE)
}

// ---- camera connect | disconnect | export ----

/// `ukiyo camera transport [usb|wifi]`: saves the preference, and a running owner switches
/// at once (it closes the session cleanly first). Never starts `serve` for it.
pub fn camera_transport(out: Out, kind: Option<&str>) -> Result<i32, Fail> {
    use ukiyo_core::camera::transport::TransportKind;
    let data = ukiyo_core::paths::data_dir();
    let Some(k) = kind else {
        let p = ukiyo_core::api::call::prefs_get(&data);
        let now = ukiyo_core::lab::transport_choice(&p);
        if out.json {
            out.data(json!({ "transport": now, "saved": p.transport }));
        } else {
            line(&format!("Transport: {}{}", now.label(), if now != p.transport { " (UKIYO_TRANSPORT)" } else { "" }));
        }
        return Ok(DONE);
    };
    let k = TransportKind::parse(k).ok_or_else(|| Fail::usage(format!("no transport {k:?}")).hint("ukiyo camera transport usb|wifi"))?;
    let c = conn::existing(Duration::from_millis(500));
    let running = c.call(Request::LabTransport { kind: k }).is_ok();
    c.close();
    let patch = ukiyo_core::library::prefs::PrefsPatch { transport: Some(k), ..Default::default() };
    ukiyo_core::api::call::prefs_set(&data, &patch)?;
    if out.json {
        out.data(json!({ "transport": k, "serve": running }));
    } else {
        line(&format!("Transport: {}.{}", k.label(), if running { " ukiyo serve looks for the camera that way now." } else { "" }));
        if k == TransportKind::Wifi {
            line("Set CONNECTION MODE to WIRELESS TETHER SHOOTING FIXED; the camera must be on the Mac's network.");
            line(ukiyo_core::read::WIFI_BATTERY_NOTE);
        }
    }
    Ok(DONE)
}

pub fn camera_connect(out: Out, on: bool) -> Result<i32, Fail> {
    // Disconnecting needs no owner when none runs: nothing holds the camera then.
    let c = if on { conn::client(out, None) } else { conn::existing(Duration::from_secs(2)) };
    let r = c.call(Request::LabConnect { on }).and_then(|_| {
        // The switch is a queued request: wait until the owner has closed the session, so
        // the next command never sees the camera still connected.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !on && std::time::Instant::now() < deadline {
            match c.call(Request::LabSnapshot)? {
                Response::LabSnapshot(s) if s.status.link == ukiyo_core::camera::owner::Link::Stopped => break,
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        Ok(())
    });
    c.close();
    match r {
        Ok(()) => {}
        Err(e) if !on && e.code == ukiyo_core::api::ErrorCode::Unavailable => {
            if out.json {
                out.data(json!({ "connect": false, "serve": false }));
            } else {
                line("No ukiyo serve runs; nothing holds the camera.");
            }
            return Ok(DONE);
        }
        Err(e) => return Err(e.into()),
    }
    if out.json {
        out.data(json!({ "connect": on, "serve": true }));
    } else if on {
        line("Connect switch on. ukiyo serve looks for the camera every 3 s, and waits 30 s after it left.");
    } else {
        line("Disconnected: session closed cleanly. ukiyo camera connect turns it back on.");
    }
    Ok(DONE)
}

pub fn camera_export(out: Out) -> Result<i32, Fail> {
    let c = conn::client(out, None);
    let r = (|| {
        conn::settled(&c, Some(Need::Connected))?;
        out.progress("ukiyo: reading every descriptor and value ...");
        match c.call(Request::LabExport)? {
            Response::LabExport(p) => Ok(p),
            r => Err(unexpected(r)),
        }
    })();
    c.close();
    let path = r?;
    if out.json {
        out.data(json!({ "path": path }));
    } else {
        line(&path);
    }
    Ok(DONE)
}

// ---- log note ----

pub fn log_note(out: Out, text: &str, group: Option<u32>) -> Result<i32, Fail> {
    if text.trim().is_empty() {
        return Err(Fail::usage("the note is empty"));
    }
    // A note goes into the running session's log; it is no reason to claim the camera.
    let c = conn::existing(Duration::from_secs(2));
    let r = c.call(Request::LabNote { group, text: text.into() });
    c.close();
    if let Err(e) = r {
        if e.code == ukiyo_core::api::ErrorCode::Unavailable {
            return Err(Fail::new("unavailable", "no ukiyo serve runs, so there is no session log to note in").hint("Notes go into a running session: start one with ukiyo status."));
        }
        return Err(e.into());
    }
    if out.json {
        out.data(json!({ "noted": true, "group": group }));
    } else {
        line("Noted.");
    }
    Ok(DONE)
}
