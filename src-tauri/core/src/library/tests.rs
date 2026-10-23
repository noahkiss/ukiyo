use super::*;
use crate::camera::owner::Link;
use crate::lab::Lab;
use crate::plan::PlanInput;
use serde_json::json;
use std::time::{Duration, Instant};

fn store() -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().unwrap();
    let s = Store::new(d.path());
    (d, s)
}

fn preset_input(name: &str, set: serde_json::Value) -> SaveInput {
    SaveInput { kind: SaveKind::Preset, name: Some(name.into()), item: Some(json!({ "set": set })), from: None }
}

#[test]
fn ids() {
    assert_eq!(Id::parse("preset:Street Day").unwrap(), Id::Item(Kind::Preset, "street-day".into()));
    assert_eq!(Id::parse("favourite:0xD381").unwrap(), Id::Favourite(0xD381));
    assert_eq!(Id::parse("prefs").unwrap(), Id::Prefs);
    assert_eq!(Id::parse("history").unwrap(), Id::History(None));
    assert!(matches!(Id::parse("cat:abc").unwrap(), Id::Catalogue(_)));
    assert!(Id::parse("nothing").is_err());
    assert!(Id::parse("preset:").is_err());
    assert_eq!(Id::of_kind(Kind::Snapshot, "Before trip").unwrap(), Id::Item(Kind::Snapshot, "before-trip".into()));
    assert!(Id::of_kind(Kind::Snapshot, "preset:x").is_err());
}

