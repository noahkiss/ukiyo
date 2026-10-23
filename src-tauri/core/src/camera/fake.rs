//! A fake X-T5 behind the same `Connector`/`Transport` seam as the USB camera. Its
//! property set has the real camera's shape (codes, datatypes, forms, which mode reads
//! which code: `fake_x-t5.json`, made by `tools/catalog/seed_xt5.py`) with made-up values.
//! A `FakeHandle` plays the person at the camera: dials, menus, the STILL/MOVIE switch,
//! auto power off.
//!
//! With `auto_sleep`, the fake keeps the X-T5's auto power off timer: it powers off after the
//! D381 timeout unless D381 changed value, a session opened, or the person did something.

use super::clock::Clock;
use super::ptp::{self, Form, PropDesc, Value};
use super::transport::{Connector, Reply, Transport, TransportError};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SHAPE: &str = include_str!("fake_x-t5.json");
/// How long the real camera answers 0x2019 before it drops off the bus.
const POWER_OFF_BUSY: Duration = Duration::from_millis(1000);

#[derive(Deserialize)]
struct ShapeFile {
    model: String,
    manufacturer: String,
    firmware: String,
    properties: Vec<ShapeProp>,
}

#[derive(Deserialize)]
struct ShapeProp {
    code: String,
    stills: bool,
    movie: bool,
    rawconv: bool,
    #[serde(rename = "type")]
    datatype: String,
    rw: bool,
    value: serde_json::Value,
    #[serde(rename = "enum")]
    enum_: Option<Vec<serde_json::Value>>,
    range: Option<Vec<serde_json::Value>>,
}

fn hex16(s: &str) -> u16 {
    u16::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0)
}

fn json_value(t: u16, v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::String(s) => Value::Str(s.clone()),
        serde_json::Value::Number(n) => Value::Int(n.as_i64().unwrap_or(0)),
        _ if t == 0xFFFF => Value::Str(String::new()),
        _ if t == 0 => Value::Raw("00000000".into()),
        _ => Value::Int(0),
    }
}

#[derive(Debug, Clone)]
pub struct FakeProp {
    pub datatype: u16,
    pub writable: bool,
    pub form: Form,
    pub value: Value,
    pub stills: bool,
    pub movie: bool,
    pub rawconv: bool,
}

/// Which USB connection mode the fake camera is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeMode {
    Tether,
    RawConv,
}

/// One operation the fake saw, for tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpRecord {
    pub at: Duration,
    pub code: u16,
    pub param: Option<u32>,
    pub rc: Option<u16>,
}

pub struct FakeState {
    pub props: BTreeMap<u16, FakeProp>,
    pub mode: FakeMode,
    pub model: String,
    pub manufacturer: String,
    pub firmware: String,
    pub menu_open: bool,
    /// While a menu is open, D22E answers 0xA001 too, like every other property.
    pub menu_hides_screen: bool,
    pub present: bool,
    /// Set by `power_off`: answer 0x2019 until this time, then leave the bus.
    pub off_at: Option<Duration>,
    /// OpenSession answers this many times with an error before it works.
    pub open_failures: u32,
    /// `Connector::open` fails this many times before it works (another process holds the
    /// interface): the camera is present, but not claimed.
    pub claim_failures: u32,
    /// Every `Connector::open`: when, and whether it asked for a reset.
    pub opens: Vec<(Duration, bool)>,
    /// Every `Connector::present` call.
    pub presence_checks: Vec<Duration>,
    pub ops: Vec<OpRecord>,
    /// SetDevicePropValue answers 0x2019 (busy) this many times before it works, as the
    /// X-T5 does just after a half-press release.
    pub write_busy: u32,
    /// Every property operation answers 0x2019 until this time: the camera is busy with
    /// something of its own (writing a card, a half-press window) but stays on the bus.
    pub busy_until: Option<Duration>,
    /// After a write of one of these codes the camera holds this value instead, as a camera
    /// rule would (ISO 125 forces D-Range 100): the read-back differs from the write.
    pub write_coerce: BTreeMap<u16, Value>,
    /// Power off after the D381 timeout without a reset, as the camera does. Off by default.
    pub auto_sleep: bool,
    /// When the auto power off timer last restarted.
    pub idle_since: Duration,
    /// The camera leaves the bus after this many more accepted SetDevicePropValue writes:
    /// an unplug or a crash between two writes.
    pub gone_after_writes: Option<u32>,
    /// The main menu (MENU/OK) is open: D22E still reads 1 and reads answer, but writes
    /// answer 0xA001 until a half-press closes it, as on the X-T5 (docs/protocol.md).
    pub main_menu: bool,
    /// D208 at each InitiateCapture: 0x0200 half-press, 0x0004 release.
    pub gestures: Vec<i64>,
    /// The C1-C7 custom slots: D18D and D18E-D1A5 per slot. D18C selects which one those
    /// codes read and write.
    pub slots: Vec<BTreeMap<u16, Value>>,
    /// The tether buffer: shots waiting for the client, by handle.
    pub buffer: Vec<FakeObject>,
    /// While the buffer holds a shot, every write answers 0xA002, as the X-T5 does over
    /// wireless tether. Off by default (not seen on USB).
    pub buffer_blocks_writes: bool,
    /// Every DeleteObject handle.
    pub deleted: Vec<u32>,
    next_object: u32,
    /// The session generation: a transport from an earlier claim is dead.
    generation: u64,
    seed: u64,
}

