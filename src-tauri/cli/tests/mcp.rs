//! `ukiyo-cli mcp` as a real process on stdio, against the fake camera
//! (`UKIYO_FAKE_CAMERA=1`) in a temp data folder, so the `serve` it spawns never touches USB.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use ukiyo_core::paths;
use ukiyo_core::serve::lock;

const EXE: &str = env!("CARGO_BIN_EXE_ukiyo-cli");

/// One spawn at a time in this process. On macOS, std makes a child's pipes with `pipe()` and
/// then sets `FD_CLOEXEC`; a spawn on another thread in that window hands the pipe ends to
/// its child, and that child then holds the write end of this child's stdin, so the end of
/// input reaches this child only when the other one exits.
static SPAWN: Mutex<()> = Mutex::new(());

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
    data: tempfile::TempDir,
    cats: tempfile::TempDir,
}

impl Mcp {
    fn start() -> Mcp {
        Self::start_mode(None)
    }

    /// With `UKIYO_FAKE_MODE`, for the serve it spawns.
    fn start_mode(mode: Option<&str>) -> Mcp {
        let cats = tempfile::tempdir().unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/x-t5.json");
        // GrainEffect stands for an unconfirmed property, whatever the mapping has reached.
        let mut cat: Value = serde_json::from_str(&std::fs::read_to_string(repo).unwrap()).unwrap();
        cat["properties"]["0xd023"]["confidence"] = "likely".into();
        std::fs::write(cats.path().join("x-t5.json"), serde_json::to_string_pretty(&cat).unwrap()).unwrap();
        let data = tempfile::tempdir().unwrap();
        let spawn = SPAWN.lock().unwrap();
        let mut child = Command::new(EXE)
            .arg("mcp")
            // Never the real camera: the spawned serve inherits this.
            .env("UKIYO_FAKE_CAMERA", "1")
            .env("UKIYO_DATA_DIR", data.path())
            .env("UKIYO_CATALOG_DIR", cats.path())
            .env("UKIYO_IDLE_EXIT", "3")
            .env("UKIYO_MCP_IDLE", "2")
            .env("UKIYO_FAKE_MODE", mode.unwrap_or("tether"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        drop(spawn);
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut m = Mcp { child, stdin, stdout, next: 0, data, cats };
        let init = m.request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }));
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
        assert_eq!(init["result"]["serverInfo"]["name"], "ukiyo");
        assert!(init["result"]["instructions"].as_str().unwrap().contains("camera_plan"));
        m.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        m
    }

    fn send(&mut self, v: Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("stdout is MCP only: {line:?} ({e})"));
        assert_eq!(v["id"], id, "{v}");
        v
    }

    /// A tool call: `Ok(structuredContent)`, or `Err` with the typed error.
    fn tool(&mut self, name: &str, args: Value) -> Result<Value, Value> {
        let r = self.request("tools/call", json!({ "name": name, "arguments": args }));
        let res = &r["result"];
        assert!(res["content"][0]["text"].as_str().is_some_and(|t| !t.is_empty()), "{r}");
        if res["isError"] == true {
            Err(res["structuredContent"]["error"].clone())
        } else {
            Ok(res["structuredContent"].clone())
        }
    }

    fn ok(&mut self, name: &str, args: Value) -> Value {
        self.tool(name, args.clone()).unwrap_or_else(|e| panic!("{name} {args}: {e}"))
    }

    fn err(&mut self, name: &str, args: Value) -> Value {
        match self.tool(name, args.clone()) {
            Ok(v) => panic!("{name} {args} should fail: {v}"),
            Err(e) => e,
        }
    }

    /// Closes stdin and checks the server exits, then that `serve` idles out and leaves
    /// nothing behind.
    fn finish(mut self, serve_pid: u32) {
        drop(self.stdin);
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "mcp did not exit at end of input");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(self.child.wait().unwrap().code(), Some(0));
        wait_until("serve idles out", 30, || !alive(serve_pid));
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

