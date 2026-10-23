//! `ukiyo serve`: the one camera owner. It takes `owner.lock`, listens on `ukiyo.sock`, runs
//! the Lab (and so the owner thread) for every client, and sends every event to every
//! subscribed client. It exits a while after its last client leaves.
//!
//! Order at start: lock, then the socket, then the Lab (the only USB open). Order at exit:
//! stop accepting and remove the socket, close the clients, then a clean Lab shutdown (AUTO
//! POWER OFF restored, session closed), which releases the lock.

use super::lock::{LockError, OwnerLock};
use super::wire::{self, Incoming, Welcome};
use crate::api::{self, Request};
use crate::lab::Lab;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write as _};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

/// How long `serve` stays up after its last client leaves.
pub const DEFAULT_IDLE_EXIT: Duration = Duration::from_secs(120);

/// How long `serve` retries a held lock before it gives up (a client's probe holds it briefly).
pub const LOCK_WAIT: Duration = Duration::from_secs(1);

/// Lines queued for one client. A client this far behind on events is cut off; it
/// reconnects and reads a fresh snapshot.
const QUEUE: usize = 4096;

/// The longest line a client may send. A request is a few kilobytes; a line past this is
/// not one, and reading it whole would hand one client the owner's memory. The client is
/// cut off instead.
pub const MAX_LINE: usize = 1 << 20;

pub struct Options {
    pub data_dir: PathBuf,
    pub catalog_dir: PathBuf,
    pub idle_exit: Duration,
    pub lock_wait: Duration,
}

impl Options {
    pub fn new(data_dir: PathBuf, catalog_dir: PathBuf) -> Options {
        Options { data_dir, catalog_dir, idle_exit: DEFAULT_IDLE_EXIT, lock_wait: LOCK_WAIT }
    }
}

#[derive(Debug)]
pub enum StartError {
    /// Another `serve` holds `owner.lock`.
    Owned,
    Io(std::io::Error),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::Owned => f.write_str("another process owns the camera (owner.lock is held by another ukiyo serve or a tools/ptp script)"),
            StartError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StartError {}

impl From<std::io::Error> for StartError {
    fn from(e: std::io::Error) -> Self {
        StartError::Io(e)
    }
}

struct Conn {
    tx: SyncSender<String>,
    stream: UnixStream,
    subscribed: AtomicBool,
    actor: Mutex<String>,
}

impl Conn {
    /// Queues a reply. Blocks while the client's queue is full; a dead client drops it.
    fn send(&self, line: String) {
        let _ = self.tx.send(line);
    }

