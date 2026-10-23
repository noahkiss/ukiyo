//! The CLI's connection to `ukiyo serve`. The CLI never opens USB: every camera read goes
//! through the socket, and the client spawns `serve` (this same binary) when no owner runs
//! (docs/agent-interface.md, "One USB owner", rule 3).

use crate::out::{Fail, Out};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ukiyo_core::api::{Event, Request, Response};
use ukiyo_core::camera::owner::Link;
use ukiyo_core::lab::Snapshot;
use ukiyo_core::paths;
use ukiyo_core::read::{self, Need};
use ukiyo_core::serve::client::{Client, EventFn, Options, SpawnFn};

/// How long a command waits for a fresh connection to settle before it reports the link.
const SETTLE: Duration = Duration::from_secs(8);
/// A `serve` this command just started gets this long to find a camera on the bus.
const FIRST_LOOK: Duration = Duration::from_secs(4);

/// Refuses a data folder whose socket path is too long to bind (`paths::SOCKET_PATH_MAX`),
/// as a usage error before anything connects or spawns.
pub fn check_data_dir() -> Result<(), Fail> {
    let data = paths::data_dir();
    paths::check_socket_path(&data).map_err(|_| {
        let sock = paths::socket_path(&data);
        Fail::usage(format!("the socket path {} is {} bytes; macOS allows {}", sock.display(), sock.as_os_str().len(), paths::SOCKET_PATH_MAX)).hint("Set UKIYO_DATA_DIR to a shorter folder.")
    })
}

/// A client that spawns `serve` when none runs. With `on_event`, it subscribes.
pub fn client(out: Out, on_event: Option<EventFn>) -> Arc<Client> {
    client_as(out, "cli", on_event)
}

/// `client` with another actor: the MCP server is `mcp:<client name>`.
pub fn client_as(out: Out, actor: &str, on_event: Option<EventFn>) -> Arc<Client> {
    let data = paths::data_dir();
    let dir = data.clone();
    let spawn: SpawnFn = Arc::new(move || {
        out.progress("ukiyo: starting ukiyo serve, the camera owner");
        let exe = std::env::current_exe()?;
        ukiyo_core::serve::spawn_detached(&exe, &dir)
    });
    Client::new(Options::new(data, actor, Some(spawn)), on_event)
}

/// A client that never spawns: for reads that only want a running owner's answer.
pub fn existing(wait: Duration) -> Arc<Client> {
    let mut o = Options::new(paths::data_dir(), "cli", None);
    o.wait = wait;
    Client::new(o, None)
}

pub fn snapshot(c: &Client) -> Result<Snapshot, Fail> {
    match c.call(Request::LabSnapshot)? {
        Response::LabSnapshot(s) => Ok(*s),
        other => Err(Fail::new("failed", format!("unexpected reply: {}", serde_json::to_value(other).unwrap_or_default()))),
    }
}

/// The Lab's state once the link has settled: a connect in progress, or a `serve` this
/// command just started, gets a few seconds. Then `need`, if any, must hold.
pub fn settled(c: &Client, need: Option<Need>) -> Result<Snapshot, Fail> {
    let start = Instant::now();
    loop {
        let s = snapshot(c)?;
        let link = s.status.link;
        let fresh = c.spawns() > 0 && start.elapsed() < FIRST_LOOK;
        let settling = link == Link::Connecting || (link == Link::Connected && s.rows.is_empty()) || (fresh && link == Link::Searching);
        if !settling || start.elapsed() >= SETTLE {
            if let Some(need) = need {
                read::readiness(&s.status, s.rows.len(), need)?;
            }
            return Ok(s);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Events from a subscribed client, through a channel.
pub fn events() -> (EventFn, std::sync::mpsc::Receiver<Event>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let f: EventFn = Arc::new(move |e| {
        let _ = tx.lock().unwrap().send(e);
    });
    (f, rx)
}
