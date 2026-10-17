//! A client of `ukiyo serve`: the app, and later the CLI and the MCP server.
//!
//! Connecting (docs/agent-interface.md, "One USB owner", rule 3):
//! - Connect to `ukiyo.sock` and say `hello`.
//! - When that fails and `owner.lock` is free, nobody owns the camera: spawn `serve`, at most
//!   once per connect, and wait for its socket. While the lock is held (an owner starting or
//!   shutting down) only wait.
//! - Spawns are spaced out: after one, the next waits 1 s, then 2, 4 ... up to 60 s, and the
//!   spacing resets after a connection that lasted 60 s. A `serve` that dies at start therefore
//!   never becomes a reconnect loop on the camera.
//!
//! A request is sent once. If the connection drops before its reply, the request fails with
//! `unavailable` and is not retried (it may have reached the camera).

use super::lock;
use super::wire::{self, Hello, Outgoing, Welcome};
use crate::api::{Error, Event, Request, Response};
use crate::paths;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Starts `serve` detached. Called with no lock held and the socket missing or dead.
pub type SpawnFn = Arc<dyn Fn() -> std::io::Result<()> + Send + Sync>;

pub type EventFn = Arc<dyn Fn(Event) + Send + Sync>;

pub struct Options {
    pub data_dir: PathBuf,
    /// `app`, `cli` or `mcp:<client name>`.
    pub actor: String,
    /// `None`: never spawn; fail when no `serve` runs.
    pub spawn: Option<SpawnFn>,
    /// How long one connect waits for the socket.
    pub wait: Duration,
    /// The spacing after the first spawn (doubles up to `max_spacing`).
    pub min_spacing: Duration,
    pub max_spacing: Duration,
    /// A connection that lasted this long resets the spacing.
    pub healthy_after: Duration,
}

impl Options {
    pub fn new(data_dir: PathBuf, actor: impl Into<String>, spawn: Option<SpawnFn>) -> Options {
        Options {
            data_dir,
            actor: actor.into(),
            spawn,
            wait: Duration::from_secs(10),
            min_spacing: Duration::from_secs(1),
            max_spacing: Duration::from_secs(60),
            healthy_after: Duration::from_secs(60),
        }
    }
}

type Pending = HashMap<u64, Sender<Result<Value, Error>>>;

struct Conn {
    writer: Mutex<UnixStream>,
    /// `None` once the connection dropped; a request is never registered after that.
    pending: Mutex<Option<Pending>>,
    next_id: AtomicU64,
    opened: Instant,
    welcome: Welcome,
    lost: (Mutex<bool>, Condvar),
}

impl Conn {
    fn alive(&self) -> bool {
        self.pending.lock().unwrap().is_some()
    }

    fn call_raw(&self, method: &str, line: impl FnOnce(u64) -> String) -> Result<Value, Error> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        match self.pending.lock().unwrap().as_mut() {
            Some(p) => p.insert(id, tx),
            None => return Err(dropped()),
        };
        let sent = writeln!(self.writer.lock().unwrap(), "{}", line(id));
        if sent.is_err() {
            if let Some(p) = self.pending.lock().unwrap().as_mut() {
                p.remove(&id);
            }
            return Err(wire::unavailable(format!("{method}: could not write to ukiyo serve")));
        }
        // The owner bounds its own waits (a write 15 s, an export 60 s).
        rx.recv_timeout(Duration::from_secs(120)).unwrap_or_else(|_| Err(wire::unavailable(format!("{method}: no reply from ukiyo serve"))))
    }

    fn mark_lost(&self) {
        let pending = self.pending.lock().unwrap().take();
        for (_, tx) in pending.into_iter().flatten() {
            let _ = tx.send(Err(dropped()));
        }
        let (m, cv) = &self.lost;
        *m.lock().unwrap() = true;
        cv.notify_all();
    }

    fn wait_lost(&self, closed: &AtomicBool) {
        let (m, cv) = &self.lost;
        let mut lost = m.lock().unwrap();
        while !*lost && !closed.load(Ordering::SeqCst) {
            lost = cv.wait_timeout(lost, Duration::from_millis(500)).unwrap().0;
        }
    }

    fn close(&self) {
        let _ = self.writer.lock().unwrap().shutdown(std::net::Shutdown::Both);
    }
}

