//! The connection owner against the fake camera, on a fake clock.

use super::clock::{Clock, FakeClock};
use super::fake::{FakeCamera, FakeHandle, FakeMode};
use super::owner::*;
use super::ptp::{self, Value};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Rig {
    owner: Owner,
    tx: Sender<Request>,
    cam: FakeHandle,
    clock: FakeClock,
    events: Arc<Mutex<Vec<Event>>>,
}

fn rig_with(mode: FakeMode, setup: impl FnOnce(&FakeHandle)) -> Rig {
    let clock = FakeClock::new();
    let arc: Arc<dyn Clock> = Arc::new(clock.clone());
    let cam = FakeCamera::x_t5(arc.clone(), mode);
    let handle = cam.handle();
    setup(&handle);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let (tx, rx) = channel();
    let owner = Owner::new(Box::new(cam), arc, Box::new(move |e| ev.lock().unwrap().push(e)), rx, Config::default());
    Rig { owner, tx, cam: handle, clock, events }
}

fn rig() -> Rig {
    rig_with(FakeMode::Tether, |_| {})
}

impl Rig {
    fn now(&self) -> Duration {
        self.clock.now()
    }

    /// Steps until `done` holds, or `limit` of fake time passes. Returns whether it held.
    fn run_until(&mut self, limit: Duration, mut done: impl FnMut(&Rig) -> bool) -> bool {
        let end = self.now() + limit;
        while self.now() < end {
            self.owner.step();
            if done(self) {
                return true;
            }
        }
        false
    }

    fn run_for(&mut self, d: Duration) {
        self.run_until(d, |_| false);
    }

    fn connect(&mut self) {
        assert!(self.run_until(Duration::from_secs(5), |r| r.owner.status().link == Link::Connected && r.owner.status().readable > 0));
    }

    fn groups(&self) -> Vec<ChangeGroup> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                Event::Changes(g) => Some(g.clone()),
                _ => None,
            })
            .collect()
    }

    fn changed_codes(&self) -> Vec<u16> {
        self.groups().iter().flat_map(|g| g.changes.iter().map(|c| c.code)).collect()
    }

    fn ops_since(&self, t: Duration) -> Vec<super::fake::OpRecord> {
        self.cam.with(|s| s.ops.iter().filter(|o| o.at >= t).cloned().collect())
    }
}

#[test]
fn connects_and_detects_tether_stills() {
    let mut r = rig();
    r.connect();
    let s = r.owner.status().clone();
    assert_eq!(s.model.as_deref(), Some("X-T5"));
    assert_eq!(s.mode, Some(UsbMode::Tether));
    assert_eq!(s.capture, Some(Capture::Stills));
    assert_eq!(s.screen, Some(Screen::Shooting));
    // The shape has 181 stills-readable properties.
    assert_eq!(s.readable, 181);
}

#[test]
fn detects_raw_conv_mode() {
    let mut r = rig_with(FakeMode::RawConv, |_| {});
    r.connect();
    assert_eq!(r.owner.status().mode, Some(UsbMode::RawConv));
}

#[test]
fn a_dial_change_shows_up_as_one_group_within_a_sweep() {
    let mut r = rig();
    r.connect();
    r.run_for(Duration::from_millis(200));
    r.cam.set(0xD023, Value::Int(5));
    assert!(r.run_until(Duration::from_secs(5), |r| r.changed_codes().contains(&0xD023)));
    let g = r.groups().into_iter().find(|g| g.changes.iter().any(|c| c.code == 0xD023)).unwrap();
    assert_eq!(g.source, ChangeSource::Camera);
    assert_eq!(g.changes.len(), 1);
    let s = r.owner.status();
    assert!(s.cycle_ms > 0.0 && s.op_ms > 0.0);
}

