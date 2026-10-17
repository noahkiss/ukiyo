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
    #[serde(default)]
    pub set: IndexMap<String, String>,
    /// Lets the plan write codes whose catalog confidence is not `confirmed`.
    #[serde(default)]
    pub allow_unconfirmed: bool,
    /// A library item to plan instead of `set`: `preset:<slug>` (its `set`) or
    /// `snapshot:<slug>` (a restore: its exact values).
    #[serde(default)]
    pub from: Option<String>,
    /// With `from` a recipe (`recipe:<slug>` or `cat:<id>`): the custom slot to write it
    /// to, `C1` to `C7`. Needs USB RAW CONV. mode.
    #[serde(default)]
    pub slot: Option<String>,
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
    /// `set`, `prerequisite` or `restore` (`purpose`).
    #[serde(default = "set_purpose")]
    pub purpose: String,
    /// What the camera changes by itself when this step is written: "0xd008 Color -> 0".
    #[serde(default)]
    pub resets: Vec<String>,
    /// Why a prerequisite or restore step is in the plan.
    #[serde(default)]
    pub note: Option<String>,
}

/// What a step is for (`Step::purpose`).
pub mod purpose {
    /// A change the plan was asked for.
    pub const SET: &str = "set";
    /// A temporary write that another step needs (Monochromatic Color takes writes only
    /// under a monochrome film simulation). A `restore` step puts it back.
    pub const PREREQUISITE: &str = "prerequisite";
    /// Puts back a prerequisite, or a setting a prerequisite made the camera change.
    pub const RESTORE: &str = "restore";
}

fn set_purpose() -> String {
    purpose::SET.into()
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
    /// The custom slot a slot plan writes: `C3`. The steps then go to that slot: select it
    /// (D18C), write, restore D18C.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    /// What a slot plan's recipe is: `recipe:<slug>` or `cat:<id>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe: Option<String>,
    /// Things the plan cannot do as asked, besides blocked steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
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
    /// What went wrong around the steps: a slot selector that was not restored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

pub const UNCHANGED: &str = "unchanged";

/// One step's raw values, kept with the stored plan.
#[derive(Debug, Clone)]
pub struct Raw {
    pub code: u16,
    pub before: Option<Value>,
    pub after: Value,
    /// `before` is what an earlier step left (a write, or a camera reset), not the camera now.
    pub derived: bool,
    /// The value before the plan: what the stale check expects and an undo puts back.
    pub was: Option<Value>,
}

/// A plan as the Lab stores it.
#[derive(Debug, Clone)]
pub struct Stored {
    pub plan: Plan,
    pub raw: Vec<Raw>,
    pub mode: Option<UsbMode>,
    pub capture: Option<Capture>,
    pub created: Instant,
    /// A slot plan's slot (1-7).
    pub slot: Option<u8>,
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
        return Err(err(ErrorCode::MenuOpen, "A menu is open on the camera; it answers nothing until the menu closes.").hint("Close the NETWORK/USB SETTING menu on the camera."));
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
        purpose: purpose::SET.into(),
        resets: vec![],
        note: None,
    }
}

/// A catalog value key as a value: a number, else the text.
fn key_value(k: &str) -> Value {
    k.trim().parse::<i64>().map(Value::Int).unwrap_or_else(|_| Value::Str(k.to_string()))
}

/// One write condition of a code, from the catalog's `requires`.
struct Rule {
    on: u16,
    values: Vec<Value>,
    temp: Value,
}

fn rules(cat: Option<&Catalog>, code: u16) -> Vec<Rule> {
    let Some(e) = entry(cat, code) else { return vec![] };
    e.requires
        .iter()
        .filter_map(|r| {
            let on = read::parse_code(&r.code)?;
            let values: Vec<Value> = r.values.iter().map(|k| key_value(k)).collect();
            let temp = r.temp.as_deref().map(key_value).or_else(|| values.first().cloned())?;
            Some(Rule { on, values, temp })
        })
        .collect()
}

/// What the camera sets by itself when `code` is written to `v`, from the catalog's `resets`.
fn resets_of(cat: Option<&Catalog>, code: u16, v: &Value) -> Vec<(u16, Value)> {
    let Some(e) = entry(cat, code) else { return vec![] };
    e.resets
        .iter()
        .filter(|r| r.when.is_empty() || r.when.iter().any(|w| *w == v.key()))
        .filter_map(|r| Some((read::parse_code(&r.code)?, key_value(&r.to))))
        .collect()
}