#[test]
fn tools_on_the_fake_camera() {
    let mut m = Mcp::start();

    let list = m.request("tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["camera_status", "camera_watch", "settings_read", "camera_plan", "camera_apply", "library_search", "library_read", "library_save", "library_delete", "recipe_parse", "catalog_label", "fake_camera"]);
    let unknown = m.request("tools/call", json!({ "name": "lab_write", "arguments": {} }));
    assert_eq!(unknown["error"]["code"], -32602, "{unknown}");

    // camera_status
    let s = m.ok("camera_status", json!({}));
    assert_eq!((s["link"].as_str(), s["model"].as_str(), s["mode"].as_str(), s["fake"].as_bool()), (Some("connected"), Some("X-T5"), Some("tether"), Some(true)), "{s}");
    let serve_pid = s["pid"].as_u64().unwrap() as u32;
    let bad = m.err("camera_status", json!({ "verbose": true }));
    assert_eq!(bad["code"], "bad_request");
    let h = m.ok("camera_status", json!({ "history": true }));
    assert_eq!(h["history"]["model"], "X-T5", "{h}");
    assert_eq!(h["history"]["connects"].as_array().unwrap().len(), 1);

    // settings_read
    let r = m.ok("settings_read", json!({ "query": "auto power off" }));
    let row = r["rows"].as_array().unwrap().iter().find(|r| r["code"] == "0xd381").cloned().expect("AutoPowerOff");
    assert_eq!((row["name"].as_str(), row["confidence"].as_str()), (Some("AutoPowerOff"), Some("confirmed")));
    assert!(row["options"].as_array().unwrap().iter().any(|o| o["label"] == "OFF"));
    let r = m.ok("settings_read", json!({ "limit": 3 }));
    assert_eq!((r["rows"].as_array().unwrap().len(), r["has_more"].as_bool()), (3, Some(true)));
    let menus = m.ok("settings_read", json!({ "view": "menus" }));
    assert!(menus["menus"].as_array().unwrap().iter().any(|x| x["path"] == "SET UP"), "{menus}");

    // camera_plan + camera_apply
    let off = if row["value_label"] == "OFF" { "5 MIN" } else { "OFF" };
    let plan = m.ok("camera_plan", json!({ "set": { "AUTO POWER OFF": off } }));
    let id = plan["plan_id"].as_str().unwrap().to_string();
    assert_eq!(plan["steps"][0]["after"], off, "{plan}");
    let cursor = m.ok("camera_watch", json!({ "wait_s": 0 }))["cursor"].as_u64().unwrap();
    let done = m.ok("camera_apply", json!({ "plan_id": id }));
    assert_eq!((done["applied"].as_u64(), done["failed"].as_u64()), (Some(1), Some(0)), "{done}");
    assert_eq!(done["steps"][0]["readback"], off);
    assert!(done["undo_plan_id"].is_string());
    assert_eq!(m.err("camera_apply", json!({ "plan_id": id }))["code"], "plan_expired", "a plan applies once");
    let w = m.ok("camera_watch", json!({ "since": cursor, "codes": ["0xd381"], "wait_s": 5 }));
    assert_eq!(w["groups"][0]["source"], "write", "{w}");
    assert_eq!(w["groups"][0]["changes"][0]["new_label"], off);

    // An unconfirmed property is refused without allow_unconfirmed.
    let e = m.err("camera_plan", json!({ "set": { "GrainEffect": "2" } }));
    assert_eq!(e["code"], "unconfirmed_property", "{e}");
    let e = m.err("camera_plan", json!({ "set": { "NoSuchSetting": "2" } }));
    assert_eq!(e["code"], "not_found", "{e}");

    // catalog_label: the evidence rule, private text, and a label that passes.
    assert_eq!(m.err("catalog_label", json!({ "code": "0xd20b", "confidence": "confirmed" }))["code"], "no_evidence");
    let plan = m.ok("camera_plan", json!({ "set": { "MemoText": "studio-cam42" }, "allow_unconfirmed": true }));
    let done = m.ok("camera_apply", json!({ "plan_id": plan["plan_id"] }));
    assert_eq!(done["applied"], 1, "{done}");
    assert_eq!(m.err("catalog_label", json!({ "code": "0xd20b", "notes": "set to Studio-Cam42" }))["code"], "private_text");
    let l = m.ok("catalog_label", json!({ "code": "0xd20b", "menu": "MEMO", "confidence": "confirmed" }));
    assert_eq!((l["code"].as_str(), l["entry"]["confidence"].as_str(), l["entry"]["name"].as_str()), (Some("0xd20b"), Some("confirmed"), Some("MemoText")), "{l}");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(m.cats.path().join("x-t5.json")).unwrap()).unwrap();
    assert_eq!(saved["properties"]["0xd20b"]["menu"], "MEMO");

    // The library: a snapshot, a preset, favourites, prefs, the trash, restore as a plan.
    let snap = m.ok("library_save", json!({ "kind": "snapshot", "name": "Before trip" }));
    assert_eq!((snap["id"].as_str(), snap["replaced"].as_bool()), (Some("snapshot:before-trip"), Some(false)), "{snap}");
    let found = m.ok("library_search", json!({ "kind": "snapshot", "query": "trip" }));
    assert_eq!(found["total"], 1, "{found}");
    let r = m.ok("library_read", json!({ "id": "snapshot:before-trip" }));
    assert_eq!((r["kind"].as_str(), r["item"]["model"].as_str()), (Some("snapshot"), Some("X-T5")), "{r}");
    let p = m.ok("library_save", json!({ "kind": "preset", "name": "Power", "item": { "set": { "AutoPowerOff": "OFF" } } }));
    assert_eq!(p["id"], "preset:power");
    let plan = m.ok("camera_plan", json!({ "preset": "preset:power" }));
    assert!(plan["steps"].as_array().unwrap().len() + plan["skipped"].as_array().unwrap().len() == 1, "{plan}");
    let e = m.err("camera_plan", json!({ "preset": "power", "set": { "AutoPowerOff": "OFF" } }));
    assert_eq!(e["code"], "bad_request");
    let e = m.err("camera_plan", json!({ "snapshot": "snapshot:nope" }));
    assert_eq!(e["code"], "not_found", "{e}");
    // The restore plans GrainEffect back only with allow_unconfirmed; AutoPowerOff is confirmed.
    let restore = m.tool("camera_plan", json!({ "snapshot": "snapshot:before-trip" }));
    let restore = restore.unwrap_or_else(|e| e);
    assert!(restore.to_string().contains("0xd381") || restore["code"].is_string(), "{restore}");
    m.ok("library_save", json!({ "kind": "favourite", "name": "AUTO POWER OFF" }));
    let favs = m.ok("settings_read", json!({ "filter": "favourites" }));
    assert_eq!(favs["rows"][0]["code"], "0xd381", "{favs}");
    m.ok("library_save", json!({ "kind": "prefs", "item": { "theme": "dark", "brand_mark": "clouds" } }));
    let prefs = m.ok("library_read", json!({ "id": "prefs" }));
    assert_eq!((prefs["item"]["theme"].as_str(), prefs["item"]["brand_mark"].as_str()), (Some("dark"), Some("clouds")));
    assert_eq!(m.err("library_save", json!({ "kind": "prefs", "item": { "app_icon": "nope" } }))["code"], "not_found");
    let t = m.ok("library_delete", json!({ "id": "preset:power" }));
    let tid = t["trash_id"].as_str().unwrap().to_string();
    assert_eq!(m.ok("library_search", json!({ "kind": "trash" }))["total"], 1);
    assert_eq!(m.ok("library_save", json!({ "kind": "preset", "from": tid }))["id"], "preset:power");
    assert_eq!(m.err("library_delete", json!({ "id": "prefs" }))["code"], "read_only");
    m.ok("library_save", json!({ "kind": "note", "item": { "text": "library test" } }));
    let log = std::fs::read_to_string(m.data.path().join("library/log.jsonl")).unwrap();
    assert!(log.lines().all(|l| l.contains("\"actor\":\"mcp:test\"")), "{log}");

    // Camera screens and the STILL/MOVIE switch show as notices, and clear on their own.
    fn ids(m: &mut Mcp) -> Vec<String> {
        m.ok("camera_status", json!({}))["notices"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap().to_string()).collect()
    }
    assert!(ids(&mut m).is_empty());
    m.ok("fake_camera", json!({ "action": "playback" }));
    wait_until("the playback notice", 10, || ids(&mut m) == ["playback"]);
    m.ok("fake_camera", json!({ "action": "closeMenu" }));
    wait_until("no notice after playback", 10, || ids(&mut m).is_empty());
    m.ok("fake_camera", json!({ "action": "openMenu" }));
    wait_until("the menu notice", 10, || ids(&mut m) == ["menu"]);
    m.ok("fake_camera", json!({ "action": "closeMenu" }));
    wait_until("no notice after the menu", 10, || ids(&mut m).is_empty());

    // fake_camera, then a needs_* error with its walkthrough.
    m.ok("fake_camera", json!({ "action": "movie" }));
    wait_until("movie mode", 10, || m.ok("camera_status", json!({}))["capture"] == "movie");
    assert_eq!(ids(&mut m), ["movie"]);
    m.ok("fake_camera", json!({ "action": "unplug" }));
    wait_until("unplugged", 10, || m.ok("camera_status", json!({}))["link"] != "connected");
    let e = m.err("settings_read", json!({}));
    assert_eq!(e["code"], "not_connected", "{e}");
    assert!(e["hint"].is_string());

    // The session log names the MCP client on every write.
    let log = std::fs::read_dir(m.data.path().join("sessions")).unwrap().flatten().map(|e| std::fs::read_to_string(e.path()).unwrap()).collect::<String>();
    let writes: Vec<Value> = log.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|v| v["kind"] == "write" || v["kind"] == "label").collect();
    assert!(writes.len() >= 3, "{log}");
    assert!(writes.iter().all(|w| w["actor"] == "mcp:test"), "{writes:?}");

    m.finish(serve_pid);
}

