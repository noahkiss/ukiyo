//! Plans on the whole Lab and the fake camera, in real time (the busy retry waits for real).

use super::*;
use crate::camera::fake::{FakeHandle, FakeMode};
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
    rig_in(FakeMode::Tether)
}

fn rig_in(mode: FakeMode) -> Rig {
    let data = tempfile::tempdir().unwrap();
    let cats = tempfile::tempdir().unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/x-t5.json");
    // GrainEffect stands for an unconfirmed property, whatever the mapping has reached.
    let mut cat: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(repo).unwrap()).unwrap();
    cat["properties"]["0xd023"]["confidence"] = "likely".into();
    std::fs::write(cats.path().join("x-t5.json"), serde_json::to_string_pretty(&cat).unwrap()).unwrap();
    let lock = OwnerLock::try_acquire(&data.path().join("owner.lock")).unwrap();
    let lab = Lab::start_in(lock, data.path().into(), cats.path().into(), Some(mode), |_| {});
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
        self.lab.plan(PlanInput { set, allow_unconfirmed: allow, ..Default::default() })
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

const FILM_SIM: u16 = 0xD001;
const COLOR: u16 = 0xD008;
const MONO_WC: u16 = 0xD104;
const CLARITY: u16 = 0xD032;
const FILE_TYPE: u16 = 0xD1B2;

fn purposes(p: &Plan) -> Vec<(String, String, String)> {
    p.steps.iter().map(|s| (s.code.clone(), s.purpose.clone(), s.after.clone())).collect()
}

/// task-o8vkpf69: Monochromatic Color takes writes only under a monochrome film simulation.
/// The plan sets one for the time of the write, puts the film simulation back, and puts back
/// the Color the temporary simulation made the camera reset.
#[test]
fn a_prerequisite_is_set_for_the_write_and_put_back() {
    let r = rig();
    r.set_on_camera(FILM_SIM, Value::Int(1));
    r.set_on_camera(COLOR, Value::Int(20));
    r.set_on_camera(MONO_WC, Value::Int(0));
    let p = r.plan(&[("MonochromaticColorWC", "30")], false).unwrap();
    let want = [
        ("0xd001", "prerequisite", "MONOCHROME"),
        ("0xd104", "set", "3"),
        ("0xd001", "restore", "PROVIA/STANDARD"),
        ("0xd008", "restore", "2"),
    ];
    let got = purposes(&p);
    assert_eq!(got.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect::<Vec<_>>(), want, "{p:#?}");
    assert!(p.steps[0].note.as_deref().is_some_and(|n| n.contains("monochrome") || n.contains("MONOCHROME")), "{:?}", p.steps[0].note);
    assert_eq!(p.steps[0].resets, ["0xd008 Color -> 0"]);
    let out = r.lab.apply("cli", &p.plan_id).unwrap();
    assert_eq!((out.applied, out.failed), (4, 0), "{out:?}");
    assert_eq!(r.cam.value(MONO_WC), Some(Value::Int(30)));
    assert_eq!(r.cam.value(FILM_SIM), Some(Value::Int(1)), "the film simulation is back");
    assert_eq!(r.cam.value(COLOR), Some(Value::Int(20)), "and so is Color");
    // The undo is planned the same way: it needs the monochrome simulation again.
    let u = r.lab.plan_show(out.undo_plan_id.as_deref().unwrap()).unwrap();
    let ups: Vec<String> = u.steps.iter().map(|s| format!("{} {}", s.code, s.purpose)).collect();
    assert_eq!(ups, ["0xd001 prerequisite", "0xd104 set", "0xd001 restore", "0xd008 restore"], "{u:#?}");
    assert!(u.steps.iter().all(|s| s.gate == "undo"));
    assert_eq!(r.lab.apply("cli", &u.plan_id).unwrap().failed, 0);
    assert_eq!(r.cam.value(MONO_WC), Some(Value::Int(0)));
    r.done();
}