/// One write, in the order a plan makes them.
#[derive(Debug, Clone)]
pub struct Seq {
    pub code: u16,
    pub before: Option<Value>,
    pub after: Value,
    pub purpose: &'static str,
    pub derived: bool,
    pub was: Option<Value>,
    pub resets: Vec<(u16, Value)>,
    pub note: Option<String>,
}

/// The writes in order, and what the sequencer left out.
#[derive(Debug, Clone, Default)]
pub struct Sequenced {
    pub seq: Vec<Seq>,
    /// Asked for, but already at the value when its turn came.
    pub unchanged: Vec<u16>,
    /// Asked for, but a prerequisite cannot be written: code, reason, message.
    pub blocked: Vec<(u16, String, String)>,
    pub warnings: Vec<String>,
}

/// Why a write does not pass a gate: the reason (an error code) and a message.
pub type Refusal = (String, String);

/// What the sequencer knows about the camera.
pub struct Ground<'a> {
    pub cat: Option<&'a Catalog>,
    /// A code's value now.
    pub now: &'a dyn Fn(u16) -> Option<Value>,
    /// Whether a write the plan was not asked for (a prerequisite, a restore) passes the
    /// gates; `Err((reason, message))` when it does not.
    pub may_write: &'a dyn Fn(u16, &Value) -> Result<(), Refusal>,
    /// The order to write unrelated codes in: lower first, then the order asked.
    pub rank: &'a dyn Fn(u16) -> usize,
}

struct Sim<'g, 'a> {
    g: &'g Ground<'a>,
    state: std::collections::HashMap<u16, Option<Value>>,
    orig: std::collections::HashMap<u16, Option<Value>>,
    /// Codes an earlier step wrote or made the camera reset: their `before` is simulated.
    touched: std::collections::HashSet<u16>,
    seq: Vec<Seq>,
}

impl Sim<'_, '_> {
    fn get(&mut self, c: u16) -> Option<Value> {
        if !self.state.contains_key(&c) {
            let v = (self.g.now)(c);
            self.state.insert(c, v.clone());
            self.orig.insert(c, v);
        }
        self.state[&c].clone()
    }

    fn what(&self, code: u16) -> String {
        describe(code, entry(self.g.cat, code))
    }

    fn label(&self, code: u16, v: &Value) -> String {
        read::display(entry(self.g.cat, code), Some(v))
    }

    fn push(&mut self, code: u16, v: Value, purpose: &'static str, note: Option<String>) {
        let before = self.get(code);
        let resets = resets_of(self.g.cat, code, &v);
        let derived = self.touched.contains(&code);
        for (c, to) in &resets {
            self.get(*c);
            self.state.insert(*c, Some(to.clone()));
            self.touched.insert(*c);
        }
        self.state.insert(code, Some(v.clone()));
        self.touched.insert(code);
        let was = self.orig.get(&code).cloned().flatten();
        self.seq.push(Seq { code, before, after: v, purpose, derived, was, resets, note });
    }

    /// Writes each prerequisite of `code` that does not hold now. `Err`: one that cannot
    /// be written. Codes set here that the plan was not asked for go into `temps`.
    fn prerequisites(&mut self, code: u16, asked: &[(u16, Value)], temps: &mut Vec<u16>) -> Result<(), (String, String)> {
        for r in rules(self.g.cat, code) {
            let now = self.get(r.on);
            if now.as_ref().is_some_and(|v| r.values.contains(v)) {
                continue;
            }
            let list: Vec<String> = r.values.iter().map(|v| self.label(r.on, v)).collect();
            let cond = format!("{} takes writes only while {} is {}", self.what(code), self.what(r.on), list.join(", "));
            if let Err((reason, msg)) = (self.g.may_write)(r.on, &r.temp) {
                return Err((reason, format!("{cond}, and that cannot be set now: {msg}")));
            }
            let note = format!("For the next step: {cond}");
            self.push(r.on, r.temp.clone(), purpose::PREREQUISITE, Some(note));
            if !asked.iter().any(|(c, _)| *c == r.on) && !temps.contains(&r.on) {
                temps.push(r.on);
            }
        }
        Ok(())
    }
}

