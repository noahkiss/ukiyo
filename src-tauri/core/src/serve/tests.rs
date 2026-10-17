//! The shared owner on the fake camera, over a socket in a temp folder.

use super::client::{Client, Options as ClientOptions, SpawnFn};
use super::server::{Options, Server, StartError};
use super::*;
use crate::api::{ErrorCode, Event, Request, Response};
use crate::camera::owner::Link;
use crate::camera::ptp::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Dirs {
    data: tempfile::TempDir,
    cats: tempfile::TempDir,
}

fn dirs() -> Dirs {
    std::env::set_var("UKIYO_FAKE_CAMERA", "1");
    Dirs { data: tempfile::tempdir().unwrap(), cats: tempfile::tempdir().unwrap() }
}

fn opts(d: &Dirs, idle: Duration) -> Options {
    let mut o = Options::new(d.data.path().into(), d.cats.path().into());
    o.idle_exit = idle;
    o
}

/// Runs a server on a thread until it idles out. Returns whether it started.
fn run_in_thread(o: Options) -> std::thread::JoinHandle<bool> {
    std::thread::spawn(move || match Server::start(o) {
        Ok(s) => {
            s.wait();
            s.shutdown();
            true
        }
        Err(StartError::Owned) => false,
        Err(e) => panic!("{e}"),
    })
}

/// A spawn function that starts an in-process server and counts its calls.
fn spawner(data: PathBuf, cats: PathBuf, idle: Duration, count: Arc<AtomicUsize>, runs: Arc<Mutex<Vec<std::thread::JoinHandle<bool>>>>) -> SpawnFn {
    Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
        let mut o = Options::new(data.clone(), cats.clone());
        o.idle_exit = idle;
        runs.lock().unwrap().push(run_in_thread(o));
        Ok(())
    })
}

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn connected(c: &Client) -> bool {
    matches!(c.call(Request::LabSnapshot), Ok(Response::LabSnapshot(s)) if s.status.link == Link::Connected && !s.rows.is_empty())
}

fn no_spawn_client(data: &Path, actor: &str) -> Arc<Client> {
    let mut o = ClientOptions::new(data.into(), actor, None);
    o.wait = Duration::from_secs(5);
    Client::new(o, None)
}

#[test]
fn two_serves_one_wins() {
    let d = dirs();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let starts: Vec<_> = (0..2)
        .map(|_| {
            let o = opts(&d, Duration::from_millis(300));
            let b = barrier.clone();
            std::thread::spawn(move || {
                b.wait();
                match Server::start(o) {
                    Ok(s) => Some(s),
                    Err(StartError::Owned) => None,
                    Err(e) => panic!("{e}"),
                }
            })
        })
        .collect();
    let mut won: Vec<Server> = starts.into_iter().filter_map(|t| t.join().unwrap()).collect();
    assert_eq!(won.len(), 1, "exactly one serve owns the camera");
    let s = won.pop().unwrap();
    // A third, later, also loses, and leaves the winner's socket alone.
    assert!(matches!(Server::start(opts(&d, Duration::from_secs(1))), Err(StartError::Owned)));
    let c = no_spawn_client(d.data.path(), "cli");
    wait_until("fake camera connects through the socket", 10, || connected(&c));
    c.close();
    s.wait();
    s.shutdown();
    assert!(lock::is_free(&crate::paths::lock_path(d.data.path())), "idle exit releases the lock");
    assert!(!crate::paths::socket_path(d.data.path()).exists(), "idle exit removes the socket");
}

