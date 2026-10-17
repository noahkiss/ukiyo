//! The Lab: the front ends' side of the connection owner. It mirrors what the owner reports
//! for the window, writes the session log, and holds the catalog the person labels into.
//! Front ends reach it through `api`; its events go out as `api::Event`. It also stores the
//! plans (`plan`) and applies them: the only path from a front end to a camera write.
//!
//! Every line a request writes to the session log names its `actor` (`app`, `cli`,
//! `mcp:<client>`).
//!
//! Files: the catalog lives in the repo's `catalog/` in a dev build (so labels can be
//! committed), `UKIYO_CATALOG_DIR` when set, else the app data dir. Session logs, dumps and
//! settings always go to the app data dir.

use crate::api::{self, Error, ErrorCode};
use crate::camera::clock::{Clock, RealClock};
use crate::camera::fake::{FakeCamera, FakeHandle, FakeMode};
use crate::camera::owner::{self, ChangeGroup, Config, Event, Link, PropRow, Request, Status, WriteOutcome};
use crate::camera::ptp::{self, Value};
use crate::plan::{self, Applied, Plan, PlanInput, Raw, Stored};
use crate::camera::transport::Connector;
use crate::camera::usb::UsbConnector;
use crate::catalog::{Catalog, LabelPatch};
use crate::library::history::Recorder;
use crate::library::prefs;
use crate::serve::OwnerLock;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Change groups kept for the window's feed.
const FEED: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub status: Status,
    pub rows: Vec<PropRow>,
    pub groups: Vec<ChangeGroup>,
    pub catalog: Option<Catalog>,
    pub catalog_path: Option<String>,
    pub session_path: Option<String>,
    pub fake: bool,
    pub noisy: Vec<u16>,
}

/// What the person at the fake camera does (`UKIYO_FAKE_CAMERA=1` only).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum FakeAction {
    OpenMenu,
    /// The main menu: D22E stays 1, writes answer 0xA001 until a half-press.
    MainMenu,
    Playback,
    CloseMenu,
    Movie,
    Stills,
    PowerOff,
    PowerOn,
    Unplug,
    Nudge,
    AutoOn,
    AutoOff,
}

/// The parts of a status worth a session-log line.
type Logged = (Link, Option<String>, Option<owner::Screen>, Option<owner::Capture>, Option<owner::UsbMode>);

#[derive(Default)]
struct Mirror {
    status: Status,
    rows: BTreeMap<u16, PropRow>,
    groups: VecDeque<ChangeGroup>,
    catalog: Option<Catalog>,
    catalog_path: Option<PathBuf>,
    /// The model the catalog was loaded for.
    catalog_model: Option<String>,
    /// The status last written to the session log (timings aside).
    logged: Option<Logged>,
}

pub struct Lab {
    tx: Mutex<Sender<Request>>,
    mirror: Arc<Mutex<Mirror>>,
    log: Arc<Mutex<Option<std::fs::File>>>,
    session_path: Option<PathBuf>,
    fake: Option<(FakeHandle, Arc<AtomicBool>)>,
    data_dir: PathBuf,
    catalog_dir: PathBuf,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Held from before the USB open until the owner thread has closed the session.
    lock: Mutex<Option<OwnerLock>>,
    /// Plans by id, until applied, discarded or expired.
    plans: Mutex<HashMap<String, Stored>>,
    next_plan: AtomicU32,
}

fn wall_ms() -> i64 {
    RealClock::new().wall_ms()
}

fn stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H%M%S").to_string()
}

/// Where catalogs are read and written.
pub fn catalog_dir(data_dir: &Path, resources: Option<PathBuf>) -> PathBuf {
    if let Some(d) = std::env::var_os("UKIYO_CATALOG_DIR") {
        return PathBuf::from(d);
    }
    if cfg!(debug_assertions) {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        if repo.is_dir() {
            return repo;
        }
    }
    // A built app: the catalog the app shipped with is copied once, then edited in place.
    let dir = data_dir.join("catalog");
    if let Some(res) = resources.map(|r| r.join("catalog")) {
        if let Ok(entries) = std::fs::read_dir(&res) {
            let _ = std::fs::create_dir_all(&dir);
            for e in entries.flatten() {
                let to = dir.join(e.file_name());
                if !to.exists() {
                    let _ = std::fs::copy(e.path(), to);
                }
            }
        }
    }
    dir
}

