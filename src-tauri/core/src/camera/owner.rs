//! The connection owner: one thread holds the camera session and does every operation on
//! it. Requests arrive on a channel; writes jump the queue and are always read back.
//! Between requests it polls without sleeping: each cycle reads the hot set (screen state,
//! the STILL/MOVIE switch, anything changed in the last 10 s, pinned codes) and the next
//! slice of a sweep over every readable property.
//!
//! Power safety (docs/protocol.md, "Camera state and power"): a burst of 0x2019 followed
//! by the device leaving the bus is auto power off. The owner never reconnects in a loop:
//! it checks presence every 3 s, waits 30 s after a disappearance before it opens the
//! camera again, and resets USB at most once, only when OpenSession on a fresh claim fails.

use super::clock::Clock;
use super::ptp::{self, DeviceInfo, Form, PropDesc, Value};
use super::transport::{Connector, Reply, Transport, TransportError};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

pub const SCREEN: u16 = 0xD22E;
pub const STILL_MOVIE: u16 = 0xD307;
pub const AUTO_POWER_OFF: u16 = 0xD381;
/// D381's OFF value.
pub const AUTO_POWER_OFF_OFF: i64 = 3;

/// Timing rules. Tests shorten nothing: the fake clock makes them free.
#[derive(Debug, Clone)]
pub struct Config {
    /// How often to look for the camera while disconnected.
    pub presence_every: Duration,
    /// How long to wait after the camera disappears (or a session fails) before opening it.
    pub reopen_after: Duration,
    /// How often to read D22E while a menu is open or the camera answers busy.
    pub probe_every: Duration,
    /// A code that changed this recently is read every cycle.
    pub hot_for: Duration,
    /// Sweep reads per cycle.
    pub sweep_slice: usize,
    /// Consecutive 0x2019 answers that mean the camera is busy (or powering off).
    pub busy_after: u32,
    /// A disappearance this soon after a busy burst is auto power off.
    pub power_off_window: Duration,
    /// How often a status event goes out while nothing else changes.
    pub status_every: Duration,
    /// The waits before each retry of a write the camera answered 0x2019 (busy). After a
    /// half-press release the X-T5 answers writes busy for about 0.2 s (docs/protocol.md).
    /// A busy write is retried on the same session; the owner never reconnects for it.
    pub write_retry: Vec<Duration>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            presence_every: Duration::from_secs(3),
            reopen_after: Duration::from_secs(30),
            probe_every: Duration::from_millis(500),
            hot_for: Duration::from_secs(10),
            sweep_slice: 8,
            busy_after: 3,
            power_off_window: Duration::from_secs(5),
            status_every: Duration::from_millis(500),
            // 100 ms doubling: five retries, 3.1 s in all.
            write_retry: [100, 200, 400, 800, 1600].into_iter().map(Duration::from_millis).collect(),
        }
    }
}

/// The USB connection mode set on the camera (SET UP > CONNECTION SETTING).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UsbMode {
    CardReader,
    RawConv,
    Tether,
    Unknown,
}