#[test]
fn camera_plan_refuses_never_write_codes() {
    let mut m = Mcp::start();
    let serve_pid = m.ok("camera_status", json!({}))["pid"].as_u64().unwrap() as u32;
    for set in [json!({ "HardwareControl": "FORMAT SLOT 2" }), json!({ "PowerControl": "SAVE AND POWER OFF" }), json!({ "0xd208": "768" })] {
        let e = m.err("camera_plan", json!({ "set": set, "allow_unconfirmed": true }));
        assert_eq!(e["code"], "never_write", "{set}: {e}");
    }
    m.finish(serve_pid);
}

/// The connection drops after `UKIYO_MCP_IDLE`, so `serve` can idle out while the agent
/// session stays open; the next call starts a new one.
#[test]
fn idle_connection_lets_serve_go() {
    let mut m = Mcp::start();
    let first = m.ok("camera_status", json!({}))["pid"].as_u64().unwrap() as u32;
    wait_until("serve idles out", 30, || !alive(first));
    let s = m.ok("camera_status", json!({}));
    assert_eq!(s["link"], "connected", "{s}");
    let second = s["pid"].as_u64().unwrap() as u32;
    assert_ne!(first, second);
    m.finish(second);
}

#[test]
fn recipe_tools_parse_save_and_write_a_slot() {
    let mut m = Mcp::start_mode(Some("rawconv"));
    let p = m.ok("recipe_parse", json!({ "text": "Film Simulation: Acros + R\nGrain Effect: Strong, Small\nMonochromatic Color: WC +2, MG -1\nHighlight: -1" }));
    assert_eq!(p["settings"]["filmSimulation"], "Acros + R filter", "{p}");
    assert_eq!(m.err("recipe_parse", json!({ "text": "x", "url": "https://example.org" }))["code"], "bad_request");
    let saved = m.ok("library_save", json!({ "kind": "recipe", "name": "Agent Acros", "item": { "settings": p["settings"] } }));
    assert_eq!(saved["id"], "recipe:agent-acros", "{saved}");
    let found = m.ok("library_search", json!({ "kind": "recipe", "film_sim": "acros" }));
    assert_eq!(found["total"], 1, "{found}");
    let cat = m.ok("library_search", json!({ "kind": "catalogue", "query": "chrome", "limit": 2 }));
    assert_eq!(cat["items"].as_array().unwrap().len(), 2);
    let slots = m.ok("library_read", json!({ "id": "slots" }));
    assert_eq!((slots["kind"].as_str(), slots["item"].as_array().map(Vec::len)), (Some("slots"), Some(7)), "{slots}");
    assert_eq!(m.err("camera_plan", json!({ "recipe": "recipe:agent-acros" }))["code"], "bad_request", "a recipe plan names its slot");
    let plan = m.ok("camera_plan", json!({ "recipe": "agent-acros", "slot": "C5", "allow_unconfirmed": true }));
    assert_eq!(plan["slot"], "C5", "{plan}");
    let done = m.ok("camera_apply", json!({ "plan_id": plan["plan_id"] }));
    assert_eq!(done["failed"], 0, "{done}");
    let c5 = m.ok("library_read", json!({ "id": "slot:C5", "compare_to": "recipe:agent-acros" }));
    assert_eq!(c5["item"]["changed"].as_array().map(Vec::len), Some(0), "{c5}");
    let s = m.ok("camera_status", json!({}));
    m.finish(s["pid"].as_u64().unwrap() as u32);
}
