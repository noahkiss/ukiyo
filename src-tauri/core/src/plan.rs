//! Plans: every camera write is a plan first (docs/agent-interface.md, section 5).
//!
//! - [`build`] resolves names and values against the Lab's snapshot and gates each step:
//!   the property reads in this mode, is writable, is `confirmed` in the catalog (unless the
//!   plan allows unconfirmed codes), and the value is one the camera and the catalog allow.
//! - The Lab stores the plan for [`LIFETIME`]; applying it re-checks that every planned code
//!   still holds the value it had at planning (`plan_stale` otherwise), then writes the steps
//!   in order through the owner, which reads each one back. The first failed step stops the
//!   apply; an undo plan restores what was written.
//!
//! This module is pure: it reads snapshots and owner outcomes. `lab` stores plans and writes.

use crate::api::{Error, ErrorCode};
use crate::camera::owner::{refusal, Capture, ChangeGroup, ChangeSource, Link, PropRow, Screen, UsbMode, WriteOutcome};
use crate::camera::ptp::{self, Form, Value};
use crate::catalog::{self, Catalog, Confidence, Entry};
use crate::lab::Snapshot;
use crate::read::{self, ChangeView, Need, RawValue};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::time::{Duration, Instant};

/// How long a plan can be applied after it was made.
pub const LIFETIME: Duration = Duration::from_secs(300);

/// What a plan asks for: names (catalog name, menu label or `0x` code) to values (a value
/// label or a raw number; the text itself for a string property).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct PlanInput {
    pub set: IndexMap<String, String>,
    /// Lets the plan write codes whose catalog confidence is not `confirmed`.
    #[serde(default)]
    pub allow_unconfirmed: bool,
}

/// The camera state a plan was made in, and needs at apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Requires {
    /// `tether`, `rawconv` ... (`ukiyo help modes`).
    pub mode: Option<String>,
    pub capture: Option<Capture>,
}

/// One write the plan will make.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Step {
    pub code: String,
    pub name: Option<String>,
    pub menu: Option<String>,
    /// The value now, as the camera shows it.
    pub before: String,
    pub after: String,
    pub before_value: Option<RawValue>,
    pub after_value: RawValue,
    pub confidence: Option<Confidence>,
    /// `ok`; `unconfirmed_allowed` when only the plan's flag lets it through; `undo` in an
    /// undo plan.
    pub gate: String,
}

/// A requested change the plan will not make.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Left {
    pub code: String,
    pub name: Option<String>,
    /// The value asked for, as given.
    pub value: String,
    /// An error code (`needs_movie`, `unconfirmed_property`, `value_out_of_range`,
    /// `read_only` ...) or `unchanged`.
    pub reason: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Plan {
    pub plan_id: String,
    pub expires_in_s: u32,
    pub requires: Requires,
    pub steps: Vec<Step>,
    /// Not written in this camera state, or already at the value.
    pub skipped: Vec<Left>,
    /// Refused by a gate.
    pub blocked: Vec<Left>,
    pub allow_unconfirmed: bool,
}

impl Plan {
    /// When the plan has no step: the error that explains why, first blocked then skipped.
    /// `None` when every request was already at its value (nothing to do) or there were none.
    pub fn empty_reason(&self) -> Option<Error> {
        if !self.steps.is_empty() {
            return None;
        }
        let l = self.blocked.first().or_else(|| self.skipped.iter().find(|l| l.reason != UNCHANGED))?;
        Some(Error::new(ErrorCode::from_name(&l.reason), l.message.clone()))
    }
}

/// One step as applied.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AppliedStep {
    pub code: String,
    pub name: Option<String>,
    pub wanted: String,
    /// The value read back right after the write; `None` when the read failed.
    pub readback: Option<String>,
    pub wanted_value: RawValue,
    pub readback_value: Option<RawValue>,
    pub ok: bool,
    /// The camera's answer (`OK`, `DeviceBusy`), or why nothing was sent.
    pub result: String,
    /// The typed error of a failed step: `readback_mismatch`, `busy`, `menu_open` ...
    pub error: Option<ErrorCode>,
    /// Writes sent: more than 1 when the camera answered busy first.
    pub attempts: u32,
    /// A half-press closed a camera menu before the write went through.
    #[serde(default)]
    pub closed_menu: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Applied {
    pub plan_id: String,
    pub applied: u32,
    pub failed: u32,
    pub not_attempted: u32,
    /// A plan that writes back what this one changed. Apply it to revert.
    pub undo_plan_id: Option<String>,
    pub steps: Vec<AppliedStep>,
    /// Other codes that changed while the plan was applied.
    pub side_effects: Vec<ChangeView>,
}