/// Detects the connection mode from what the camera advertises (docs/protocol.md, "USB
/// connection modes"): card reader shows a handful of properties, RAW CONV. about 60
/// including the recipe fields, tether more than 200.
pub fn detect_mode(info: &DeviceInfo, descriptors_work: bool) -> UsbMode {
    let n = info.properties.len();
    let has = |c: u16| info.properties.contains(&c);
    if n >= 150 && has(SCREEN) {
        UsbMode::Tether
    } else if has(0xD18E) && (!descriptors_work || n < 150) {
        UsbMode::RawConv
    } else if n <= 8 {
        UsbMode::CardReader
    } else {
        UsbMode::Unknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Capture {
    Stills,
    Movie,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Screen {
    Shooting,
    Playback,
    Menu,
    Unknown,
}

/// Where the connection stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Link {
    /// No camera on the bus; looking every 3 s.
    Searching,
    /// Waiting out the reopen delay after a disappearance or a failed session.
    Waiting,
    Connecting,
    Connected,
    /// The camera answers 0x2019 (busy); probing slowly.
    Busy,
    /// The camera powered itself off (busy burst, then gone).
    Asleep,
    /// Disconnected on request; reconnects only on a Connect request.
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeepAwake {
    pub enabled: bool,
    /// The AUTO POWER OFF value to restore on a clean disconnect, while it is held at OFF.
    pub previous: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub link: Link,
    /// Why the link is in its state, for the status bar ("auto power off", "unplugged").
    pub reason: Option<String>,
    pub model: Option<String>,
    pub firmware: Option<String>,
    pub mode: Option<UsbMode>,
    pub capture: Option<Capture>,
    pub screen: Option<Screen>,
    /// Seconds until the next open attempt is allowed.
    pub reopen_in_s: Option<f64>,
    /// The last poll cycle (hot set and one sweep slice).
    pub cycle_ms: f64,
    /// The last full sweep of every readable property.
    pub sweep_ms: f64,
    /// Mean time per PTP operation.
    pub op_ms: f64,
    pub hot: u32,
    pub readable: u32,
    pub keep_awake: KeepAwake,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            link: Link::Searching,
            reason: None,
            model: None,
            firmware: None,
            mode: None,
            capture: None,
            screen: None,
            reopen_in_s: None,
            cycle_ms: 0.0,
            sweep_ms: 0.0,
            op_ms: 0.0,
            hot: 0,
            readable: 0,
            keep_awake: KeepAwake { enabled: false, previous: None },
        }
    }
}

/// One property as the UI shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PropRow {
    pub code: u16,
    pub datatype: u16,
    pub writable: bool,
    pub form: Form,
    pub value: Option<Value>,
    /// The last response code other than OK, by name.
    pub error: Option<String>,
    /// Readable in the current mode.
    pub available: bool,
    /// Wall-clock ms of the last change seen this session.
    pub changed_at: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub code: u16,
    pub old: Option<Value>,
    pub new: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChangeSource {
    /// Seen while polling: a person changed something on the camera.
    Camera,
    /// The read-back after a write from the app.
    Write,
}

/// The changes one poll cycle found: usually one physical action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChangeGroup {
    pub id: u32,
    pub at: f64,
    pub source: ChangeSource,
    pub changes: Vec<Change>,
    /// Who asked for the write (`app`, `cli`, `mcp:<client>`); `None` for a camera change
    /// and for the owner's own writes (keep-awake).
    #[serde(default)]
    pub actor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WriteOutcome {
    pub code: u16,
    pub ok: bool,
    /// The response code's name, or why nothing was sent.
    pub result: String,
    pub before: Option<Value>,
    pub readback: Option<Value>,
    /// The camera's response code; `None` when nothing was sent.
    #[serde(default)]
    pub rc: Option<u16>,
    /// SetDevicePropValue operations sent: 1, more when the camera answered busy.
    #[serde(default)]
    pub attempts: u32,
}

impl WriteOutcome {
    /// A write refused before anything reached the camera.
    pub fn refused(code: u16, why: impl Into<String>) -> WriteOutcome {
        WriteOutcome { code, ok: false, result: why.into(), before: None, readback: None, rc: None, attempts: 0 }
    }
}

/// Why the owner refused a write without sending it (`WriteOutcome::result`).
pub mod refusal {
    pub const NOT_CONNECTED: &str = "not connected";
    pub const PAUSED: &str = "camera in menu or busy";
    pub const UNKNOWN: &str = "unknown property";
    pub const NO_DESCRIPTOR: &str = "no descriptor";
    pub const READ_ONLY: &str = "read-only";
    pub const UNAVAILABLE: &str = "not readable in this mode";
    pub const LOST: &str = "connection lost";
}

/// A dump of every descriptor and value, like `tools/ptp/dump_props.py`.
#[derive(Debug, Clone, Serialize)]
pub struct Dump {
    pub model: String,
    pub manufacturer: String,
    pub firmware: String,
    pub mode: UsbMode,
    pub capture: Option<Capture>,
    pub properties: BTreeMap<String, serde_json::Value>,
}

pub enum Request {
    /// `actor` goes on the read-back's change group.
    Write { code: u16, value: Value, actor: Option<String>, reply: Option<Sender<WriteOutcome>> },
    SetPinned(Vec<u16>),
    KeepAwake(bool),
    Export(Sender<Result<Dump, String>>),
    /// Restores AUTO POWER OFF, closes the session, and stays off until `Connect`.
    Disconnect,
    Connect,
    /// A clean disconnect, then the thread ends.
    Shutdown,
}

impl Request {
    fn is_write(&self) -> bool {
        matches!(self, Request::Write { .. })
    }
}

/// Pending requests: writes go ahead of everything else, each kind in arrival order.
#[derive(Default)]
pub struct RequestQueue {
    writes: VecDeque<Request>,
    rest: VecDeque<Request>,
}

impl RequestQueue {
    pub fn push(&mut self, r: Request) {
        if r.is_write() {
            self.writes.push_back(r)
        } else {
            self.rest.push_back(r)
        }
    }
    pub fn pop(&mut self) -> Option<Request> {
        self.writes.pop_front().or_else(|| self.rest.pop_front())
    }
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty() && self.rest.is_empty()
    }
}

/// What the owner reports.
#[derive(Debug, Clone)]
pub enum Event {
    Status(Status),
    /// Every property, after the camera was described (connect, mode switch).
    Props(Vec<PropRow>),
    Changes(ChangeGroup),
    /// One line for the session log.
    Log(String),
}

pub type Sink = Box<dyn FnMut(Event) + Send>;

struct Prop {
    desc: Option<PropDesc>,
    value: Option<Value>,
    error: Option<u16>,
    available: bool,
    changed: Option<Duration>,
    changed_wall: Option<i64>,
}