/// A setting the camera resets with another goes after it, whatever the order asked.
#[test]
fn a_reset_setting_is_written_after_the_one_that_resets_it() {
    let r = rig();
    r.set_on_camera(FILE_TYPE, Value::Int(7));
    r.set_on_camera(CLARITY, Value::Int(0));
    let p = r.plan(&[("Clarity", "30"), ("ImageFormat", "HEIF")], false).unwrap();
    let order: Vec<&str> = p.steps.iter().map(|s| s.code.as_str()).collect();
    assert_eq!(order, ["0xd1b2", "0xd032"], "{p:#?}");
    assert_eq!(p.steps[0].resets, ["0xd032 Clarity -> 0"]);
    let out = r.lab.apply("cli", &p.plan_id).unwrap();
    assert_eq!(out.failed, 0, "{out:?}");
    assert_eq!(r.cam.value(CLARITY), Some(Value::Int(30)), "HEIF reset it first; the plan wrote it after");
    // Already at the value, but the plan also resets it: still written.
    r.set_on_camera(FILE_TYPE, Value::Int(7));
    let p = r.plan(&[("Clarity", "30"), ("ImageFormat", "HEIF")], false).unwrap();
    assert_eq!(p.steps.len(), 2, "{p:#?}");
    r.done();
}

/// A recipe for the library, a monochrome one with Monochromatic Color.
fn mono_recipe(r: &Rig) -> String {
    let store = crate::library::Store::new(r.data.path());
    let s = crate::recipes::Settings {
        film_simulation: Some("Acros + R filter".into()),
        grain_effect: Some("Strong".into()),
        grain_size: Some("Small".into()),
        monochromatic_color_wc: Some(2.0),
        highlight_tone: Some(-1.0),
        ..Default::default()
    };
    store.save_recipe_input("test", Some("Test Acros".into()), Some(serde_json::json!({ "settings": s })), None).unwrap().id
}

fn slot_plan(r: &Rig, recipe: &str, slot: &str, allow: bool) -> Result<Plan, Error> {
    r.lab.plan(PlanInput { from: Some(recipe.into()), slot: Some(slot.into()), allow_unconfirmed: allow, ..Default::default() })
}

#[test]
fn a_recipe_goes_into_a_slot_film_simulation_first_name_last() {
    use crate::recipes::slot;
    let r = rig_in(FakeMode::RawConv);
    let id = mono_recipe(&r);
    // The slot fields are not confirmed yet: every step is blocked without the flag.
    let p = slot_plan(&r, &id, "C3", false).unwrap();
    assert!(p.steps.is_empty());
    assert_eq!(p.empty_reason().unwrap().code, ErrorCode::UnconfirmedProperty);
    let p = slot_plan(&r, &id, "C3", true).unwrap();
    assert_eq!((p.slot.as_deref(), p.recipe.as_deref()), (Some("C3"), Some(id.as_str())));
    let codes: Vec<&str> = p.steps.iter().map(|s| s.code.as_str()).collect();
    assert_eq!(codes.first(), Some(&"0xd192"), "{codes:?}");
    assert_eq!(codes.last(), Some(&"0xd18d"), "the name last");
    let at = |c: &str| codes.iter().position(|x| *x == c).unwrap();
    assert!(at("0xd192") < at("0xd193"), "the simulation before Monochromatic Color");
    assert!(p.steps.iter().all(|s| s.purpose == "set"), "C3 needs no prerequisite: the recipe sets the simulation first");
    assert!(p.skipped.iter().any(|l| l.code == "0xd18e") || !codes.contains(&"0xd18e"), "image size is never written");
    let sel_before = r.cam.value(slot::SELECT);
    let out = r.lab.apply("cli", &p.plan_id).unwrap();
    assert_eq!(out.failed, 0, "{out:#?}");
    assert_eq!(out.applied as usize, p.steps.len());
    assert_eq!(r.cam.value(slot::SELECT), sel_before, "the selector is restored");
    assert_eq!(r.cam.slot_value(3, slot::FILM_SIM), Some(Value::Int(14)));
    assert_eq!(r.cam.slot_value(3, slot::MONO_WC), Some(Value::Int(20)));
    assert_eq!(r.cam.slot_value(3, slot::NAME), Some(Value::Str("Test Acros".into())));
    // Read back into recipe form: the slot matches the library recipe.
    let v = r.lab.read_slots("cli", vec![3]).unwrap().remove(0);
    assert_eq!((v.name.as_str(), v.settings.film_simulation.as_deref()), ("Test Acros", Some("Acros + R filter")));
    assert_eq!(v.matches, std::slice::from_ref(&id));
    // The undo plan puts C3 back.
    let u = r.lab.plan_show(out.undo_plan_id.as_deref().unwrap()).unwrap();
    assert_eq!(u.slot.as_deref(), Some("C3"));
    let ua = r.lab.apply("cli", &u.plan_id).unwrap();
    assert_eq!(ua.failed, 0, "{u:#?}\n{ua:#?}");
    assert_eq!(r.cam.slot_value(3, slot::FILM_SIM), Some(Value::Int(1)));
    assert_eq!(r.cam.slot_value(3, slot::NAME), Some(Value::Str(String::new())));
    let log = r.log();
    assert!(log.iter().any(|l| l["kind"] == "write" && l["slot"] == "C3"));
    assert!(log.iter().any(|l| l["kind"] == "log" && l["text"].as_str().is_some_and(|t| t.starts_with("slot C3 write"))));
    r.done();
}