pub const UNCHANGED: &str = "unchanged";

/// One step's raw values, kept with the stored plan.
#[derive(Debug, Clone)]
pub struct Raw {
    pub code: u16,
    pub before: Option<Value>,
    pub after: Value,
}

/// A plan as the Lab stores it.
#[derive(Debug, Clone)]
pub struct Stored {
    pub plan: Plan,
    pub raw: Vec<Raw>,
    pub mode: Option<UsbMode>,
    pub capture: Option<Capture>,
    pub created: Instant,
}

impl Stored {
    pub fn expired(&self) -> bool {
        self.created.elapsed() >= LIFETIME
    }

    /// The plan with its time left now.
    pub fn view(&self) -> Plan {
        let mut p = self.plan.clone();
        p.expires_in_s = LIFETIME.saturating_sub(self.created.elapsed()).as_secs() as u32;
        p
    }
}

fn err(code: ErrorCode, message: impl Into<String>) -> Error {
    Error::new(code, message)
}

fn entry(cat: Option<&Catalog>, code: u16) -> Option<&Entry> {
    cat?.properties.get(&catalog::key(code))
}

fn describe(code: u16, e: Option<&Entry>) -> String {
    match e.and_then(|e| e.name.as_deref()) {
        Some(n) => format!("{} {n}", read::code_text(code)),
        None => read::code_text(code),
    }
}

/// Whether the camera can take writes now: connected, and not in a menu.
pub fn ready(snap: &Snapshot) -> Result<(), Error> {
    read::readiness(&snap.status, snap.rows.len(), Need::Connected)?;
    if snap.status.screen == Some(Screen::Menu) {
        return Err(err(ErrorCode::MenuOpen, "A menu is open on the camera; it answers nothing until the menu closes.").hint("Close the SET UP or NETWORK/USB SETTING menu on the camera."));
    }
    if snap.status.screen == Some(Screen::Playback) {
        return Err(err(ErrorCode::InPlayback, "The camera shows playback; it answers nothing until it returns to shooting.").hint("Leave playback on the camera: press the playback button."));
    }
    Ok(())
}

/// Finds the code a name means: a `0x` code, a catalog name, a menu path or its last part,
/// or four hex digits. Any case.
pub fn resolve_code(snap: &Snapshot, name: &str) -> Result<u16, Error> {
    let n = name.trim();
    let not_found = || err(ErrorCode::NotFound, format!("no setting called {n:?}")).hint("ukiyo settings QUERY finds a setting; ukiyo catalog show lists every code.");
    if n.starts_with("0x") || n.starts_with("0X") {
        return read::parse_code(n).ok_or_else(not_found);
    }
    let cat = snap.catalog.as_ref();
    let props = cat.map(|c| c.properties.iter().collect::<Vec<_>>()).unwrap_or_default();
    let code_of = |k: &str| read::parse_code(k);
    let mut hits: Vec<u16> = props.iter().filter(|(_, e)| e.name.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(n))).filter_map(|(k, _)| code_of(k)).collect();
    if hits.is_empty() {
        hits = props
            .iter()
            .filter(|(_, e)| {
                e.menu.as_deref().is_some_and(|m| m.trim().eq_ignore_ascii_case(n) || m.rsplit('>').next().is_some_and(|last| last.trim().eq_ignore_ascii_case(n)))
            })
            .filter_map(|(k, _)| code_of(k))
            .collect();
    }
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => match read::parse_code(n).filter(|_| n.len() == 4) {
            Some(c) => Ok(c),
            None => Err(not_found()),
        },
        many => {
            let list: Vec<String> = many.iter().map(|c| describe(*c, entry(cat, *c))).collect();
            Err(err(ErrorCode::Ambiguous, format!("{n:?} could mean {}", list.join(", "))).hint("Name it by its code: 0x...."))
        }
    }
}