const D22E: u16 = 0xD22E;
const D307: u16 = 0xD307;
const D381: u16 = 0xD381;
const D208: u16 = 0xD208;
const D18C: u16 = 0xD18C;
const LIVE_FILM_SIM: u16 = 0xD001;
const LIVE_COLOR: u16 = 0xD008;
const LIVE_CLARITY: u16 = 0xD032;
const FILE_TYPE: u16 = 0xD1B2;
const LIVE_MONO_WC: u16 = 0xD104;
const LIVE_MONO_MG: u16 = 0xD031;

/// D18D and D18E-D1A5: the codes that read and write the selected slot.
fn is_slot_code(code: u16) -> bool {
    code == 0xD18D || (0xD18E..=0xD1A5).contains(&code)
}

/// A slot as the fake ships it: made-up values in the slot encoding, the camera's defaults
/// elsewhere (grain reads 6 for OFF on a default slot, as ptp-recipes reports).
fn fake_slot(n: usize) -> BTreeMap<u16, Value> {
    use crate::recipes::slot::*;
    let mut m: BTreeMap<u16, Value> = [
        (NAME, Value::Str(String::new())),
        (0xD18E, Value::Int(7)),
        (0xD18F, Value::Int(4)),
        (DRANGE, Value::Int(100)),
        (DRP, Value::Int(0)),
        (FILM_SIM, Value::Int(1)),
        (MONO_WC, Value::Int(0)),
        (MONO_MG, Value::Int(0)),
        (GRAIN, Value::Int(6)),
        (COLOR_CHROME, Value::Int(1)),
        (FX_BLUE, Value::Int(1)),
        (SMOOTH_SKIN, Value::Int(1)),
        (WB, Value::Int(2)),
        (WB_SHIFT_R, Value::Int(0)),
        (WB_SHIFT_B, Value::Int(0)),
        (KELVIN, Value::Int(5600)),
        (HIGHLIGHT, Value::Int(0)),
        (SHADOW, Value::Int(0)),
        (COLOR, Value::Int(0)),
        (SHARPNESS, Value::Int(0)),
        (HIGH_ISO_NR, Value::Int(8192)),
        (CLARITY, Value::Int(0)),
        (0xD1A3, Value::Int(1)),
        (0xD1A4, Value::Int(1)),
        (0xD1A5, Value::Int(16)),
    ]
    .into_iter()
    .collect();
    // Two slots hold a recipe, so a read has something to show.
    if n == 0 {
        for (c, v) in [(FILM_SIM, 11), (GRAIN, 2), (COLOR_CHROME, 2), (HIGHLIGHT, -10), (SHADOW, 10), (COLOR, 20), (WB_SHIFT_R, 2), (WB_SHIFT_B, -4)] {
            m.insert(c, Value::Int(v));
        }
        m.insert(NAME, Value::Str("FAKE CHROME".into()));
    }
    if n == 1 {
        for (c, v) in [(FILM_SIM, 14), (GRAIN, 5), (MONO_WC, 20), (MONO_MG, -10), (DRP, 32768), (DRANGE, 65535)] {
            m.insert(c, Value::Int(v));
        }
        m.insert(NAME, Value::Str("FAKE MONO".into()));
    }
    m
}

