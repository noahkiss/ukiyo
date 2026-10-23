//! The fake camera behind a wireless tether endpoint on loopback, so `WifiConnector` and
//! `WifiTransport` run against real sockets in tests and in `UKIYO_FAKE_CAMERA=1` with the
//! Wi-Fi transport.
//!
//! It plays the X-T5 as observed (docs/wifi.md): it answers a PCSS datagram by connecting
//! to the client's tether port with `DSC`, `CAMERANAME` and `DSCPORT`; it checks the init
//! GUID (INIT_FAIL 0x201D otherwise); the first good init on a connection answers
//! INIT_FAIL 0x2019, and a resend on the same socket is acked. Then each container goes to
//! the fake camera's transport. A camera that powers off or leaves closes the socket.

use super::fake::FakeCamera;
use super::ptp::{self, KIND_COMMAND, KIND_DATA};
use super::transport::{Connector, TransportError};
use super::wifi::{self, INIT_BUSY, INIT_CMD_ACK, INIT_CMD_REQ, INIT_FAIL, INIT_LEN};
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What the fake endpoint does, for tests.
#[derive(Debug, Clone)]
pub struct FakeWifiOptions {
    /// The first good init on a connection answers INIT_FAIL 0x2019, as the X-T5 does.
    pub busy_first: bool,
    /// How many inits answered INIT_FAIL 0x2019 before the ack, on each connection.
    pub busy_count: u32,
}

impl Default for FakeWifiOptions {
    fn default() -> Self {
        FakeWifiOptions { busy_first: true, busy_count: 1 }
    }
}

/// Counters the tests read.
#[derive(Debug, Default)]
pub struct FakeWifiStats {
    pub datagrams: AtomicU32,
    pub invites: AtomicU32,
    pub inits: AtomicU32,
    pub init_fails: AtomicU32,
    pub sessions: AtomicU32,
}

pub struct FakeWifiCamera {
    stop: Arc<AtomicBool>,
    pub pcss_port: u16,
    pub cmd_port: u16,
    pub stats: Arc<FakeWifiStats>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;

impl FakeWifiCamera {
    /// Starts the endpoint on loopback. `tether_port` is where the client listens for the
    /// invitation.
    pub fn start(cam: FakeCamera, tether_port: u16, opts: FakeWifiOptions) -> std::io::Result<FakeWifiCamera> {
        let udp = UdpSocket::bind(SocketAddrV4::new(LOOPBACK, 0))?;
        udp.set_read_timeout(Some(Duration::from_millis(100)))?;
        let tcp = TcpListener::bind(SocketAddrV4::new(LOOPBACK, 0))?;
        tcp.set_nonblocking(true)?;
        let pcss_port = udp.local_addr()?.port();
        let cmd_port = tcp.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(FakeWifiStats::default());
        let cam = Arc::new(Mutex::new(cam));
        let mut threads = vec![];
        {
            let (stop, stats, cam) = (stop.clone(), stats.clone(), cam.clone());
            threads.push(std::thread::spawn(move || answer_discovery(udp, tether_port, cmd_port, &cam, &stop, &stats)));
        }
        {
            let (stop, stats) = (stop.clone(), stats.clone());
            threads.push(std::thread::spawn(move || serve_commands(tcp, &cam, &stop, &stats, &opts)));
        }
        Ok(FakeWifiCamera { stop, pcss_port, cmd_port, stats, threads })
    }