fn is_string(row: Option<&PropRow>, e: Option<&Entry>) -> bool {
    match row {
        Some(r) if r.datatype != 0 => r.datatype == 0xFFFF,
        _ => e.and_then(|e| e.datatype.as_deref()) == Some("string"),
    }
}

/// The values a code takes, as `LABEL (raw)`, for error messages.
fn valid_values(row: Option<&PropRow>, e: Option<&Entry>) -> String {
    match row.map(|r| &r.form) {
        Some(Form::Enum { values }) => {
            let v: Vec<String> = values
                .iter()
                .map(|v| match read::value_label(e, v) {
                    Some(l) => format!("{l} ({})", v.key()),
                    None => v.key(),
                })
                .collect();
            format!("values: {}", v.join(", "))
        }
        Some(Form::Range { min, max, step }) => format!("range: {} to {}, step {}", min.key(), max.key(), step.key()),
        _ => match e.filter(|e| !e.values.is_empty()) {
            Some(e) => format!("values: {}", e.values.iter().map(|(k, l)| format!("{l} ({k})")).collect::<Vec<_>>().join(", ")),
            None => "any value of its type".into(),
        },
    }
}

/// Turns a value as given (a label, a raw number, `0x` hex, or a string's text) into the
/// value to write.
pub fn resolve_value(row: Option<&PropRow>, e: Option<&Entry>, text: &str, what: &str) -> Result<Value, Error> {
    let t = text.trim();
    let labelled: Vec<&String> = e.map(|e| e.values.iter().filter(|(_, l)| l.trim().eq_ignore_ascii_case(t)).map(|(k, _)| k).collect()).unwrap_or_default();
    if is_string(row, e) {
        // A string property writes the text as given, unless it names a label.
        return Ok(Value::Str(labelled.first().map(|k| k.to_string()).unwrap_or_else(|| text.to_string())));
    }
    if labelled.len() > 1 {
        return Err(err(ErrorCode::Ambiguous, format!("{t:?} is the label of {} values of {what}", labelled.len())).hint(valid_values(row, e)));
    }
    if let Some(n) = labelled.first().and_then(|k| k.parse::<i64>().ok()) {
        return Ok(Value::Int(n));
    }
    let n = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => i64::from_str_radix(h, 16).ok(),
        None => t.parse::<i64>().ok(),
    };
    n.map(Value::Int).ok_or_else(|| err(ErrorCode::NotFound, format!("{t:?} is not a value of {what}")).hint(valid_values(row, e)))
}

/// Which mode or switch position reads a code the camera does not offer now.
fn needs(snap: &Snapshot, row: Option<&PropRow>, e: Option<&Entry>) -> ErrorCode {
    let modes = e.map(|e| e.modes.clone()).unwrap_or_default();
    let tether_code = modes.is_empty() || modes.iter().any(|m| m == "stills" || m == "movie");
    match snap.status.mode {
        Some(UsbMode::Tether) => match row {
            Some(_) if snap.status.capture == Some(Capture::Movie) => ErrorCode::NeedsStills,
            Some(_) => ErrorCode::NeedsMovie,
            None if modes.iter().any(|m| m == "rawconv") => ErrorCode::NeedsRawconv,
            None => ErrorCode::NotFound,
        },
        _ if tether_code => ErrorCode::NeedsTether,
        _ => ErrorCode::NeedsRawconv,
    }
}

fn in_form(form: &Form, v: &Value) -> bool {
    match (form, v) {
        (Form::Enum { values }, v) => values.contains(v),
        (Form::Range { min: Value::Int(lo), max: Value::Int(hi), step: Value::Int(step) }, Value::Int(n)) => *n >= *lo && *n <= *hi && (*step <= 0 || (*n - *lo) % *step == 0),
        _ => true,
    }
}