/// Film simulations that take Monochromatic Color.
fn mono_sim(v: &Value) -> bool {
    matches!(v, Value::Int(n) if crate::recipes::slot::is_mono(*n))
}

impl FakeState {
    fn slot_index(&self) -> usize {
        match self.props.get(&D18C).map(|p| &p.value) {
            Some(Value::Int(n)) if (1..=7).contains(n) => (*n - 1) as usize,
            _ => 0,
        }
    }

    /// A code's value: the selected slot's for a slot code.
    fn value_of(&self, code: u16) -> Option<Value> {
        if is_slot_code(code) {
            return self.slots.get(self.slot_index()).and_then(|s| s.get(&code).cloned());
        }
        self.props.get(&code).map(|p| p.value.clone())
    }

    /// Why the camera refuses a write of `v` to `code` in this state (0x201C), as the X-T5
    /// does: Monochromatic Color under a colour film simulation, the slot's dynamic range and
    /// tones while its D-range priority is on, the slot grain value 6.
    fn gate(&self, code: u16, v: &Value) -> Option<u16> {
        use crate::recipes::slot as sl;
        const INVALID: u16 = 0x201C;
        let slot = |c: u16| self.slots.get(self.slot_index()).and_then(|s| s.get(&c).cloned());
        let refused = match code {
            LIVE_MONO_WC | LIVE_MONO_MG => !self.props.get(&LIVE_FILM_SIM).is_some_and(|p| mono_sim(&p.value)),
            sl::MONO_WC | sl::MONO_MG => !slot(sl::FILM_SIM).is_some_and(|f| mono_sim(&f)),
            sl::DRANGE | sl::HIGHLIGHT | sl::SHADOW => slot(sl::DRP).is_some_and(|d| d != Value::Int(0)),
            sl::GRAIN => *v == Value::Int(6),
            _ => false,
        };
        refused.then_some(INVALID)
    }

    /// The camera's own rules after a write: choosing a monochrome film simulation sets Color
    /// to 0 (seen for ACROS) and opens the Monochromatic Color range; HEIF sets Clarity to 0.
    fn after_write(&mut self, code: u16, v: &Value) {
        match code {
            LIVE_FILM_SIM => {
                let mono = mono_sim(v) || *v == Value::Int(10);
                if mono {
                    if let Some(p) = self.props.get_mut(&LIVE_COLOR) {
                        p.value = Value::Int(0);
                    }
                }
                self.mono_ranges();
            }
            FILE_TYPE if *v == Value::Int(18) => {
                if let Some(p) = self.props.get_mut(&LIVE_CLARITY) {
                    p.value = Value::Int(0);
                }
            }
            _ => {}
        }
    }

    /// Monochromatic Color's range: -180 to 180 under a monochrome film simulation, 0 under
    /// a colour one, as the descriptor reports.
    fn mono_ranges(&mut self) {
        let mono = self.props.get(&LIVE_FILM_SIM).is_some_and(|p| mono_sim(&p.value));
        let r = if mono { 180 } else { 0 };
        for c in [LIVE_MONO_WC, LIVE_MONO_MG] {
            if let Some(p) = self.props.get_mut(&c) {
                p.form = Form::Range { min: Value::Int(-r), max: Value::Int(r), step: Value::Int(10) };
            }
        }
    }

    fn movie(&self) -> bool {
        matches!(self.props.get(&D307).map(|p| &p.value), Some(Value::Int(1)))
    }