fn dropped() -> Error {
    wire::unavailable("the connection to ukiyo serve dropped")
}

struct Spacing {
    next_spawn: Instant,
    gap: Duration,
}

pub struct Client {
    opts: Options,
    on_event: Option<EventFn>,
    /// The connection. Held while connecting, so two callers never spawn twice.
    conn: Mutex<Option<Arc<Conn>>>,
    spacing: Mutex<Spacing>,
    closed: AtomicBool,
    spawns: AtomicUsize,
}

impl Client {
    /// A client. With `on_event`, every connection subscribes and events go to it.
    pub fn new(opts: Options, on_event: Option<EventFn>) -> Arc<Client> {
        let gap = opts.min_spacing;
        Arc::new(Client {
            opts,
            on_event,
            conn: Mutex::new(None),
            spacing: Mutex::new(Spacing { next_spawn: Instant::now(), gap }),
            closed: AtomicBool::new(false),
            spawns: AtomicUsize::new(0),
        })
    }

    /// How many times this client spawned `serve`.
    pub fn spawns(&self) -> usize {
        self.spawns.load(Ordering::SeqCst)
    }

    /// The current connection's `hello` answer, if connected.
    pub fn welcome(&self) -> Option<Welcome> {
        self.conn.lock().unwrap().as_ref().filter(|c| c.alive()).map(|c| c.welcome.clone())
    }

    /// Sends one request and waits for its reply. Connects (and maybe spawns) first if needed.
    pub fn call(&self, req: Request) -> Result<Response, Error> {
        let conn = self.connection()?;
        let method = req.method();
        let v = conn.call_raw(method, |id| wire::api_request_line(id, &req))?;
        wire::response_from(method, v)
    }

    /// Keeps a connection open on a background thread: after a drop it waits 1 s, then
    /// reconnects (spawning under the spacing rule). For a client that must hear events.
    pub fn keep_connected(self: &Arc<Self>) {
        let me = self.clone();
        std::thread::Builder::new()
            .name("ukiyo-client".into())
            .spawn(move || {
                let mut retry = Duration::from_secs(1);
                while !me.closed.load(Ordering::SeqCst) {
                    match me.connection() {
                        Ok(c) => {
                            retry = Duration::from_secs(1);
                            c.wait_lost(&me.closed);
                        }
                        Err(e) => {
                            eprintln!("ukiyo: {}", e.message);
                            retry = (retry * 2).min(Duration::from_secs(30));
                        }
                    }
                    let until = Instant::now() + retry;
                    while Instant::now() < until && !me.closed.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            })
            .expect("client thread starts");
    }

    /// Drops the connection; the next call connects again. `serve` counts one client fewer.
    pub fn disconnect(&self) {
        if let Some(c) = self.conn.lock().unwrap().take() {
            c.close();
        }
    }

    /// Disconnects for good: no more calls, no reconnects.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.disconnect();
    }

    fn connection(&self) -> Result<Arc<Conn>, Error> {
        let mut slot = self.conn.lock().unwrap();
        if let Some(c) = slot.as_ref().filter(|c| c.alive()) {
            return Ok(c.clone());
        }
        if self.closed.load(Ordering::SeqCst) {
            return Err(wire::unavailable("the client is closed"));
        }
        if let Some(old) = slot.take() {
            if old.opened.elapsed() >= self.opts.healthy_after {
                let mut s = self.spacing.lock().unwrap();
                s.gap = self.opts.min_spacing;
                s.next_spawn = Instant::now();
            }
        }
        let c = self.open()?;
        *slot = Some(c.clone());
        Ok(c)
    }

