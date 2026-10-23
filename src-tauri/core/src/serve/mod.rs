//! The shared owner (docs/agent-interface.md, "One USB owner").
//!
//! - `lock`: `owner.lock`, the proof of ownership that `Lab::start` takes.
//! - `wire`: the socket protocol.
//! - `server`: `ukiyo serve` itself.
//! - `client`: connect, spawn on demand, requests and events.
//!
//! [`main`] is the `serve` subcommand of both the app binary and `ukiyo-cli`.

pub mod client;
pub mod lock;
pub mod server;
pub mod wire;

pub use client::Client;
pub use lock::OwnerLock;
pub use server::Server;

use std::path::Path;
use std::time::Duration;

/// Starts `exe serve` detached in its own session (`setsid`), so it outlives the process
/// that started it and no terminal signal reaches it. Its stderr goes to `serve.log` in the
/// data folder, and it gets `UKIYO_DATA_DIR` so it resolves the same folder as its spawner.
///
/// It gets no other file descriptor of its spawner. A fork passes on every descriptor not
/// marked close-on-exec, and on macOS std marks a new pipe's ends only after `pipe()` returns,
/// so a spawn on another thread at that instant would hand `serve` a pipe end it would then
/// hold open for its whole life (the far side never sees the end of that pipe).
pub fn spawn_detached(exe: &Path, data_dir: &Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt as _;
    crate::paths::prepare_data_dir(data_dir)?;
    let log_path = crate::paths::serve_log_path(data_dir);
    if std::fs::metadata(&log_path).is_ok_and(|m| m.len() > 1 << 20) {
        let _ = std::fs::rename(&log_path, log_path.with_extension("log.old"));
    }
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("serve")
        .env("UKIYO_DATA_DIR", data_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(log);
    // SAFETY: setsid, getdtablesize and fcntl are async-signal-safe and touch no memory of
    // the parent. This runs after the child's 0, 1 and 2 are in place.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            for fd in 3..libc::getdtablesize().clamp(3, 1 << 16) {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    // Reap it when it exits, so a long-lived spawner (the app) keeps no zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

const USAGE: &str = "usage: serve [--idle-exit SECONDS]   (default 120, or UKIYO_IDLE_EXIT)";

/// `serve [--idle-exit S]`: runs the owner until idle exit or SIGINT, SIGTERM or SIGHUP.
/// Returns the process exit code: 0 after a clean stop, 1 when another `serve` owns the
/// camera or on an error, 2 on a usage error.
pub fn main(args: &[String]) -> i32 {
    // `UKIYO_IDLE_EXIT` (seconds) sets the default, for a spawned serve in development.
    let mut idle_exit = std::env::var("UKIYO_IDLE_EXIT")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| *v >= 0.0)
        .map_or(server::DEFAULT_IDLE_EXIT, Duration::from_secs_f64);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--idle-exit" => match it.next().and_then(|v| v.parse::<f64>().ok()).filter(|v| *v >= 0.0) {
                Some(s) => idle_exit = Duration::from_secs_f64(s),
                None => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return 0;
            }
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    // Before any thread starts, so every thread inherits the mask and only `sigwait` sees them.
    let signals = block_stop_signals();
    let data_dir = crate::paths::data_dir();
    let resources = std::env::current_exe().ok().and_then(|e| crate::paths::bundle_resources(&e));
    let catalog_dir = crate::lab::catalog_dir(&data_dir, resources);
    let mut opts = server::Options::new(data_dir, catalog_dir);
    opts.idle_exit = idle_exit;
    let server = match Server::start(opts) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ukiyo serve: {e}");
            return 1;
        }
    };
    let stopper = server.stopper();
    std::thread::spawn(move || {
        let mut sig = 0;
        // SAFETY: `signals` is an initialised set; sigwait only writes `sig`.
        if unsafe { libc::sigwait(&signals, &mut sig) } == 0 {
            eprintln!("ukiyo serve: signal {sig}");
        }
        stopper.stop();
    });
    server.wait();
    server.shutdown();
    0
}

fn block_stop_signals() -> libc::sigset_t {
    // SAFETY: plain libc calls on a local set.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::sigaddset(&mut set, s);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    }
}

#[cfg(test)]
mod tests;