    fn readable(&self, p: &FakeProp) -> bool {
        match self.mode {
            FakeMode::RawConv => p.rawconv,
            FakeMode::Tether if self.movie() => p.movie,
            FakeMode::Tether => p.stills,
        }
    }

    fn advertised(&self) -> Vec<u16> {
        self.props
            .iter()
            .filter(|(_, p)| match self.mode {
                FakeMode::RawConv => p.rawconv,
                FakeMode::Tether => p.stills || p.movie,
            })
            .map(|(c, _)| *c)
            .collect()
    }
}

/// One object in the fake tether buffer.
#[derive(Debug, Clone)]
pub struct FakeObject {
    pub handle: u32,
    pub filename: String,
    pub format: u16,
    pub data: Vec<u8>,
}

impl FakeState {
    fn add_shot(&mut self) {
        let n = self.next_object;
        self.next_object += 2;
        let stem = format!("DSCF{:04}", n.div_ceil(2));
        // 0x3000: an undefined format, as a RAF reports.
        self.buffer.push(FakeObject { handle: 0x0100_0000 + n, filename: format!("{stem}.RAF"), format: 0x3000, data: vec![0x46; 4096] });
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpeg.resize(2048, 0);
        jpeg.extend([0xFF, 0xD9]);
        self.buffer.push(FakeObject { handle: 0x0100_0001 + n, filename: format!("{stem}.JPG"), format: ptp::FORMAT_JPEG, data: jpeg });
    }
}

#[derive(Clone)]
pub struct FakeHandle {
    state: Arc<Mutex<FakeState>>,
    clock: Arc<dyn Clock>,
}

pub struct FakeCamera {
    handle: FakeHandle,
    op_cost: Duration,
}

impl FakeCamera {
    /// A fake X-T5 in USB TETHER SHOOTING FIXED mode, stills, shooting screen, on.
    pub fn x_t5(clock: Arc<dyn Clock>, mode: FakeMode) -> Self {
        let shape: ShapeFile = serde_json::from_str(SHAPE).expect("fake_x-t5.json parses");
        let mut props = BTreeMap::new();
        for p in shape.properties {
            let t = hex16(&p.datatype);
            let vals = |a: &Vec<serde_json::Value>| a.iter().map(|v| json_value(t, v)).collect::<Vec<_>>();
            let form = match (&p.enum_, &p.range) {
                (Some(e), _) => Form::Enum { values: vals(e) },
                (_, Some(r)) if r.len() == 3 => {
                    let r = vals(r);
                    Form::Range { min: r[0].clone(), max: r[1].clone(), step: r[2].clone() }
                }
                _ => Form::None,
            };
            props.insert(
                hex16(&p.code),
                FakeProp {
                    datatype: t,
                    writable: p.rw,
                    form,
                    value: json_value(t, &p.value),
                    stills: p.stills,
                    movie: p.movie,
                    rawconv: p.rawconv,
                },
            );
        }
        // Start in stills on the shooting screen, whatever the shape's defaults say.
        if let Some(p) = props.get_mut(&D307) {
            p.value = Value::Int(2);
        }
        if let Some(p) = props.get_mut(&D22E) {
            p.value = Value::Int(1);
        }
        // The slot fields have no descriptor in RAW CONV. mode (the shape says datatype 0);
        // the fake keeps them in their slot encoding, writable.
        for (code, p) in props.iter_mut().filter(|(c, _)| is_slot_code(**c) && **c != 0xD18D) {
            if let Some(t) = crate::recipes::slot::datatype(*code) {
                p.datatype = t;
                p.writable = true;
            }
        }
        let mut state = FakeState {
            props,
            mode,
            model: shape.model,
            manufacturer: shape.manufacturer,
            firmware: shape.firmware,
            menu_open: false,
            menu_hides_screen: false,
            present: true,
            off_at: None,
            open_failures: 0,
            claim_failures: 0,
            opens: Vec::new(),
            presence_checks: Vec::new(),
            ops: Vec::new(),
            write_busy: 0,
            busy_until: None,
            write_coerce: BTreeMap::new(),
            auto_sleep: false,
            idle_since: Duration::ZERO,
            gone_after_writes: None,
            main_menu: false,
            gestures: Vec::new(),
            slots: (0..7).map(fake_slot).collect(),
            generation: 0,
            buffer: Vec::new(),
            buffer_blocks_writes: false,
            deleted: Vec::new(),
            next_object: 1,
            seed: 0x9E37_79B9_7F4A_7C15,
        };
        state.mono_ranges();
        Self {
            handle: FakeHandle { state: Arc::new(Mutex::new(state)), clock },
            op_cost: Duration::from_millis(2),
        }
    }