impl Prop {
    fn datatype(&self) -> u16 {
        self.desc.as_ref().map(|d| d.datatype).unwrap_or(0)
    }
    fn row(&self, code: u16) -> PropRow {
        PropRow {
            code,
            datatype: self.datatype(),
            writable: self.desc.as_ref().is_some_and(|d| d.writable),
            form: self.desc.as_ref().map(|d| d.form.clone()).unwrap_or(Form::None),
            value: self.value.clone(),
            error: self.error.map(|rc| ptp::rc_name(rc).to_string()),
            available: self.available,
            changed_at: self.changed_wall.map(|w| w as f64),
        }
    }
}

enum Pause {
    None,
    Menu,
    Busy,
}

struct Session {
    t: Box<dyn Transport>,
    info: DeviceInfo,
    mode: UsbMode,
    props: BTreeMap<u16, Prop>,
    pause: Pause,
    next_probe: Duration,
    busy_run: u32,
    last_busy: Option<Duration>,
    /// This cycle's remaining reads.
    plan: VecDeque<u16>,
    cycle_start: Duration,
    cycle_changes: Vec<Change>,
    sweep: Vec<u16>,
    sweep_pos: usize,
    sweep_start: Duration,
    redescribe: bool,
    ops: u64,
    op_time: Duration,
}

enum Phase {
    Idle { next_check: Duration },
    Connected(Box<Session>),
}

pub struct Owner {
    connector: Box<dyn Connector>,
    clock: Arc<dyn Clock>,
    sink: Sink,
    rx: Receiver<Request>,
    queue: RequestQueue,
    cfg: Config,
    phase: Phase,
    status: Status,
    last_status: Option<Status>,
    last_status_at: Duration,
    /// When the camera last went away or a session failed: no open before `reopen_after`.
    blocked_since: Option<Duration>,
    stopped: bool,
    pinned: HashSet<u16>,
    group_id: u32,
    done: bool,
}

impl Owner {
    pub fn new(connector: Box<dyn Connector>, clock: Arc<dyn Clock>, sink: Sink, rx: Receiver<Request>, cfg: Config) -> Self {
        Self {
            connector,
            clock,
            sink,
            rx,
            queue: RequestQueue::default(),
            cfg,
            phase: Phase::Idle { next_check: Duration::ZERO },
            status: Status::default(),
            last_status: None,
            last_status_at: Duration::ZERO,
            blocked_since: None,
            stopped: false,
            pinned: HashSet::new(),
            group_id: 0,
            done: false,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Runs until a Shutdown request, or until every sender is gone.
    pub fn run(mut self) {
        while !self.done {
            self.step();
        }
    }

    /// One unit of work: a request, one poll read, or a short idle wait.
    pub fn step(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(r) => self.queue.push(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if self.queue.is_empty() {
                        self.queue.push(Request::Shutdown);
                    }
                    break;
                }
            }
        }
        if let Some(r) = self.queue.pop() {
            self.handle(r);
        } else {
            match &self.phase {
                Phase::Idle { .. } => self.idle_step(),
                Phase::Connected(_) => self.poll_step(),
            }
        }
        self.publish(false);
    }

    fn emit(&mut self, e: Event) {
        (self.sink)(e)
    }

    fn log(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        self.emit(Event::Log(m));
    }

    fn publish(&mut self, force: bool) {
        let now = self.clock.now();
        if let Phase::Idle { .. } = self.phase {
            self.status.reopen_in_s = self
                .blocked_since
                .map(|t| (t + self.cfg.reopen_after).saturating_sub(now).as_secs_f64())
                .filter(|s| *s > 0.0);
        } else {
            self.status.reopen_in_s = None;
        }
        let changed = self.last_status.as_ref().is_none_or(|s| {
            let mut a = s.clone();
            let mut b = self.status.clone();
            // Timings move every cycle; they alone go out at the status rate.
            for x in [&mut a, &mut b] {
                x.cycle_ms = 0.0;
                x.sweep_ms = 0.0;
                x.op_ms = 0.0;
                x.hot = 0;
                x.reopen_in_s = x.reopen_in_s.map(|s| s.ceil());
            }
            a != b
        });
        if force || changed || now >= self.last_status_at + self.cfg.status_every {
            self.last_status = Some(self.status.clone());
            self.last_status_at = now;
            let s = self.status.clone();
            self.emit(Event::Status(s));
        }
    }

    // ---- requests ----