/// The step as shown: labels, raw values, the catalog's word on it.
pub fn step_view(snap: &Snapshot, code: u16, before: Option<&Value>, after: &Value, gate: &str) -> Step {
    let e = entry(snap.catalog.as_ref(), code);
    Step {
        code: read::code_text(code),
        name: e.and_then(|e| e.name.clone()),
        menu: e.and_then(|e| e.menu.clone()),
        before: read::display(e, before),
        after: read::display(e, Some(after)),
        before_value: before.map(read::raw_value),
        after_value: read::raw_value(after),
        confidence: e.and_then(|e| e.confidence),
        gate: gate.into(),
    }
}

/// Builds a plan against the snapshot. Fails as a whole on a camera that cannot take writes
/// and on a name or value it cannot resolve; a gate only moves its step to `blocked` or
/// `skipped`. The plan's id and lifetime are the Lab's to set.
pub fn build(snap: &Snapshot, input: &PlanInput) -> Result<(Plan, Vec<Raw>), Error> {
    ready(snap)?;
    if input.set.is_empty() {
        return Err(err(ErrorCode::NotFound, "the plan names no setting").hint("Give NAME=VALUE pairs."));
    }
    let cat = snap.catalog.as_ref();
    let mut plan = Plan {
        plan_id: String::new(),
        expires_in_s: LIFETIME.as_secs() as u32,
        requires: Requires { mode: snap.status.mode.map(|m| read::mode_id(m).to_string()), capture: snap.status.capture },
        steps: vec![],
        skipped: vec![],
        blocked: vec![],
        allow_unconfirmed: input.allow_unconfirmed,
    };
    let mut raw = vec![];
    let mut seen = std::collections::HashSet::new();
    for (name, text) in &input.set {
        let code = resolve_code(snap, name)?;
        if !seen.insert(code) {
            return Err(err(ErrorCode::Ambiguous, format!("{} is set twice in one plan", read::code_text(code))));
        }
        let e = entry(cat, code);
        let row = snap.rows.iter().find(|r| r.code == code);
        let what = describe(code, e);
        let left = |reason: &str, message: String| Left { code: read::code_text(code), name: e.and_then(|e| e.name.clone()), value: text.clone(), reason: reason.into(), message };
        let Some(row) = row.filter(|r| r.available) else {
            let need = needs(snap, row, e);
            let msg = match need {
                ErrorCode::NotFound => format!("the camera does not offer {what}"),
                n => format!("{what} is written in another mode ({})", ErrorCode::name(n)),
            };
            plan.skipped.push(left(ErrorCode::name(need), msg));
            continue;
        };
        let value = resolve_value(Some(row), e, text, &what)?;
        if !row.writable {
            plan.blocked.push(left("read_only", format!("{what} is read-only")));
            continue;
        }
        if row.value.as_ref() == Some(&value) {
            plan.skipped.push(left(UNCHANGED, format!("{what} is already {}", read::display(e, Some(&value)))));
            continue;
        }
        let confirmed = e.and_then(|e| e.confidence) == Some(Confidence::Confirmed);
        if !confirmed && !input.allow_unconfirmed {
            let c = e.and_then(|e| e.confidence).map(|c| format!("{c:?}").to_lowercase()).unwrap_or_else(|| "unmapped".into());
            plan.blocked.push(left("unconfirmed_property", format!("{what} is not confirmed in the catalog ({c}); its meaning may be wrong. Pass allow_unconfirmed (--allow-unconfirmed) to write it")));
            continue;
        }
        let in_catalog = !confirmed || e.is_none_or(|e| e.values.is_empty() || e.values.contains_key(&value.key()));
        if !in_form(&row.form, &value) || !in_catalog {
            plan.blocked.push(left("value_out_of_range", format!("{} is not a value {what} takes; {}", read::display(e, Some(&value)), valid_values(Some(row), e))));
            continue;
        }
        let gate = if confirmed { "ok" } else { "unconfirmed_allowed" };
        plan.steps.push(step_view(snap, code, row.value.as_ref(), &value, gate));
        raw.push(Raw { code, before: row.value.clone(), after: value });
    }
    Ok((plan, raw))
}