    /// The time each operation takes (the clock sleeps for it). Default 2 ms.
    pub fn with_op_cost(mut self, d: Duration) -> Self {
        self.op_cost = d;
        self
    }

    pub fn handle(&self) -> FakeHandle {
        self.handle.clone()
    }

    /// Another connector onto the same fake camera: its state, its clock.
    pub fn from_handle(handle: FakeHandle, op_cost: Duration) -> Self {
        Self { handle, op_cost }
    }
}

impl FakeHandle {
    pub fn with<R>(&self, f: impl FnOnce(&mut FakeState) -> R) -> R {
        f(&mut self.state.lock().unwrap())
    }

    /// The person touched the camera: the auto power off timer restarts.
    fn touched(&self, s: &mut FakeState) {
        s.idle_since = self.clock.now();
    }

    /// The person opens the SET UP menu (D22E 3): everything but D22E answers 0xA001.
    pub fn open_menu(&self) {
        self.with(|s| {
            self.touched(s);
            s.menu_open = true;
            if let Some(p) = s.props.get_mut(&D22E) {
                p.value = Value::Int(3);
            }
        })
    }

    /// The main menu (MENU/OK): D22E stays 1, writes answer 0xA001 until a half-press.
    pub fn open_main_menu(&self) {
        self.with(|s| {
            self.touched(s);
            s.main_menu = true;
        })
    }

    /// Playback: D22E reads 2 and everything else, the half-press included, answers 0xA001.
    pub fn open_playback(&self) {
        self.with(|s| {
            self.touched(s);
            s.menu_open = true;
            if let Some(p) = s.props.get_mut(&D22E) {
                p.value = Value::Int(2);
            }
        })
    }

    /// Closes any menu or playback.
    pub fn close_menu(&self) {
        self.with(|s| {
            self.touched(s);
            s.main_menu = false;
            s.menu_open = false;
            if let Some(p) = s.props.get_mut(&D22E) {
                p.value = Value::Int(1);
            }
        })
    }

    /// The STILL/MOVIE switch.
    pub fn set_movie(&self, movie: bool) {
        self.set(D307, Value::Int(if movie { 1 } else { 2 }));
    }

    /// The person changes a setting on the camera.
    pub fn set(&self, code: u16, v: Value) {
        self.with(|s| {
            self.touched(s);
            if let Some(p) = s.props.get_mut(&code) {
                p.value = v;
            }
            if code == LIVE_FILM_SIM {
                s.mono_ranges();
            }
        })
    }

    /// A code's value now; a slot code's in the selected slot.
    pub fn value(&self, code: u16) -> Option<Value> {
        self.with(|s| s.value_of(code))
    }

    /// One field of slot `n` (1-7), whichever slot is selected.
    pub fn slot_value(&self, n: usize, code: u16) -> Option<Value> {
        self.with(|s| s.slots.get(n - 1).and_then(|m| m.get(&code).cloned()))
    }

    /// Auto power off: a second of 0x2019, then the camera leaves the bus.
    pub fn power_off(&self) {
        let now = self.clock.now();
        self.with(|s| s.off_at = Some(now + POWER_OFF_BUSY))
    }

    /// A shot: RAW and JPEG land in the tether buffer, as when the person presses the
    /// shutter on the body.
    pub fn shoot_on_body(&self) {
        self.with(|s| s.add_shot())
    }

    /// The person turns the camera on again.
    pub fn power_on(&self) {
        self.with(|s| {
            self.touched(s);
            s.off_at = None;
            s.present = true;
            s.menu_open = false;
        })
    }

