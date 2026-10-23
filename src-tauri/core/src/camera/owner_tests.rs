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
    rig_cfg(mode, Config::default(), setup)
}

fn rig_cfg(mode: FakeMode, cfg: Config, setup: impl FnOnce(&FakeHandle)) -> Rig {
    let clock = FakeClock::new();
    let arc: Arc<dyn Clock> = Arc::new(clock.clone());
    let cam = FakeCamera::x_t5(arc.clone(), mode);
    let handle = cam.handle();
    setup(&handle);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let (tx, rx) = channel();
    let owner = Owner::new(Box::new(cam), arc, Box::new(move |e| ev.lock().unwrap().push(e)), rx, cfg);
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

    /// SetDevicePropValue operations on D381 since `t`.
    fn apo_sets_since(&self, t: Duration) -> Vec<super::fake::OpRecord> {
        self.ops_since(t).into_iter().filter(|o| o.code == ptp::OP_SET_PROP_VALUE && o.param == Some(AUTO_POWER_OFF as u32)).collect()
    }

    /// Runs for `d` with someone using Ukiyo: an activity ping every 20 s. Returns whether
    /// the link stayed connected.
    fn run_in_use(&mut self, d: Duration) -> bool {
        let end = self.now() + d;
        while self.now() < end {
            self.tx.send(Request::Activity).unwrap();
            let step = (end - self.now()).min(Duration::from_secs(20));
            if self.run_until(step, |r| r.owner.status().link != Link::Connected) {
                return false;
            }
        }
        true
    }

    fn write(&mut self, code: u16, v: i64) -> WriteOutcome {
        let (wtx, wrx) = channel();
        self.tx.send(Request::Write { code, value: Value::Int(v), actor: Some("cli".into()), reply: Some(wtx) }).unwrap();
        self.owner.step();
        wrx.recv().unwrap()
    }
}

/// A fake X-T5 that sleeps after its AUTO POWER OFF time, set to `apo`.
fn sleepy(apo: i64) -> Rig {
    rig_with(FakeMode::Tether, move |h| {
        h.with(|s| s.auto_sleep = true);
        h.set(AUTO_POWER_OFF, Value::Int(apo));
    })
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
    q.push(Request::Write { code: 1, value: Value::Int(1), actor: None, reply: None });
    q.push(Request::KeepAwake(true));
    q.push(Request::Write { code: 2, value: Value::Int(2), actor: None, reply: None });
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
    r.tx.send(Request::Write { code: 0xD023, value: Value::Int(3), actor: None, reply: Some(wtx) }).unwrap();
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
    r.tx.send(Request::Write { code: 0xD023, value: Value::Int(4), actor: None, reply: Some(wtx) }).unwrap();
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
    r.tx.send(Request::Write { code: 0xD023, value: Value::Int(9), actor: None, reply: Some(wtx) }).unwrap();
    r.owner.step();
    let out = wrx.recv().unwrap();
    assert!(!out.ok);
    assert_eq!(out.readback, Some(Value::Int(4)));
    assert_eq!(r.ops_since(t)[1].code, ptp::OP_GET_PROP_VALUE);
}

/// The poll holds a cycle's changes until the cycle ends. A write's read-back of the same code
/// in the meantime must not be undone by them: in the order emitted, the last value is the
/// read-back's. (The Lab's mirror applies the groups in that order.)
#[test]
fn a_write_in_mid_cycle_is_not_overwritten_by_the_poll_changes_held_before_it() {
    let mut r = rig();
    r.connect();
    // Make the code hot, so the poll reads it every cycle.
    r.cam.set(0xD023, Value::Int(5));
    assert!(r.run_until(Duration::from_secs(5), |r| r.changed_codes().contains(&0xD023)));
    let groups_before = r.groups().len();
    // Someone turns it on the camera; step until the poll has read it, not yet reported it.
    r.cam.set(0xD023, Value::Int(2));
    let t = r.now();
    assert!(r.run_until(Duration::from_secs(1), |r| r.ops_since(t).iter().any(|o| o.code == ptp::OP_GET_PROP_VALUE && o.param == Some(0xD023))));
    assert_eq!(r.groups().len(), groups_before, "the poll's change is still held for the cycle's end");
    let out = r.write(0xD023, 3);
    assert_eq!(out.readback, Some(Value::Int(3)));
    r.run_for(Duration::from_millis(500));
    let last = r.groups().iter().flat_map(|g| g.changes.clone()).rfind(|c| c.code == 0xD023).unwrap();
    assert_eq!(last.new, Some(Value::Int(3)), "{:#?}", r.groups());
    assert_eq!(r.cam.value(0xD023), Some(Value::Int(3)));
}