    fn close(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

struct Shared {
    conns: Mutex<HashMap<u64, Arc<Conn>>>,
    next: AtomicU64,
    /// Since when no client has been connected; `None` while one is.
    idle_since: Mutex<Option<Instant>>,
    closing: AtomicBool,
    stop: AtomicBool,
    lab: OnceLock<Arc<Lab>>,
}

impl Shared {
    fn broadcast(&self, e: &api::Event) {
        let line = wire::event_line(e);
        let conns = self.conns.lock().unwrap();
        for c in conns.values().filter(|c| c.subscribed.load(Ordering::Relaxed)) {
            match c.tx.try_send(line.clone()) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => {}
                Err(TrySendError::Full(_)) => {
                    eprintln!("ukiyo serve: a client fell {QUEUE} lines behind; closing it");
                    c.close();
                }
            }
        }
    }
}

/// Stops a running server from another thread (a signal, a test).
#[derive(Clone)]
pub struct Stopper(Arc<Shared>);

impl Stopper {
    pub fn stop(&self) {
        self.0.stop.store(true, Ordering::SeqCst);
    }
}

pub struct Server {
    shared: Arc<Shared>,
    socket: PathBuf,
    idle_exit: Duration,
    accept: Option<std::thread::JoinHandle<()>>,
    lab: Arc<Lab>,
}

impl Server {
    /// Takes the lock, binds the socket, starts the Lab. Fails with [`StartError::Owned`]
    /// without touching the socket or USB when another `serve` holds the lock.
    pub fn start(opts: Options) -> Result<Server, StartError> {
        crate::paths::prepare_data_dir(&opts.data_dir)?;
        let lock = match OwnerLock::acquire_within(&crate::paths::lock_path(&opts.data_dir), opts.lock_wait) {
            Ok(l) => l,
            Err(LockError::Held) => return Err(StartError::Owned),
            Err(LockError::Io(e)) => return Err(StartError::Io(e)),
        };
        // We hold the lock, so any socket file is a dead owner's.
        let socket = crate::paths::socket_path(&opts.data_dir);
        if std::fs::symlink_metadata(&socket).is_ok() {
            std::fs::remove_file(&socket)?;
        }
        let listener = UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;

        let shared = Arc::new(Shared {
            conns: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            idle_since: Mutex::new(Some(Instant::now())),
            closing: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            lab: OnceLock::new(),
        });
        let weak: Weak<Shared> = Arc::downgrade(&shared);
        let lab = Lab::start(lock, opts.data_dir.clone(), opts.catalog_dir.clone(), move |e| {
            if let Some(s) = weak.upgrade() {
                s.broadcast(&e);
            }
        });
        let _ = shared.lab.set(lab.clone());
        let accept = {
            let shared = shared.clone();
            std::thread::Builder::new().name("serve-accept".into()).spawn(move || accept_loop(listener, shared))?
        };
        eprintln!("ukiyo serve: pid {} on {}", std::process::id(), socket.display());
        Ok(Server { shared, socket, idle_exit: opts.idle_exit, accept: Some(accept), lab })
    }

    pub fn stopper(&self) -> Stopper {
        Stopper(self.shared.clone())
    }

    /// Clients connected now.
    pub fn clients(&self) -> usize {
        self.shared.conns.lock().unwrap().len()
    }

    pub fn lab(&self) -> &Arc<Lab> {
        &self.lab
    }

    /// Blocks until a stop, or until no client has been connected for `idle_exit`.
    pub fn wait(&self) {
        loop {
            std::thread::sleep(Duration::from_millis(50));
            if self.shared.stop.load(Ordering::SeqCst) {
                eprintln!("ukiyo serve: stopping");
                return;
            }
            if self.shared.idle_since.lock().unwrap().is_some_and(|t| t.elapsed() >= self.idle_exit) {
                eprintln!("ukiyo serve: no clients for {} s; exiting", self.idle_exit.as_secs_f32());
                return;
            }
        }
    }

