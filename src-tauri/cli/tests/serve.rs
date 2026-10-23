//! `ukiyo-cli serve` as real processes, on the fake camera, in temp folders.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ukiyo_core::api::{Request, Response};
use ukiyo_core::camera::owner::Link;
use ukiyo_core::paths;
use ukiyo_core::serve::client::{Client, Options};
use ukiyo_core::serve::lock;

const EXE: &str = env!("CARGO_BIN_EXE_ukiyo-cli");

/// One spawn at a time in this process. On macOS, std makes a child's pipes with `pipe()` and
/// then sets `FD_CLOEXEC`; a spawn on another thread in that window hands the pipe ends to
/// its child, which then holds this child's stderr open until it exits.
static SPAWN: Mutex<()> = Mutex::new(());

fn serve(data: &Path, cats: &Path, idle_s: &str) -> Child {
    let _one = SPAWN.lock().unwrap();
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
    // A pipe whose write end lacks close-on-exec while serve is spawned: what std's `pipe()`
    // leaves for an instant on macOS. Both ends are marked close-on-exec at once so no other
    // spawn in this process gets them; the write end is unmarked only under the spawn lock.
    let mut fds = [0i32; 2];
    {
        let _one = SPAWN.lock().unwrap();
        // SAFETY: a pipe of our own, and fcntl on its two ends.
        unsafe {
            assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
            libc::fcntl(fds[0], libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(fds[1], libc::F_SETFD, libc::FD_CLOEXEC);
        }
    }
    let (r, w) = (fds[0], fds[1]);
    let spawn: ukiyo_core::serve::client::SpawnFn = Arc::new(move || {
        let _one = SPAWN.lock().unwrap();
        // SAFETY: fcntl on our own fd.
        unsafe { libc::fcntl(w, libc::F_SETFD, 0) };
        let out = ukiyo_core::serve::spawn_detached(Path::new(EXE), &dir);
        // SAFETY: as above.
        unsafe { libc::fcntl(w, libc::F_SETFD, libc::FD_CLOEXEC) };
        out
    });
    let c = Client::new(Options::new(data.path().into(), "cli", Some(spawn)), None);
    wait_until("spawned serve answers", 15, || connected(&c));
    assert_eq!(c.spawns(), 1);
    let pid = c.welcome().unwrap().pid;
    assert_ne!(pid, std::process::id());
    // SAFETY: getsid only reads.
    assert_eq!(unsafe { libc::getsid(pid as i32) }, pid as i32, "serve leads its own session");
    // serve kept no copy of the write end: with ours closed, the read end is at its end.
    // SAFETY: our own pipe ends; a one-byte non-blocking read.
    let n = unsafe {
        libc::close(w);
        libc::fcntl(r, libc::F_SETFL, libc::O_NONBLOCK);
        let mut byte = 0u8;
        let n = libc::read(r, &mut byte as *mut u8 as *mut libc::c_void, 1);
        libc::close(r);
        n
    };
    assert_eq!(n, 0, "serve holds a pipe end its spawner left open");
    c.disconnect();
    wait_until("serve idles out", 15, || !alive(pid));
    assert!(lock::is_free(&paths::lock_path(data.path())));
    let log = std::fs::read_to_string(paths::serve_log_path(data.path())).unwrap();
    assert!(log.contains("stopped"), "{log}");
}

#[test]
fn usage_errors() {
    let run = |args: &[&str]| {
        let child = {
            let _one = SPAWN.lock().unwrap();
            Command::new(EXE).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
        };
        child.wait_with_output().unwrap()
    };
    assert_eq!(run(&["bogus"]).status.code(), Some(2));
    assert_eq!(run(&["serve", "--idle-exit", "x"]).status.code(), Some(2));
}