#[test]
fn read_only_properties_are_never_written() {
    let mut r = rig();
    r.connect();
    let (wtx, wrx) = channel();
    let t = r.now();
    r.tx.send(Request::Write { code: SCREEN, value: Value::Int(2), actor: None, reply: Some(wtx) }).unwrap();
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
    r.tx.send(Request::Write { code: 0xD023, value: Value::Int(3), actor: None, reply: Some(wtx) }).unwrap();
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

/// Another process holds the interface: the claim fails before OpenSession is sent. That is
/// no reason to reset the camera's USB device; the owner waits and tries again.
#[test]
fn a_failed_claim_never_resets_usb() {
    let mut r = rig_with(FakeMode::Tether, |h| h.with(|s| s.claim_failures = 1));
    r.run_for(Duration::from_secs(1));
    assert_eq!(r.owner.status().link, Link::Waiting, "{:?}", r.owner.status());
    assert!(r.run_until(Duration::from_secs(60), |r| r.owner.status().link == Link::Connected));
    let opens = r.cam.with(|s| s.opens.clone());
    assert_eq!(opens.iter().map(|o| o.1).collect::<Vec<_>>(), [false, false], "{opens:?}");
    assert!(opens[1].0 - opens[0].0 >= Duration::from_secs(30), "{opens:?}");
}

/// OpenSession answered 0x2019: the camera is powering off. A USB reset then is what
/// soft-locked the X-T5 (docs/protocol.md): none is sent, and the owner waits.
#[test]
fn a_busy_open_session_never_resets_usb() {
    let mut r = rig_with(FakeMode::Tether, |h| h.power_off());
    r.run_for(Duration::from_millis(500));
    let opens = r.cam.with(|s| s.opens.clone());
    assert_eq!(opens.iter().map(|o| o.1).collect::<Vec<_>>(), [false], "{opens:?}");
    assert_eq!(r.owner.status().link, Link::Waiting, "{:?}", r.owner.status());
    // The camera leaves the bus; it comes back later and connects after the wait.
    assert!(r.run_until(Duration::from_secs(10), |r| r.owner.status().link == Link::Searching));
    r.cam.power_on();
    assert!(r.run_until(Duration::from_secs(60), |r| r.owner.status().link == Link::Connected));
    assert!(r.cam.with(|s| s.opens.iter().all(|o| !o.1)), "no reset at any point");
}

/// The camera answers 0x2019 to everything while it is described (busy with a card, or the
/// second before auto power off): the owner pauses as it does mid-poll, probes D22E, and
/// describes again once the camera answers. It never settles on an empty sweep.
#[test]
fn a_camera_busy_while_described_is_described_again_once_it_answers() {
    let mut r = rig_with(FakeMode::Tether, |h| h.with(|s| s.busy_until = Some(Duration::from_secs(3))));
    assert!(r.run_until(Duration::from_secs(2), |r| r.owner.status().link == Link::Busy), "{:?}", r.owner.status());
    assert_eq!(r.owner.status().readable, 0);
    let t = r.now();
    r.run_for(Duration::from_millis(900));
    let ops = r.ops_since(t);
    assert!(ops.iter().all(|o| o.param == Some(SCREEN as u32)), "only D22E probes while busy: {:?}", &ops[..ops.len().min(5)]);
    assert!(ops.len() <= 3, "{} probes in 0.9 s", ops.len());
    assert!(r.run_until(Duration::from_secs(5), |r| r.owner.status().link == Link::Connected && r.owner.status().readable == 181), "{:?}", r.owner.status());
}

/// Busy while described, then gone: that is auto power off, and the owner says so.
#[test]
fn a_camera_busy_while_described_that_then_leaves_is_asleep() {
    let mut r = rig_with(FakeMode::Tether, |h| h.with(|s| s.busy_until = Some(Duration::from_secs(60))));
    assert!(r.run_until(Duration::from_secs(2), |r| r.owner.status().link == Link::Busy));
    r.cam.unplug();
    assert!(r.run_until(Duration::from_secs(2), |r| r.owner.status().link == Link::Asleep), "{:?}", r.owner.status());
    assert_eq!(r.owner.status().reason.as_deref(), Some("auto power off"));
}

/// A session with nothing readable sends no property reads, so it checks the link itself:
/// the camera leaving is seen within a few seconds, not never.
#[test]
fn a_session_with_nothing_readable_still_sees_the_camera_leave() {
    let mut r = rig_with(FakeMode::Tether, |h| h.with(|s| s.props.clear()));
    assert!(r.run_until(Duration::from_secs(5), |r| r.owner.status().link == Link::Connected));
    assert_eq!(r.owner.status().readable, 0);
    r.run_for(Duration::from_secs(10));
    assert_eq!(r.owner.status().link, Link::Connected);
    r.cam.unplug();
    assert!(r.run_until(Duration::from_secs(10), |r| r.owner.status().link == Link::Searching), "{:?}", r.owner.status());
    assert_eq!(r.owner.status().reason.as_deref(), Some("unplugged"));
}

#[test]
fn without_keep_awake_the_fake_sleeps_at_its_timeout() {
    let mut r = sleepy(6);
    r.connect();
    assert!(r.run_until(Duration::from_secs(20), |r| r.owner.status().link == Link::Asleep));
    assert_eq!(r.owner.status().reason.as_deref(), Some("auto power off"));
}

#[test]
fn keep_awake_toggles_d381_and_keeps_the_camera_awake() {
    let mut r = sleepy(6); // 15 s
    r.tx.send(Request::KeepAwake(true)).unwrap();
    r.connect();
    let groups = r.groups().len();
    // The fake keeps its last 10 000 operations: count in 10 s pieces.
    let mut sets = vec![];
    for _ in 0..12 {
        let t = r.now();
        assert!(r.run_in_use(Duration::from_secs(10)), "slept while in use: {:?}", r.owner.status());
        sets.extend(r.apo_sets_since(t));
    }
    let ka = r.owner.status().keep_awake.clone();
    assert_eq!(ka, KeepAwake { enabled: true, active: true, period_s: Some(10) });
    // Two writes per toggle: one toggle at once, then every 10 s.
    assert!((24..=26).contains(&sets.len()), "{} D381 writes in 120 s", sets.len());
    assert!(sets.iter().all(|o| o.rc == Some(ptp::RC_OK)));
    assert_eq!(r.cam.value(AUTO_POWER_OFF), Some(Value::Int(6)), "back at the owner's value");
    assert!(r.groups()[groups..].iter().all(|g| g.changes.iter().all(|c| c.code != AUTO_POWER_OFF)), "a toggle reports no change");
}

#[test]
fn keep_awake_stops_when_ukiyo_is_idle_and_the_camera_sleeps() {
    let mut r = sleepy(5); // 30 s
    r.tx.send(Request::KeepAwake(true)).unwrap();
    r.connect();
    r.tx.send(Request::Activity).unwrap();
    // In use for 5 minutes after the one ping: awake, toggling every 20 s.
    assert!(!r.run_until(Duration::from_secs(290), |r| r.owner.status().link != Link::Connected));
    assert_eq!(r.owner.status().keep_awake.period_s, Some(20));
    // Then idle: no more toggles, and the camera sleeps on its own schedule.
    assert!(r.run_until(Duration::from_secs(60), |r| !r.owner.status().keep_awake.active));
    let t = r.now();
    assert!(r.run_until(Duration::from_secs(40), |r| r.owner.status().link == Link::Asleep));
    assert!(r.apo_sets_since(t).is_empty(), "no toggle while idle");
    assert_eq!(r.cam.value(AUTO_POWER_OFF), Some(Value::Int(5)));
}

#[test]
fn keep_awake_never_writes_when_auto_power_off_is_off() {
    let mut r = sleepy(AUTO_POWER_OFF_OFF);
    r.tx.send(Request::KeepAwake(true)).unwrap();
    r.connect();
    let t = r.now();
    assert!(r.run_in_use(Duration::from_secs(60)));
    assert!(r.apo_sets_since(t).is_empty());
    assert!(!r.owner.status().keep_awake.active);
}

#[test]
fn keep_awake_never_writes_in_movie_mode() {
    let mut r = rig_with(FakeMode::Tether, |h| {
        h.set(AUTO_POWER_OFF, Value::Int(6));
        h.set_movie(true);
    });
    r.tx.send(Request::KeepAwake(true)).unwrap();
    r.connect();
    assert_eq!(r.owner.status().capture, Some(Capture::Movie));
    let t = r.now();
    assert!(r.run_in_use(Duration::from_secs(60)));
    assert!(r.apo_sets_since(t).is_empty());
    assert!(!r.owner.status().keep_awake.active);
}

#[test]
fn keep_awake_writes_nothing_while_the_setting_is_off() {
    let mut r = sleepy(6);
    r.connect();
    let t = r.now();
    r.tx.send(Request::Activity).unwrap();
    r.run_for(Duration::from_secs(12));
    assert!(r.apo_sets_since(t).is_empty());
    assert!(!r.owner.status().keep_awake.active);
}

/// The neighbour of every timeout is another timeout, never OFF.
#[test]
fn the_toggle_value_is_never_off() {
    for own in [1, 2, 4, 5, 6] {
        let away = keep_awake_neighbour(own);
        assert_ne!(away, AUTO_POWER_OFF_OFF);
        assert_ne!(away, own);
        assert!(auto_power_off_secs(away).is_some());
        let secs = auto_power_off_secs(own).unwrap();
        assert!(keep_awake_period(secs) < Duration::from_secs(secs as u64));
    }
    let periods: Vec<u64> = [15, 30, 60, 120, 300].iter().map(|s| keep_awake_period(*s).as_secs()).collect();
    assert_eq!(periods, [10, 20, 45, 90, 240]);
}

/// The camera leaves the bus between the two writes: it holds the neighbouring timeout,
/// not OFF, and the owner puts its value back after the reconnect.
#[test]
fn a_loss_between_the_two_writes_leaves_a_timeout_and_is_put_back() {
    let mut r = sleepy(6);
    r.tx.send(Request::KeepAwake(true)).unwrap();
    r.connect();
    r.run_in_use(Duration::from_secs(1));
    r.cam.with(|s| s.gone_after_writes = Some(1));
    assert!(!r.run_in_use(Duration::from_secs(15)), "the camera left");
    assert_eq!(r.cam.value(AUTO_POWER_OFF), Some(Value::Int(1)), "5 min, not OFF");
    r.cam.power_on();
    // The put-back runs once the camera is back, in use or not.
    assert!(r.run_until(Duration::from_secs(60), |r| r.owner.status().link == Link::Connected));
    assert!(r.run_until(Duration::from_secs(2), |r| r.cam.value(AUTO_POWER_OFF) == Some(Value::Int(6))));
}

#[test]
fn a_busy_toggle_write_is_retried() {
    let mut r = sleepy(6);
    r.tx.send(Request::KeepAwake(true)).unwrap();
    r.connect();
    r.cam.with(|s| s.write_busy = 2);
    let t = r.now();
    assert!(r.run_in_use(Duration::from_secs(5)));
    let sets = r.apo_sets_since(t);
    assert_eq!(sets.iter().filter(|o| o.rc == Some(ptp::RC_DEVICE_BUSY)).count(), 2, "{sets:?}");
    assert_eq!(sets.iter().filter(|o| o.rc == Some(ptp::RC_OK)).count(), 2, "{sets:?}");
    assert_eq!(r.cam.value(AUTO_POWER_OFF), Some(Value::Int(6)));
    assert_eq!(r.owner.status().link, Link::Connected);
}

/// A menu over the shooting screen refuses the toggle: that tick is skipped, with no
/// half-press (the person in the menu keeps the camera awake).
#[test]
fn a_menu_skips_the_toggle_without_a_half_press() {
    let mut r = sleepy(6);
    r.tx.send(Request::KeepAwake(true)).unwrap();
    r.connect();
    r.cam.open_main_menu();
    let t = r.now();
    r.run_in_use(Duration::from_secs(1));
    let sets = r.apo_sets_since(t);
    assert_eq!(sets.len(), 1, "one refused write, no write back: {sets:?}");
    assert_eq!(sets[0].rc, Some(ptp::RC_FUJI_MENU));
    assert!(r.cam.with(|s| s.gestures.is_empty()), "no half-press");
    assert_eq!(r.cam.value(AUTO_POWER_OFF), Some(Value::Int(6)));
}

#[test]
fn shutdown_closes_the_session_and_leaves_auto_power_off_as_it_was() {
    let mut r = sleepy(2);
    r.tx.send(Request::KeepAwake(true)).unwrap();
    r.connect();
    r.run_in_use(Duration::from_secs(1));
    r.tx.send(Request::Shutdown).unwrap();
    r.owner.step();
    assert!(r.owner.is_done());
    assert_eq!(r.cam.value(AUTO_POWER_OFF), Some(Value::Int(2)));
    assert_eq!(r.cam.with(|s| s.ops.last().map(|o| o.code)), Some(ptp::OP_CLOSE_SESSION));
}

#[test]
fn disconnect_stays_stopped_until_connect() {
    let mut r = rig();
    r.connect();
    r.tx.send(Request::Disconnect).unwrap();
    r.owner.step();
    assert_eq!(r.owner.status().link, Link::Stopped);
    let n = r.cam.with(|s| s.opens.len());
    r.run_for(Duration::from_secs(60));
    assert_eq!(r.cam.with(|s| s.opens.len()), n);
    r.tx.send(Request::Connect).unwrap();
    r.connect();
}

/// A shooting-side menu leaves D22E at 1 and refuses writes: one half-press closes it, and
/// the write goes through on the retry.
#[test]
fn a_write_in_a_shooting_menu_closes_it_with_a_half_press() {
    let mut r = rig();
    r.connect();
    r.cam.open_main_menu();
    let out = r.write(0xD023, 3);
    assert!(out.ok && out.closed_menu, "{out:?}");
    assert_eq!((out.readback, out.attempts), (Some(Value::Int(3)), 2));
    assert_eq!(r.cam.with(|s| s.gestures.clone()), [RELEASE_HALF_PRESS, RELEASE_S1_OFF]);
    let g = r.groups().into_iter().find(|g| g.source == ChangeSource::Write).unwrap();
    assert!(g.closed_menu);
}

#[test]
fn a_write_in_a_shooting_menu_with_the_setting_off_fails_as_menu_open() {
    let mut r = rig_cfg(FakeMode::Tether, Config { close_menu_to_write: false, ..Config::default() }, |_| {});
    r.connect();
    r.cam.open_main_menu();
    let out = r.write(0xD023, 3);
    assert!(!out.ok && !out.closed_menu);
    assert_eq!(out.rc, Some(ptp::RC_FUJI_MENU));
    assert!(r.cam.with(|s| s.gestures.is_empty()));
}

/// Playback refuses everything, the half-press included: no half-press, no retry, and the
/// owner pauses as for the SET UP menu.
#[test]
fn a_write_in_playback_fails_fast() {
    let mut r = rig();
    r.connect();
    r.cam.open_playback();
    let t = r.now();
    let out = r.write(0xD023, 3);
    assert!(!out.ok && !out.closed_menu);
    assert_eq!((out.rc, out.result.as_str()), (Some(ptp::RC_FUJI_MENU), refusal::PLAYBACK));
    assert_eq!(r.ops_since(t).iter().filter(|o| o.code == ptp::OP_SET_PROP_VALUE).count(), 1, "one write, no retry");
    assert!(r.cam.with(|s| s.gestures.is_empty()));
    assert_eq!(r.owner.status().screen, Some(Screen::Playback));
    // Paused: the next write is refused before it reaches the camera.
    let out = r.write(0xD023, 3);
    assert_eq!((out.rc, out.result.as_str()), (None, refusal::PLAYBACK));
    r.cam.close_menu();
    assert!(r.run_until(Duration::from_secs(2), |r| r.owner.status().screen == Some(Screen::Shooting)));
}

/// The SET UP menu (D22E 3) refuses the half-press too: no half-press.
#[test]
fn a_write_in_the_set_up_menu_fails_fast() {
    let mut r = rig();
    r.connect();
    r.cam.open_menu();
    let out = r.write(0xD023, 3);
    assert!(!out.ok && !out.closed_menu);
    assert_eq!(out.rc, Some(ptp::RC_FUJI_MENU));
    assert!(r.cam.with(|s| s.gestures.is_empty()));
    assert_eq!(r.owner.status().screen, Some(Screen::Menu));
}

/// After a half-press release the X-T5 answers writes 0x2019 for about 0.2 s: the owner
/// waits and sends the same write again on the same session, and never reconnects.
#[test]
fn a_busy_write_is_retried_with_backoff() {
    let mut r = rig();
    r.connect();
    let opens = r.cam.with(|s| s.opens.len());
    r.cam.with(|s| s.write_busy = 2);
    let (wtx, wrx) = channel();
    let t = r.now();
    r.tx.send(Request::Write { code: 0xD023, value: Value::Int(3), actor: Some("cli".into()), reply: Some(wtx) }).unwrap();
    r.owner.step();
    let out = wrx.recv().unwrap();
    assert!(out.ok, "{out:?}");
    assert_eq!((out.attempts, out.rc), (3, Some(ptp::RC_OK)));
    assert_eq!(out.readback, Some(Value::Int(3)));
    let sets: Vec<_> = r.ops_since(t).into_iter().filter(|o| o.code == ptp::OP_SET_PROP_VALUE).collect();
    assert_eq!(sets.len(), 3);
    // 100 ms, then 200 ms between the tries.
    assert!(sets[1].at - sets[0].at >= Duration::from_millis(100));
    assert!(sets[2].at - sets[1].at >= Duration::from_millis(200));
    assert_eq!(r.cam.with(|s| s.opens.len()), opens, "no reconnect");
    let g = r.groups().into_iter().find(|g| g.source == ChangeSource::Write).unwrap();
    assert_eq!(g.actor.as_deref(), Some("cli"), "the read-back group names the actor");
}

#[test]
fn a_write_busy_for_too_long_fails_as_busy() {
    let mut r = rig();
    r.connect();
    let opens = r.cam.with(|s| s.opens.len());
    r.cam.with(|s| s.write_busy = 100);
    let (wtx, wrx) = channel();
    let t = r.now();
    r.tx.send(Request::Write { code: 0xD023, value: Value::Int(3), actor: None, reply: Some(wtx) }).unwrap();
    r.owner.step();
    let out = wrx.recv().unwrap();
    assert!(!out.ok);
    assert_eq!((out.rc, out.attempts, out.result.as_str()), (Some(ptp::RC_DEVICE_BUSY), 6, "DeviceBusy"));
    let spent = r.now() - t;
    assert!(spent >= Duration::from_millis(3100) && spent < Duration::from_secs(4), "{spent:?}");
    assert_eq!(r.cam.with(|s| s.opens.len()), opens, "no reconnect");
    assert_eq!(r.owner.status().link, Link::Connected);
    assert_eq!(out.readback, Some(Value::Int(1)), "read back anyway");
}

fn slot_op(r: &mut Rig, op: SlotOp) -> SlotOutcome {
    let (tx, rx) = channel();
    r.tx.send(Request::Slot { op, actor: Some("cli".into()), reply: tx }).unwrap();
    r.owner.step();
    rx.recv().unwrap()
}

#[test]
fn slot_reads_select_each_slot_and_restore_the_selector() {
    use crate::recipes::slot;
    let mut r = rig_with(FakeMode::RawConv, |c| c.with(|s| s.props.get_mut(&SLOT_SELECT).unwrap().value = Value::Int(4)));
    r.connect();
    let t0 = r.now();
    let out = slot_op(&mut r, SlotOp::Read { slots: vec![1, 2], fields: vec![(slot::NAME, slot::STRING), (slot::FILM_SIM, slot::UINT16), (slot::HIGHLIGHT, slot::INT16)] });
    assert_eq!((out.refused.as_deref(), out.selected, out.restored), (None, Some(4), true), "{out:?}");
    assert_eq!(out.reads[0].fields[0].1, Some(Value::Str("FAKE CHROME".into())));
    assert_eq!(out.reads[0].fields[2].1, Some(Value::Int(-10)), "int16 decodes signed");
    assert_eq!(out.reads[1].fields[1].1, Some(Value::Int(14)));
    let selects = r.ops_since(t0).iter().filter(|o| o.code == ptp::OP_SET_PROP_VALUE && o.param == Some(SLOT_SELECT as u32)).count();
    assert_eq!(selects, 3, "select C1, select C2, restore");
    assert_eq!(r.cam.value(SLOT_SELECT), Some(Value::Int(4)));
}

/// The restore of D18C is read back: an OK that leaves another slot selected is a failed
/// restore.
#[test]
fn a_slot_selector_restore_that_reads_back_wrong_is_not_restored() {
    use crate::recipes::slot;
    let mut r = rig_with(FakeMode::RawConv, |c| {
        c.with(|s| {
            s.props.get_mut(&SLOT_SELECT).unwrap().value = Value::Int(4);
            // Every D18C write lands on C2: the select works, the restore to C4 does not.
            s.write_coerce.insert(SLOT_SELECT, Value::Int(2));
        })
    });
    r.connect();
    let out = slot_op(&mut r, SlotOp::Read { slots: vec![2], fields: vec![(slot::FILM_SIM, slot::UINT16)] });
    assert_eq!(out.reads.len(), 1, "{out:?}");
    assert!(!out.restored, "{out:?}");
    assert!(out.refused.as_deref().is_some_and(|m| m.contains("reads back 2")), "{out:?}");
    assert_eq!(r.cam.value(SLOT_SELECT), Some(Value::Int(2)));
}

#[test]
fn slot_writes_read_back_and_stop_at_the_first_refusal() {
    use crate::recipes::slot;
    let mut r = rig_with(FakeMode::RawConv, |_| {});
    r.connect();
    // C3 holds PROVIA: Monochromatic Color is refused (0x201C) until a monochrome simulation.
    let out = slot_op(&mut r, SlotOp::Write { slot: 3, writes: vec![(slot::GRAIN, slot::UINT16, Value::Int(3)), (slot::MONO_WC, slot::INT16, Value::Int(30)), (slot::COLOR, slot::INT16, Value::Int(10))] });
    assert!(out.restored);
    assert_eq!(out.writes.len(), 2, "stops at the refused write: {out:?}");
    assert!(out.writes[0].ok && out.writes[0].readback == Some(Value::Int(3)));
    assert_eq!(out.writes[0].before, Some(Value::Int(6)));
    assert_eq!(out.writes[1].rc, Some(0x201C));
    assert_eq!(r.cam.slot_value(3, slot::GRAIN), Some(Value::Int(3)));
    assert_eq!(r.cam.slot_value(1, slot::GRAIN), Some(Value::Int(2)), "other slots untouched");
    let out = slot_op(&mut r, SlotOp::Write { slot: 3, writes: vec![(slot::FILM_SIM, slot::UINT16, Value::Int(12)), (slot::MONO_WC, slot::INT16, Value::Int(30))] });
    assert!(out.writes.iter().all(|w| w.ok), "{out:?}");
    assert_eq!(r.cam.slot_value(3, slot::MONO_WC), Some(Value::Int(30)));
}

#[test]
fn slot_requests_need_raw_conv_mode() {
    let mut r = rig();
    r.connect();
    let out = slot_op(&mut r, SlotOp::Read { slots: vec![1], fields: vec![] });
    assert_eq!(out.refused.as_deref(), Some(refusal::NOT_RAWCONV));
}

#[test]
fn the_fake_refuses_mono_color_under_a_colour_simulation_and_acros_resets_color() {
    let mut r = rig();
    r.connect();
    r.cam.set(0xD001, Value::Int(1));
    r.cam.set(0xD008, Value::Int(20));
    assert_eq!(r.write(0xD104, 0).rc, Some(0x201C));
    assert!(r.write(0xD001, 12).ok);
    assert_eq!(r.cam.value(0xD008), Some(Value::Int(0)), "ACROS sets Color to 0");
    assert!(r.write(0xD104, 30).ok, "a monochrome simulation takes Monochromatic Color");
    assert!(r.write(0xD1B2, 18).ok);
    assert_eq!(r.cam.value(0xD032), Some(Value::Int(0)), "HEIF sets Clarity to 0");
}

// ---- the tether buffer and capture ----

fn shots_rig(dir: &std::path::Path) -> Rig {
    let cfg = Config { shots_dir: Some(dir.to_path_buf()), ..Config::default() };
    rig_cfg(FakeMode::Tether, cfg, |c| c.with(|s| s.buffer_blocks_writes = true))
}

fn shots(r: &Rig) -> Vec<Shot> {
    r.events.lock().unwrap().iter().filter_map(|e| if let Event::Shot(s) = e { Some(s.clone()) } else { None }).collect()
}

/// A write answered 0xA002 imports every buffer object (JPEG first), deletes each only once
/// it is saved, and goes through on the one retry.
#[test]
fn a_write_blocked_by_the_tether_buffer_imports_the_shots() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = shots_rig(dir.path());
    r.connect();
    r.cam.shoot_on_body();
    let out = r.write(AUTO_POWER_OFF, 2);
    assert!(out.ok, "{out:?}");
    assert_eq!(out.attempts, 2, "the blocked write, then the retry");
    let s = shots(&r);
    assert_eq!(s.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["DSCF0001.JPG", "DSCF0001.RAF"]);
    assert!(s[0].jpeg && !s[1].jpeg);
    for shot in &s {
        assert_eq!(std::fs::metadata(&shot.path).unwrap().len(), u64::from(shot.bytes));
    }
    assert!(r.cam.with(|c| c.buffer.is_empty() && c.deleted.len() == 2));
}

