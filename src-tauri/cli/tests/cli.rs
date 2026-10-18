//! The CLI as real processes. Every run uses the fake camera (`UKIYO_FAKE_CAMERA=1`)
//! in a temp data folder, so the `serve` it spawns never touches USB.

use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use ukiyo_core::api::Request;
use ukiyo_core::lab::FakeAction;
use ukiyo_core::paths;
use ukiyo_core::serve::client::{Client, Options};
use ukiyo_core::serve::{lock, OwnerLock};

const EXE: &str = env!("CARGO_BIN_EXE_ukiyo-cli");

struct Env {
    data: tempfile::TempDir,
    cats: tempfile::TempDir,
    /// `UKIYO_FAKE_MODE` for every command, so a respawned serve fakes the same mode.
    mode: Option<&'static str>,
}

impl Env {
    fn new() -> Env {
        let cats = tempfile::tempdir().unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/x-t5.json");
        // GrainEffect stands for an unconfirmed property, whatever the mapping has reached.
        let mut cat: Value = serde_json::from_str(&std::fs::read_to_string(repo).unwrap()).unwrap();
        cat["properties"]["0xd023"]["confidence"] = "likely".into();
        std::fs::write(cats.path().join("x-t5.json"), serde_json::to_string_pretty(&cat).unwrap()).unwrap();
        Env { data: tempfile::tempdir().unwrap(), cats, mode: None }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(EXE);
        c.args(args)
            // Never the real camera: the spawned serve inherits this.
            .env("UKIYO_FAKE_CAMERA", "1")
            .env("UKIYO_DATA_DIR", self.data.path())
            .env("UKIYO_CATALOG_DIR", self.cats.path())
            // Long enough that one test's commands share one serve on a loaded machine.
            .env("UKIYO_IDLE_EXIT", "8")
            .env_remove("UKIYO_FAKE_MODE")
            .stdin(Stdio::null());
        if let Some(m) = self.mode {
            c.env("UKIYO_FAKE_MODE", m);
        }
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().unwrap()
    }

    /// Runs with `--json` and returns the exit code and the one JSON document.
    fn json(&self, args: &[&str]) -> (i32, Value) {
        let mut a = args.to_vec();
        a.push("--json");
        let out = self.run(&a);
        let text = String::from_utf8(out.stdout).unwrap();
        assert_eq!(text.lines().count(), 1, "one JSON document: {text:?}, stderr {}", String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap(), serde_json::from_str(&text).unwrap())
    }

    fn pid(&self) -> u32 {
        let (_, v) = self.json(&["status"]);
        v["data"]["pid"].as_u64().unwrap() as u32
    }

    /// Waits for the spawned serve to idle out and checks it left nothing behind.
    fn finish(&self, pid: u32) {
        wait_until("serve idles out", 30, || !alive(pid));
        assert!(lock::is_free(&paths::lock_path(self.data.path())));
        assert!(!paths::socket_path(self.data.path()).exists());
    }
}

fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn status_and_settings_on_the_fake_camera() {
    let env = Env::new();
    let (code, v) = env.json(&["status"]);
    assert_eq!(code, 0, "{v}");
    let d = &v["data"];
    assert_eq!(v["ok"], true);
    assert_eq!(d["link"], "connected");
    assert_eq!(d["model"], "X-T5");
    assert_eq!(d["mode"], "tether");
    assert_eq!(d["owner"], "serve");
    assert_eq!(d["fake"], true);
    assert_eq!(d["available"], serde_json::json!({ "settings": true, "slots": false, "movie_props": false }));
    let pid = d["pid"].as_u64().unwrap() as u32;
    assert_ne!(pid, 0);

    let o = env.run(&["status"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o).starts_with("X-T5 · firmware 9.99 · tether, stills, shooting"), "{}", text(&o));
    assert!(text(&o).contains("Needs RAW CONV.: recipe slots (ukiyo help modes rawconv)"));

    let (code, v) = env.json(&["settings", "--code", "0xd381"]);
    assert_eq!(code, 0, "{v}");
    let row = &v["data"]["rows"][0];
    assert_eq!(row["code"], "0xd381");
    assert_eq!(row["name"], "AutoPowerOff");
    assert_eq!(row["confidence"], "confirmed");
    assert!(row["value"].is_number());
    assert!(row["value_label"].is_string());
    assert!(row["options"].as_array().unwrap().iter().any(|o| o["label"] == "OFF"));
    assert_eq!((v["data"]["total"].as_u64(), v["data"]["has_more"].as_bool()), (Some(1), Some(false)));

    let o = env.run(&["settings", "auto", "power"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o).contains("AutoPowerOff"), "{}", text(&o));
    let (code, v) = env.json(&["settings", "--menus"]);
    assert_eq!(code, 0);
    assert!(v["data"]["menus"].as_array().unwrap().iter().any(|m| m["path"] == "SET UP > POWER MANAGEMENT"));
    let (code, v) = env.json(&["settings", "--favourites"]);
    assert_eq!((code, v["data"]["total"].as_u64()), (0, Some(0)), "no favourites yet: {v}");
    let (code, _) = env.json(&["settings", "fav", "add", "AutoPowerOff"]);
    assert_eq!(code, 0);
    let (_, v) = env.json(&["settings", "--favourites"]);
    assert_eq!(v["data"]["rows"][0]["code"], "0xd381", "{v}");

    // Snapshots and presets.
    let (code, v) = env.json(&["snapshots", "save", "Before trip"]);
    assert_eq!((code, v["data"]["id"].as_str()), (0, Some("snapshot:before-trip")), "{v}");
    let (code, v) = env.json(&["snapshots", "diff", "before-trip"]);
    assert_eq!(code, 0, "{v}");
    assert!(v["data"]["plan"]["steps"].as_array().unwrap().is_empty(), "nothing moved since: {v}");
    let (code, v) = env.json(&["presets", "save", "Power", "AutoPowerOff"]);
    assert_eq!((code, v["data"]["id"].as_str()), (0, Some("preset:power")), "{v}");
    let (_, v) = env.json(&["presets", "show", "power"]);
    assert_eq!(v["data"]["set"].as_object().unwrap().len(), 1, "{v}");
    let (code, v) = env.json(&["presets", "apply", "power", "--yes"]);
    assert_eq!(code, 0, "already set: nothing to change: {v}");
    let (code, v) = env.json(&["presets", "save", "Off", "--set", "AutoPowerOff=OFF", "--set", "AutoPowerOff=5 MIN"]);
    assert_eq!(code, 0, "{v}");
    let (code, _) = env.json(&["snapshots", "restore", "before-trip", "--dry-run"]);
    assert!(code == 0 || code == 6, "nothing to restore, or a plan not applied");
    let o = env.run(&["presets", "ls"]);
    assert!(text(&o).contains("preset:power"), "{}", text(&o));
    let (code, v) = env.json(&["presets", "rm", "power"]);
    assert_eq!(code, 0);
    let tid = v["data"]["trash_id"].as_str().unwrap().to_string();
    let (code, v) = env.json(&["trash", "restore", &tid]);
    assert_eq!((code, v["data"]["id"].as_str()), (0, Some("preset:power")), "{v}");
    let (code, v) = env.json(&["presets", "show", "nope"]);
    assert_eq!((code, v["error"]["code"].as_str()), (2, Some("not_found")));

    // Prefs and history.
    let (code, v) = env.json(&["prefs", "set", "theme=dark", "keep-awake=off"]);
    assert_eq!((code, v["data"]["theme"].as_str(), v["data"]["keep_awake"].as_bool()), (0, Some("dark"), Some(false)), "{v}");
    // The owner thread takes the change from its queue, so status can lag a moment behind.
    let mut v = Value::Null;
    for _ in 0..50 {
        v = env.json(&["status"]).1;
        if v["data"]["keep_awake"] == false {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(v["data"]["keep_awake"], false, "the owner got keep-awake off: {v}");
    let (code, v) = env.json(&["prefs", "set", "brand-mark=nope"]);
    assert_eq!((code, v["error"]["code"].as_str()), (2, Some("not_found")));
    let (code, v) = env.json(&["status", "--history"]);
    assert_eq!((code, v["data"]["history"]["model"].as_str()), (0, Some("X-T5")), "{v}");
    let settings: Value = serde_json::from_str(&std::fs::read_to_string(env.data.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!((settings["theme"].as_str(), settings["in_use_minutes"].as_u64()), (Some("dark"), Some(5)));

    // Every command found the same owner: one spawn for all of them.
    assert_eq!(env.pid(), pid);
    env.finish(pid);
}

#[test]
fn no_camera_is_exit_3() {
    let env = Env::new();
    let pid = env.pid();
    let c = Client::new(Options::new(env.data.path().into(), "test", None), None);
    c.call(Request::LabFake { action: FakeAction::Unplug }).unwrap();
    wait_until("the fake camera is gone", 10, || env.json(&["status"]).1["data"]["link"] != "connected");
    let (code, v) = env.json(&["status"]);
    assert_eq!(code, 0, "status reports a missing camera without failing: {v}");
    let (code, v) = env.json(&["settings"]);
    assert_eq!(code, 3, "{v}");
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "not_connected");
    assert!(v["error"]["hint"].is_string());
    let o = env.run(&["settings"]);
    assert_eq!(o.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&o.stderr).contains("No camera on USB"));
    c.close();
    env.finish(pid);
}

#[test]
fn wrong_mode_is_exit_4() {
    let env = Env { mode: Some("rawconv"), ..Env::new() };
    let (code, v) = env.json(&["settings"]);
    assert_eq!(code, 4, "{v}");
    assert_eq!(v["error"]["code"], "needs_tether");
    assert_eq!(v["error"]["mode"], "rawconv");
    assert_eq!(v["error"]["walkthrough"], "ukiyo help modes tether");
    // The Lab's view of one code works in any mode.
    let (code, v) = env.json(&["settings", "--code", "0xd18e"]);
    assert_eq!(code, 0, "{v}");
    let (_, s) = env.json(&["status"]);
    assert_eq!(s["data"]["mode"], "rawConv");
    assert_eq!(s["data"]["available"]["slots"], true);
    env.finish(s["data"]["pid"].as_u64().unwrap() as u32);
}

#[test]
fn recipes_library_catalogue_and_slots() {
    let env = Env { mode: Some("rawconv"), ..Env::new() };
    // The catalogue and the library run in this process.
    let (code, v) = env.json(&["recipes", "search", "--sensor", "X-Trans V", "--sim", "classic chrome", "--limit", "3"]);
    assert_eq!(code, 0, "{v}");
    let items = v["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|i| i["id"].as_str().unwrap().starts_with("cat:") && i["credit"].is_string()));
    let file = env.data.path().join("recipe.txt");
    std::fs::write(&file, "Film Simulation: Classic Chrome\nGrain Effect: Weak, Small\nHighlight: -1\nShadow: +1\nISO: up to 6400\n").unwrap();
    let (code, v) = env.json(&["recipes", "import", "--text", file.to_str().unwrap()]);
    assert_eq!((code, v["data"]["settings"]["filmSimulation"].as_str()), (0, Some("Classic Chrome")), "{v}");
    let (code, v) = env.json(&["recipes", "import", "--text", file.to_str().unwrap(), "--name", "My CC", "--save"]);
    assert_eq!((code, v["data"]["id"].as_str()), (0, Some("recipe:my-cc")), "{v}");
    let (_, v) = env.json(&["recipes", "edit", "my-cc", "Color=+2"]);
    assert_eq!(v["data"]["replaced"], true, "{v}");
    let (_, v) = env.json(&["recipes", "show", "my-cc"]);
    assert_eq!((v["data"]["kind"].as_str(), v["data"]["item"]["settings"]["color"].as_f64()), (Some("recipe"), Some(2.0)), "{v}");
    let (_, v) = env.json(&["recipes", "ls", "chrome"]);
    assert_eq!(v["data"]["total"], 1);
    // The camera's slots, through serve.
    let (code, v) = env.json(&["recipes", "read", "C1"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!((v["data"]["kind"].as_str(), v["data"]["item"][0]["name"].as_str()), (Some("slots"), Some("FAKE CHROME")), "{v}");
    // Slot fields are not confirmed: refused without the flag (exit 5).
    let (code, v) = env.json(&["recipes", "write", "C3", "my-cc", "--yes"]);
    assert_eq!((code, v["error"]["code"].as_str()), (5, Some("unconfirmed_property")), "{v}");
    let (code, v) = env.json(&["recipes", "write", "C3", "my-cc", "--yes", "--allow-unconfirmed"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["plan"]["slot"], "C3");
    assert_eq!(v["data"]["result"]["failed"], 0, "{v}");
    let (_, v) = env.json(&["recipes", "read", "C3", "--compare", "my-cc"]);
    assert_eq!((v["data"]["kind"].as_str(), v["data"]["item"]["changed"].as_array().map(Vec::len)), (Some("recipediff"), Some(0)), "{v}");
    let out = env.run(&["recipes", "read"]);
    assert!(text(&out).contains("C3  My CC = recipe:my-cc"), "{}", text(&out));
    let (code, _) = env.json(&["recipes", "rm", "my-cc"]);
    assert_eq!(code, 0);
    let pid = env.pid();
    env.finish(pid);
    // Tether mode: a slot read is a typed error.
    let env = Env::new();
    let (code, v) = env.json(&["recipes", "read", "C1"]);
    assert_eq!((code, v["error"]["code"].as_str(), v["error"]["walkthrough"].as_str()), (4, Some("needs_rawconv"), Some("ukiyo help modes rawconv")), "{v}");
    env.finish(env.pid());
}

#[test]
fn serve_unavailable_is_exit_1() {
    let env = Env::new();
    // Another process holds owner.lock with no socket: what a tools/ptp script looks like.
    let held = OwnerLock::try_acquire(&paths::lock_path(env.data.path())).unwrap();
    let (code, v) = env.json(&["status"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(v["error"]["code"], "unavailable");
    assert!(v["error"]["message"].as_str().unwrap().contains("owner.lock is held"), "{v}");
    drop(held);
    assert!(!paths::socket_path(env.data.path()).exists(), "the CLI never started an owner");
}

#[test]
fn watch_streams_until_ctrl_c() {
    let env = Env::new();
    let mut child = env.cmd(&["camera", "watch", "--json", "--quiet"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(first["event"], "status");
    assert_eq!(first["data"]["link"], "connected");
    let pid = first["data"]["pid"].as_u64().unwrap() as u32;
    // The fake camera's noise and nudges arrive as change groups within a few seconds.
    let change: Value = loop {
        let v: Value = serde_json::from_str(&lines.next().expect("a change group").unwrap()).unwrap();
        if v["event"] == "changes" {
            break v;
        }
    };
    let c = &change["data"]["changes"][0];
    assert!(c["code"].as_str().unwrap().starts_with("0x"));
    assert!(change["data"]["source"].is_string());
    // SAFETY: a signal to our own child.
    unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0), "Ctrl-C is a clean stop");
    env.finish(pid);
}

#[test]
fn watch_for_a_while_with_a_code_filter() {
    let env = Env::new();
    let out = env.run(&["camera", "watch", "--json", "--for", "3s", "--code", "0x5011"]);
    assert_eq!(out.status.code(), Some(0));
    let lines: Vec<Value> = text(&out).lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines[0]["event"], "status");
    let changes: Vec<&Value> = lines.iter().filter(|l| l["event"] == "changes").collect();
    assert!(!changes.is_empty(), "the fake DateTime ticks every second");
    assert!(changes.iter().all(|g| g["data"]["changes"].as_array().unwrap().iter().all(|c| c["code"] == "0x5011")));
    env.finish(lines[0]["data"]["pid"].as_u64().unwrap() as u32);
}

#[test]
fn local_commands_need_no_owner() {
    let env = Env::new();
    let (code, v) = env.json(&["catalog", "show", "AutoPowerOff", "--model", "X-T5"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["code"], "0xd381");
    assert_eq!(v["data"]["entry"]["values"]["3"], "OFF");
    // One catalog in the folder and no camera: that one.
    let (code, v) = env.json(&["catalog", "show"]);
    assert_eq!(code, 0, "{v}");
    assert!(v["data"]["properties"]["0xd381"].is_object());
    let (code, v) = env.json(&["catalog", "show", "0x0001"]);
    assert_eq!((code, v["error"]["code"].as_str()), (2, Some("not_found")));

    let (code, v) = env.json(&["log", "show"]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["entries"], serde_json::json!([]));
    let (code, v) = env.json(&["log", "show", "--since", "whenever"]);
    assert_eq!((code, v["error"]["code"].as_str()), (2, Some("usage")));

    let (code, v) = env.json(&["help", "modes", "rawconv"]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["modes"][0]["name"], "USB RAW CONV./BACKUP RESTORE");
    let (code, v) = env.json(&["help", "modes", "cardreader"]);
    assert_eq!(code, 2, "{v}");
    let (code, v) = env.json(&["bogus"]);
    assert_eq!((code, v["error"]["code"].as_str()), (2, Some("usage")));

    assert!(!paths::socket_path(env.data.path()).exists(), "nothing here started serve");
    assert!(!paths::serve_log_path(env.data.path()).exists(), "nothing here started serve");
}

#[test]
fn log_show_reads_the_session() {
    let env = Env::new();
    let pid = env.pid();
    env.finish(pid);
    let (code, v) = env.json(&["log", "show"]);
    assert_eq!(code, 0);
    let kinds: Vec<&str> = v["data"]["entries"].as_array().unwrap().iter().filter_map(|e| e["kind"].as_str()).collect();
    assert_eq!(kinds.first(), Some(&"start"));
    assert_eq!(kinds.last(), Some(&"stop"));
    let o = env.run(&["log", "show"]);
    assert!(text(&o).contains("clean stop"), "{}", text(&o));
}

/// A plan, then its apply: `--dry-run`, `--yes`, no terminal, `plan show|apply|discard`.
#[test]
fn settings_set_goes_through_a_plan() {
    let env = Env::new();
    // A known start: AUTO POWER OFF at 5 MIN.
    let (code, v) = env.json(&["settings", "set", "AutoPowerOff=5 MIN", "--yes"]);
    assert!(code == 0, "{v}");
    let pid = env.pid();

    let (code, v) = env.json(&["settings", "set", "AUTO POWER OFF=OFF", "--dry-run"]);
    assert_eq!(code, 6, "a dry run is not applied: {v}");
    let plan = &v["data"]["plan"];
    let id = plan["plan_id"].as_str().unwrap().to_string();
    assert_eq!(plan["steps"][0]["before"], "5 MIN");
    assert_eq!(plan["steps"][0]["after"], "OFF");
    assert_eq!(plan["requires"]["mode"], "tether");
    assert!(v["data"]["result"].is_null());

    let (code, v) = env.json(&["plan", "show", &id]);
    assert_eq!((code, v["data"]["plan_id"].as_str()), (0, Some(id.as_str())));
    // No terminal and no --yes: the plan is shown, not applied.
    let (code, _) = env.json(&["plan", "apply", &id]);
    assert_eq!(code, 6);
    let o = env.run(&["plan", "apply", &id]);
    assert_eq!(o.status.code(), Some(6));
    assert!(text(&o).contains("AutoPowerOff  5 MIN -> OFF"), "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stderr).contains("no terminal"));

    let (code, v) = env.json(&["plan", "apply", &id, "--yes"]);
    assert_eq!(code, 0, "{v}");
    let r = &v["data"]["result"];
    assert_eq!((r["applied"].as_u64(), r["failed"].as_u64()), (Some(1), Some(0)));
    assert_eq!(r["steps"][0]["readback"], "OFF");
    let undo = r["undo_plan_id"].as_str().unwrap().to_string();
    let (code, v) = env.json(&["plan", "apply", &id, "--yes"]);
    assert_eq!((code, v["error"]["code"].as_str()), (6, Some("plan_expired")));

    let o = env.run(&["plan", "apply", &undo, "--yes"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o).contains("AutoPowerOff  5 MIN  read back 5 MIN  OK"), "{}", text(&o));
    assert!(text(&o).contains("Applied 1 of 1. Undo: ukiyo plan apply p-"), "{}", text(&o));

    // Already at the value: nothing to change, exit 0.
    let o = env.run(&["settings", "set", "AutoPowerOff=5 MIN", "--yes"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o).contains("Nothing to change."));
    let (code, v) = env.json(&["settings", "set", "AutoPowerOff=OFF", "--dry-run"]);
    assert_eq!(code, 6);
    let id = v["data"]["plan"]["plan_id"].as_str().unwrap().to_string();
    assert_eq!(env.json(&["plan", "discard", &id]).0, 0);
    assert_eq!(env.json(&["plan", "show", &id]).1["error"]["code"], "plan_expired");

    // Lookups and gates, with their exit codes.
    let (code, v) = env.json(&["settings", "set", "NoSuchThing=1"]);
    assert_eq!((code, v["error"]["code"].as_str()), (2, Some("not_found")));
    let (code, v) = env.json(&["settings", "set", "AutoPowerOff=SOMETIMES"]);
    assert_eq!((code, v["error"]["code"].as_str()), (2, Some("not_found")));
    assert!(v["error"]["hint"].as_str().unwrap().contains("OFF (3)"));
    let (code, v) = env.json(&["settings", "set", "nonsense"]);
    assert_eq!((code, v["error"]["code"].as_str()), (2, Some("usage")));
    let (code, v) = env.json(&["settings", "set", "GrainEffect=WEAK SMALL", "--yes"]);
    assert_eq!((code, v["error"]["code"].as_str()), (5, Some("unconfirmed_property")), "{v}");
    let (code, v) = env.json(&["settings", "set", "GrainEffect=WEAK SMALL", "--yes", "--allow-unconfirmed"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["plan"]["steps"][0]["gate"], "unconfirmed_allowed");

    // Every write is in the session log with its actor and plan.
    let (_, log) = env.json(&["log", "show"]);
    let writes: Vec<&Value> = log["data"]["entries"].as_array().unwrap().iter().filter(|e| e["kind"] == "write").collect();
    assert_eq!(writes.len(), 4);
    assert!(writes.iter().all(|w| w["actor"] == "cli" && w["plan"].as_str().is_some_and(|p| p.starts_with("p-"))));
    assert!(text(&env.run(&["log", "show"])).contains("[cli] 0xd381"));

    // `settings QUERY` still reads; `set` is the only word taken.
    assert!(text(&env.run(&["settings", "auto", "power"])).contains("AutoPowerOff"));
    env.finish(pid);
}

#[test]
fn settings_set_refuses_never_write_codes() {
    let env = Env::new();
    let pid = env.pid();
    for item in ["HardwareControl=FORMAT SLOT 1", "0xd17b=1", "CaptureFunction=S1 ON"] {
        let (code, v) = env.json(&["settings", "set", item, "--yes", "--allow-unconfirmed"]);
        assert_eq!((code, v["error"]["code"].as_str()), (5, Some("never_write")), "{item}: {v}");
        assert!(v["error"]["message"].as_str().unwrap().contains("never_write"), "{v}");
    }
    let (_, log) = env.json(&["log", "show"]);
    assert!(log["data"]["entries"].as_array().unwrap().iter().all(|e| e["kind"] != "write"), "nothing written: {log}");
    env.finish(pid);
}

#[test]
fn owner_strings_are_gated() {
    let env = Env::new();
    let (code, v) = env.json(&["owner", "get"]);
    assert_eq!(code, 0, "{v}");
    let fields = v["data"]["fields"].as_array().unwrap();
    assert_eq!(fields.iter().map(|f| f["field"].as_str().unwrap()).collect::<Vec<_>>(), ["copyright", "author", "prefix", "device-name"]);
    assert_eq!(fields[0]["code"], "0xd215");
    let (code, v) = env.json(&["owner", "set", "copyright=A. Person", "--yes"]);
    assert_eq!((code, v["error"]["code"].as_str()), (5, Some("unconfirmed_property")));
    let (code, v) = env.json(&["owner", "set", "copyright=A. Person", "--yes", "--allow-unconfirmed"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["result"]["steps"][0]["readback"], "A. Person");
    let (_, v) = env.json(&["owner", "get"]);
    assert_eq!(v["data"]["fields"][0]["value"], "A. Person");
    let (code, v) = env.json(&["owner", "set", "nickname=x"]);
    assert_eq!((code, v["error"]["code"].as_str()), (2, Some("usage")));
    env.finish(env.pid());
}

#[test]
fn connect_disconnect_export_and_notes() {
    let env = Env::new();
    // No serve yet: a note has no session to go into, and nothing is started for it.
    let (code, v) = env.json(&["log", "note", "hello"]);
    assert_eq!((code, v["error"]["code"].as_str()), (1, Some("unavailable")));
    let (code, v) = env.json(&["camera", "disconnect"]);
    assert_eq!((code, v["data"]["serve"].as_bool()), (0, Some(false)));
    assert!(!paths::socket_path(env.data.path()).exists(), "neither started serve");

    let pid = env.pid();
    let (code, v) = env.json(&["camera", "export"]);
    assert_eq!(code, 0, "{v}");
    assert!(Path::new(v["data"]["path"].as_str().unwrap()).is_file());
    assert_eq!(env.json(&["log", "note", "turned the dial", "--group", "2"]).0, 0);

    let (code, _) = env.json(&["camera", "disconnect"]);
    assert_eq!(code, 0);
    let (_, s) = env.json(&["status"]);
    assert_eq!(s["data"]["link"], "stopped");
    let (code, v) = env.json(&["settings", "set", "AutoPowerOff=OFF", "--yes"]);
    assert_eq!((code, v["error"]["code"].as_str()), (3, Some("not_connected")));
    assert_eq!(env.json(&["camera", "connect"]).0, 0);
    wait_until("reconnected", 15, || env.json(&["status"]).1["data"]["link"] == "connected");

    let (_, log) = env.json(&["log", "show"]);
    let e = log["data"]["entries"].as_array().unwrap();
    assert!(e.iter().any(|l| l["kind"] == "note" && l["actor"] == "cli" && l["group"] == 2));
    assert!(e.iter().any(|l| l["kind"] == "export" && l["actor"] == "cli"));
    assert!(e.iter().any(|l| l["kind"] == "connect" && l["on"] == false));
    env.finish(pid);
}