    /// Unplugged: gone at once, with no busy burst.
    pub fn unplug(&self) {
        self.with(|s| {
            s.present = false;
            s.generation += 1;
        })
    }

    /// Changes one writable enum setting to another of its values, as a person might.
    pub fn nudge(&self) -> Option<u16> {
        self.with(|s| {
            self.touched(s);
            let movie = s.movie();
            let candidates: Vec<u16> = s
                .props
                .iter()
                .filter(|(c, p)| {
                    **c != D307
                        && p.writable
                        && (if movie { p.movie } else { p.stills })
                        && matches!(&p.form, Form::Enum { values } if values.len() > 1)
                })
                .map(|(c, _)| *c)
                .collect();
            if candidates.is_empty() {
                return None;
            }
            s.seed ^= s.seed << 13;
            s.seed ^= s.seed >> 7;
            s.seed ^= s.seed << 17;
            let code = candidates[(s.seed % candidates.len() as u64) as usize];
            let p = s.props.get_mut(&code)?;
            if let Form::Enum { values } = &p.form {
                let i = values.iter().position(|v| *v == p.value).map(|i| (i + 1) % values.len()).unwrap_or(0);
                p.value = values[i].clone();
            }
            Some(code)
        })
    }

    /// Moves the clock-like and battery-like properties, as the camera does by itself.
    pub fn tick_noise(&self, wall_ms: i64) {
        self.with(|s| {
            if let Some(p) = s.props.get_mut(&0x5011) {
                let secs = (wall_ms / 1000) % 86_400;
                p.value = Value::Str(format!("20261017T{:02}{:02}{:02}", secs / 3600, secs / 60 % 60, secs % 60));
            }
            if let Some(p) = s.props.get_mut(&0xD171) {
                if let Value::Int(n) = p.value {
                    p.value = Value::Int((n + 7) % 500);
                }
            }
        })
    }

    /// Advances the power-off sequence: the auto power off timer, then after the busy second,
    /// the camera is gone.
    fn settle(&self, s: &mut FakeState) {
        let now = self.clock.now();
        if s.auto_sleep && s.present && s.off_at.is_none() {
            let timeout = match s.props.get(&D381).map(|p| &p.value) {
                Some(Value::Int(v)) => super::owner::auto_power_off_secs(*v),
                _ => None,
            };
            if timeout.is_some_and(|t| now.saturating_sub(s.idle_since) >= Duration::from_secs(t as u64)) {
                s.off_at = Some(now + POWER_OFF_BUSY);
            }
        }
        if let Some(t) = s.off_at {
            if self.clock.now() >= t && s.present {
                s.present = false;
                s.generation += 1;
            }
        }
    }
}

impl Connector for FakeCamera {
    fn present(&mut self) -> bool {
        let now = self.handle.clock.now();
        let h = self.handle.clone();
        self.handle.with(|s| {
            h.settle(s);
            if s.presence_checks.len() >= 20_000 {
                s.presence_checks.drain(..10_000);
            }
            s.presence_checks.push(now);
            s.present
        })
    }

    fn open(&mut self, reset: bool) -> Result<Box<dyn Transport>, TransportError> {
        let now = self.handle.clock.now();
        let h = self.handle.clone();
        let generation = self.handle.with(|s| {
            h.settle(s);
            s.opens.push((now, reset));
            if !s.present {
                Err(TransportError::Gone)
            } else if s.claim_failures > 0 {
                s.claim_failures -= 1;
                Err(TransportError::Io("interface busy".into()))
            } else {
                Ok(s.generation)
            }
        })?;
        Ok(Box::new(FakeTransport { handle: h, generation, session: false, op_cost: self.op_cost }))
    }
}

pub struct FakeTransport {
    handle: FakeHandle,
    generation: u64,
    session: bool,
    op_cost: Duration,
}

fn ok(data: Vec<u8>) -> Result<Reply, TransportError> {
    Ok(Reply { rc: ptp::RC_OK, data })
}

