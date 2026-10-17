//! Plans on the whole Lab and the fake camera, in real time (the busy retry waits for real).

use super::*;
use crate::camera::fake::FakeHandle;
use crate::lab::{FakeAction, Lab};
use crate::serve::OwnerLock;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

const APO: u16 = 0xD381; // AutoPowerOff, confirmed: 1 5 MIN, 2 2 MIN, 3 OFF ...
const GRAIN: u16 = 0xD023; // GrainEffect, made likely in the test copy: 1 OFF, 2 WEAK SMALL ...

struct Rig {
    lab: Arc<Lab>,
    cam: FakeHandle,
    data: tempfile::TempDir,
    _cats: tempfile::TempDir,
}

fn rig() -> Rig {
    std::env::set_var("UKIYO_FAKE_CAMERA", "1");
    let data = tempfile::tempdir().unwrap();
    let cats = tempfile::tempdir().unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/x-t5.json");
    // GrainEffect stands for an unconfirmed property, whatever the mapping has reached.
    let mut cat: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(repo).unwrap()).unwrap();
    cat["properties"]["0xd023"]["confidence"] = "likely".into();
    std::fs::write(cats.path().join("x-t5.json"), serde_json::to_string_pretty(&cat).unwrap()).unwrap();
    let lock = OwnerLock::try_acquire(&data.path().join("owner.lock")).unwrap();
    let lab = Lab::start(lock, data.path().into(), cats.path().into(), |_| {});
    // No random dial turns from the fake person: they would make plans stale.
    lab.fake(FakeAction::AutoOff).unwrap();
    let cam = lab.fake_handle().unwrap();
    let r = Rig { lab, cam, data, _cats: cats };
    r.wait("connected with a catalog", |s| s.status.link == Link::Connected && !s.rows.is_empty() && s.catalog.is_some());
    r
}

