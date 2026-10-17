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
    /// Every `Connector::open`: when, and whether it asked for a reset.
    pub opens: Vec<(Duration, bool)>,
    /// Every `Connector::present` call.
    pub presence_checks: Vec<Duration>,
    pub ops: Vec<OpRecord>,
    /// SetDevicePropValue answers 0x2019 (busy) this many times before it works, as the
    /// X-T5 does just after a half-press release.
    pub write_busy: u32,
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
    /// The session generation: a transport from an earlier claim is dead.
    generation: u64,
    seed: u64,
}

const D22E: u16 = 0xD22E;
const D307: u16 = 0xD307;
const D381: u16 = 0xD381;
const D208: u16 = 0xD208;

impl FakeState {
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
        let state = FakeState {
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
            opens: Vec::new(),
            presence_checks: Vec::new(),
            ops: Vec::new(),
            write_busy: 0,
            write_coerce: BTreeMap::new(),
            auto_sleep: false,
            idle_since: Duration::ZERO,
            gone_after_writes: None,
            main_menu: false,
            gestures: Vec::new(),
            generation: 0,
            seed: 0x9E37_79B9_7F4A_7C15,
        };
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
        })
    }

    pub fn value(&self, code: u16) -> Option<Value> {
        self.with(|s| s.props.get(&code).map(|p| p.value.clone()))
    }

    /// Auto power off: a second of 0x2019, then the camera leaves the bus.
    pub fn power_off(&self) {
        let now = self.clock.now();
        self.with(|s| s.off_at = Some(now + POWER_OFF_BUSY))
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
            if s.present {
                Ok(s.generation)
            } else {
                Err(TransportError::Gone)
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
                        ptp::OP_GET_PROP_VALUE => ok(ptp::encode_any(p.datatype, &p.value)),
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
                            if s.write_busy > 0 {
                                s.write_busy -= 1;
                                return rc(ptp::RC_DEVICE_BUSY);
                            }
                            let v = s.write_coerce.get(&prop).cloned().unwrap_or(v);
                            let slot = &mut s.props.get_mut(&prop).unwrap().value;
                            // Only a new D381 value restarts the timer; its own value does not.
                            let restart = prop == D381 && *slot != v;
                            *slot = v;
                            if restart {
                                s.idle_since = now;
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
                ptp::OP_INITIATE_CAPTURE if s.menu_open => rc(ptp::RC_FUJI_MENU),
                ptp::OP_INITIATE_CAPTURE => {
                    let g = match s.props.get(&D208).map(|p| &p.value) {
                        Some(Value::Int(v)) => *v,
                        _ => 0,
                    };
                    s.gestures.push(g);
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