#[test]
fn recently_changed_codes_are_read_every_cycle() {
    let mut r = rig();
    r.connect();
    r.cam.set(0xD023, Value::Int(5));
    assert!(r.run_until(Duration::from_secs(5), |r| r.changed_codes().contains(&0xD023)));
    let t = r.now();
    r.run_for(Duration::from_secs(2));
    let reads = r.ops_since(t).iter().filter(|o| o.code == ptp::OP_GET_PROP_VALUE && o.param == Some(0xD023)).count();
    // A full sweep of 181 codes at 2 ms each takes ~0.4 s; hot codes are read far more often.
    assert!(reads > 50, "hot code read {reads} times in 2 s");
    // After the hot window, it falls back to the sweep rate.
    r.run_for(Duration::from_secs(11));
    let t = r.now();
    r.run_for(Duration::from_secs(2));
    let reads = r.ops_since(t).iter().filter(|o| o.code == ptp::OP_GET_PROP_VALUE && o.param == Some(0xD023)).count();
    assert!(reads < 10, "cold code read {reads} times in 2 s");
}

#[test]
fn queue_puts_writes_first() {
    let mut q = RequestQueue::default();
    let (tx, _rx) = channel();
    q.push(Request::SetPinned(vec![1]));
    q.push(Request::Export(tx));
    q.push(Request::Write { code: 1, value: Value::Int(1), reply: None });
    q.push(Request::KeepAwake(true));
    q.push(Request::Write { code: 2, value: Value::Int(2), reply: None });
    let order: Vec<String> = std::iter::from_fn(|| q.pop())
        .map(|r| match r {
            Request::Write { code, .. } => format!("w{code}"),
            Request::SetPinned(_) => "pin".into(),
            Request::Export(_) => "export".into(),
            Request::KeepAwake(_) => "awake".into(),
            _ => "other".into(),
        })
        .collect();
    assert_eq!(order, ["w1", "w2", "pin", "export", "awake"]);
}

#[test]
fn a_write_runs_before_an_earlier_export() {
    let mut r = rig();
    r.connect();
    let (etx, erx) = channel();
    let (wtx, wrx) = channel();
    r.tx.send(Request::Export(etx)).unwrap();
    r.tx.send(Request::Write { code: 0xD023, value: Value::Int(3), reply: Some(wtx) }).unwrap();
    let t = r.now();
    r.owner.step();
    assert!(wrx.try_recv().is_ok(), "write answered on the first step");
    assert!(erx.try_recv().is_err(), "export still waiting");
    // The first operation after the requests arrived was the write.
    assert_eq!(r.ops_since(t)[0].code, ptp::OP_SET_PROP_VALUE);
    r.owner.step();
    let dump = erx.try_recv().unwrap().unwrap();
    assert_eq!(dump.model, "X-T5");
    assert!(dump.properties.len() > 200);
}

#[test]
fn every_write_is_read_back() {
    let mut r = rig();
    r.connect();
    let (wtx, wrx) = channel();
    let t = r.now();
    r.tx.send(Request::Write { code: 0xD023, value: Value::Int(4), reply: Some(wtx) }).unwrap();
    r.owner.step();
    let out = wrx.recv().unwrap();
    assert!(out.ok);
    assert_eq!(out.readback, Some(Value::Int(4)));
    let ops = r.ops_since(t);
    assert_eq!((ops[0].code, ops[0].param), (ptp::OP_SET_PROP_VALUE, Some(0xD023)));
    assert_eq!((ops[1].code, ops[1].param), (ptp::OP_GET_PROP_VALUE, Some(0xD023)));
    // The read-back reports as a write group.
    assert!(r.groups().iter().any(|g| g.source == ChangeSource::Write && g.changes[0].code == 0xD023));

    // A refused value is still read back.
    let (wtx, wrx) = channel();
    let t = r.now();
    r.tx.send(Request::Write { code: 0xD023, value: Value::Int(9), reply: Some(wtx) }).unwrap();
    r.owner.step();
    let out = wrx.recv().unwrap();
    assert!(!out.ok);
    assert_eq!(out.readback, Some(Value::Int(4)));
    assert_eq!(r.ops_since(t)[1].code, ptp::OP_GET_PROP_VALUE);
}