    /// The discovery target a `WifiConfig` needs to find this endpoint.
    pub fn target(&self) -> (Ipv4Addr, SocketAddrV4) {
        (LOOPBACK, SocketAddrV4::new(LOOPBACK, self.pcss_port))
    }
}

impl Drop for FakeWifiCamera {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn answer_discovery(udp: UdpSocket, tether_port: u16, cmd_port: u16, cam: &Mutex<FakeCamera>, stop: &AtomicBool, stats: &FakeWifiStats) {
    let mut buf = [0u8; 1024];
    while !stop.load(Ordering::Relaxed) {
        let Ok((n, _)) = udp.recv_from(&mut buf) else { continue };
        stats.datagrams.fetch_add(1, Ordering::Relaxed);
        let text = String::from_utf8_lossy(&buf[..n]).to_string();
        if !text.starts_with("DISCOVERY * HTTP/1.1") || !text.contains("SERVICE: PCSS/1.0") {
            continue;
        }
        // A camera that is off or gone does not answer.
        if !cam.lock().unwrap().present() {
            continue;
        }
        let host = text.lines().find_map(|l| l.strip_prefix("HOST:")).and_then(|h| h.trim().parse::<Ipv4Addr>().ok()).unwrap_or(LOOPBACK);
        let Ok(mut c) = TcpStream::connect_timeout(&SocketAddrV4::new(host, tether_port).into(), Duration::from_secs(1)) else { continue };
        let msg = format!("NOTIFY * HTTP/1.1\r\nDSC: {LOOPBACK}\r\nCAMERANAME: X-T5\r\nDSCPORT: {cmd_port}\r\n\r\n");
        if c.write_all(msg.as_bytes()).is_ok() {
            stats.invites.fetch_add(1, Ordering::Relaxed);
            let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
            let mut ok = [0u8; 64];
            let _ = c.read(&mut ok);
        }
    }
}

fn serve_commands(tcp: TcpListener, cam: &Mutex<FakeCamera>, stop: &AtomicBool, stats: &FakeWifiStats, opts: &FakeWifiOptions) {
    while !stop.load(Ordering::Relaxed) {
        match tcp.accept() {
            Ok((s, _)) => {
                let _ = s.set_nonblocking(false);
                let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
                session(s, cam, stop, stats, opts);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Reads exactly `buf.len()` bytes, waiting through read timeouts until `stop`.
fn read_full(s: &mut TcpStream, buf: &mut [u8], stop: &AtomicBool) -> bool {
    let mut got = 0;
    while got < buf.len() {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        match s.read(&mut buf[got..]) {
            Ok(0) => return false,
            Ok(n) => got += n,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return false,
        }
    }
    true
}

fn read_packet(s: &mut TcpStream, stop: &AtomicBool) -> Option<Vec<u8>> {
    let mut head = [0u8; 4];
    if !read_full(s, &mut head, stop) {
        return None;
    }
    let len = u32::from_le_bytes(head) as usize;
    if !(8..=64 * 1024 * 1024).contains(&len) {
        return None;
    }
    let mut p = vec![0u8; len];
    p[..4].copy_from_slice(&head);
    read_full(s, &mut p[4..], stop).then_some(p)
}

fn init_fail(reason: u32) -> Vec<u8> {
    let mut b = vec![];
    b.extend_from_slice(&12u32.to_le_bytes());
    b.extend_from_slice(&INIT_FAIL.to_le_bytes());
    b.extend_from_slice(&reason.to_le_bytes());
    b
}

fn init_ack(guid: &[u8]) -> Vec<u8> {
    let mut body = vec![];
    body.extend_from_slice(&INIT_CMD_ACK.to_le_bytes());
    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(guid);
    body.extend("X-T5\0".encode_utf16().flat_map(u16::to_le_bytes));
    let mut b = ((body.len() + 4) as u32).to_le_bytes().to_vec();
    b.extend(body);
    b
}

fn session(mut s: TcpStream, cam: &Mutex<FakeCamera>, stop: &AtomicBool, stats: &FakeWifiStats, opts: &FakeWifiOptions) {
    let want = wifi::fuji_guid(LOOPBACK);
    let mut busy_left = if opts.busy_first { opts.busy_count } else { 0 };
    // The init, resent on this socket while busy.
    loop {
        let Some(p) = read_packet(&mut s, stop) else { return };
        stats.inits.fetch_add(1, Ordering::Relaxed);
        let typ = u32::from_le_bytes(p[4..8].try_into().unwrap());
        if p.len() != INIT_LEN || typ != INIT_CMD_REQ || p[12..28] != want {
            stats.init_fails.fetch_add(1, Ordering::Relaxed);
            let _ = s.write_all(&init_fail(0x201D));
            return;
        }
        if busy_left > 0 {
            busy_left -= 1;
            stats.init_fails.fetch_add(1, Ordering::Relaxed);
            if s.write_all(&init_fail(INIT_BUSY)).is_err() {
                return;
            }
            continue;
        }
        if s.write_all(&init_ack(&want)).is_err() {
            return;
        }
        break;
    }
    let Ok(mut t) = cam.lock().unwrap().open(false) else { return };
    stats.sessions.fetch_add(1, Ordering::Relaxed);
    while let Some(p) = read_packet(&mut s, stop) {
        if p.len() == 8 {
            break; // Goodbye.
        }
        let Some(h) = ptp::header(&p) else { break };
        if h.kind != KIND_COMMAND {
            continue;
        }
        let tid = u32::from_le_bytes(p[8..12].try_into().unwrap());
        let params: Vec<u32> = p[12..].as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect();
        // SetDevicePropValue is the one operation here with a data phase from the client.
        let data = if h.code == ptp::OP_SET_PROP_VALUE {
            match read_packet(&mut s, stop) {
                Some(d) if ptp::header(&d).is_some_and(|x| x.kind == KIND_DATA) => Some(d[12..].to_vec()),
                _ => break,
            }
        } else {
            None
        };
        match t.op(h.code, &params, data.as_deref()) {
            Ok(r) => {
                let mut out = vec![];
                if !r.data.is_empty() {
                    out.extend(ptp::data(h.code, tid, &r.data));
                }
                out.extend(response(r.rc, tid));
                if s.write_all(&out).is_err() {
                    break;
                }
            }
            Err(TransportError::Gone) | Err(_) => break,
        }
    }
    let _ = s.shutdown(Shutdown::Both);
}

fn response(rc: u16, tid: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(12);
    b.extend_from_slice(&12u32.to_le_bytes());
    b.extend_from_slice(&ptp::KIND_RESPONSE.to_le_bytes());
    b.extend_from_slice(&rc.to_le_bytes());
    b.extend_from_slice(&tid.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::clock::{Clock, RealClock};
    use crate::camera::fake::FakeMode;
    use crate::camera::owner::{self, Config, Event, Link};
    use crate::camera::transport::TransportKind;
    use crate::camera::wifi::{WifiConfig, WifiConnector};
    use std::time::Instant;

    fn rig(opts: FakeWifiOptions) -> (WifiConnector, FakeWifiCamera, crate::camera::fake::FakeHandle) {
        let clock: Arc<dyn Clock> = Arc::new(RealClock::new());
        let cam = FakeCamera::x_t5(clock, FakeMode::Tether);
        let h = cam.handle();
        let cfg = WifiConfig { listen: SocketAddrV4::new(LOOPBACK, 0), broadcast_every: Duration::from_millis(200), resend_wait: Duration::from_millis(50), ..Default::default() };
        let c = WifiConnector::start(cfg);
        let ep = FakeWifiCamera::start(cam, c.tether_port(), opts).unwrap();
        c.set_targets(vec![ep.target()]);
        (c, ep, h)
    }

    fn wait(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(secs);
        while !f() {
            assert!(Instant::now() < end, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Discovery, the busy init and its resend, then PTP over the socket.
    #[test]
    fn transport_over_loopback() {
        let (mut c, ep, _h) = rig(FakeWifiOptions::default());
        assert_eq!(c.kind(), TransportKind::Wifi);
        wait("an invitation", 5, || c.present());
        let mut t = c.open(false).unwrap();
        assert_eq!(ep.stats.init_fails.load(Ordering::Relaxed), 1, "the first init answers busy");
        assert_eq!(ep.stats.inits.load(Ordering::Relaxed), 2, "resent on the same socket");
        assert_eq!(t.op(ptp::OP_OPEN_SESSION, &[1], None).unwrap().rc, ptp::RC_OK);
        let info = t.op(ptp::OP_GET_DEVICE_INFO, &[], None).unwrap();
        assert_eq!(ptp::device_info(&info.data).unwrap().model, "X-T5");
        let v = t.op(ptp::OP_GET_PROP_VALUE, &[0xD381], None).unwrap();
        assert_eq!((v.rc, v.data.len()), (ptp::RC_OK, 2));
        let w = t.op(ptp::OP_SET_PROP_VALUE, &[0xD381], Some(&[2, 0])).unwrap();
        assert_eq!(w.rc, ptp::RC_OK, "a data phase reaches the camera");
        assert!(c.open(false).is_err(), "an invitation opens once");
    }

    /// A camera that never answers busy is acked at once; a wrong GUID is refused.
    #[test]
    fn init_without_busy() {
        let (mut c, ep, _h) = rig(FakeWifiOptions { busy_first: false, busy_count: 0 });
        wait("an invitation", 5, || c.present());
        let t = c.open(false).unwrap();
        assert_eq!(ep.stats.init_fails.load(Ordering::Relaxed), 0);
        drop(t);
        let mut s = TcpStream::connect(SocketAddrV4::new(LOOPBACK, ep.cmd_port)).unwrap();
        let r = wifi::init(&mut s, &WifiConfig::default(), Ipv4Addr::new(10, 0, 0, 1));
        assert!(matches!(r, Err(TransportError::Io(ref m)) if m.contains("0x201d")), "{r:?}");
    }

    /// The owner on Wi-Fi: it connects, sees the transport, and after the camera powers off
    /// it waits out the reopen delay; it never reconnects in a loop.
    #[test]
    fn owner_on_wifi() {
        let (c, ep, h) = rig(FakeWifiOptions::default());
        let status = Arc::new(Mutex::new(owner::Status::default()));
        let sink = {
            let status = status.clone();
            Box::new(move |e: Event| {
                if let Event::Status(s) = e {
                    *status.lock().unwrap() = s;
                }
            })
        };
        let shots = tempfile::tempdir().unwrap();
        h.with(|s| s.buffer_blocks_writes = true);
        let cfg = Config { presence_every: Duration::from_millis(100), reopen_after: Duration::from_secs(2), shots_dir: Some(shots.path().into()), ..Config::default() };
        let (tx, th) = owner::spawn(Box::new(c), Arc::new(RealClock::new()), sink, cfg);
        wait("connected", 10, || {
            let s = status.lock().unwrap();
            s.link == Link::Connected && s.readable > 0
        });
        assert_eq!(status.lock().unwrap().transport, TransportKind::Wifi);
        assert_eq!(ep.stats.sessions.load(Ordering::Relaxed), 1);
        // A shot taken on the body: the owner imports it from the tether buffer by itself.
        h.shoot_on_body();
        wait("the shot imported", 10, || h.with(|s| s.buffer.is_empty() && s.deleted.len() == 2));
        h.power_off();
        // Busy first (the power-off burst), then the socket closes.
        wait("lost", 15, || matches!(status.lock().unwrap().link, Link::Searching | Link::Asleep | Link::Waiting));
        let lost_at = Instant::now();
        // Off: no invitation, so no init. On again: one session after the reopen delay.
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(ep.stats.sessions.load(Ordering::Relaxed), 1, "no reconnect while off");
        h.power_on();
        wait("reconnected", 15, || status.lock().unwrap().link == Link::Connected);
        assert!(lost_at.elapsed() >= Duration::from_millis(1900), "the reopen delay held");
        assert_eq!(ep.stats.sessions.load(Ordering::Relaxed), 2);
        let _ = tx.send(owner::Request::Shutdown);
        th.join().unwrap();
    }
}