    /// Stops accepting, removes the socket, closes every client, then shuts the Lab down
    /// cleanly, which releases `owner.lock`.
    pub fn shutdown(mut self) {
        self.shared.closing.store(true, Ordering::SeqCst);
        // Wake the accept loop so it sees `closing`.
        let _ = UnixStream::connect(&self.socket);
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
        let _ = std::fs::remove_file(&self.socket);
        for c in self.shared.conns.lock().unwrap().values() {
            c.close();
        }
        self.lab.shutdown();
        eprintln!("ukiyo serve: stopped");
    }
}

/// Accepts clients until `closing`. It polls with a timeout rather than blocking in `accept`:
/// `shutdown` also wakes it through the socket path, but a path that is gone (a deleted data
/// folder) must not leave `serve` running, and holding the camera, for ever.
fn accept_loop(listener: UnixListener, shared: Arc<Shared>) {
    use std::os::fd::AsRawFd as _;
    loop {
        if shared.closing.load(Ordering::SeqCst) {
            return;
        }
        let mut p = libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd for the listener, which outlives the call.
        if unsafe { libc::poll(&mut p, 1, 200) } <= 0 {
            continue;
        }
        if shared.closing.load(Ordering::SeqCst) {
            return;
        }
        match listener.accept().map(|(s, _)| s) {
            Ok(s) => {
                if let Err(e) = add_client(&shared, s) {
                    eprintln!("ukiyo serve: client not added: {e}");
                }
            }
            Err(e) => {
                eprintln!("ukiyo serve: accept: {e}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn add_client(shared: &Arc<Shared>, stream: UnixStream) -> std::io::Result<()> {
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let (tx, rx) = sync_channel(QUEUE);
    let conn = Arc::new(Conn { tx, stream: stream.try_clone()?, subscribed: AtomicBool::new(false), actor: Mutex::new("unknown".into()) });
    let writer = stream.try_clone()?;
    let id = shared.next.fetch_add(1, Ordering::Relaxed);
    {
        let mut conns = shared.conns.lock().unwrap();
        conns.insert(id, conn.clone());
        *shared.idle_since.lock().unwrap() = None;
    }
    std::thread::Builder::new().name(format!("serve-write-{id}")).spawn(move || write_loop(writer, rx))?;
    let shared = shared.clone();
    std::thread::Builder::new().name(format!("serve-read-{id}")).spawn(move || {
        read_loop(&shared, &conn, stream);
        let mut conns = shared.conns.lock().unwrap();
        conns.remove(&id);
        if conns.is_empty() {
            *shared.idle_since.lock().unwrap() = Some(Instant::now());
        }
        drop(conns);
        conn.close();
    })?;
    Ok(())
}

fn write_loop(mut stream: UnixStream, rx: Receiver<String>) {
    for line in rx {
        if writeln!(stream, "{line}").is_err() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return;
        }
    }
}

fn read_loop(shared: &Arc<Shared>, conn: &Arc<Conn>, stream: UnixStream) {
    use std::io::Read as _;
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        // Bounded: at most one byte past the limit, which tells a long line from a full one.
        match (&mut reader).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if buf.len() > MAX_LINE {
            eprintln!("ukiyo serve: a client sent a line over {MAX_LINE} bytes; closing it");
            return;
        }
        let Ok(line) = std::str::from_utf8(&buf) else { return };
        if line.trim().is_empty() {
            continue;
        }
        if shared.closing.load(Ordering::SeqCst) {
            return;
        }
        handle(shared, conn, line);
    }
}

/// Methods that wait on the camera run on their own thread, so the same client's quick calls
/// are not held behind them. Everything else is answered in order.
fn slow(r: &Request) -> bool {
    matches!(r, Request::PlanApply { .. } | Request::LabExport)
}

fn handle(shared: &Arc<Shared>, conn: &Arc<Conn>, line: &str) {
    match wire::parse_incoming(line) {
        Incoming::Hello(id, h) => {
            if h.protocol != wire::PROTOCOL {
                let e = wire::bad_request(format!("protocol {} is not this serve's {}", h.protocol, wire::PROTOCOL));
                return conn.send(wire::error_line(&id, &e));
            }
            *conn.actor.lock().unwrap() = h.actor;
            let lab = shared.lab.get();
            let w = Welcome {
                protocol: wire::PROTOCOL,
                version: env!("CARGO_PKG_VERSION").into(),
                pid: std::process::id(),
                fake: lab.is_some_and(|l| l.snapshot().fake),
            };
            conn.send(wire::result_line(&id, serde_json::to_value(w).unwrap_or_default()));
        }
        Incoming::Subscribe(id) => {
            conn.subscribed.store(true, Ordering::Relaxed);
            conn.send(wire::result_line(&id, serde_json::Value::Null));
        }
        Incoming::Api(id, req) => {
            let Some(lab) = shared.lab.get().cloned() else {
                return conn.send(wire::error_line(&id, &wire::unavailable("serve is starting")));
            };
            let is_slow = slow(&req);
            let actor = conn.actor.lock().unwrap().clone();
            let answer = move |conn: &Conn| {
                let line = match api::dispatch(&lab, &actor, *req) {
                    Ok(r) => wire::result_line(&id, wire::response_result(&r)),
                    Err(e) => wire::error_line(&id, &e),
                };
                conn.send(line);
            };
            if is_slow {
                let conn = conn.clone();
                std::thread::spawn(move || answer(&conn));
            } else {
                answer(conn);
            }
        }
        Incoming::Bad(id, e) => conn.send(wire::error_line(&id, &e)),
    }
}