#[test]
fn read_only_properties_are_never_written() {
    let mut r = rig();
    r.connect();
    let (wtx, wrx) = channel();
    let t = r.now();
    r.tx.send(Request::Write { code: SCREEN, value: Value::Int(2), reply: Some(wtx) }).unwrap();
    r.owner.step();
    let out = wrx.recv().unwrap();
    assert_eq!(out.result, "read-only");
    assert!(r.ops_since(t).iter().all(|o| o.code != ptp::OP_SET_PROP_VALUE));
}

#[test]
fn an_open_menu_pauses_the_sweep_and_probes_slowly() {
    let mut r = rig();
    r.connect();
    r.cam.open_menu();
    assert!(r.run_until(Duration::from_secs(2), |r| r.owner.status().screen == Some(Screen::Menu)));
    let groups = r.groups().len();
    let t = r.now();
    r.run_for(Duration::from_secs(10));
    let ops = r.ops_since(t);
    // Only D22E, about twice a second; no flood of 0xA001 reads.
    assert!(ops.iter().all(|o| o.param == Some(SCREEN as u32)), "{:?}", &ops[..ops.len().min(5)]);
    assert!(ops.len() <= 21, "{} probes in 10 s", ops.len());
    assert_eq!(r.groups().len(), groups, "no change events while paused");
    let (wtx, wrx) = channel();
    r.tx.send(Request::Write { code: 0xD023, value: Value::Int(3), reply: Some(wtx) }).unwrap();
    r.owner.step();
    assert!(!wrx.recv().unwrap().ok, "writes refused in a menu");
    r.cam.close_menu();
    assert!(r.run_until(Duration::from_secs(2), |r| r.owner.status().screen == Some(Screen::Shooting)));
    let t = r.now();
    r.run_for(Duration::from_secs(1));
    assert!(r.ops_since(t).len() > 100, "polling resumed at full speed");
}

#[test]
fn a_menu_that_answers_a001_everywhere_also_pauses() {
    let mut r = rig();
    r.connect();
    // Some firmware may answer 0xA001 for D22E too.
    r.cam.with(|s| {
        s.menu_open = true;
        s.menu_hides_screen = true;
    });
    assert!(r.run_until(Duration::from_secs(2), |r| r.owner.status().screen == Some(Screen::Menu)));
}

#[test]
fn switching_to_movie_redescribes_the_camera() {
    let mut r = rig();
    r.connect();
    r.cam.set_movie(true);
    assert!(r.run_until(Duration::from_secs(3), |r| r.owner.status().capture == Some(Capture::Movie) && r.owner.status().readable == 170));
    r.cam.set_movie(false);
    assert!(r.run_until(Duration::from_secs(3), |r| r.owner.status().readable == 181));
}

#[test]
fn auto_power_off_is_detected_and_not_chased() {
    let mut r = rig();
    r.connect();
    r.cam.power_off();
    assert!(r.run_until(Duration::from_secs(5), |r| r.owner.status().link == Link::Asleep));
    assert_eq!(r.owner.status().reason.as_deref(), Some("auto power off"));
    let gone_at = r.now();
    let opens_before = r.cam.with(|s| s.opens.len());
    // The person turns it on again a second later.
    r.run_for(Duration::from_secs(1));
    r.cam.power_on();
    assert!(r.run_until(Duration::from_secs(60), |r| r.owner.status().link == Link::Connected));
    let opens: Vec<_> = r.cam.with(|s| s.opens[opens_before..].to_vec());
    assert_eq!(opens.len(), 1, "one open after the wait: {opens:?}");
    assert!(opens[0].0 >= gone_at + Duration::from_secs(30), "reopened at {:?}, gone at {gone_at:?}", opens[0].0);
    assert!(!opens[0].1, "no USB reset");
}