/// C2 holds D-range priority AUTO: the slot's tones answer 0x201C until it is off, so a
/// recipe with priority off writes it first. A colour recipe over C2's monochrome one
/// leaves Monochromatic Color alone.
#[test]
fn slot_plans_order_d_range_priority_before_the_tones() {
    use crate::recipes::slot;
    let r = rig_in(FakeMode::RawConv);
    let p = slot_plan(&r, &format!("cat:{}", classic_chrome()), "C2", true).unwrap();
    let codes: Vec<&str> = p.steps.iter().map(|s| s.code.as_str()).collect();
    let at = |c: &str| codes.iter().position(|x| *x == c);
    assert!(at("0xd191").unwrap() < at("0xd19d").unwrap_or(usize::MAX), "{codes:?}");
    assert_eq!(at("0xd193"), None, "no Monochromatic Color for a colour recipe");
    let out = r.lab.apply("cli", &p.plan_id).unwrap();
    assert_eq!(out.failed, 0, "{out:#?}");
    assert_eq!(r.cam.slot_value(2, slot::DRP), Some(Value::Int(0)));
    r.done();
}

/// A catalogue recipe for X-Trans V with Classic Chrome, D-range priority off.
fn classic_chrome() -> String {
    crate::recipes::bundle::catalog()
        .recipes
        .iter()
        .find(|r| r.settings.film_simulation.as_deref() == Some("Classic Chrome") && r.settings.d_range_priority.as_deref().is_none_or(|d| d == "Off") && crate::recipes::slot::encode(&r.settings).is_ok())
        .unwrap()
        .id
        .clone()
}

#[test]
fn a_slot_changed_on_the_camera_makes_the_plan_stale() {
    use crate::recipes::slot;
    let r = rig_in(FakeMode::RawConv);
    let id = mono_recipe(&r);
    let p = slot_plan(&r, &id, "C4", true).unwrap();
    r.cam.with(|s| s.slots[3].insert(slot::GRAIN, Value::Int(4)));
    assert_eq!(r.lab.apply("cli", &p.plan_id).unwrap_err().code, ErrorCode::PlanStale);
    assert_eq!(r.cam.slot_value(4, slot::FILM_SIM), Some(Value::Int(1)), "nothing written");
    r.done();
}