#[test]
fn requests_and_errors_over_the_socket() {
    let d = dirs();
    let s = Server::start(opts(&d, Duration::from_secs(30))).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(crate::paths::socket_path(d.data.path())).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let c = no_spawn_client(d.data.path(), "cli");
    assert!(c.welcome().is_none());
    wait_until("connected", 10, || connected(&c));
    let w = c.welcome().unwrap();
    assert!(w.fake);
    assert_eq!((w.protocol, w.pid), (wire::PROTOCOL, std::process::id()));
    let Response::LabWrite(out) = c.call(Request::LabWrite { code: 0xD023, value: Value::Int(2) }).unwrap() else { panic!() };
    assert!(out.ok, "{out:?}");
    assert!(matches!(c.call(Request::LabPin { codes: vec![0xD023] }).unwrap(), Response::LabPin));
    let Response::LabExport(path) = c.call(Request::LabExport).unwrap() else { panic!() };
    assert!(Path::new(&path).is_file());
    assert_eq!(s.clients(), 1);

    // A raw line that is not a request gets a typed error, and the connection stays usable.
    use std::io::{BufRead, Write};
    let mut raw = std::os::unix::net::UnixStream::connect(crate::paths::socket_path(d.data.path())).unwrap();
    writeln!(raw, r#"{{"jsonrpc":"2.0","id":9,"method":"lab_reset_usb"}}"#).unwrap();
    let mut line = String::new();
    std::io::BufReader::new(raw.try_clone().unwrap()).read_line(&mut line).unwrap();
    let wire::Outgoing::Reply(9, Err(e)) = wire::parse_outgoing(&line) else { panic!("{line}") };
    assert_eq!(e.code, ErrorCode::BadRequest);
    drop(raw);

    // A wrong protocol is refused at hello.
    let mut raw = std::os::unix::net::UnixStream::connect(crate::paths::socket_path(d.data.path())).unwrap();
    writeln!(raw, "{}", wire::request_line(1, wire::HELLO, Some(serde_json::json!({ "actor": "x", "protocol": 999 })))).unwrap();
    let mut line = String::new();
    std::io::BufReader::new(raw).read_line(&mut line).unwrap();
    assert!(matches!(wire::parse_outgoing(&line), wire::Outgoing::Reply(1, Err(_))));

    c.close();
    s.stopper().stop();
    s.wait();
    s.shutdown();
}

#[test]
fn events_reach_every_subscriber() {
    let d = dirs();
    let s = Server::start(opts(&d, Duration::from_secs(30))).unwrap();
    let seen: Vec<Arc<Mutex<Vec<&'static str>>>> = (0..2).map(|_| Arc::new(Mutex::new(Vec::new()))).collect();
    let clients: Vec<_> = seen
        .iter()
        .map(|log| {
            let log = log.clone();
            let mut o = ClientOptions::new(d.data.path().into(), "app", None);
            o.wait = Duration::from_secs(5);
            let on: client::EventFn = Arc::new(move |e: Event| {
                log.lock().unwrap().push(match e {
                    Event::Status(_) => "status",
                    Event::Props(_) => "props",
                    Event::Changes(_) => "changes",
                    Event::Catalog(_) => "catalog",
                })
            });
            let c = Client::new(o, Some(on));
            c.keep_connected();
            c
        })
        .collect();
    wait_until("both connect", 5, || s.clients() == 2);
    // A write from a third client (no subscription) shows up in both subscribers' feeds.
    let cli = no_spawn_client(d.data.path(), "cli");
    wait_until("connected", 10, || connected(&cli));
    for log in &seen {
        log.lock().unwrap().clear();
    }
    let Response::LabWrite(out) = cli.call(Request::LabWrite { code: 0xD023, value: Value::Int(3) }).unwrap() else { panic!() };
    assert!(out.ok, "{out:?}");
    for (i, log) in seen.iter().enumerate() {
        wait_until(&format!("subscriber {i} hears status and the write's change group"), 5, || {
            let l = log.lock().unwrap();
            l.contains(&"status") && l.contains(&"changes")
        });
    }
    for c in &clients {
        c.close();
    }
    cli.close();
    s.stopper().stop();
    s.wait();
    s.shutdown();
}

#[test]
fn a_client_respawns_once_after_idle_exit() {
    let d = dirs();
    let idle = Duration::from_millis(300);
    let count = Arc::new(AtomicUsize::new(0));
    let runs = Arc::new(Mutex::new(Vec::new()));
    let spawn = spawner(d.data.path().into(), d.cats.path().into(), idle, count.clone(), runs.clone());
    let mut o = ClientOptions::new(d.data.path().into(), "cli", Some(spawn));
    o.min_spacing = Duration::ZERO;
    let c = Client::new(o, None);

    // Nobody owns the camera: the first call spawns serve, once.
    wait_until("connected", 10, || connected(&c));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(c.spawns(), 1);

    // The client leaves; serve idles out and releases everything.
    c.disconnect();
    let first = runs.lock().unwrap().remove(0);
    assert!(first.join().unwrap(), "the spawned serve ran");
    assert!(lock::is_free(&crate::paths::lock_path(d.data.path())));

    // The next call finds a dead socket and a free lock: one more spawn, then it works.
    assert!(c.call(Request::LabSnapshot).is_ok());
    assert_eq!(count.load(Ordering::SeqCst), 2);
    wait_until("connected again", 10, || connected(&c));
    assert_eq!(count.load(Ordering::SeqCst), 2, "no further spawns while serve is up");
    c.close();
    for t in runs.lock().unwrap().drain(..) {
        assert!(t.join().unwrap());
    }
}

#[test]
fn one_connect_spawns_at_most_once() {
    let d = dirs();
    let count = Arc::new(AtomicUsize::new(0));
    let n = count.clone();
    // A spawn that never brings serve up.
    let spawn: SpawnFn = Arc::new(move || {
        n.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let mut o = ClientOptions::new(d.data.path().into(), "cli", Some(spawn));
    o.wait = Duration::from_millis(400);
    o.min_spacing = Duration::from_secs(30);
    let c = Client::new(o, None);
    let e = c.call(Request::LabSnapshot).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(count.load(Ordering::SeqCst), 1, "one spawn per connect");
    // Right after, the spacing rule refuses a second spawn instead of looping.
    let e = c.call(Request::LabSnapshot).unwrap_err();
    assert!(e.message.contains("next start"), "{}", e.message);
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn a_held_lock_means_wait_not_spawn() {
    let d = dirs();
    crate::paths::prepare_data_dir(d.data.path()).unwrap();
    // Some owner holds the lock but has no socket yet (starting, or shutting down).
    let held = OwnerLock::try_acquire(&crate::paths::lock_path(d.data.path())).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let n = count.clone();
    let spawn: SpawnFn = Arc::new(move || {
        n.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let mut o = ClientOptions::new(d.data.path().into(), "cli", Some(spawn));
    o.wait = Duration::from_millis(300);
    let c = Client::new(o, None);
    assert_eq!(c.call(Request::LabSnapshot).unwrap_err().code, ErrorCode::Unavailable);
    assert_eq!(count.load(Ordering::SeqCst), 0, "never spawn while the lock is held");
    drop(held);
}

#[test]
fn the_app_keeps_serve_up_and_its_close_lets_serve_exit() {
    let d = dirs();
    let idle = Duration::from_millis(300);
    let count = Arc::new(AtomicUsize::new(0));
    let runs = Arc::new(Mutex::new(Vec::new()));
    let spawn = spawner(d.data.path().into(), d.cats.path().into(), idle, count.clone(), runs.clone());
    let statuses = Arc::new(AtomicUsize::new(0));
    let st = statuses.clone();
    let on: client::EventFn = Arc::new(move |e| {
        if matches!(e, Event::Status(_)) {
            st.fetch_add(1, Ordering::SeqCst);
        }
    });
    let mut o = ClientOptions::new(d.data.path().into(), "app", Some(spawn));
    o.min_spacing = Duration::ZERO;
    let app = Client::new(o, Some(on));
    app.keep_connected();
    wait_until("events arrive", 10, || statuses.load(Ordering::SeqCst) > 0);
    // While the app is connected, serve does not idle out.
    std::thread::sleep(idle * 3);
    assert!(app.welcome().is_some());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    // Close the app: serve idles out, and nothing respawns it.
    app.close();
    let first = runs.lock().unwrap().remove(0);
    assert!(first.join().unwrap());
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(lock::is_free(&crate::paths::lock_path(d.data.path())));
}

#[test]
fn the_app_respawns_once_when_serve_dies() {
    let d = dirs();
    let first = Server::start(opts(&d, Duration::from_secs(30))).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let runs = Arc::new(Mutex::new(Vec::new()));
    let spawn = spawner(d.data.path().into(), d.cats.path().into(), Duration::from_millis(300), count.clone(), runs.clone());
    let mut o = ClientOptions::new(d.data.path().into(), "app", Some(spawn));
    o.min_spacing = Duration::ZERO;
    let on: client::EventFn = Arc::new(|_| {});
    let app = Client::new(o, Some(on));
    app.keep_connected();
    wait_until("app connects to the running serve", 5, || first.clients() == 1);
    assert_eq!(count.load(Ordering::SeqCst), 0, "an owner was up: no spawn");
    let first_pid_conn = app.welcome().unwrap();
    // The owner goes away (a crash or a kill) with the app still open.
    first.stopper().stop();
    first.wait();
    first.shutdown();
    wait_until("the app reconnects to a new serve", 10, || app.welcome().is_some() && runs.lock().unwrap().len() == 1);
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(count.load(Ordering::SeqCst), 1, "exactly one respawn");
    assert_eq!(app.welcome().unwrap().protocol, first_pid_conn.protocol);
    assert!(matches!(app.call(Request::LabSnapshot), Ok(Response::LabSnapshot(_))));
    app.close();
    for t in runs.lock().unwrap().drain(..) {
        assert!(t.join().unwrap());
    }
}