#[test]
fn presence_is_checked_every_three_seconds_while_away() {
    let mut r = rig();
    r.connect();
    r.cam.unplug();
    assert!(r.run_until(Duration::from_secs(5), |r| r.owner.status().link == Link::Searching));
    assert_eq!(r.owner.status().reason.as_deref(), Some("unplugged"));
    let t = r.now();
    r.run_for(Duration::from_secs(60));
    let checks: Vec<Duration> = r.cam.with(|s| s.presence_checks.iter().copied().filter(|c| *c >= t).collect());
    assert!((19..=21).contains(&checks.len()), "{} checks in 60 s", checks.len());
    for w in checks.windows(2) {
        assert!(w[1] - w[0] >= Duration::from_secs(3));
    }
    assert!(r.cam.with(|s| s.opens.len()) == 1, "no open while absent");
}

#[test]
fn a_failed_open_session_resets_once_then_backs_off() {
    let mut r = rig_with(FakeMode::Tether, |h| h.with(|s| s.open_failures = 1));
    r.connect();
    let opens = r.cam.with(|s| s.opens.clone());
    assert_eq!(opens.iter().map(|o| o.1).collect::<Vec<_>>(), [false, true]);

    let mut r = rig_with(FakeMode::Tether, |h| h.with(|s| s.open_failures = 100));
    r.run_for(Duration::from_secs(100));
    let opens = r.cam.with(|s| s.opens.clone());
    // One try, one reset, then one try per 30 s wait, never another reset in a row.
    assert!(opens.len() <= 8, "{} opens in 100 s", opens.len());
    for pair in opens.windows(2) {
        if pair[0].1 {
            assert!(pair[1].0 - pair[0].0 >= Duration::from_secs(30), "{opens:?}");
        }
    }
    for w in opens.windows(2) {
        assert!(!(w[0].1 && w[1].1), "two resets in a row: {opens:?}");
    }
    assert_eq!(r.owner.status().link, Link::Waiting);
}

#[test]
fn keep_awake_sets_off_and_restores_on_disconnect() {
    let mut r = rig();
    r.cam.set(AUTO_POWER_OFF, Value::Int(1));
    r.tx.send(Request::KeepAwake(true)).unwrap();
    r.connect();
    r.run_for(Duration::from_millis(50));
    assert_eq!(r.cam.value(AUTO_POWER_OFF), Some(Value::Int(AUTO_POWER_OFF_OFF)));
    assert_eq!(r.owner.status().keep_awake.previous, Some(1));
    r.tx.send(Request::Disconnect).unwrap();
    r.owner.step();
    assert_eq!(r.cam.value(AUTO_POWER_OFF), Some(Value::Int(1)));
    assert_eq!(r.owner.status().link, Link::Stopped);
    // Stopped means stopped: no opens until Connect.
    let n = r.cam.with(|s| s.opens.len());
    r.run_for(Duration::from_secs(60));
    assert_eq!(r.cam.with(|s| s.opens.len()), n);
    r.tx.send(Request::Connect).unwrap();
    r.connect();
}

#[test]
fn shutdown_restores_auto_power_off() {
    let mut r = rig();
    r.cam.set(AUTO_POWER_OFF, Value::Int(2));
    r.connect();
    r.tx.send(Request::KeepAwake(true)).unwrap();
    r.owner.step();
    assert_eq!(r.cam.value(AUTO_POWER_OFF), Some(Value::Int(3)));
    r.tx.send(Request::Shutdown).unwrap();
    r.owner.step();
    assert!(r.owner.is_done());
    assert_eq!(r.cam.value(AUTO_POWER_OFF), Some(Value::Int(2)));
    assert_eq!(r.cam.with(|s| s.ops.last().map(|o| o.code)), Some(ptp::OP_CLOSE_SESSION));
}
