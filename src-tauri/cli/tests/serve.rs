//! `ukiyo-cli serve` as real processes, on the fake camera, in temp folders.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ukiyo_core::api::{Request, Response};
use ukiyo_core::camera::owner::Link;
use ukiyo_core::paths;
use ukiyo_core::serve::client::{Client, Options};
use ukiyo_core::serve::lock;

const EXE: &str = env!("CARGO_BIN_EXE_ukiyo-cli");

fn serve(data: &Path, cats: &Path, idle_s: &str) -> Child {
    Command::new(EXE)
        .args(["serve", "--idle-exit", idle_s])
        .env("UKIYO_DATA_DIR", data)
        .env("UKIYO_CATALOG_DIR", cats)
        .env("UKIYO_FAKE_CAMERA", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn client(data: &Path) -> Arc<Client> {
    Client::new(Options::new(data.into(), "cli", None), None)
}

fn connected(c: &Client) -> bool {
    matches!(c.call(Request::LabSnapshot), Ok(Response::LabSnapshot(s)) if s.status.link == Link::Connected && !s.rows.is_empty())
}

fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn stderr(c: Child) -> (i32, String) {
    let out = c.wait_with_output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn two_serve_processes_one_owner() {
    let data = tempfile::tempdir().unwrap();
    let cats = tempfile::tempdir().unwrap();
    let a = serve(data.path(), cats.path(), "1");
    let b = serve(data.path(), cats.path(), "1");
    let c = client(data.path());
    wait_until("one serve answers", 15, || connected(&c));
    let owner = c.welcome().unwrap().pid;
    let (winner, loser) = if owner == a.id() { (a, b) } else { (b, a) };
    assert_eq!(owner, winner.id());
    let (code, err) = stderr(loser);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("another process owns the camera"), "{err}");

    c.close();
    let (code, err) = stderr(winner);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("no clients for 1 s"), "{err}");
    assert!(err.contains("stopped"), "{err}");
    assert!(lock::is_free(&paths::lock_path(data.path())));
    assert!(!paths::socket_path(data.path()).exists());
}

#[test]
fn sigterm_is_a_clean_stop() {
    let data = tempfile::tempdir().unwrap();
    let cats = tempfile::tempdir().unwrap();
    let s = serve(data.path(), cats.path(), "120");
    let c = client(data.path());
    wait_until("serve answers", 15, || connected(&c));
    // SAFETY: a signal to our own child.
    unsafe { libc::kill(s.id() as i32, libc::SIGTERM) };
    let (code, err) = stderr(s);
    assert_eq!(code, 0, "{err}");
    assert!(lock::is_free(&paths::lock_path(data.path())));
    // The Lab closed its session before the lock went: the session log ends clean.
    let sessions: Vec<_> = std::fs::read_dir(data.path().join("sessions")).unwrap().flatten().collect();
    let log = std::fs::read_to_string(sessions[0].path()).unwrap();
    assert!(log.lines().last().unwrap().contains(r#""clean":true"#), "{log}");
    // The client's next call fails instead of hanging; no spawn function, no new owner.
    assert!(c.call(Request::LabSnapshot).is_err());
}

#[test]
fn spawn_on_demand_detaches_and_idles_out() {
    let data = tempfile::tempdir().unwrap();
    let cats = tempfile::tempdir().unwrap();
    std::env::set_var("UKIYO_FAKE_CAMERA", "1");
    std::env::set_var("UKIYO_CATALOG_DIR", cats.path());
    std::env::set_var("UKIYO_IDLE_EXIT", "1");
    let dir = data.path().to_path_buf();
    let spawn: ukiyo_core::serve::client::SpawnFn = Arc::new(move || ukiyo_core::serve::spawn_detached(Path::new(EXE), &dir));
    let c = Client::new(Options::new(data.path().into(), "cli", Some(spawn)), None);
    wait_until("spawned serve answers", 15, || connected(&c));
    assert_eq!(c.spawns(), 1);
    let pid = c.welcome().unwrap().pid;
    assert_ne!(pid, std::process::id());
    // SAFETY: getsid only reads.
    assert_eq!(unsafe { libc::getsid(pid as i32) }, pid as i32, "serve leads its own session");
    c.disconnect();
    wait_until("serve idles out", 15, || !alive(pid));
    assert!(lock::is_free(&paths::lock_path(data.path())));
    let log = std::fs::read_to_string(paths::serve_log_path(data.path())).unwrap();
    assert!(log.contains("stopped"), "{log}");
}

#[test]
fn usage_errors() {
    let out = Command::new(EXE).arg("bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = Command::new(EXE).args(["serve", "--idle-exit", "x"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