/// Orders the writes a plan was asked for, from the catalog's `requires` and `resets`
/// (task-o8vkpf69):
///
/// - a write that needs another code at some value comes after that code's write when the
///   plan sets it to such a value, and before it otherwise;
/// - a write the camera resets with another write comes after that write (Color after the
///   film simulation, Clarity after the file type);
/// - a prerequisite that does not hold at a write's turn is set first for the time of the
///   write (`prerequisite`), and put back at the end (`restore`), with anything that
///   prerequisite made the camera change;
/// - a requested code a later step reset is written again.
///
/// Unrelated codes keep `rank` order, then the order asked. Values are simulated through
/// the steps, so "already at the value" means at that step's turn.
pub fn sequence(g: &Ground, asked: &[(u16, Value)]) -> Sequenced {
    let cat = g.cat;
    let n = asked.len();
    let idx = |c: u16| asked.iter().position(|(x, _)| *x == c);
    let mut next: Vec<Vec<usize>> = vec![vec![]; n];
    let mut indeg = vec![0usize; n];
    let mut edge = |a: usize, b: usize| {
        if a != b && !next[a].contains(&b) {
            next[a].push(b);
            indeg[b] += 1;
        }
    };
    for (j, (code, _)) in asked.iter().enumerate() {
        for r in rules(cat, *code) {
            if let Some(i) = idx(r.on) {
                if r.values.contains(&asked[i].1) {
                    edge(i, j);
                } else {
                    edge(j, i);
                }
            }
        }
    }
    for (i, (code, v)) in asked.iter().enumerate() {
        for (c, _) in resets_of(cat, *code, v) {
            if let Some(j) = idx(c) {
                edge(i, j);
            }
        }
    }
    // Kahn's order, lowest rank first; a cycle falls back to the order asked.
    let mut order = vec![];
    let mut done = vec![false; n];
    while order.len() < n {
        let free = (0..n).filter(|i| !done[*i] && indeg[*i] == 0).min_by_key(|i| ((g.rank)(asked[*i].0), *i));
        let i = free.or_else(|| (0..n).find(|i| !done[*i])).unwrap();
        done[i] = true;
        order.push(i);
        for &j in &next[i] {
            indeg[j] = indeg[j].saturating_sub(1);
        }
    }
    let mut sim = Sim { g, state: Default::default(), orig: Default::default(), touched: Default::default(), seq: vec![] };
    let mut out = Sequenced::default();
    let mut temps: Vec<u16> = vec![];
    let mut blocked: Vec<u16> = vec![];
    for i in order {
        let (code, v) = &asked[i];
        if sim.get(*code).as_ref() == Some(v) {
            out.unchanged.push(*code);
            continue;
        }
        if let Err((reason, msg)) = sim.prerequisites(*code, asked, &mut temps) {
            out.blocked.push((*code, reason, msg));
            blocked.push(*code);
            continue;
        }
        sim.push(*code, v.clone(), purpose::SET, None);
    }
    // Put back the temporary prerequisites.
    for p in temps.clone() {
        let Some(o) = sim.orig.get(&p).cloned().flatten() else { continue };
        if sim.get(p).as_ref() != Some(&o) {
            let note = format!("Back to {} after the steps that needed it", sim.label(p, &o));
            sim.push(p, o, purpose::RESTORE, Some(note));
        }
    }
    // What a prerequisite or restore made the camera change: back to its value before the
    // plan. A requested code a later step reset: write it again.
    let incidental: Vec<(u16, String)> = sim
        .seq
        .iter()
        .filter(|s| s.purpose != purpose::SET)
        .flat_map(|s| s.resets.iter().map(move |(c, _)| (*c, s.code)))
        .map(|(c, by)| (c, describe(by, entry(cat, by))))
        .collect();
    for (c, by) in incidental {
        if idx(c).is_some() || temps.contains(&c) {
            continue;
        }
        let Some(o) = sim.orig.get(&c).cloned().flatten() else { continue };
        if sim.get(c).as_ref() == Some(&o) {
            continue;
        }
        let what = sim.what(c);
        if let Err((_, msg)) = (g.may_write)(c, &o).and_then(|_| sim.prerequisites(c, asked, &mut temps)) {
            out.warnings.push(format!("{what} stays at the value the camera set when {by} changed: {msg}"));
            continue;
        }
        let note = format!("Back to {}: the camera changed it when {by} changed", sim.label(c, &o));
        sim.push(c, o, purpose::RESTORE, Some(note));
    }
    for (code, v) in asked {
        if blocked.contains(code) || out.unchanged.contains(code) && sim.get(*code).as_ref() == Some(v) {
            continue;
        }
        if sim.get(*code).as_ref() != Some(v) {
            out.unchanged.retain(|c| c != code);
            let note = Some("Again: a later step made the camera change it".to_string());
            if sim.prerequisites(*code, asked, &mut temps).is_ok() {
                sim.push(*code, v.clone(), purpose::SET, note);
            }
        }
    }
    out.seq = sim.seq;
    out
}

