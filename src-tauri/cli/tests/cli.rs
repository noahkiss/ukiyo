//! The read-only CLI as real processes. Every run uses the fake camera (`UKIYO_FAKE_CAMERA=1`)
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
}

impl Env {
    fn new() -> Env {
        let cats = tempfile::tempdir().unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/x-t5.json");
        std::fs::copy(repo, cats.path().join("x-t5.json")).unwrap();
        Env { data: tempfile::tempdir().unwrap(), cats }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(EXE);
        c.args(args)
            // Never the real camera: the spawned serve inherits this.
            .env("UKIYO_FAKE_CAMERA", "1")
            .env("UKIYO_DATA_DIR", self.data.path())
            .env("UKIYO_CATALOG_DIR", self.cats.path())
            .env("UKIYO_IDLE_EXIT", "3")
            .env_remove("UKIYO_FAKE_MODE")
            .stdin(Stdio::null());
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
        wait_until("serve idles out", 20, || !alive(pid));
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
    assert_eq!((code, v["error"]["code"].as_str()), (1, Some("not_built")));

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
    let env = Env::new();
    let mut cmd = env.cmd(&["settings", "--json"]);
    let out = cmd.env("UKIYO_FAKE_MODE", "rawconv").output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(4), "{v}");
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
