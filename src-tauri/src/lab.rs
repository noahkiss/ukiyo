//! The Lab: the app side of the connection owner. It mirrors what the owner reports for
//! the window, writes the session log, and holds the catalog the person labels into.
//!
//! Files: the catalog lives in the repo's `catalog/` in a dev build (so labels can be
//! committed), `UKIYO_CATALOG_DIR` when set, else the app data dir. Session logs, dumps and
//! settings always go to the app data dir.

use crate::camera::clock::{Clock, RealClock};
use crate::camera::fake::{FakeCamera, FakeHandle, FakeMode};
use crate::camera::owner::{self, ChangeGroup, Config, Event, Link, PropRow, Request, Status, WriteOutcome};
use crate::camera::ptp::{self, Value};
use crate::camera::transport::Connector;
use crate::camera::usb::UsbConnector;
use crate::catalog::{Catalog, LabelPatch};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{BTreeMap, VecDeque};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Change groups kept for the window's feed.
const FEED: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct LabStatus(pub Status);

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct LabProps(pub Vec<PropRow>);

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct LabChanges(pub ChangeGroup);

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CatalogChanged(pub Option<Catalog>);

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Settings {
    keep_awake: bool,
}

/// What the person at the fake camera does (`UKIYO_FAKE_CAMERA=1` only).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum FakeAction {
    OpenMenu,
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
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../catalog");
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

impl Lab {
    /// Starts the camera thread. `emit` sends an event to the window.
    pub fn start(data_dir: PathBuf, catalog_dir: PathBuf, emit: impl Fn(LabEvent) + Send + Sync + 'static) -> Arc<Lab> {
        let _ = std::fs::create_dir_all(data_dir.join("sessions"));
        let session_path = data_dir.join("sessions").join(format!("{}.jsonl", stamp()));
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&session_path).ok();
        let session_path = file.as_ref().map(|_| session_path);
        let log = Arc::new(Mutex::new(file));
        let mirror = Arc::new(Mutex::new(Mirror::default()));
        let settings: Settings = std::fs::read_to_string(data_dir.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let clock: Arc<dyn Clock> = Arc::new(RealClock::new());
        let use_fake = std::env::var("UKIYO_FAKE_CAMERA").is_ok_and(|v| v == "1");
        let (connector, fake): (Box<dyn Connector>, _) = if use_fake {
            let mode = if std::env::var("UKIYO_FAKE_MODE").is_ok_and(|v| v == "rawconv") { FakeMode::RawConv } else { FakeMode::Tether };
            let cam = FakeCamera::x_t5(clock.clone(), mode).with_op_cost(Duration::from_millis(3));
            let h = cam.handle();
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
            Box::new(move |e: Event| on_event(e, &mirror, &log, &catalog_dir, &*emit))
        };
        let (tx, thread) = owner::spawn(connector, clock, sink, Config::default());
        if settings.keep_awake {
            let _ = tx.send(Request::KeepAwake(true));
        }
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
        let lab = Arc::new(Lab {
            tx: Mutex::new(tx),
            mirror,
            log,
            session_path,
            fake,
            data_dir,
            catalog_dir,
            thread: Mutex::new(Some(thread)),
        });
        lab.log_line(serde_json::json!({ "kind": "start", "fake": lab.fake.is_some() }));
        lab
    }

    fn send(&self, r: Request) {
        let _ = self.tx.lock().unwrap().send(r);
    }

    fn log_line(&self, v: serde_json::Value) {
        write_log(&self.log, v)
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

    pub fn write(&self, code: u16, value: Value) -> WriteOutcome {
        let (tx, rx) = std::sync::mpsc::channel();
        self.send(Request::Write { code, value, reply: Some(tx) });
        let out = rx.recv_timeout(Duration::from_secs(15)).unwrap_or(WriteOutcome {
            code,
            ok: false,
            result: "no answer from the camera thread".into(),
            before: None,
            readback: None,
        });
        self.log_line(serde_json::json!({ "kind": "write", "outcome": out }));
        out
    }

    pub fn pin(&self, codes: Vec<u16>) {
        self.send(Request::SetPinned(codes));
    }

    pub fn keep_awake(&self, on: bool) {
        self.send(Request::KeepAwake(on));
        let s = Settings { keep_awake: on };
        let _ = std::fs::write(self.data_dir.join("settings.json"), serde_json::to_string_pretty(&s).unwrap_or_default());
    }

    pub fn connect(&self, on: bool) {
        self.send(if on { Request::Connect } else { Request::Disconnect });
    }

    pub fn export(&self) -> Result<String, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.send(Request::Export(tx));
        let dump = rx.recv_timeout(Duration::from_secs(60)).map_err(|_| "no answer from the camera thread".to_string())??;
        let dir = self.data_dir.join("dumps");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mode = serde_json::to_value(dump.mode).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let path = dir.join(format!("{}-{mode}-{}.json", crate::catalog::slug(&dump.model), stamp()));
        std::fs::write(&path, serde_json::to_string_pretty(&dump).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        self.log_line(serde_json::json!({ "kind": "export", "path": path.display().to_string() }));
        Ok(path.display().to_string())
    }

    /// Applies a label and saves the catalog file.
    pub fn label(&self, patch: LabelPatch) -> Result<Catalog, String> {
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
        self.log_line(serde_json::json!({ "kind": "label", "patch": patch }));
        Ok(out)
    }

    /// Text the person typed about what they just changed, tied to a change group.
    pub fn note(&self, group: Option<u32>, text: String) {
        self.log_line(serde_json::json!({ "kind": "note", "group": group, "text": text }));
    }

    pub fn fake(&self, a: FakeAction) -> Result<(), String> {
        let Some((h, auto)) = &self.fake else { return Err("no fake camera".into()) };
        match a {
            FakeAction::OpenMenu => h.open_menu(),
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

    /// A clean disconnect (AUTO POWER OFF restored, session closed), then the thread ends.
    pub fn shutdown(&self) {
        self.send(Request::Shutdown);
        if let Some(t) = self.thread.lock().unwrap().take() {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !t.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        self.log_line(serde_json::json!({ "kind": "stop" }));
    }
}

pub enum LabEvent {
    Status(LabStatus),
    Props(LabProps),
    Changes(LabChanges),
    Catalog(CatalogChanged),
}

fn write_log(log: &Mutex<Option<std::fs::File>>, mut v: serde_json::Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert("t".into(), serde_json::json!(wall_ms()));
    }
    if let Some(f) = log.lock().unwrap().as_mut() {
        let _ = writeln!(f, "{v}");
    }
}

fn on_event(e: Event, mirror: &Mutex<Mirror>, log: &Mutex<Option<std::fs::File>>, catalog_dir: &Path, emit: &dyn Fn(LabEvent)) {
    match e {
        Event::Status(s) => {
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
                emit(LabEvent::Catalog(CatalogChanged(c)));
            }
            emit(LabEvent::Status(LabStatus(s)));
        }
        Event::Props(rows) => {
            let mut m = mirror.lock().unwrap();
            m.rows = rows.iter().map(|r| (r.code, r.clone())).collect();
            drop(m);
            emit(LabEvent::Props(LabProps(rows)));
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
            drop(m);
            write_log(log, serde_json::json!({ "kind": "changes", "group": g }));
            emit(LabEvent::Changes(LabChanges(g)));
        }
        Event::Log(line) => write_log(log, serde_json::json!({ "kind": "log", "text": line })),
    }
}