#[test]
fn slots_need_raw_conv_mode() {
    let r = rig();
    let e = slot_plan(&r, &format!("cat:{}", classic_chrome()), "C1", true).unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsRawconv);
    assert_eq!(e.walkthrough.as_deref(), Some("ukiyo help modes rawconv"));
    assert_eq!(r.lab.read_slots("cli", vec![1]).unwrap_err().code, ErrorCode::NeedsRawconv);
    assert_eq!(slot_plan(&r, "recipe:nope", "C9", true).unwrap_err().code, ErrorCode::NotFound);
    r.done();
}

#[test]
fn never_write_codes_fail_every_plan() {
    let r = rig();
    let format = r.cam.value(0xD17F);
    // Asked for by code, name or label, with or without allow_unconfirmed: the plan fails.
    for (name, value) in [("0xd17f", "2"), ("HardwareControl", "FORMAT SLOT 1"), ("PowerControl", "1"), ("0xd208", "512"), ("PTPError", "0")] {
        for allow in [false, true] {
            let e = r.plan(&[(name, value)], allow).unwrap_err();
            assert_eq!(e.code, ErrorCode::NeverWrite, "{name}: {e:?}");
            assert!(e.message.contains("never_write"), "{}", e.message);
        }
    }
    // With a setting beside it, nothing is planned either.
    let e = r.plan(&[("AutoPowerOff", "OFF"), ("0xd17b", "1")], true).unwrap_err();
    assert_eq!(e.code, ErrorCode::NeverWrite);
    assert!(e.message.contains("0xd17b PowerControl"), "{}", e.message);
    // A snapshot restore by exact value.
    let e = build_values(&r.lab.snapshot(), &[(0xD17F, Value::Int(4))], true).unwrap_err();
    assert_eq!(e.code, ErrorCode::NeverWrite);
    // A prerequisite on a never_write code is blocked, not written (the fake holds 4).
    let mut snap = r.lab.snapshot();
    let cat = snap.catalog.as_mut().unwrap();
    cat.properties.get_mut("0xd381").unwrap().requires = vec![catalog::Requirement { code: "0xd17f".into(), values: vec!["6".into()], temp: None }];
    let now = snap.rows.iter().find(|x| x.code == APO).and_then(|x| x.value.clone());
    let to = if now == Some(Value::Int(3)) { "5 MIN" } else { "OFF" };
    let (p, raw) = build(&snap, &PlanInput { set: [("AutoPowerOff".to_string(), to.to_string())].into(), ..Default::default() }).unwrap();
    assert!(p.steps.is_empty() && raw.is_empty(), "{p:?}");
    assert_eq!(p.blocked[0].reason, "never_write", "{p:?}");
    assert_eq!(p.empty_reason().unwrap().code, ErrorCode::NeverWrite);
    // A slot plan with a never_write field writes nothing.
    let current: std::collections::HashMap<u16, Value> = [(0xD17F, Value::Int(4))].into();
    let (p, raw) = super::slot_plan(&r.lab.snapshot(), 1, &current, &[(0xD17F, Value::Int(2))], true);
    assert!(raw.is_empty());
    assert_eq!(p.blocked[0].reason, "never_write");
    assert_eq!(r.cam.value(0xD17F), format, "nothing written");
    r.done();
}

#[test]
fn apply_refuses_a_plan_whose_code_became_never_write() {
    let r = rig();
    r.set_on_camera(APO, Value::Int(1));
    let p = r.plan(&[("AutoPowerOff", "OFF")], false).unwrap();
    assert_eq!(p.steps.len(), 1);
    // The catalog marks the code since the plan was made: the re-check names it.
    let mut snap = r.lab.snapshot();
    snap.catalog.as_mut().unwrap().properties.get_mut("0xd381").unwrap().never_write = true;
    let (_, raw) = build_values(&r.lab.snapshot(), &[(APO, Value::Int(3))], false).unwrap();
    let e = never_write_in(&snap, &raw).unwrap();
    assert_eq!(e.code, ErrorCode::NeverWrite);
    assert!(e.message.contains("0xd381 AutoPowerOff"), "{}", e.message);
    assert!(never_write_in(&r.lab.snapshot(), &raw).is_none());
    r.done();
}