impl Rig {
    fn wait(&self, what: &str, f: impl Fn(&Snapshot) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f(&self.lab.snapshot()) {
            assert!(Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn value(&self, code: u16) -> Option<Value> {
        self.lab.snapshot().rows.iter().find(|r| r.code == code).and_then(|r| r.value.clone())
    }

    /// The person at the camera sets a value; waits until the Lab has seen it.
    fn set_on_camera(&self, code: u16, v: Value) {
        self.cam.set(code, v.clone());
        self.wait("the Lab sees the change", |s| s.rows.iter().any(|r| r.code == code && r.value.as_ref() == Some(&v)));
    }

    fn plan(&self, pairs: &[(&str, &str)], allow: bool) -> Result<Plan, Error> {
        let set = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        self.lab.plan(PlanInput { set, allow_unconfirmed: allow })
    }

    fn log(&self) -> Vec<serde_json::Value> {
        let f = crate::session::files(self.data.path()).pop().unwrap();
        std::fs::read_to_string(f).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    fn done(self) {
        self.lab.shutdown();
    }
}

#[test]
fn plan_then_apply_writes_reads_back_and_logs_the_actor() {
    let r = rig();
    r.set_on_camera(APO, Value::Int(1));
    // A menu label and a value label, any case.
    let p = r.plan(&[("auto power off", "off")], false).unwrap();
    assert!(p.plan_id.starts_with("p-"));
    assert_eq!(p.requires, Requires { mode: Some("tether".into()), capture: Some(Capture::Stills) });
    assert_eq!(p.steps.len(), 1, "{p:?}");
    let s = &p.steps[0];
    assert_eq!((s.code.as_str(), s.name.as_deref(), s.before.as_str(), s.after.as_str(), s.gate.as_str()), ("0xd381", Some("AutoPowerOff"), "5 MIN", "OFF", "ok"));
    assert_eq!(s.confidence, Some(Confidence::Confirmed));
    assert!(p.expires_in_s > 290 && p.expires_in_s <= 300);
    assert_eq!(r.value(APO), Some(Value::Int(1)), "a plan writes nothing");
    assert_eq!(r.lab.plan_show(&p.plan_id).unwrap().steps.len(), 1);

    let out = r.lab.apply("cli", &p.plan_id).unwrap();
    assert_eq!((out.applied, out.failed, out.not_attempted), (1, 0, 0), "{out:?}");
    let st = &out.steps[0];
    assert!(st.ok && st.error.is_none());
    assert_eq!((st.wanted.as_str(), st.readback.as_deref(), st.result.as_str(), st.attempts), ("OFF", Some("OFF"), "OK", 1));
    assert_eq!(r.cam.value(APO), Some(Value::Int(3)));
    assert_eq!(r.lab.apply("cli", &p.plan_id).unwrap_err().code, ErrorCode::PlanExpired, "a plan applies once");

    // The undo plan writes the old value back.
    let undo = out.undo_plan_id.clone().unwrap();
    let u = r.lab.plan_show(&undo).unwrap();
    assert_eq!((u.steps[0].before.as_str(), u.steps[0].after.as_str(), u.steps[0].gate.as_str()), ("OFF", "5 MIN", "undo"));
    assert_eq!(r.lab.apply("app", &undo).unwrap().applied, 1);
    assert_eq!(r.cam.value(APO), Some(Value::Int(1)));

    let log = r.log();
    let writes: Vec<&serde_json::Value> = log.iter().filter(|l| l["kind"] == "write").collect();
    assert_eq!(writes.len(), 2);
    assert_eq!((writes[0]["actor"].as_str(), writes[0]["plan"].as_str()), (Some("cli"), Some(p.plan_id.as_str())));
    assert_eq!(writes[0]["outcome"]["code"], APO);
    assert_eq!(writes[1]["actor"], "app");
    let applies: Vec<&serde_json::Value> = log.iter().filter(|l| l["kind"] == "apply").collect();
    assert_eq!((applies[0]["actor"].as_str(), applies[0]["applied"].as_u64(), applies[0]["undo"].as_str()), (Some("cli"), Some(1), Some(undo.as_str())));
    // The read-back's change group names the actor too.
    assert!(r.lab.snapshot().groups.iter().any(|g| g.source == ChangeSource::Write && g.actor.as_deref() == Some("cli")));
    r.done();
}

#[test]
fn a_stale_plan_is_refused_and_writes_nothing() {
    let r = rig();
    r.set_on_camera(APO, Value::Int(1));
    let p = r.plan(&[("AutoPowerOff", "OFF")], false).unwrap();
    r.set_on_camera(APO, Value::Int(2)); // Someone turns it to 2 MIN on the body.
    let e = r.lab.apply("cli", &p.plan_id).unwrap_err();
    assert_eq!(e.code, ErrorCode::PlanStale);
    assert!(e.message.contains("5 MIN -> 2 MIN"), "{}", e.message);
    assert_eq!(r.cam.value(APO), Some(Value::Int(2)));
    assert!(!r.log().iter().any(|l| l["kind"] == "write"));
    r.done();
}

#[test]
fn unconfirmed_codes_need_the_flag() {
    let r = rig();
    r.set_on_camera(GRAIN, Value::Int(1));
    let p = r.plan(&[("GrainEffect", "WEAK SMALL")], false).unwrap();
    assert!(p.steps.is_empty());
    assert_eq!(p.blocked[0].reason, "unconfirmed_property");
    assert_eq!(p.empty_reason().unwrap().code, ErrorCode::UnconfirmedProperty);
    // A blocked step is never written, even when the plan is applied.
    assert_eq!(r.lab.apply("cli", &p.plan_id).unwrap().applied, 0);
    assert_eq!(r.cam.value(GRAIN), Some(Value::Int(1)));

    let p = r.plan(&[("GrainEffect", "WEAK SMALL")], true).unwrap();
    assert_eq!((p.steps.len(), p.steps[0].gate.as_str()), (1, "unconfirmed_allowed"));
    assert_eq!(r.lab.apply("mcp:test", &p.plan_id).unwrap().applied, 1);
    assert_eq!(r.cam.value(GRAIN), Some(Value::Int(2)));
    r.done();
}

#[test]
fn gates_and_lookups() {
    let r = rig();
    r.set_on_camera(APO, Value::Int(1));
    // A raw value the descriptor does not offer.
    let p = r.plan(&[("0xd381", "9")], false).unwrap();
    assert_eq!(p.blocked[0].reason, "value_out_of_range");
    assert!(p.blocked[0].message.contains("OFF (3)"), "{}", p.blocked[0].message);
    // Already at the value: nothing to do, and no error.
    let p = r.plan(&[("AutoPowerOff", "5 MIN")], false).unwrap();
    assert_eq!((p.steps.len(), p.skipped[0].reason.as_str()), (0, UNCHANGED));
    assert!(p.empty_reason().is_none());
    // A read-only code.
    let p = r.plan(&[("0xd22e", "2")], true).unwrap();
    assert_eq!(p.blocked[0].reason, "read_only");
    // Names and values that resolve to nothing.
    assert_eq!(r.plan(&[("NoSuchSetting", "1")], false).unwrap_err().code, ErrorCode::NotFound);
    let e = r.plan(&[("AutoPowerOff", "SOMETIMES")], false).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    assert!(e.hint.unwrap().contains("OFF (3)"));
    assert_eq!(r.plan(&[("AutoPowerOff", "OFF"), ("0xd381", "1")], false).unwrap_err().code, ErrorCode::Ambiguous);
    // A menu open on the camera: no plan.
    r.lab.fake(FakeAction::OpenMenu).unwrap();
    r.wait("menu", |s| s.status.screen == Some(Screen::Menu));
    assert_eq!(r.plan(&[("AutoPowerOff", "OFF")], false).unwrap_err().code, ErrorCode::MenuOpen);
    r.done();
}

#[test]
fn a_readback_mismatch_stops_the_apply() {
    let r = rig();
    r.set_on_camera(GRAIN, Value::Int(1));
    r.set_on_camera(APO, Value::Int(1));
    // The camera holds STRONG SMALL whatever is written, as a camera rule would.
    r.cam.with(|s| s.write_coerce.insert(GRAIN, Value::Int(3)));
    let p = r.plan(&[("GrainEffect", "WEAK SMALL"), ("AutoPowerOff", "OFF")], true).unwrap();
    assert_eq!(p.steps.len(), 2);
    let out = r.lab.apply("cli", &p.plan_id).unwrap();
    assert_eq!((out.applied, out.failed, out.not_attempted), (0, 1, 1), "{out:?}");
    let st = &out.steps[0];
    assert_eq!((st.error, st.wanted.as_str(), st.readback.as_deref(), st.result.as_str()), (Some(ErrorCode::ReadbackMismatch), "WEAK SMALL", Some("STRONG SMALL"), "OK"));
    assert_eq!(r.cam.value(APO), Some(Value::Int(1)), "later steps are not attempted");
    // It was written, so the undo plan restores it.
    let u = r.lab.plan_show(out.undo_plan_id.as_deref().unwrap()).unwrap();
    assert_eq!((u.steps[0].before.as_str(), u.steps[0].after.as_str()), ("STRONG SMALL", "OFF"));
    r.done();
}

#[test]
fn a_busy_camera_is_retried_then_reported_as_busy() {
    let r = rig();
    r.set_on_camera(APO, Value::Int(1));
    r.cam.with(|s| s.write_busy = 2);
    let p = r.plan(&[("AutoPowerOff", "OFF")], false).unwrap();
    let out = r.lab.apply("cli", &p.plan_id).unwrap();
    assert_eq!((out.applied, out.steps[0].attempts), (1, 3), "{out:?}");

    r.cam.with(|s| s.write_busy = 1000);
    let p = r.plan(&[("AutoPowerOff", "5 MIN")], false).unwrap();
    let t = Instant::now();
    let out = r.lab.apply("cli", &p.plan_id).unwrap();
    assert!(t.elapsed() >= Duration::from_millis(3100), "{:?}", t.elapsed());
    let st = &out.steps[0];
    assert_eq!((st.ok, st.error, st.attempts, st.result.as_str()), (false, Some(ErrorCode::Busy), 6, "DeviceBusy"));
    assert!(out.undo_plan_id.is_none(), "nothing was written");
    assert_eq!(r.lab.snapshot().status.link, Link::Connected, "no reconnect");
    r.cam.with(|s| s.write_busy = 0);
    r.done();
}

#[test]
fn discard_and_a_disconnected_camera() {
    let r = rig();
    r.set_on_camera(APO, Value::Int(1));
    let p = r.plan(&[("AutoPowerOff", "OFF")], false).unwrap();
    r.lab.discard(&p.plan_id).unwrap();
    assert_eq!(r.lab.apply("cli", &p.plan_id).unwrap_err().code, ErrorCode::PlanExpired);
    assert_eq!(r.lab.discard(&p.plan_id).unwrap_err().code, ErrorCode::PlanExpired);

    // With the connect switch off, apply refuses and keeps the plan for later.
    let p = r.plan(&[("AutoPowerOff", "OFF")], false).unwrap();
    r.lab.connect("cli", false);
    r.wait("stopped", |s| s.status.link == Link::Stopped);
    assert_eq!(r.lab.apply("cli", &p.plan_id).unwrap_err().code, ErrorCode::NotConnected);
    assert!(r.lab.plan_show(&p.plan_id).is_ok());
    assert_eq!(r.plan(&[("AutoPowerOff", "OFF")], false).unwrap_err().code, ErrorCode::NotConnected);
    assert_eq!(r.lab.export("cli").unwrap_err().code, ErrorCode::NotConnected);
    let log = r.log();
    assert!(log.iter().any(|l| l["kind"] == "connect" && l["actor"] == "cli" && l["on"] == false));
    r.done();
}