/// What one step asks for: a value as given (a label or raw text), or an exact value
/// (a snapshot restore).
enum Want<'a> {
    Text(&'a str),
    Exact(Value),
}

/// Builds a plan against the snapshot. Fails as a whole on a camera that cannot take writes
/// and on a name or value it cannot resolve; a gate only moves its step to `blocked` or
/// `skipped`. The plan's id and lifetime are the Lab's to set.
pub fn build(snap: &Snapshot, input: &PlanInput) -> Result<(Plan, Vec<Raw>), Error> {
    ready(snap)?;
    if input.set.is_empty() {
        return Err(err(ErrorCode::NotFound, "the plan names no setting").hint("Give NAME=VALUE pairs."));
    }
    let wants = input.set.iter().map(|(name, text)| Ok((resolve_code(snap, name)?, Want::Text(text)))).collect::<Result<Vec<_>, Error>>()?;
    gate(snap, wants, input.allow_unconfirmed)
}

/// Builds a plan that writes exact values by code: a snapshot restore. Codes the camera
/// does not read now are skipped with the mode they need, like any plan.
pub fn build_values(snap: &Snapshot, values: &[(u16, Value)], allow_unconfirmed: bool) -> Result<(Plan, Vec<Raw>), Error> {
    ready(snap)?;
    if values.is_empty() {
        return Err(err(ErrorCode::NotFound, "the plan names no setting"));
    }
    gate(snap, values.iter().map(|(c, v)| (*c, Want::Exact(v.clone()))).collect(), allow_unconfirmed)
}

/// A plan with nothing in it yet, for the camera's state now.
pub fn empty_plan(snap: &Snapshot, allow_unconfirmed: bool) -> Plan {
    Plan {
        plan_id: String::new(),
        expires_in_s: LIFETIME.as_secs() as u32,
        requires: Requires { mode: snap.status.mode.map(|m| read::mode_id(m).to_string()), capture: snap.status.capture },
        steps: vec![],
        skipped: vec![],
        blocked: vec![],
        allow_unconfirmed,
        slot: None,
        recipe: None,
        warnings: vec![],
    }
}

/// Why a code fails the confirmed gate, as a blocked entry's reason and message.
fn unconfirmed(e: Option<&Entry>, what: &str) -> (String, String) {
    let c = e.and_then(|e| e.confidence).map(|c| format!("{c:?}").to_lowercase()).unwrap_or_else(|| "unmapped".into());
    ("unconfirmed_property".into(), format!("{what} is not confirmed in the catalog ({c}); its meaning may be wrong. Pass allow_unconfirmed (--allow-unconfirmed) to write it"))
}

/// The gate a step passes: `ok`, or `unconfirmed_allowed` when only the plan's flag lets it.
pub fn gate_of(e: Option<&Entry>) -> &'static str {
    if e.and_then(|e| e.confidence) == Some(Confidence::Confirmed) {
        "ok"
    } else {
        "unconfirmed_allowed"
    }
}

/// Turns the sequencer's writes into the plan's steps and raw values.
pub fn assemble(snap: &Snapshot, plan: &mut Plan, s: Sequenced, texts: &std::collections::HashMap<u16, String>, now: &dyn Fn(u16) -> Option<Value>) -> Vec<Raw> {
    let cat = snap.catalog.as_ref();
    let mut raw = vec![];
    for q in s.seq {
        let e = entry(cat, q.code);
        let mut step = step_view(snap, q.code, q.before.as_ref(), &q.after, gate_of(e));
        step.purpose = q.purpose.into();
        step.note = q.note;
        step.resets = q.resets.iter().map(|(c, to)| format!("{} -> {}", describe(*c, entry(cat, *c)), read::display(entry(cat, *c), Some(to)))).collect();
        plan.steps.push(step);
        raw.push(Raw { code: q.code, before: q.before, after: q.after, derived: q.derived, was: q.was });
    }
    let left = |code: u16, reason: &str, message: String| Left {
        code: read::code_text(code),
        name: entry(cat, code).and_then(|e| e.name.clone()),
        value: texts.get(&code).cloned().unwrap_or_default(),
        reason: reason.into(),
        message,
    };
    for c in s.unchanged {
        let e = entry(cat, c);
        plan.skipped.push(left(c, UNCHANGED, format!("{} is already {}", describe(c, e), read::display(e, now(c).as_ref()))));
    }
    for (c, reason, msg) in s.blocked {
        plan.blocked.push(left(c, &reason, msg));
    }
    plan.warnings.extend(s.warnings);
    raw
}