impl Snapshot {
    /// A snapshot with no camera, only a catalog: for resolving names offline.
    pub fn of_catalog(catalog: Option<Catalog>) -> Snapshot {
        Snapshot { status: Status::default(), rows: vec![], groups: vec![], catalog, catalog_path: None, session_path: None, fake: false, noisy: vec![] }
    }
}

impl Lab {
    /// Starts the camera thread. `emit` sends an event to the front ends.
    ///
    /// `lock` is the held `owner.lock`: no process opens USB without it. The Lab keeps it until
    /// [`Lab::shutdown`] has closed the session.
    pub fn start(lock: OwnerLock, data_dir: PathBuf, catalog_dir: PathBuf, emit: impl Fn(api::Event) + Send + Sync + 'static) -> Arc<Lab> {
        let _ = std::fs::create_dir_all(data_dir.join("sessions"));
        let session_path = data_dir.join("sessions").join(format!("{}.jsonl", stamp()));
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&session_path).ok();
        let session_path = file.as_ref().map(|_| session_path);
        let log = Arc::new(Mutex::new(file));
        let mirror = Arc::new(Mutex::new(Mirror::default()));
        let settings = prefs::load(&data_dir);
        let history = Arc::new(Mutex::new(Recorder::new(&data_dir)));

        let clock: Arc<dyn Clock> = Arc::new(RealClock::new());
        let use_fake = std::env::var("UKIYO_FAKE_CAMERA").is_ok_and(|v| v == "1");
        let (connector, fake): (Box<dyn Connector>, _) = if use_fake {
            let mode = if std::env::var("UKIYO_FAKE_MODE").is_ok_and(|v| v == "rawconv") { FakeMode::RawConv } else { FakeMode::Tether };
            let cam = FakeCamera::x_t5(clock.clone(), mode).with_op_cost(Duration::from_millis(3));
            let h = cam.handle();
            // The fake sleeps after its AUTO POWER OFF time, so keep-awake has work to do.
            h.with(|s| s.auto_sleep = true);
            (Box::new(cam), Some((h, Arc::new(AtomicBool::new(true)))))
        } else {
            (Box::new(UsbConnector), None)
        };