    fn handle(&mut self, r: Request) {
        match r {
            Request::Write { code, value, actor, reply } => {
                let out = self.write(code, value, actor);
                if let Some(tx) = reply {
                    let _ = tx.send(out);
                }
            }
            Request::SetPinned(codes) => self.pinned = codes.into_iter().collect(),
            Request::KeepAwake(on) => {
                self.status.keep_awake.enabled = on;
                if on {
                    self.hold_awake();
                } else {
                    self.release_awake();
                }
            }
            Request::Export(tx) => {
                let _ = tx.send(self.export());
            }
            Request::Disconnect => {
                self.close_clean();
                self.stopped = true;
                self.set_idle(Link::Stopped, Some("disconnected".into()));
            }
            Request::Connect => {
                self.stopped = false;
                if let Phase::Idle { .. } = self.phase {
                    self.phase = Phase::Idle { next_check: self.clock.now() };
                    self.status.link = if self.blocked_since.is_some() { Link::Waiting } else { Link::Searching };
                    self.status.reason = None;
                }
            }
            Request::Shutdown => {
                self.close_clean();
                self.done = true;
            }
        }
    }

    fn session(&mut self) -> Option<&mut Session> {
        match &mut self.phase {
            Phase::Connected(s) => Some(s),
            Phase::Idle { .. } => None,
        }
    }

    fn write(&mut self, code: u16, value: Value, actor: Option<String>) -> WriteOutcome {
        use refusal::*;
        let refuse = |code: u16, why: &str| WriteOutcome::refused(code, why);
        let Phase::Connected(s) = &mut self.phase else { return refuse(code, NOT_CONNECTED) };
        if !matches!(s.pause, Pause::None) {
            return refuse(code, PAUSED);
        }
        let Some(p) = s.props.get(&code) else { return refuse(code, UNKNOWN) };
        let Some(desc) = p.desc.as_ref() else { return refuse(code, NO_DESCRIPTOR) };
        if !desc.writable {
            return refuse(code, READ_ONLY);
        }
        if !p.available {
            return refuse(code, UNAVAILABLE);
        }
        let (before, datatype) = (p.value.clone(), desc.datatype);
        let bytes = match ptp::encode(datatype, &value) {
            Ok(b) => b,
            Err(e) => return refuse(code, &e.0),
        };
        // 0x2019 means "not now": wait and send the same write again, on the same session.
        let mut attempts = 0u32;
        let retry = self.cfg.write_retry.clone();
        let rc = loop {
            attempts += 1;
            let rc = match self.op(ptp::OP_SET_PROP_VALUE, &[code as u32], Some(&bytes)) {
                Ok(r) => r.rc,
                Err(e) => {
                    self.lost(e);
                    return WriteOutcome { attempts, ..refuse(code, LOST) };
                }
            };
            match retry.get(attempts as usize - 1) {
                Some(wait) if rc == ptp::RC_DEVICE_BUSY => self.clock.sleep(*wait),
                _ => break rc,
            }
        };
        // Always read back, whatever the answer.
        let readback = self.read_value(code, ChangeSource::Write, actor.clone());
        let tries = if attempts > 1 { format!(", {attempts} tries") } else { String::new() };
        let by = actor.as_deref().map(|a| format!(" by {a}")).unwrap_or_default();
        self.log(format!(
            "write {code:#06x}{by} {} -> {}: {}{tries} (read back {})",
            before.as_ref().map(Value::key).unwrap_or_default(),
            value.key(),
            ptp::rc_name(rc),
            readback.as_ref().map(Value::key).unwrap_or_else(|| "nothing".into())
        ));
        WriteOutcome { code, ok: rc == ptp::RC_OK, result: ptp::rc_name(rc).into(), before, readback, rc: Some(rc), attempts }
    }

    /// Reads one value now and reports a change as its own group.
    fn read_value(&mut self, code: u16, source: ChangeSource, actor: Option<String>) -> Option<Value> {
        let before = self.session()?.props.get(&code).and_then(|p| p.value.clone());
        let r = match self.op(ptp::OP_GET_PROP_VALUE, &[code as u32], None) {
            Ok(r) => r,
            Err(e) => {
                self.lost(e);
                return None;
            }
        };
        if r.rc != ptp::RC_OK {
            return None;
        }
        let now = self.clock.now();
        let wall = self.clock.wall_ms();
        let s = self.session()?;
        let p = s.props.get_mut(&code)?;
        let v = ptp::decode_value(p.datatype(), &r.data);
        p.error = None;
        if p.value.as_ref() != Some(&v) {
            p.value = Some(v.clone());
            p.changed = Some(now);
            p.changed_wall = Some(wall);
            let change = Change { code, old: before, new: Some(v.clone()) };
            self.emit_group(source, vec![change], actor);
        }
        Some(v)
    }

    fn emit_group(&mut self, source: ChangeSource, changes: Vec<Change>, actor: Option<String>) {
        self.group_id += 1;
        let g = ChangeGroup { id: self.group_id, at: self.clock.wall_ms() as f64, source, changes, actor };
        self.emit(Event::Changes(g));
    }