fn gate(snap: &Snapshot, wants: Vec<(u16, Want)>, allow_unconfirmed: bool) -> Result<(Plan, Vec<Raw>), Error> {
    let cat = snap.catalog.as_ref();
    let mut plan = empty_plan(snap, allow_unconfirmed);
    let mut seen = std::collections::HashSet::new();
    let mut texts = std::collections::HashMap::new();
    // Each code on its own: offered now, a value it takes.
    let mut cands: Vec<(u16, &PropRow, Value)> = vec![];
    for (code, want) in wants {
        if !seen.insert(code) {
            return Err(err(ErrorCode::Ambiguous, format!("{} is set twice in one plan", read::code_text(code))));
        }
        let e = entry(cat, code);
        let row = snap.rows.iter().find(|r| r.code == code);
        let what = describe(code, e);
        let text = match &want {
            Want::Text(t) => t.to_string(),
            Want::Exact(v) => read::display(e, Some(v)),
        };
        texts.insert(code, text.clone());
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
        let value = match want {
            Want::Text(t) => resolve_value(Some(row), e, t, &what)?,
            Want::Exact(v) => v,
        };
        cands.push((code, row, value));
    }
    // A code another request makes the camera reset is never "already set".
    let reset_by_plan: std::collections::HashSet<u16> = cands.iter().flat_map(|(c, _, v)| resets_of(cat, *c, v).into_iter().map(|(x, _)| x)).collect();
    let mut asked: Vec<(u16, Value)> = vec![];
    for (code, row, value) in cands {
        let e = entry(cat, code);
        let what = describe(code, e);
        let left = |reason: &str, message: String| Left { code: read::code_text(code), name: e.and_then(|e| e.name.clone()), value: texts[&code].clone(), reason: reason.into(), message };
        if !row.writable {
            plan.blocked.push(left("read_only", format!("{what} is read-only")));
            continue;
        }
        if row.value.as_ref() == Some(&value) && !reset_by_plan.contains(&code) {
            plan.skipped.push(left(UNCHANGED, format!("{what} is already {}", read::display(e, Some(&value)))));
            continue;
        }
        let confirmed = e.and_then(|e| e.confidence) == Some(Confidence::Confirmed);
        if !confirmed && !allow_unconfirmed {
            let (reason, msg) = unconfirmed(e, &what);
            plan.blocked.push(left(&reason, msg));
            continue;
        }
        let in_catalog = !confirmed || e.is_none_or(|e| e.values.is_empty() || e.values.contains_key(&value.key()));
        // A code with write conditions reports the range of the state it was described in
        // (Monochromatic Color reads 0 to 0 under a colour film simulation): the camera's
        // answer decides.
        let form_ok = !rules(cat, code).is_empty() || in_form(&row.form, &value);
        if !form_ok || !in_catalog {
            plan.blocked.push(left("value_out_of_range", format!("{} is not a value {what} takes; {}", read::display(e, Some(&value)), valid_values(Some(row), e))));
            continue;
        }
        asked.push((code, value));
    }
    let now = |c: u16| snap.rows.iter().find(|r| r.code == c).and_then(|r| r.value.clone());
    let may_write = |c: u16, v: &Value| -> Result<(), (String, String)> {
        let e = entry(cat, c);
        let what = describe(c, e);
        let Some(row) = snap.rows.iter().find(|r| r.code == c).filter(|r| r.available) else {
            return Err((ErrorCode::name(needs(snap, None, e)).into(), format!("{what} does not read in this mode")));
        };
        if !row.writable {
            return Err(("read_only".into(), format!("{what} is read-only")));
        }
        let confirmed = e.and_then(|e| e.confidence) == Some(Confidence::Confirmed);
        if !confirmed && !allow_unconfirmed {
            return Err(unconfirmed(e, &what));
        }
        if !in_form(&row.form, v) {
            return Err(("value_out_of_range".into(), format!("{} is not a value {what} takes", read::display(e, Some(v)))));
        }
        Ok(())
    };
    let order: Vec<u16> = asked.iter().map(|(c, _)| *c).collect();
    let rank = |c: u16| order.iter().position(|x| *x == c).unwrap_or(usize::MAX);
    let s = sequence(&Ground { cat, now: &now, may_write: &may_write, rank: &rank }, &asked);
    let raw = assemble(snap, &mut plan, s, &texts, &now);
    Ok((plan, raw))
}