/// Why a stored plan can no longer be applied as made: the camera's mode or switch moved,
/// or a planned code holds another value than at planning.
pub fn stale(snap: &Snapshot, s: &Stored) -> Option<Error> {
    if snap.status.mode != s.mode || snap.status.capture != s.capture {
        return Some(err(ErrorCode::PlanStale, "the camera's USB mode or STILL/MOVIE switch changed since the plan").hint("Make a new plan."));
    }
    let cat = snap.catalog.as_ref();
    let moved: Vec<String> = s
        .raw
        .iter()
        .filter_map(|r| {
            let now = snap.rows.iter().find(|x| x.code == r.code).and_then(|x| x.value.clone());
            (now != r.before).then(|| {
                let e = entry(cat, r.code);
                format!("{} {} -> {}", describe(r.code, e), read::display(e, r.before.as_ref()), read::display(e, now.as_ref()))
            })
        })
        .collect();
    (!moved.is_empty()).then(|| err(ErrorCode::PlanStale, format!("changed since the plan: {}", moved.join("; "))).hint("Make a new plan."))
}

/// A step's result from the owner's outcome. `link` is the camera's link now, to tell a busy
/// pause from a menu.
pub fn applied_step(snap: &Snapshot, r: &Raw, out: &WriteOutcome, link: Link) -> AppliedStep {
    let e = entry(snap.catalog.as_ref(), r.code);
    let readback_ok = out.readback.as_ref() == Some(&r.after);
    let error = match out.rc {
        Some(ptp::RC_OK) if readback_ok => None,
        Some(ptp::RC_OK) => Some(ErrorCode::ReadbackMismatch),
        Some(ptp::RC_DEVICE_BUSY) => Some(ErrorCode::Busy),
        Some(ptp::RC_FUJI_MENU) if out.result == refusal::PLAYBACK => Some(ErrorCode::InPlayback),
        Some(ptp::RC_FUJI_MENU) => Some(ErrorCode::MenuOpen),
        Some(0x201C) | Some(0x201D) => Some(ErrorCode::ValueOutOfRange),
        Some(ptp::RC_ACCESS_DENIED) => Some(ErrorCode::ReadOnly),
        Some(_) => Some(ErrorCode::Failed),
        None => Some(match out.result.as_str() {
            refusal::PLAYBACK => ErrorCode::InPlayback,
            refusal::PAUSED if link == Link::Busy => ErrorCode::Busy,
            refusal::PAUSED => ErrorCode::MenuOpen,
            refusal::NOT_CONNECTED | refusal::LOST => ErrorCode::NotConnected,
            refusal::READ_ONLY => ErrorCode::ReadOnly,
            _ => ErrorCode::Failed,
        }),
    };
    AppliedStep {
        code: read::code_text(r.code),
        name: e.and_then(|e| e.name.clone()),
        wanted: read::display(e, Some(&r.after)),
        readback: out.readback.as_ref().map(|v| read::display(e, Some(v))),
        wanted_value: read::raw_value(&r.after),
        readback_value: out.readback.as_ref().map(read::raw_value),
        ok: error.is_none(),
        result: out.result.clone(),
        error,
        attempts: out.attempts,
        closed_menu: out.closed_menu,
    }
}

/// Codes outside the plan that changed in groups after `since` (a group id), each once:
/// its first old and last new value.
pub fn side_effects(cat: Option<&Catalog>, groups: &[ChangeGroup], since: u32, planned: &[u16]) -> Vec<ChangeView> {
    let mut order: Vec<u16> = vec![];
    let mut seen: std::collections::HashMap<u16, (Option<Value>, Option<Value>)> = Default::default();
    let mut gs: Vec<&ChangeGroup> = groups.iter().filter(|g| g.id > since).collect();
    gs.sort_by_key(|g| g.id);
    for g in gs {
        for c in g.changes.iter().filter(|c| !planned.contains(&c.code)) {
            if g.source == ChangeSource::Write && g.actor.is_none() {
                continue; // The owner's own keep-awake write, not this plan's doing.
            }
            match seen.get_mut(&c.code) {
                Some(v) => v.1 = c.new.clone(),
                None => {
                    order.push(c.code);
                    seen.insert(c.code, (c.old.clone(), c.new.clone()));
                }
            }
        }
    }
    order
        .into_iter()
        .filter_map(|code| {
            let (old, new) = seen.remove(&code)?;
            (old != new).then(|| read::change_view(cat, &crate::camera::owner::Change { code, old, new }))
        })
        .collect()
}

#[cfg(test)]
mod tests;