#[test]
fn presets_save_replace_trash_and_restore() {
    let (_d, s) = store();
    let a = s.save("test", &preset_input("Street", json!({ "AutoPowerOff": "OFF" })), Context::default()).unwrap();
    assert_eq!((a.id.as_str(), a.replaced), ("preset:street", false));
    let b = s.save("test", &preset_input("street", json!({ "AutoPowerOff": "5 MIN", "0xd023": 2 })), Context::default()).unwrap();
    assert!(b.replaced, "same slug replaces");
    let p = s.preset("street").unwrap();
    assert_eq!(p.set["0xd023"], "2");
    assert_eq!(p.name, "street");
    let hits = s.search(&SearchInput { kind: SearchKind::Preset, query: Some("power".into()), ..Default::default() });
    assert_eq!(hits.total, 1);
    assert!(hits.items[0].summary.starts_with("2 settings"));
    assert_eq!(s.search(&SearchInput { kind: SearchKind::Trash, query: None, ..Default::default() }).total, 1, "the replaced version");

    let t = s.delete("test", "preset:street").unwrap();
    assert!(t.trashed_at.is_some());
    assert!(s.preset("street").is_err());
    assert_eq!(s.delete("test", "preset:street").unwrap_err().code, ErrorCode::NotFound);
    let trash = s.search(&SearchInput { kind: SearchKind::Trash, query: None, ..Default::default() });
    assert_eq!(trash.total, 2);
    let back = s.save("test", &SaveInput { kind: SaveKind::Preset, name: None, item: None, from: t.trash_id.clone() }, Context::default()).unwrap();
    assert_eq!(back.id, "preset:street");
    assert_eq!(s.preset("street").unwrap().set.len(), 2, "the deleted version came back");
    assert_eq!(s.delete("test", "prefs").unwrap_err().code, ErrorCode::ReadOnly);
    assert!(s.delete("test", "cat:x").is_err());

    let log = std::fs::read_to_string(s.root().join("log.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 4);
    assert!(log.contains("\"actor\":\"test\"") && log.contains("\"op\":\"restore\""));
    // No temp file left behind.
    let leftovers: Vec<_> = std::fs::read_dir(s.root().join("presets")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
    assert!(leftovers.is_empty());
}

/// The 30 days count from the delete, not from the item's last save.
#[test]
fn the_trash_keeps_an_old_item_for_thirty_days_after_the_delete() {
    use std::time::{Duration, SystemTime};
    let (_d, s) = store();
    let days = |n: u64| SystemTime::now() - Duration::from_secs(n * 86_400);
    let backdate = |p: &Path, n: u64| std::fs::OpenOptions::new().write(true).open(p).unwrap().set_modified(days(n)).unwrap();
    s.save("test", &preset_input("Old", json!({ "AutoPowerOff": "OFF" })), Context::default()).unwrap();
    backdate(&s.path(Kind::Preset, "old"), 40);
    let t = s.delete("test", "preset:old").unwrap();
    let trash = s.trash();
    assert_eq!(trash.len(), 1, "a 40-day-old preset survives its delete");
    assert!(trash[0].at.unwrap() >= t.trashed_at.unwrap() - 60_000.0, "listed by the time it was trashed");
    let file = s.trash_dir().join(format!("{}.json", t.trash_id.unwrap().strip_prefix("trash:").unwrap()));
    backdate(&file, TRASH_DAYS as u64 - 1);
    assert_eq!(s.trash().len(), 1, "still inside the window");
    backdate(&file, TRASH_DAYS as u64 + 1);
    assert!(s.trash().is_empty(), "purged after the window");
}

#[test]
fn favourites_by_code_and_prefs() {
    let (d, s) = store();
    let f = s.save("test", &SaveInput { kind: SaveKind::Favourite, name: Some("0xD381".into()), item: None, from: None }, Context::default()).unwrap();
    assert_eq!(f.id, "favourite:0xd381");
    s.save("test", &SaveInput { kind: SaveKind::Favourite, name: Some("0xd381".into()), item: None, from: None }, Context::default()).unwrap();
    assert_eq!(s.favourites().codes(), [0xD381], "added once");
    assert!(s.save("test", &SaveInput { kind: SaveKind::Favourite, name: Some("NoSuchName".into()), item: None, from: None }, Context::default()).is_err());
    s.delete("test", "favourite:0xd381").unwrap();
    assert!(s.favourites().items.is_empty());

    s.save("test", &SaveInput { kind: SaveKind::Prefs, name: None, item: Some(json!({ "theme": "dark", "keep_awake": false })), from: None }, Context::default()).unwrap();
    let Read::Prefs(p) = s.read("prefs", None).unwrap() else { panic!() };
    assert_eq!((p.theme, p.keep_awake), (prefs::Theme::Dark, false));
    assert_eq!(prefs::load(d.path()).theme, prefs::Theme::Dark);
    assert!(s.save("test", &SaveInput { kind: SaveKind::Prefs, name: None, item: Some(json!({ "colour": "red" })), from: None }, Context::default()).is_err());
}

fn fake_lab() -> (tempfile::TempDir, tempfile::TempDir, std::sync::Arc<Lab>) {
    std::env::set_var("UKIYO_FAKE_CAMERA", "1");
    let data = tempfile::tempdir().unwrap();
    let cats = tempfile::tempdir().unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/x-t5.json");
    let mut cat: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(repo).unwrap()).unwrap();
    cat["properties"]["0xd023"]["confidence"] = "likely".into();
    std::fs::write(cats.path().join("x-t5.json"), serde_json::to_string(&cat).unwrap()).unwrap();
    let lock = crate::serve::OwnerLock::try_acquire(&data.path().join("owner.lock")).unwrap();
    let lab = Lab::start(lock, data.path().into(), cats.path().into(), |_| {});
    let deadline = Instant::now() + Duration::from_secs(10);
    while lab.snapshot().status.link != Link::Connected || lab.snapshot().rows.is_empty() {
        assert!(Instant::now() < deadline, "fake camera never connected");
        std::thread::sleep(Duration::from_millis(20));
    }
    (data, cats, lab)
}

/// Snapshots, presets and history against the fake camera: capture, diff, restore as a plan.
#[test]
fn library_on_the_fake_camera() {
    let (data, _cats, lab) = fake_lab();
    let s = Store::new(data.path());
    let snap = lab.snapshot();
    let ctx = Context { camera: Some(&snap), catalog: None };

    // History: the connect is recorded under a salted id, and the status carries it.
    let id = snap.status.camera_id.clone().expect("a camera id");
    let Read::History(h) = s.read("history", None).unwrap() else { panic!() };
    assert_eq!(h.len(), 1);
    assert_eq!((h[0].camera.as_str(), h[0].model.as_deref()), (id.as_str(), Some("X-T5")));
    assert_eq!(h[0].connects.len(), 1);
    assert!(serde_json::to_string(&snap.status).unwrap().contains(&id));
    assert!(!serde_json::to_string(&snap.status).unwrap().contains("serial"), "the digest never serializes");

    let a = s.save("test", &SaveInput { kind: SaveKind::Snapshot, name: Some("Before".into()), item: None, from: None }, ctx).unwrap();
    assert_eq!(a.id, "snapshot:before");
    let before = s.snapshot("before").unwrap();
    assert_eq!(before.model.as_deref(), Some("X-T5"));
    assert_eq!(before.mode.as_deref(), Some("tether"));
    assert!(before.values.len() > 10, "{}", before.values.len());
    assert!(before.values.iter().any(|v| v.code == "0xd023"), "a likely setting is kept");
    let text = serde_json::to_string(&before).unwrap();
    for v in &before.values {
        assert!(!matches!(v.value, RawValue::Str(_)), "no text values: {}", v.code);
    }
    assert!(!text.contains("TotalShotCount"));

    // Change GrainEffect, snapshot again, diff.
    let old = before.values.iter().find(|v| v.code == "0xd023").unwrap().value.clone();
    let new = if old == RawValue::Int(2) { "3" } else { "2" };
    let mut set = IndexMap::new();
    set.insert("0xd023".to_string(), new.to_string());
    let p = lab.plan(PlanInput { set, allow_unconfirmed: true, from: None, slot: None }).unwrap();
    assert_eq!(lab.apply("test", &p.plan_id).unwrap().applied, 1);
    let snap2 = lab.snapshot();
    s.save("test", &SaveInput { kind: SaveKind::Snapshot, name: Some("After".into()), item: None, from: None }, Context { camera: Some(&snap2), catalog: None }).unwrap();
    let Read::Diff(d) = s.read("snapshot:before", Some("snapshot:after")).unwrap() else { panic!() };
    assert!(d.changed.iter().any(|c| c.code == "0xd023" && c.a == old), "{d:?}");
    assert!(d.same > 0);

    // Restore: a plan from the snapshot. GrainEffect is likely, so blocked without the flag.
    let plan = lab.plan(PlanInput { from: Some("snapshot:before".into()), ..Default::default() }).unwrap();
    assert!(plan.blocked.iter().any(|b| b.code == "0xd023" && b.reason == "unconfirmed_property"), "{plan:?}");
    let plan = lab.plan(PlanInput { from: Some("snapshot:before".into()), allow_unconfirmed: true, ..Default::default() }).unwrap();
    assert!(plan.steps.iter().any(|s| s.code == "0xd023"));
    assert!(plan.skipped.iter().all(|l| l.reason == crate::plan::UNCHANGED || l.reason.starts_with("needs_")), "{:?}", plan.skipped);
    let out = lab.apply("test", &plan.plan_id).unwrap();
    assert_eq!(out.failed, 0, "{out:?}");
    let row = lab.snapshot().rows.into_iter().find(|r| r.code == 0xD023).unwrap();
    assert_eq!(row.value.map(|v| crate::read::raw_value(&v)), Some(old));

    // A preset of current values, applied as a plan.
    let cam = lab.snapshot();
    let saved = s.save("test", &SaveInput { kind: SaveKind::Preset, name: Some("Grain".into()), item: Some(json!({ "settings": ["GrainEffect"] })), from: None }, Context { camera: Some(&cam), catalog: None }).unwrap();
    assert_eq!(saved.id, "preset:grain");
    assert_eq!(s.preset("grain").unwrap().set.len(), 1);
    let plan = lab.plan(PlanInput { from: Some("preset:grain".into()), allow_unconfirmed: true, ..Default::default() }).unwrap();
    assert!(plan.steps.is_empty() && plan.skipped.len() == 1, "already set: {plan:?}");
    // A preset from a snapshot.
    s.save("test", &SaveInput { kind: SaveKind::Preset, name: Some("From before".into()), item: None, from: Some("snapshot:before".into()) }, Context::default()).unwrap();
    assert_eq!(s.preset("from-before").unwrap().set.len(), before.values.len());
    assert!(lab.plan(PlanInput { from: Some("prefs".into()), ..Default::default() }).is_err());
    // Favourites by name, with the camera's catalog.
    s.save("test", &SaveInput { kind: SaveKind::Favourite, name: Some("AutoPowerOff".into()), item: None, from: None }, Context { camera: Some(&cam), catalog: None }).unwrap();
    assert_eq!(s.favourites().items[0].name.as_deref(), Some("AutoPowerOff"));
    lab.shutdown();
}