/// A slot plan: write `wants` (slot codes and values in slot encoding) to slot `n`, whose
/// fields hold `current` now (read through the owner, since the poll reads only the
/// selected slot). Gated like any plan: a field whose catalog entry is not `confirmed` is
/// blocked unless the plan allows unconfirmed codes. Ordered by the catalog's `requires`
/// and `resets` (the slot's film simulation before Monochromatic Color, D-range priority
/// off before the tones), then the slot field order (`recipes::slot::order`).
pub fn slot_plan(snap: &Snapshot, n: u8, current: &std::collections::HashMap<u16, Value>, wants: &[(u16, Value)], allow_unconfirmed: bool) -> (Plan, Vec<Raw>) {
    use crate::recipes::slot;
    let cat = snap.catalog.as_ref();
    let mut plan = empty_plan(snap, allow_unconfirmed);
    plan.slot = Some(slot::slot_name(n));
    let texts: std::collections::HashMap<u16, String> = wants.iter().map(|(c, v)| (*c, read::display(entry(cat, *c), Some(v)))).collect();
    let mut asked = vec![];
    for (code, v) in wants {
        let e = entry(cat, *code);
        let what = describe(*code, e);
        let left = |reason: &str, message: String| Left { code: read::code_text(*code), name: e.and_then(|e| e.name.clone()), value: texts[code].clone(), reason: reason.into(), message };
        if !slot::writable(*code) {
            plan.blocked.push(left("read_only", format!("{what} is not a recipe setting; slot plans never write it")));
            continue;
        }
        if !current.contains_key(code) {
            plan.skipped.push(left("not_found", format!("the camera did not report {what} for {}", slot::slot_name(n))));
            continue;
        }
        let confirmed = e.and_then(|e| e.confidence) == Some(Confidence::Confirmed);
        if !confirmed && !allow_unconfirmed {
            let (reason, msg) = unconfirmed(e, &what);
            plan.blocked.push(left(&reason, msg));
            continue;
        }
        asked.push((*code, v.clone()));
    }
    let now = |c: u16| current.get(&c).cloned();
    let may_write = |c: u16, _: &Value| -> Result<(), (String, String)> {
        let e = entry(cat, c);
        let what = describe(c, e);
        if !slot::writable(c) || !current.contains_key(&c) {
            return Err(("read_only".into(), format!("{what} is not a slot setting")));
        }
        if e.and_then(|e| e.confidence) != Some(Confidence::Confirmed) && !allow_unconfirmed {
            return Err(unconfirmed(e, &what));
        }
        Ok(())
    };
    let s = sequence(&Ground { cat, now: &now, may_write: &may_write, rank: &slot::order }, &asked);
    let raw = assemble(snap, &mut plan, s, &texts, &now);
    // A slot holds one recipe: half of one, under the other's name, would mislead. With any
    // field blocked, nothing is written.
    if let Some(first) = plan.blocked.first().cloned() {
        let steps = std::mem::take(&mut plan.steps);
        let mut seen = std::collections::HashSet::new();
        for st in steps.into_iter().filter(|s| seen.insert(s.code.clone())) {
            let message = format!("{} is not written: the slot gets the whole recipe or nothing, and {}", st.name.clone().unwrap_or_else(|| st.code.clone()), first.message);
            plan.blocked.push(Left { code: st.code, name: st.name, value: st.after, reason: first.reason.clone(), message });
        }
        return (plan, vec![]);
    }
    (plan, raw)
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
        .enumerate()
        .filter(|(i, r)| !s.raw[..*i].iter().any(|x| x.code == r.code))
        .filter_map(|(_, r)| {
            let now = snap.rows.iter().find(|x| x.code == r.code).and_then(|x| x.value.clone());
            (now != r.was).then(|| {
                let e = entry(cat, r.code);
                format!("{} {} -> {}", describe(r.code, e), read::display(e, r.was.as_ref()), read::display(e, now.as_ref()))
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