    fn hold_awake(&mut self) {
        let Phase::Connected(s) = &mut self.phase else { return };
        if s.mode != UsbMode::Tether || self.status.capture != Some(Capture::Stills) || !s.props.get(&AUTO_POWER_OFF).is_some_and(|p| p.available && p.desc.as_ref().is_some_and(|d| d.writable)) {
            return;
        }
        let current = s.props.get(&AUTO_POWER_OFF).and_then(|p| p.value.clone());
        let Some(Value::Int(cur)) = current else { return };
        if cur == AUTO_POWER_OFF_OFF {
            return; // Already OFF; a previous value held from an earlier session stays.
        }
        let out = self.write(AUTO_POWER_OFF, Value::Int(AUTO_POWER_OFF_OFF), None);
        if out.ok && out.readback == Some(Value::Int(AUTO_POWER_OFF_OFF)) && self.status.keep_awake.previous.is_none() {
            self.status.keep_awake.previous = Some(cur);
        }
    }

    fn release_awake(&mut self) {
        let Some(prev) = self.status.keep_awake.previous else { return };
        if self.session().is_none() {
            return;
        }
        let out = self.write(AUTO_POWER_OFF, Value::Int(prev), None);
        if out.ok {
            self.status.keep_awake.previous = None;
        }
    }

    fn close_clean(&mut self) {
        if self.session().is_none() {
            return;
        }
        // A menu blocks writes; give the restore its chance only when the camera answers.
        self.release_awake();
        if let Some(s) = self.session() {
            let _ = s.t.op(ptp::OP_CLOSE_SESSION, &[], None);
        }
        self.log("session closed");
    }

    fn export(&mut self) -> Result<Dump, String> {
        let Phase::Connected(s) = &mut self.phase else { return Err("not connected".into()) };
        if !matches!(s.pause, Pause::None) {
            return Err("camera in menu or busy".into());
        }
        let codes: Vec<u16> = s.info.properties.clone();
        let (info, mode) = (s.info.clone(), s.mode);
        let mut props = BTreeMap::new();
        for code in codes {
            let key = format!("{code:#06x}");
            let d = self.op(ptp::OP_GET_PROP_DESC, &[code as u32], None).map_err(|e| e.to_string())?;
            let entry = if d.rc == ptp::RC_OK {
                match ptp::prop_desc(&d.data) {
                    Ok(desc) => serde_json::json!({
                        "type": format!("{:#06x}", desc.datatype),
                        "rw": desc.writable,
                        "default": desc.default,
                        "current": desc.current,
                        "form": desc.form,
                    }),
                    Err(e) => serde_json::json!({ "parse_error": e.0, "raw": ptp::hex(&d.data) }),
                }
            } else {
                let v = self.op(ptp::OP_GET_PROP_VALUE, &[code as u32], None).map_err(|e| e.to_string())?;
                if v.rc == ptp::RC_OK {
                    serde_json::json!({ "bytes": v.data.len(), "raw": ptp::hex(&v.data) })
                } else {
                    serde_json::json!({ "error": format!("desc {:#x}, value {:#x}", d.rc, v.rc) })
                }
            };
            props.insert(key, entry);
        }
        let capture = self.status.capture;
        Ok(Dump {
            model: info.model,
            manufacturer: info.manufacturer,
            firmware: info.firmware,
            mode,
            capture,
            properties: props,
        })
    }

    // ---- connection ----

    fn op(&mut self, code: u16, params: &[u32], data: Option<&[u8]>) -> Result<Reply, TransportError> {
        let start = self.clock.now();
        let clock = self.clock.clone();
        let s = self.session().ok_or(TransportError::Gone)?;
        let r = s.t.op(code, params, data);
        s.ops += 1;
        s.op_time += clock.now().saturating_sub(start);
        r
    }

    fn set_idle(&mut self, link: Link, reason: Option<String>) {
        let now = self.clock.now();
        self.phase = Phase::Idle { next_check: now + self.cfg.presence_every };
        self.status.link = link;
        self.status.reason = reason;
        self.status.screen = None;
        self.status.capture = None;
        self.status.mode = None;
        self.status.cycle_ms = 0.0;
        self.status.sweep_ms = 0.0;
        self.status.hot = 0;
        self.status.readable = 0;
        self.publish(true);
    }

    /// The transport failed: decide whether the camera left, and back off either way.
    fn lost(&mut self, e: TransportError) {
        let now = self.clock.now();
        let last_busy = self.session().and_then(|s| s.last_busy);
        let present = e != TransportError::Gone && self.connector.present();
        self.blocked_since = Some(now);
        let (link, reason) = if present {
            (Link::Waiting, format!("USB error: {e}"))
        } else if last_busy.is_some_and(|t| now.saturating_sub(t) <= self.cfg.power_off_window) {
            (Link::Asleep, "auto power off".to_string())
        } else {
            (Link::Searching, "unplugged".to_string())
        };
        self.log(format!("connection lost: {reason}"));
        self.set_idle(link, Some(reason));
    }