        let emit = Arc::new(emit);
        let sink = {
            let mirror = mirror.clone();
            let log = log.clone();
            let emit = emit.clone();
            let catalog_dir = catalog_dir.clone();
            let history = history.clone();
            Box::new(move |e: Event| on_event(e, &mirror, &log, &catalog_dir, &history, &*emit))
        };
        // The first line, before the owner thread can log a status.
        write_log(&log, serde_json::json!({ "kind": "start", "fake": fake.is_some() }));
        let cfg = Config {
            in_use_for: Duration::from_secs(u64::from(settings.in_use_minutes.max(1)) * 60),
            close_menu_to_write: settings.close_menu_to_write,
            ..Config::default()
        };
        let (tx, thread) = owner::spawn(connector, clock, sink, cfg);
        let _ = tx.send(Request::KeepAwake(settings.keep_awake));
        if let Some((h, auto)) = &fake {
            let (h, auto) = (h.clone(), auto.clone());
            std::thread::spawn(move || {
                let mut n = 0u64;
                loop {
                    std::thread::sleep(Duration::from_millis(500));
                    n += 1;
                    h.tick_noise(wall_ms());
                    if auto.load(Ordering::Relaxed) && n.is_multiple_of(8) {
                        h.nudge();
                    }
                }
            });
        }
        Arc::new(Lab {
            tx: Mutex::new(tx),
            mirror,
            log,
            session_path,
            fake,
            data_dir,
            catalog_dir,
            thread: Mutex::new(Some(thread)),
            lock: Mutex::new(Some(lock)),
            plans: Mutex::new(HashMap::new()),
            next_plan: AtomicU32::new(1),
        })
    }

    fn send(&self, r: Request) {
        let _ = self.tx.lock().unwrap().send(r);
    }

    fn log_line(&self, v: serde_json::Value) {
        write_log(&self.log, v)
    }

    /// The data folder: sessions, dumps, `settings.json`, the library.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn snapshot(&self) -> Snapshot {
        let m = self.mirror.lock().unwrap();
        Snapshot {
            status: m.status.clone(),
            rows: m.rows.values().cloned().collect(),
            groups: m.groups.iter().cloned().collect(),
            catalog: m.catalog.clone(),
            catalog_path: m.catalog_path.as_ref().map(|p| p.display().to_string()),
            session_path: self.session_path.as_ref().map(|p| p.display().to_string()),
            fake: self.fake.is_some(),
            noisy: owner::NOISY.to_vec(),
        }
    }

    /// One write through the owner, which reads it back; logged with its actor and plan.
    fn write(&self, code: u16, value: Value, actor: &str, plan_id: &str) -> WriteOutcome {
        let (tx, rx) = std::sync::mpsc::channel();
        self.send(Request::Write { code, value, actor: Some(actor.into()), reply: Some(tx) });
        let out = rx.recv_timeout(Duration::from_secs(15)).unwrap_or_else(|_| WriteOutcome::refused(code, "no answer from the camera thread"));
        self.log_line(serde_json::json!({ "kind": "write", "actor": actor, "plan": plan_id, "outcome": out }));
        out
    }

    pub fn pin(&self, codes: Vec<u16>) {
        self.send(Request::SetPinned(codes));
    }

    /// The keep-awake setting: while Ukiyo is in use, the owner resets the camera's auto power
    /// off timer. Saved in `settings.json`.
    pub fn keep_awake(&self, actor: &str, on: bool) {
        self.send(Request::KeepAwake(on));
        if let Err(e) = prefs::update(&self.data_dir, &prefs::PrefsPatch { keep_awake: Some(on), ..Default::default() }) {
            eprintln!("ukiyo: settings.json not saved: {e}");
        }
        self.log_line(serde_json::json!({ "kind": "keep_awake", "actor": actor, "on": on }));
    }

    /// Someone is using Ukiyo (a client request, or input in the window): keep-awake runs for
    /// `in_use_minutes` after the last one. Not logged.
    pub fn activity(&self) {
        self.send(Request::Activity);
    }

    /// The Lab's connect switch. Off closes the session cleanly and
    /// keeps the owner off USB; on lets the owner look again under its 3 s and 30 s rules.
    pub fn connect(&self, actor: &str, on: bool) {
        self.send(if on { Request::Connect } else { Request::Disconnect });
        self.log_line(serde_json::json!({ "kind": "connect", "actor": actor, "on": on }));
    }

    pub fn export(&self, actor: &str) -> Result<String, Error> {
        let snap = self.snapshot();
        plan::ready(&snap)?;
        let failed = |m: String| Error::new(ErrorCode::Failed, m);
        let (tx, rx) = std::sync::mpsc::channel();
        self.send(Request::Export(tx));
        let dump = rx.recv_timeout(Duration::from_secs(60)).map_err(|_| failed("no answer from the camera thread".into()))?.map_err(failed)?;
        let dir = self.data_dir.join("dumps");
        std::fs::create_dir_all(&dir).map_err(|e| failed(e.to_string()))?;
        let mode = serde_json::to_value(dump.mode).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let path = dir.join(format!("{}-{mode}-{}.json", crate::catalog::slug(&dump.model), stamp()));
        let text = serde_json::to_string_pretty(&dump).map_err(|e| failed(e.to_string()))?;
        std::fs::write(&path, text).map_err(|e| failed(e.to_string()))?;
        self.log_line(serde_json::json!({ "kind": "export", "actor": actor, "path": path.display().to_string() }));
        Ok(path.display().to_string())
    }

    // ---- plans ----

    /// Stores a plan under a fresh id, dropping expired ones.
    fn store(&self, mut s: Stored) -> Plan {
        let mut plans = self.plans.lock().unwrap();
        plans.retain(|_, p| !p.expired());
        let id = loop {
            let n = self.next_plan.fetch_add(1, Ordering::Relaxed);
            let x = (n.wrapping_mul(0x9E37_79B9) ^ std::process::id().wrapping_mul(0x85EB_CA6B) ^ wall_ms() as u32) & 0xFFFF;
            let id = format!("p-{x:04x}");
            if !plans.contains_key(&id) {
                break id;
            }
        };
        s.plan.plan_id = id.clone();
        let view = s.view();
        plans.insert(id, s);
        view
    }

    /// Builds and stores a plan against the camera now. Writes nothing.
    /// With `from`, the plan comes from the library: a preset's `set`, or a snapshot's exact
    /// values (a restore). `set` must then be empty.
    pub fn plan(&self, input: PlanInput) -> Result<Plan, Error> {
        let snap = self.snapshot();
        let (plan, raw) = match input.from.as_deref() {
            None => plan::build(&snap, &input)?,
            Some(from) => {
                if !input.set.is_empty() {
                    return Err(Error::new(ErrorCode::BadRequest, "a plan has one source: set, or from (a preset or snapshot), not both"));
                }
                let store = crate::library::Store::new(&self.data_dir);
                match crate::library::Id::parse(from)? {
                    crate::library::Id::Item(crate::library::Kind::Preset, slug) => {
                        let p = store.preset(&slug)?;
                        plan::build(&snap, &PlanInput { set: p.set, allow_unconfirmed: input.allow_unconfirmed, from: None })?
                    }
                    crate::library::Id::Item(crate::library::Kind::Snapshot, slug) => {
                        let s = store.snapshot(&slug)?;
                        plan::build_values(&snap, &crate::library::snapshot::as_values(&s), input.allow_unconfirmed)?
                    }
                    _ => return Err(Error::new(ErrorCode::BadRequest, format!("{from} is not a preset or a snapshot"))),
                }
            }
        };
        Ok(self.store(Stored { plan, raw, mode: snap.status.mode, capture: snap.status.capture, created: std::time::Instant::now() }))
    }

    fn gone(plan_id: &str) -> Error {
        Error::new(ErrorCode::PlanExpired, format!("no plan {plan_id}: plans last 5 minutes, apply once, and end when ukiyo serve stops")).hint("Make a new plan.")
    }

    pub fn plan_show(&self, plan_id: &str) -> Result<Plan, Error> {
        let plans = self.plans.lock().unwrap();
        plans.get(plan_id).filter(|p| !p.expired()).map(Stored::view).ok_or_else(|| Self::gone(plan_id))
    }

    pub fn discard(&self, plan_id: &str) -> Result<(), Error> {
        self.plans.lock().unwrap().remove(plan_id).filter(|p| !p.expired()).map(|_| ()).ok_or_else(|| Self::gone(plan_id))
    }

    /// Applies a stored plan: re-checks it, writes each step in order (the owner reads each
    /// back), and stops at the first failed step. Returns the steps, an undo plan for what
    /// was written, and other codes that changed meanwhile. A plan applies once; a camera
    /// that cannot take writes now leaves the plan stored, to apply later.
    pub fn apply(&self, actor: &str, plan_id: &str) -> Result<Applied, Error> {
        let stored = self.plans.lock().unwrap().remove(plan_id).filter(|p| !p.expired()).ok_or_else(|| Self::gone(plan_id))?;
        let snap = self.snapshot();
        if let Err(e) = plan::ready(&snap) {
            self.plans.lock().unwrap().insert(plan_id.into(), stored);
            return Err(e);
        }
        if let Some(e) = plan::stale(&snap, &stored) {
            self.log_line(serde_json::json!({ "kind": "apply", "actor": actor, "plan": plan_id, "refused": e.code }));
            return Err(e);
        }
        let since = snap.groups.first().map_or(0, |g| g.id);
        let mut steps = vec![];
        let mut undo: Vec<Raw> = vec![];
        let mut not_attempted = 0;
        for (i, r) in stored.raw.iter().enumerate() {
            let out = self.write(r.code, r.after.clone(), actor, plan_id);
            let link = self.mirror.lock().unwrap().status.link;
            let step = plan::applied_step(&snap, r, &out, link);
            if out.rc == Some(ptp::RC_OK) {
                if let (Some(before), Some(now)) = (r.before.clone(), out.readback.clone()) {
                    undo.push(Raw { code: r.code, before: Some(now), after: before });
                }
            }
            let ok = step.ok;
            steps.push(step);
            if !ok {
                not_attempted = (stored.raw.len() - i - 1) as u32;
                break;
            }
        }
        let after = self.snapshot();
        let planned: Vec<u16> = stored.raw.iter().map(|r| r.code).collect();
        let side_effects = plan::side_effects(after.catalog.as_ref(), &after.groups, since, &planned);
        let undo_plan_id = (!undo.is_empty()).then(|| {
            let steps = undo.iter().map(|r| plan::step_view(&after, r.code, r.before.as_ref(), &r.after, "undo")).collect();
            let p = Plan {
                plan_id: String::new(),
                expires_in_s: 0,
                requires: stored.plan.requires.clone(),
                steps,
                skipped: vec![],
                blocked: vec![],
                allow_unconfirmed: true,
            };
            self.store(Stored { plan: p, raw: undo, mode: after.status.mode, capture: after.status.capture, created: std::time::Instant::now() }).plan_id
        });
        let applied = steps.iter().filter(|s| s.ok).count() as u32;
        let failed = steps.len() as u32 - applied;
        let out = Applied { plan_id: plan_id.into(), applied, failed, not_attempted, undo_plan_id, steps, side_effects };
        self.log_line(serde_json::json!({
            "kind": "apply", "actor": actor, "plan": plan_id, "applied": out.applied, "failed": out.failed,
            "not_attempted": out.not_attempted, "undo": out.undo_plan_id,
        }));
        Ok(out)
    }

    /// Applies a label and saves the catalog file.
    pub fn label(&self, actor: &str, patch: LabelPatch) -> Result<Catalog, String> {
        let mut m = self.mirror.lock().unwrap();
        let model = m.status.model.clone().or_else(|| m.catalog_model.clone()).ok_or("no camera model known yet")?;
        let datatype = m.rows.get(&patch.code).map(|r| ptp::type_name(r.datatype).to_string());
        if m.catalog.is_none() {
            m.catalog = Some(Catalog::empty(&model, "FUJIFILM"));
            m.catalog_path = Some(Catalog::path(&self.catalog_dir, &model));
            m.catalog_model = Some(model.clone());
        }
        let path = m.catalog_path.clone().unwrap_or_else(|| Catalog::path(&self.catalog_dir, &model));
        let firmware = m.status.firmware.clone();
        let cat = m.catalog.as_mut().unwrap();
        cat.apply(&patch, datatype.as_deref());
        if let Some(fw) = firmware {
            if !cat.firmware.contains(&fw) {
                cat.firmware.push(fw);
            }
        }
        cat.save(&path).map_err(|e| e.to_string())?;
        let out = cat.clone();
        drop(m);
        self.log_line(serde_json::json!({ "kind": "label", "actor": actor, "patch": patch }));
        Ok(out)
    }

    /// Text the person typed about what they just changed, tied to a change group.
    pub fn note(&self, actor: &str, group: Option<u32>, text: String) {
        self.log_line(serde_json::json!({ "kind": "note", "actor": actor, "group": group, "text": text }));
    }

    /// The fake camera's handle, for tests that play the person at the camera.
    pub fn fake_handle(&self) -> Option<FakeHandle> {
        self.fake.as_ref().map(|(h, _)| h.clone())
    }

    pub fn fake(&self, a: FakeAction) -> Result<(), String> {
        let Some((h, auto)) = &self.fake else { return Err("no fake camera".into()) };
        match a {
            FakeAction::OpenMenu => h.open_menu(),
            FakeAction::MainMenu => h.open_main_menu(),
            FakeAction::Playback => h.open_playback(),
            FakeAction::CloseMenu => h.close_menu(),
            FakeAction::Movie => h.set_movie(true),
            FakeAction::Stills => h.set_movie(false),
            FakeAction::PowerOff => h.power_off(),
            FakeAction::PowerOn => h.power_on(),
            FakeAction::Unplug => h.unplug(),
            FakeAction::Nudge => {
                h.nudge();
            }
            FakeAction::AutoOn => auto.store(true, Ordering::Relaxed),
            FakeAction::AutoOff => auto.store(false, Ordering::Relaxed),
        }
        Ok(())
    }

    /// A clean disconnect (session closed), then the thread ends.
    ///
    /// Releases `owner.lock` only when the thread has ended. A thread that did not end in 5 s
    /// may still hold USB, so the lock stays until the process exits.
    pub fn shutdown(&self) {
        self.send(Request::Shutdown);
        let mut finished = true;
        if let Some(t) = self.thread.lock().unwrap().take() {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !t.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            finished = t.is_finished();
        }
        self.log_line(serde_json::json!({ "kind": "stop", "clean": finished }));
        if finished {
            self.lock.lock().unwrap().take();
        }
    }
}