    /// One connect: at most one spawn, at most `wait`.
    fn open(&self) -> Result<Arc<Conn>, Error> {
        let socket = paths::socket_path(&self.opts.data_dir);
        let lock_file = paths::lock_path(&self.opts.data_dir);
        let deadline = Instant::now() + self.opts.wait;
        let mut spawned = false;
        // The lock held and the socket silent the whole wait: an owner that is stuck, or a
        // `tools/ptp` script holding the camera.
        let mut held_throughout = true;
        loop {
            if let Ok(stream) = UnixStream::connect(&socket) {
                match self.handshake(stream) {
                    Ok(c) => return Ok(c),
                    // A protocol mismatch will not fix itself.
                    Err(e) if e.code == crate::api::ErrorCode::BadRequest => return Err(e),
                    // An owner on its way out closed us; wait for the next one.
                    Err(_) => {}
                }
            }
            let free = lock::is_free(&lock_file);
            held_throughout &= !free;
            if !spawned && free {
                let Some(spawn) = &self.opts.spawn else {
                    return Err(wire::unavailable("ukiyo serve is not running"));
                };
                {
                    let mut s = self.spacing.lock().unwrap();
                    let now = Instant::now();
                    if now < s.next_spawn {
                        let left = (s.next_spawn - now).as_secs_f32().ceil();
                        return Err(wire::unavailable(format!("ukiyo serve is not running; the next start is allowed in {left} s")));
                    }
                    s.next_spawn = now + s.gap;
                    s.gap = (s.gap * 2).min(self.opts.max_spacing);
                }
                spawned = true;
                self.spawns.fetch_add(1, Ordering::SeqCst);
                spawn().map_err(|e| wire::unavailable(format!("could not start ukiyo serve: {e}")))?;
            }
            if Instant::now() >= deadline {
                if held_throughout && !spawned {
                    return Err(wire::unavailable(format!(
                        "{} is held but nothing answers on {}: another process owns the camera (a tools/ptp script, or a ukiyo serve that is stuck)",
                        lock_file.display(),
                        socket.display()
                    )));
                }
                return Err(wire::unavailable(format!("ukiyo serve did not answer on {} within {} s", socket.display(), self.opts.wait.as_secs())));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn handshake(&self, stream: UnixStream) -> Result<Arc<Conn>, Error> {
        let io = |e: std::io::Error| wire::unavailable(format!("ukiyo serve: {e}"));
        stream.set_write_timeout(Some(Duration::from_secs(10))).map_err(io)?;
        stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(io)?;
        let mut writer = stream.try_clone().map_err(io)?;
        let mut reader = BufReader::new(stream);
        let hello = Hello { actor: self.opts.actor.clone(), protocol: wire::PROTOCOL };
        writeln!(writer, "{}", wire::request_line(0, wire::HELLO, serde_json::to_value(hello).ok())).map_err(io)?;
        let mut line = String::new();
        if reader.read_line(&mut line).map_err(io)? == 0 {
            return Err(dropped());
        }
        let welcome: Welcome = match wire::parse_outgoing(&line) {
            Outgoing::Reply(0, Ok(v)) => serde_json::from_value(v).map_err(|e| wire::bad_request(format!("hello reply: {e}")))?,
            Outgoing::Reply(0, Err(e)) => return Err(e),
            _ => return Err(wire::bad_request(format!("unexpected hello reply: {line}"))),
        };
        reader.get_ref().set_read_timeout(None).map_err(io)?;
        let conn = Arc::new(Conn {
            writer: Mutex::new(writer),
            pending: Mutex::new(Some(HashMap::new())),
            next_id: AtomicU64::new(1),
            opened: Instant::now(),
            welcome,
            lost: (Mutex::new(false), Condvar::new()),
        });
        {
            let conn = conn.clone();
            let on_event = self.on_event.clone();
            std::thread::Builder::new()
                .name("ukiyo-client-read".into())
                .spawn(move || {
                    for line in reader.lines() {
                        let Ok(line) = line else { break };
                        match wire::parse_outgoing(&line) {
                            Outgoing::Reply(id, r) => {
                                let tx = conn.pending.lock().unwrap().as_mut().and_then(|p| p.remove(&id));
                                if let Some(tx) = tx {
                                    let _ = tx.send(r);
                                }
                            }
                            Outgoing::Event(e) => {
                                if let Some(f) = &on_event {
                                    f(*e);
                                }
                            }
                            Outgoing::Other => {}
                        }
                    }
                    conn.mark_lost();
                })
                .map_err(io)?;
        }
        if self.on_event.is_some() {
            conn.call_raw(wire::SUBSCRIBE, |id| wire::request_line(id, wire::SUBSCRIBE, None))?;
        }
        Ok(conn)
    }
}