    fn idle_step(&mut self) {
        let now = self.clock.now();
        let Phase::Idle { next_check } = self.phase else { return };
        if self.stopped {
            self.clock.sleep(Duration::from_millis(100));
            return;
        }
        if now < next_check {
            self.clock.sleep((next_check - now).min(Duration::from_millis(100)));
            return;
        }
        self.phase = Phase::Idle { next_check: now + self.cfg.presence_every };
        let present = self.connector.present();
        let wait_over = self.blocked_since.is_none_or(|t| now >= t + self.cfg.reopen_after);
        if !present {
            if self.status.link == Link::Waiting {
                self.status.link = Link::Searching;
            }
            return;
        }
        if !wait_over {
            if self.status.link == Link::Searching {
                self.status.link = Link::Waiting;
            }
            return;
        }
        self.connect();
    }

    fn connect(&mut self) {
        self.status.link = Link::Connecting;
        self.status.reason = None;
        self.publish(true);
        let mut reset_used = false;
        let t = loop {
            match self.open_session(reset_used) {
                Ok(t) => break t,
                Err(e) if !reset_used && self.connector.present() => {
                    // OpenSession on a fresh claim failed: reset once and try again.
                    self.log(format!("OpenSession failed ({e}); resetting USB once"));
                    reset_used = true;
                }
                Err(e) => {
                    self.blocked_since = Some(self.clock.now());
                    self.log(format!("OpenSession failed ({e}); waiting before the next try"));
                    let link = if self.connector.present() { Link::Waiting } else { Link::Searching };
                    self.set_idle(link, Some(format!("could not open a session: {e}")));
                    return;
                }
            }
        };
        self.blocked_since = None;
        let now = self.clock.now();
        let mut s = Box::new(Session {
            t,
            info: DeviceInfo {
                vendor_ext: String::new(),
                manufacturer: String::new(),
                model: String::new(),
                firmware: String::new(),
                operations: Vec::new(),
                properties: Vec::new(),
            },
            mode: UsbMode::Unknown,
            props: BTreeMap::new(),
            pause: Pause::None,
            next_probe: now,
            busy_run: 0,
            last_busy: None,
            plan: VecDeque::new(),
            cycle_start: now,
            cycle_changes: Vec::new(),
            sweep: Vec::new(),
            sweep_pos: 0,
            sweep_start: now,
            redescribe: false,
            ops: 0,
            op_time: Duration::ZERO,
        });
        let info = match s.t.op(ptp::OP_GET_DEVICE_INFO, &[], None) {
            Ok(r) if r.rc == ptp::RC_OK => ptp::device_info(&r.data).map_err(|e| e.0),
            Ok(r) => Err(ptp::rc_name(r.rc).to_string()),
            Err(e) => Err(e.to_string()),
        };
        let info = match info {
            Ok(i) => i,
            Err(e) => {
                self.blocked_since = Some(self.clock.now());
                self.set_idle(Link::Waiting, Some(format!("GetDeviceInfo failed: {e}")));
                return;
            }
        };
        s.info = info.clone();
        self.phase = Phase::Connected(s);
        self.status.model = Some(info.model.clone());
        self.status.firmware = Some(info.firmware.clone());
        self.log(format!("connected: {} {} fw {}, {} properties", info.manufacturer, info.model, info.firmware, info.properties.len()));
        self.status.link = Link::Connected;
        self.describe();
    }