fn write_log(log: &Mutex<Option<std::fs::File>>, mut v: serde_json::Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert("t".into(), serde_json::json!(wall_ms()));
    }
    if let Some(f) = log.lock().unwrap().as_mut() {
        let _ = writeln!(f, "{v}");
    }
}

fn on_event(e: Event, mirror: &Mutex<Mirror>, log: &Mutex<Option<std::fs::File>>, catalog_dir: &Path, history: &Mutex<Recorder>, emit: &dyn Fn(api::Event)) {
    match e {
        Event::Status(mut s) => {
            history.lock().unwrap().on_status(&mut s);
            let mut m = mirror.lock().unwrap();
            let key = (s.link, s.reason.clone(), s.screen, s.capture, s.mode);
            let log_it = m.logged.as_ref() != Some(&key);
            m.logged = Some(key);
            m.status = s.clone();
            let mut catalog = None;
            if let Some(model) = s.model.clone().filter(|md| m.catalog_model.as_ref() != Some(md)) {
                let path = Catalog::path(catalog_dir, &model);
                m.catalog = Catalog::load(&path).ok();
                m.catalog_path = Some(path);
                m.catalog_model = Some(model);
                catalog = Some(m.catalog.clone());
            }
            drop(m);
            if log_it {
                write_log(log, serde_json::json!({ "kind": "status", "status": s }));
            }
            if let Some(c) = catalog {
                emit(api::Event::Catalog(c));
            }
            emit(api::Event::Status(s));
        }
        Event::Props(rows) => {
            let mut m = mirror.lock().unwrap();
            m.rows = rows.iter().map(|r| (r.code, r.clone())).collect();
            history.lock().unwrap().on_rows(&rows, m.catalog.as_ref(), None);
            drop(m);
            emit(api::Event::Props(rows));
        }
        Event::Changes(g) => {
            let mut m = mirror.lock().unwrap();
            for c in &g.changes {
                if let Some(r) = m.rows.get_mut(&c.code) {
                    r.value = c.new.clone();
                    r.changed_at = Some(g.at);
                }
            }
            m.groups.push_front(g.clone());
            m.groups.truncate(FEED);
            let rows: Vec<PropRow> = m.rows.values().cloned().collect();
            history.lock().unwrap().on_rows(&rows, m.catalog.as_ref(), Some(&g.changes));
            drop(m);
            write_log(log, serde_json::json!({ "kind": "changes", "group": g }));
            emit(api::Event::Changes(g));
        }
        Event::Log(line) => write_log(log, serde_json::json!({ "kind": "log", "text": line })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole Lab on the fake camera, in real time: connect, label, export, shut down.
    #[test]
    fn lab_on_the_fake_camera() {
        std::env::set_var("UKIYO_FAKE_CAMERA", "1");
        let data = tempfile::tempdir().unwrap();
        let cats = tempfile::tempdir().unwrap();
        let lock = OwnerLock::try_acquire(&data.path().join("owner.lock")).unwrap();
        let lab = Lab::start(lock, data.path().into(), cats.path().into(), |_| {});
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while lab.snapshot().status.link != Link::Connected || lab.snapshot().rows.is_empty() {
            assert!(std::time::Instant::now() < deadline, "fake camera never connected");
            std::thread::sleep(Duration::from_millis(20));
        }
        let snap = lab.snapshot();
        assert!(snap.fake);
        assert_eq!(snap.status.model.as_deref(), Some("X-T5"));

        let cat = lab
            .label("test", LabelPatch {
                code: 0xD1B2,
                name: Some("JpegHeif".into()),
                values: vec![("7".into(), "JPEG".into())],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(cat.properties["0xd1b2"].name.as_deref(), Some("JpegHeif"));
        let saved = Catalog::load(&cats.path().join("x-t5.json")).unwrap();
        assert_eq!(saved.properties["0xd1b2"].values["7"], "JPEG");
        assert_eq!(saved.firmware, ["9.99"]);

        let mut set = indexmap::IndexMap::new();
        set.insert("0xd023".to_string(), "2".to_string());
        let p = lab.plan(PlanInput { set, allow_unconfirmed: true, ..Default::default() }).unwrap();
        let out = lab.apply("test", &p.plan_id).unwrap();
        assert_eq!(out.applied, 1, "{out:?}");
        let path = lab.export("test").unwrap();
        let dump: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(dump["model"], "X-T5");

        lab.shutdown();
        assert!(crate::serve::lock::is_free(&data.path().join("owner.lock")), "shutdown releases the lock");
        let log = std::fs::read_to_string(snap.session_path.unwrap()).unwrap();
        for kind in ["\"start\"", "\"status\"", "\"label\"", "\"write\"", "\"export\"", "\"stop\""] {
            assert!(log.contains(kind), "session log lacks {kind}");
        }
    }
}