fn rc(rc: u16) -> Result<Reply, TransportError> {
    Ok(Reply { rc, data: Vec::new() })
}

impl Transport for FakeTransport {
    fn op(&mut self, code: u16, params: &[u32], data: Option<&[u8]>) -> Result<Reply, TransportError> {
        let now = self.handle.clock.now();
        self.handle.clock.sleep(self.op_cost);
        let h = self.handle.clone();
        let generation = self.generation;
        let session = &mut self.session;
        let result = self.handle.with(|s| {
            h.settle(s);
            if !s.present || s.generation != generation {
                return Err(TransportError::Gone);
            }
            if s.off_at.is_some() {
                return rc(ptp::RC_DEVICE_BUSY);
            }
            let param = params.first().copied();
            let prop = param.map(|p| p as u16);
            match code {
                ptp::OP_OPEN_SESSION => {
                    if s.open_failures > 0 {
                        s.open_failures -= 1;
                        return Err(TransportError::Io("stalled endpoint".into()));
                    }
                    if *session {
                        return rc(ptp::RC_SESSION_ALREADY_OPEN);
                    }
                    *session = true;
                    s.idle_since = now; // OpenSession restarts the timer (docs/protocol.md).
                    ok(Vec::new())
                }
                _ if !*session => rc(0x2003), // SessionNotOpen
                ptp::OP_CLOSE_SESSION => {
                    *session = false;
                    ok(Vec::new())
                }
                ptp::OP_GET_DEVICE_INFO => ok(ptp::encode_device_info(&ptp::DeviceInfo {
                    vendor_ext: "fujifilm.co.jp: 1.0;".into(),
                    manufacturer: s.manufacturer.clone(),
                    model: s.model.clone(),
                    firmware: s.firmware.clone(),
                    operations: vec![0x1001, 0x1002, 0x1003, 0x100E, 0x1014, 0x1015, 0x1016],
                    properties: s.advertised(),
                })),
                ptp::OP_GET_PROP_DESC | ptp::OP_GET_PROP_VALUE | ptp::OP_SET_PROP_VALUE => {
                    let Some(prop) = prop else { return rc(0x201D) };
                    if s.busy_until.is_some_and(|t| now < t) {
                        return rc(ptp::RC_DEVICE_BUSY);
                    }
                    // Over the shooting screen, the shutter gesture is accepted with a menu open.
                    let gesture = prop == D208 && code == ptp::OP_SET_PROP_VALUE;
                    if s.menu_open && !(prop == D22E && code == ptp::OP_GET_PROP_VALUE && !s.menu_hides_screen) {
                        return rc(ptp::RC_FUJI_MENU);
                    }
                    if s.main_menu && code == ptp::OP_SET_PROP_VALUE && !gesture {
                        return rc(ptp::RC_FUJI_MENU);
                    }
                    let Some(p) = s.props.get(&prop).cloned() else {
                        return rc(ptp::RC_PROP_NOT_SUPPORTED);
                    };
                    if code == ptp::OP_GET_PROP_DESC && s.mode == FakeMode::RawConv {
                        return rc(ptp::RC_GENERAL_ERROR);
                    }
                    if !s.readable(&p) {
                        return rc(ptp::RC_GENERAL_ERROR);
                    }
                    match code {
                        ptp::OP_GET_PROP_DESC => ok(ptp::encode_prop_desc(&PropDesc {
                            code: prop,
                            datatype: p.datatype,
                            writable: p.writable,
                            default: p.value.clone(),
                            current: p.value.clone(),
                            form: p.form.clone(),
                        })),
                        ptp::OP_GET_PROP_VALUE => ok(ptp::encode_any(p.datatype, &s.value_of(prop).unwrap_or(p.value))),
                        _ => {
                            if !p.writable {
                                return rc(ptp::RC_ACCESS_DENIED);
                            }
                            let v = ptp::decode_value(p.datatype, data.unwrap_or_default());
                            let allowed = match &p.form {
                                Form::Enum { values } => values.contains(&v),
                                Form::Range { min: Value::Int(lo), max: Value::Int(hi), .. } => {
                                    matches!(v, Value::Int(n) if n >= *lo && n <= *hi)
                                }
                                _ => true,
                            };
                            if !allowed {
                                return rc(0x201D); // InvalidDevicePropValue
                            }
                            if s.buffer_blocks_writes && !s.buffer.is_empty() {
                                return rc(ptp::RC_FUJI_BUFFER);
                            }
                            if s.write_busy > 0 {
                                s.write_busy -= 1;
                                return rc(ptp::RC_DEVICE_BUSY);
                            }
                            if let Some(refused) = s.gate(prop, &v) {
                                return rc(refused);
                            }
                            let v = s.write_coerce.get(&prop).cloned().unwrap_or(v);
                            if is_slot_code(prop) {
                                let i = s.slot_index();
                                s.slots[i].insert(prop, v);
                            } else {
                                let slot = &mut s.props.get_mut(&prop).unwrap().value;
                                // Only a new D381 value restarts the timer; its own value does not.
                                let restart = prop == D381 && *slot != v;
                                *slot = v.clone();
                                if restart {
                                    s.idle_since = now;
                                }
                                s.after_write(prop, &v);
                            }
                            if let Some(n) = s.gone_after_writes.as_mut() {
                                if *n <= 1 {
                                    s.gone_after_writes = None;
                                    s.present = false;
                                    s.generation += 1;
                                } else {
                                    *n -= 1;
                                }
                            }
                            ok(Vec::new())
                        }
                    }
                }
                ptp::OP_GET_OBJECT_HANDLES => {
                    let mut b = (s.buffer.len() as u32).to_le_bytes().to_vec();
                    for o in &s.buffer {
                        b.extend(o.handle.to_le_bytes());
                    }
                    ok(b)
                }
                ptp::OP_GET_OBJECT_INFO | ptp::OP_GET_OBJECT | ptp::OP_DELETE_OBJECT => {
                    let h = param.unwrap_or(0);
                    let Some(i) = s.buffer.iter().position(|o| o.handle == h) else { return rc(ptp::RC_INVALID_OBJECT_HANDLE) };
                    match code {
                        ptp::OP_GET_OBJECT_INFO => {
                            let o = &s.buffer[i];
                            ok(ptp::encode_object_info(&ptp::ObjectInfo { storage: 0x1000_0001, format: o.format, size: o.data.len() as u32, filename: o.filename.clone() }))
                        }
                        ptp::OP_GET_OBJECT => ok(s.buffer[i].data.clone()),
                        _ => {
                            s.buffer.remove(i);
                            s.deleted.push(h);
                            ok(Vec::new())
                        }
                    }
                }
                ptp::OP_INITIATE_CAPTURE if s.menu_open => rc(ptp::RC_FUJI_MENU),
                ptp::OP_INITIATE_CAPTURE => {
                    let g = match s.props.get(&D208).map(|p| &p.value) {
                        Some(Value::Int(v)) => *v,
                        _ => 0,
                    };
                    s.gestures.push(g);
                    if s.buffer_blocks_writes && !s.buffer.is_empty() {
                        return rc(ptp::RC_FUJI_BUFFER);
                    }
                    if g == 0x0304 {
                        s.add_shot();
                    }
                    if g == 0x0200 {
                        // A half-press closes any menu and restarts the auto power off timer.
                        s.main_menu = false;
                        s.menu_open = false;
                        if let Some(p) = s.props.get_mut(&D22E) {
                            p.value = Value::Int(1);
                        }
                        s.idle_since = now;
                    }
                    ok(Vec::new())
                }
                _ => rc(ptp::RC_OPERATION_NOT_SUPPORTED),
            }
        });
        let rec_rc = result.as_ref().ok().map(|r| r.rc);
        self.handle.with(|s| {
            // Bounded, for a fake that runs for hours behind the UI.
            if s.ops.len() >= 20_000 {
                s.ops.drain(..10_000);
            }
            s.ops.push(OpRecord { at: now, code, param: params.first().copied(), rc: rec_rc })
        });
        result
    }
}