    fn open_session(&mut self, reset: bool) -> Result<Box<dyn Transport>, String> {
        let mut t = self.connector.open(reset).map_err(|e| e.to_string())?;
        match t.op(ptp::OP_OPEN_SESSION, &[1], None) {
            Ok(r) if r.rc == ptp::RC_OK || r.rc == ptp::RC_SESSION_ALREADY_OPEN => Ok(t),
            Ok(r) => Err(ptp::rc_name(r.rc).to_string()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Reads every advertised descriptor (values only where descriptors fail), detects the
    /// mode, and rebuilds the sweep.
    fn describe(&mut self) {
        let Phase::Connected(s) = &mut self.phase else { return };
        let codes = s.info.properties.clone();
        let mut props = BTreeMap::new();
        let mut desc_ok = 0usize;
        for code in &codes {
            let code = *code;
            let r = match self.op(ptp::OP_GET_PROP_DESC, &[code as u32], None) {
                Ok(r) => r,
                Err(e) => return self.lost(e),
            };
            if r.rc == ptp::RC_FUJI_MENU {
                // A menu opened mid-describe: pause, and describe again when it closes.
                return self.enter_pause(Pause::Menu, true);
            }
            let mut p = Prop { desc: None, value: None, error: None, available: false, changed: None, changed_wall: None };
            if r.rc == ptp::RC_OK {
                if let Ok(d) = ptp::prop_desc(&r.data) {
                    desc_ok += 1;
                    p.value = (d.datatype != 0).then(|| d.current.clone());
                    p.available = true;
                    p.desc = Some(d);
                }
            }
            if p.value.is_none() {
                let v = match self.op(ptp::OP_GET_PROP_VALUE, &[code as u32], None) {
                    Ok(v) => v,
                    Err(e) => return self.lost(e),
                };
                if v.rc == ptp::RC_OK {
                    p.value = Some(ptp::decode_value(p.datatype(), &v.data));
                    p.available = true;
                } else {
                    p.error = Some(if r.rc != ptp::RC_OK { r.rc } else { v.rc });
                    p.available = false;
                }
            }
            props.insert(code, p);
        }
        let now = self.clock.now();
        let pinned_hot = self.cfg.hot_for;
        let Phase::Connected(s) = &mut self.phase else { return };
        // Keep the change times of codes seen before (a mode switch re-describes).
        for (code, p) in props.iter_mut() {
            if let Some(old) = s.props.get(code) {
                p.changed = old.changed.filter(|t| now.saturating_sub(*t) < pinned_hot);
                p.changed_wall = old.changed_wall;
            }
        }
        s.mode = detect_mode(&s.info, desc_ok > 0);
        s.props = props;
        s.sweep = s.props.iter().filter(|(_, p)| p.available).map(|(c, _)| *c).collect();
        s.sweep_pos = 0;
        s.sweep_start = now;
        s.plan.clear();
        s.redescribe = false;
        let mode = s.mode;
        let readable = s.sweep.len() as u32;
        let screen = s.props.get(&SCREEN).and_then(|p| p.value.clone());
        let capture = s.props.get(&STILL_MOVIE).and_then(|p| p.value.clone());
        let rows: Vec<PropRow> = s.props.iter().map(|(c, p)| p.row(*c)).collect();
        self.status.mode = Some(mode);
        self.status.readable = readable;
        self.status.capture = capture.and_then(capture_of);
        self.status.screen = screen.map(|v| screen_of(&v)).or(Some(Screen::Unknown));
        self.emit(Event::Props(rows));
        self.publish(true);
        if self.status.keep_awake.enabled {
            self.hold_awake();
        }
    }

    fn enter_pause(&mut self, pause: Pause, redescribe: bool) {
        let now = self.clock.now();
        let probe = self.cfg.probe_every;
        let Phase::Connected(s) = &mut self.phase else { return };
        let menu = matches!(pause, Pause::Menu);
        s.pause = pause;
        s.next_probe = now + probe;
        s.redescribe |= redescribe;
        s.plan.clear();
        s.cycle_changes.clear();
        if menu {
            self.status.screen = Some(Screen::Menu);
        } else {
            self.status.link = Link::Busy;
        }
        self.publish(true);
    }

    // ---- polling ----

    fn poll_step(&mut self) {
        let now = self.clock.now();
        let Phase::Connected(s) = &mut self.phase else { return };
        if !matches!(s.pause, Pause::None) {
            if now < s.next_probe {
                let wait = (s.next_probe - now).min(Duration::from_millis(50));
                self.clock.sleep(wait);
                return;
            }
            s.next_probe = now + self.cfg.probe_every;
            return self.probe();
        }
        if s.redescribe {
            return self.describe();
        }
        if s.plan.is_empty() {
            self.finish_cycle();
            self.plan_cycle();
        }
        let Some(code) = self.session().and_then(|s| s.plan.pop_front()) else {
            // Nothing readable (card reader with no values): idle a little.
            self.clock.sleep(Duration::from_millis(100));
            return;
        };
        self.poll_read(code);
    }

    /// A slow D22E read while paused. Any OK answer ends a busy pause; a screen other than
    /// the menu ends a menu pause.
    fn probe(&mut self) {
        let r = match self.op(ptp::OP_GET_PROP_VALUE, &[SCREEN as u32], None) {
            Ok(r) => r,
            Err(e) => return self.lost(e),
        };
        let now = self.clock.now();
        let Phase::Connected(s) = &mut self.phase else { return };
        match r.rc {
            ptp::RC_OK => {
                let v = ptp::decode_value(0x0004, &r.data);
                let screen = screen_of(&v);
                s.busy_run = 0;
                if let Some(p) = s.props.get_mut(&SCREEN) {
                    p.value = Some(v);
                }
                if screen == Screen::Menu {
                    s.pause = Pause::Menu;
                    self.status.screen = Some(Screen::Menu);
                    self.status.link = Link::Connected;
                    return;
                }
                s.pause = Pause::None;
                s.plan.clear();
                s.cycle_start = now;
                self.status.screen = Some(screen);
                self.status.link = Link::Connected;
                self.log("camera back from menu");
            }
            ptp::RC_DEVICE_BUSY => {
                s.busy_run += 1;
                s.last_busy = Some(now);
            }
            _ => {}
        }
    }

    fn plan_cycle(&mut self) {
        let now = self.clock.now();
        let hot_for = self.cfg.hot_for;
        let slice = self.cfg.sweep_slice;
        let pinned = self.pinned.clone();
        let Phase::Connected(s) = &mut self.phase else { return };
        let mut hot: Vec<u16> = vec![SCREEN, STILL_MOVIE];
        hot.extend(
            s.props
                .iter()
                .filter(|(c, p)| p.available && (pinned.contains(c) || p.changed.is_some_and(|t| now.saturating_sub(t) < hot_for)))
                .map(|(c, _)| *c),
        );
        hot.retain(|c| s.props.get(c).is_some_and(|p| p.available));
        let mut seen = HashSet::new();
        hot.retain(|c| seen.insert(*c));
        let hot_n = hot.len() as u32;
        let mut plan: VecDeque<u16> = hot.into();
        let mut sweep_done = None;
        for _ in 0..slice.min(s.sweep.len()) {
            if s.sweep_pos >= s.sweep.len() {
                s.sweep_pos = 0;
                sweep_done = Some(now.saturating_sub(s.sweep_start));
                s.sweep_start = now;
            }
            let c = s.sweep[s.sweep_pos];
            s.sweep_pos += 1;
            if seen.insert(c) {
                plan.push_back(c);
            }
        }
        s.plan = plan;
        s.cycle_start = now;
        self.status.hot = hot_n;
        if let Some(d) = sweep_done {
            self.status.sweep_ms = d.as_secs_f64() * 1000.0;
        }
    }

    fn finish_cycle(&mut self) {
        let now = self.clock.now();
        let Phase::Connected(s) = &mut self.phase else { return };
        let changes = std::mem::take(&mut s.cycle_changes);
        let cycle = now.saturating_sub(s.cycle_start);
        let op_ms = if s.ops > 0 { s.op_time.as_secs_f64() * 1000.0 / s.ops as f64 } else { 0.0 };
        self.status.cycle_ms = cycle.as_secs_f64() * 1000.0;
        self.status.op_ms = op_ms;
        if !changes.is_empty() {
            self.emit_group(ChangeSource::Camera, changes, None);
        }
    }

    fn poll_read(&mut self, code: u16) {
        let r = match self.op(ptp::OP_GET_PROP_VALUE, &[code as u32], None) {
            Ok(r) => r,
            Err(e) => return self.lost(e),
        };
        let now = self.clock.now();
        let wall = self.clock.wall_ms();
        let busy_after = self.cfg.busy_after;
        let Phase::Connected(s) = &mut self.phase else { return };
        match r.rc {
            ptp::RC_OK => {
                s.busy_run = 0;
                let Some(p) = s.props.get_mut(&code) else { return };
                let v = ptp::decode_value(p.datatype(), &r.data);
                p.error = None;
                if p.value.as_ref() == Some(&v) {
                    return;
                }
                let old = p.value.replace(v.clone());
                p.changed = Some(now);
                p.changed_wall = Some(wall);
                s.cycle_changes.push(Change { code, old, new: Some(v.clone()) });
                if code == STILL_MOVIE {
                    // The readable set differs between stills and movie.
                    s.redescribe = true;
                    s.plan.clear();
                    self.status.capture = capture_of(v);
                    self.finish_cycle();
                } else if code == SCREEN {
                    let screen = screen_of(&v);
                    self.status.screen = Some(screen);
                    if screen == Screen::Menu {
                        self.finish_cycle();
                        self.enter_pause(Pause::Menu, false);
                    }
                }
            }
            ptp::RC_FUJI_MENU => {
                self.finish_cycle();
                self.enter_pause(Pause::Menu, false);
            }
            ptp::RC_DEVICE_BUSY => {
                s.busy_run += 1;
                s.last_busy = Some(now);
                if s.busy_run >= busy_after {
                    self.log("camera busy (0x2019)");
                    self.enter_pause(Pause::Busy, false);
                }
            }
            rc => {
                if let Some(p) = s.props.get_mut(&code) {
                    p.error = Some(rc);
                }
            }
        }
    }
}

pub fn screen_of(v: &Value) -> Screen {
    match v {
        Value::Int(1) => Screen::Shooting,
        Value::Int(2) => Screen::Playback,
        Value::Int(3) => Screen::Menu,
        _ => Screen::Unknown,
    }
}

pub fn capture_of(v: Value) -> Option<Capture> {
    match v {
        Value::Int(2) => Some(Capture::Stills),
        Value::Int(1) => Some(Capture::Movie),
        _ => None,
    }
}

/// Starts the owner on its own thread.
pub fn spawn(connector: Box<dyn Connector>, clock: Arc<dyn Clock>, sink: Sink, cfg: Config) -> (Sender<Request>, std::thread::JoinHandle<()>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let owner = Owner::new(connector, clock, sink, rx, cfg);
    let h = std::thread::Builder::new().name("camera".into()).spawn(move || owner.run()).expect("camera thread starts");
    (tx, h)
}

/// Codes the change feed hides by default: they move by themselves.
pub const NOISY: &[u16] = &[0x5001, 0x5011, 0xD22F, 0xD36A, 0xD36B, 0xD212, 0xD242, 0xD171, 0xD170, 0xD347];