/// Without a folder for shots the owner never deletes a buffer object: the write fails.
#[test]
fn no_shots_folder_leaves_the_buffer_alone() {
    let mut r = rig_with(FakeMode::Tether, |c| c.with(|s| s.buffer_blocks_writes = true));
    r.connect();
    r.cam.shoot_on_body();
    let out = r.write(AUTO_POWER_OFF, 2);
    assert_eq!((out.ok, out.rc), (false, Some(ptp::RC_FUJI_BUFFER)));
    assert!(r.cam.with(|c| c.buffer.len() == 2 && c.deleted.is_empty()));
}

fn capture(r: &mut Rig) -> CaptureOutcome {
    let (tx, rx) = channel();
    r.tx.send(Request::Capture { actor: Some("cli".into()), reply: tx }).unwrap();
    r.owner.step();
    rx.recv().unwrap()
}

/// A capture: priority to PC, half-press, release, the shot imported, priority back; the
/// shots already waiting come first.
#[test]
fn a_capture_takes_one_shot_and_puts_priority_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = shots_rig(dir.path());
    r.cam.set(PRIORITY, Value::Int(1));
    r.connect();
    r.cam.shoot_on_body();
    let g0 = r.cam.with(|s| s.gestures.len());
    let out = capture(&mut r);
    assert!(out.fired && out.restored && out.refused.is_none(), "{out:?}");
    assert_eq!(out.shots.len(), 4, "the waiting pair, then the new pair: {out:?}");
    assert_eq!(out.shots[2].name, "DSCF0002.JPG");
    assert_eq!(r.cam.with(|s| s.gestures[g0..].to_vec()), vec![RELEASE_HALF_PRESS, RELEASE_SHOOT]);
    assert_eq!(r.cam.value(PRIORITY), Some(Value::Int(1)), "camera priority again");
    let w: Vec<(u16, Option<Value>)> = out.writes.iter().map(|w| (w.code, w.readback.clone())).collect();
    assert_eq!(w, vec![(PRIORITY, Some(Value::Int(2))), (PRIORITY, Some(Value::Int(1)))], "each write read back");
    assert!(r.cam.with(|s| s.buffer.is_empty()));
}

/// A capture needs stills, and refuses in a menu without touching the camera.
#[test]
fn a_capture_needs_stills_and_no_menu() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = shots_rig(dir.path());
    r.connect();
    r.cam.set_movie(true);
    r.run_for(Duration::from_secs(2));
    let g0 = r.cam.with(|s| s.gestures.len());
    assert_eq!(capture(&mut r).refused.as_deref(), Some(refusal::NOT_STILLS));
    r.cam.set_movie(false);
    r.run_for(Duration::from_secs(2));
    r.cam.open_menu();
    r.run_for(Duration::from_secs(2));
    assert_eq!(capture(&mut r).refused.as_deref(), Some(refusal::PAUSED));
    assert_eq!(r.cam.with(|s| s.gestures.len()), g0, "no gesture sent");
}
