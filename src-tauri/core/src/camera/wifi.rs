//! Wireless tether: PTP over one TCP connection (docs/wifi.md, "Wireless tether discovery").
//! Ported from `tools/wifi/fujiptpip.py`.
//!
//! - Discovery (PCSS): the Mac broadcasts `DISCOVERY * HTTP/1.1` to UDP 51562; a camera in
//!   WIRELESS TETHER SHOOTING FIXED connects back to the Mac's TCP 51560 with its address
//!   and command port, and the Mac answers `HTTP/1.1 200 OK`.
//! - Init: the Fuji 82-byte request. The X-T5 checks the GUID: a fixed 12-byte prefix, then
//!   the camera's IPv4 address as a little-endian u32. The first correct init answers
//!   INIT_FAIL 0x2019 (busy); the client resends on the same socket about 1 s later.
//!   Closing the socket instead makes the camera stop listening.
//! - Then plain PTP: every packet is a USB container (`ptp::command`, `ptp::data`).
//!
//! The connector keeps a discovery thread. It listens on 51560 all the time and broadcasts
//! every 2 s only while the owner asks `present()` (it does so every 3 s while idle). An
//! invitation is good for `invite_fresh`; `open()` uses it once.

use super::ptp::{self, KIND_DATA, KIND_RESPONSE};
use super::transport::{Connector, Reply, Transport, TransportError, TransportKind};
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The UDP port the camera listens on for the PCSS datagram.
pub const PCSS_PORT: u16 = 51562;
/// The TCP port the camera connects back to on the Mac.
pub const TETHER_PORT: u16 = 51560;
/// The command port when an invitation names none (standard PTP/IP).
pub const CMD_PORT: u16 = 15740;

const INIT_VERSION: u32 = 0x8F53_E4F2;
/// The X-T4 closes the socket on any other length (libgphoto2 fujiptpip.c).
pub const INIT_LEN: usize = 82;
pub const INIT_CMD_REQ: u32 = 1;
pub const INIT_CMD_ACK: u32 = 2;
pub const INIT_FAIL: u32 = 5;
/// INIT_FAIL reasons.
pub const INIT_BUSY: u32 = 0x2019;
/// The 8-byte packet that ends a connection.
pub const GOODBYE: [u8; 8] = [8, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF];
/// Container lengths above this are a broken stream.
const MAX_PACKET: u32 = 512 * 1024 * 1024;

/// The first 12 bytes of the GUID the X-T5 accepts (Capture One 16.5, observed 2026-10-17).
pub const GUID_PREFIX: [u8; 12] = [0xad, 0xa5, 0x48, 0x5d, 0x87, 0xb2, 0x7f, 0x0b, 0xd3, 0xd5, 0xde, 0xd0];

/// The init GUID for a camera at `ip`: the prefix, then the address as a little-endian u32.
pub fn fuji_guid(ip: Ipv4Addr) -> [u8; 16] {
    let mut g = [0u8; 16];
    g[..12].copy_from_slice(&GUID_PREFIX);
    let mut tail = ip.octets();
    tail.reverse();
    g[12..].copy_from_slice(&tail);
    g
}

/// The fixed 82-byte init command request: length, type 1, version, GUID, the client name
/// in UTF-16LE (cut to fit, NUL-terminated), zero padding.
pub fn init_request(name: &str, guid: &[u8; 16]) -> Vec<u8> {
    let mut b = Vec::with_capacity(INIT_LEN);
    b.extend_from_slice(&(INIT_LEN as u32).to_le_bytes());
    b.extend_from_slice(&INIT_CMD_REQ.to_le_bytes());
    b.extend_from_slice(&INIT_VERSION.to_le_bytes());
    b.extend_from_slice(guid);
    let room = INIT_LEN - b.len() - 2;
    let name: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).take(room & !1).collect();
    b.extend_from_slice(&name);
    b.resize(INIT_LEN, 0);
    b
}

/// The PCSS discovery datagram. `host` is the Mac's address on the camera's network.
pub fn pcss_datagram(host: Ipv4Addr) -> Vec<u8> {
    format!("DISCOVERY * HTTP/1.1\r\nHOST: {host}\r\nMX: 5\r\nSERVICE: PCSS/1.0\r\n").into_bytes()
}

/// A camera's answer to discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub ip: Ipv4Addr,
    pub port: u16,
    /// `CAMERANAME`: the model.
    pub camera: Option<String>,
}

/// Parses the camera's message on the tether port (`DSC`, `CAMERANAME`, `DSCPORT`). `None`
/// without `DSCPORT`. `peer` stands in for a missing `DSC`.
pub fn parse_invite(text: &str, peer: Ipv4Addr) -> Option<Invite> {
    let mut ip = None;
    let mut port = None;
    let mut camera = None;
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        match k.trim().to_ascii_uppercase().as_str() {
            "DSC" => ip = v.parse().ok(),
            "DSCPORT" => port = v.parse().ok(),
            "CAMERANAME" => camera = Some(v.to_string()).filter(|s| !s.is_empty()),
            _ => {}
        }
    }
    Some(Invite { ip: ip.unwrap_or(peer), port: port?, camera })
}

/// Each IPv4 interface that can broadcast: its address and broadcast address. Loopback and
/// point-to-point links (VPNs) are left out.
pub fn broadcast_targets() -> Vec<(Ipv4Addr, SocketAddrV4)> {
    let mut out = vec![];
    // SAFETY: getifaddrs fills a list that freeifaddrs releases; each node is read only
    // while the list lives.
    unsafe {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) != 0 {
            return out;
        }
        let mut p = head;
        while !p.is_null() {
            let ifa = &*p;
            p = ifa.ifa_next;
            let flags = ifa.ifa_flags as libc::c_int;
            let wanted = flags & libc::IFF_UP != 0 && flags & libc::IFF_BROADCAST != 0 && flags & libc::IFF_LOOPBACK == 0;
            if !wanted || ifa.ifa_addr.is_null() || i32::from((*ifa.ifa_addr).sa_family) != libc::AF_INET {
                continue;
            }
            let addr = &*(ifa.ifa_addr as *const libc::sockaddr_in);
            let ip = Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            let bcast = ifa.ifa_dstaddr;
            #[cfg(not(any(target_os = "macos", target_os = "ios")))]
            let bcast = ifa.ifa_ifu;
            let b = if bcast.is_null() || i32::from((*bcast).sa_family) != libc::AF_INET {
                Ipv4Addr::BROADCAST
            } else {
                Ipv4Addr::from(u32::from_be((*(bcast as *const libc::sockaddr_in)).sin_addr.s_addr))
            };
            if ip.is_link_local() {
                continue;
            }
            out.push((ip, SocketAddrV4::new(b, PCSS_PORT)));
        }
        libc::freeifaddrs(head);
    }
    out
}

/// How the Wi-Fi connector finds and talks to the camera. Tests point it at loopback.
#[derive(Debug, Clone)]
pub struct WifiConfig {
    /// The name the camera shows for this client.
    pub client_name: String,
    /// Where the discovery thread listens for the camera's invitation.
    pub listen: SocketAddrV4,
    /// Where discovery datagrams go, each with its `HOST`. `None`: every broadcast-capable
    /// interface (`broadcast_targets`), read again each round.
    pub targets: Option<Vec<(Ipv4Addr, SocketAddrV4)>>,
    /// How long discovery keeps broadcasting after the last `present()`.
    pub want_for: Duration,
    /// How long an invitation stays good.
    pub invite_fresh: Duration,
    pub broadcast_every: Duration,
    pub connect_timeout: Duration,
    /// The init answer can wait on an OK on the camera's screen (libfuji).
    pub init_timeout: Duration,
    /// Inits resent after INIT_FAIL 0x2019, and the wait before each.
    pub busy_resends: u32,
    pub resend_wait: Duration,
    /// One read: a large object keeps going as long as bytes arrive.
    pub op_timeout: Duration,
}

impl Default for WifiConfig {
    fn default() -> Self {
        WifiConfig {
            client_name: "Ukiyo".into(),
            listen: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, TETHER_PORT),
            targets: None,
            want_for: Duration::from_secs(10),
            invite_fresh: Duration::from_secs(10),
            broadcast_every: Duration::from_secs(2),
            connect_timeout: Duration::from_secs(5),
            init_timeout: Duration::from_secs(30),
            busy_resends: 4,
            resend_wait: Duration::from_secs(1),
            op_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Default)]
struct Shared {
    invite: Option<(Instant, Invite)>,
    wanted_until: Option<Instant>,
    /// Why discovery cannot run (the tether port is taken), for the status reason.
    problem: Option<String>,
    /// Replaces `WifiConfig::targets` once set (`WifiConnector::set_targets`).
    targets: Option<Vec<(Ipv4Addr, SocketAddrV4)>>,
}

/// Finds a camera in wireless tether mode and opens a session on it.
pub struct WifiConnector {
    cfg: WifiConfig,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// The port the discovery thread listens on (the configured one, or the one the OS
    /// picked for port 0).
    port: u16,
}

impl WifiConnector {
    /// Starts the discovery thread. The listen port is bound now; when it is taken (another
    /// wireless tether client), the thread tries again every 2 s and `problem()` says why.
    pub fn start(cfg: WifiConfig) -> WifiConnector {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let listener = bind(&cfg, &shared);
        let port = listener.as_ref().and_then(|l| l.local_addr().ok()).map_or(cfg.listen.port(), |a| a.port());
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (cfg, shared, stop) = (cfg.clone(), shared.clone(), stop.clone());
            std::thread::Builder::new().name("wifi-discovery".into()).spawn(move || discovery(cfg, listener, shared, stop)).ok()
        };
        WifiConnector { cfg, shared, stop, thread, port }
    }

    /// The port the camera's invitation must reach.
    pub fn tether_port(&self) -> u16 {
        self.port
    }

    /// Where discovery datagrams go from now on, each with its `HOST`.
    pub fn set_targets(&self, targets: Vec<(Ipv4Addr, SocketAddrV4)>) {
        self.shared.lock().unwrap().targets = Some(targets);
    }
}

fn bind(cfg: &WifiConfig, shared: &Mutex<Shared>) -> Option<TcpListener> {
    let r = TcpListener::bind(cfg.listen).and_then(|l| l.set_nonblocking(true).map(|_| l));
    let mut s = shared.lock().unwrap();
    match r {
        Ok(l) => {
            s.problem = None;
            Some(l)
        }
        Err(e) => {
            s.problem = Some(format!("cannot listen on TCP {}: {e} (another wireless tether client?)", cfg.listen));
            None
        }
    }
}

impl Drop for WifiConnector {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn discovery(cfg: WifiConfig, mut listener: Option<TcpListener>, shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>) {
    let udp = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).ok();
    if let Some(u) = &udp {
        let _ = u.set_broadcast(true);
    }
    let mut next_broadcast = Instant::now();
    let mut next_bind = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        let Some(l) = listener.as_ref() else {
            if now >= next_bind {
                next_bind = now + Duration::from_secs(2);
                listener = bind(&cfg, &shared);
            }
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        let (wanted, targets) = {
            let s = shared.lock().unwrap();
            (s.wanted_until.is_some_and(|t| now < t), s.targets.clone())
        };
        if wanted && now >= next_broadcast {
            next_broadcast = now + cfg.broadcast_every;
            let targets = targets.or_else(|| cfg.targets.clone()).unwrap_or_else(broadcast_targets);
            if let Some(u) = &udp {
                for (host, dest) in targets {
                    let _ = u.send_to(&pcss_datagram(host), dest);
                }
            }
        }
        match l.accept() {
            Ok((conn, peer)) => {
                let peer = match peer {
                    SocketAddr::V4(a) => *a.ip(),
                    SocketAddr::V6(_) => continue,
                };
                if let Some(inv) = read_invite(conn, peer) {
                    shared.lock().unwrap().invite = Some((Instant::now(), inv));
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

/// Reads the invitation up to its blank line (at most 5 s), and answers 200 OK.
fn read_invite(mut conn: TcpStream, peer: Ipv4Addr) -> Option<Invite> {
    let _ = conn.set_nonblocking(false);
    let _ = conn.set_read_timeout(Some(Duration::from_millis(500)));
    let end = Instant::now() + Duration::from_secs(5);
    let mut raw = Vec::new();
    let mut buf = [0u8; 2048];
    while !raw.windows(4).any(|w| w == b"\r\n\r\n") && Instant::now() < end && raw.len() < 16 * 1024 {
        match conn.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                // One recv can come back empty before the rest; stop once a port was named.
                if parse_invite(&String::from_utf8_lossy(&raw), peer).is_some() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let inv = parse_invite(&String::from_utf8_lossy(&raw), peer)?;
    let _ = conn.write_all(b"HTTP/1.1 200 OK\r\n");
    let _ = conn.shutdown(Shutdown::Both);
    Some(inv)
}

fn net_error(e: std::io::Error) -> TransportError {
    match e.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => TransportError::Timeout,
        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe | ErrorKind::UnexpectedEof | ErrorKind::NotConnected | ErrorKind::ConnectionRefused => TransportError::Gone,
        // macOS answers EHOSTUNREACH at once when the process has no Local Network access.
        ErrorKind::HostUnreachable => TransportError::Io(format!("{e} (no route, or no Local Network permission for this app)")),
        _ => TransportError::Io(e.to_string()),
    }
}

/// One whole packet: the u32 length, then the rest.
fn read_packet(s: &mut TcpStream) -> Result<Vec<u8>, TransportError> {
    let mut head = [0u8; 4];
    s.read_exact(&mut head).map_err(net_error)?;
    let len = u32::from_le_bytes(head);
    if !(8..=MAX_PACKET).contains(&len) {
        return Err(TransportError::Io(format!("bad packet length {len}")));
    }
    let mut buf = vec![0u8; len as usize];
    buf[..4].copy_from_slice(&head);
    s.read_exact(&mut buf[4..]).map_err(net_error)?;
    Ok(buf)
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Sends the init and waits for the ack, resending on INIT_FAIL 0x2019. Returns the
/// camera's name from the ack.
pub fn init(s: &mut TcpStream, cfg: &WifiConfig, camera: Ipv4Addr) -> Result<String, TransportError> {
    let req = init_request(&cfg.client_name, &fuji_guid(camera));
    s.set_read_timeout(Some(cfg.init_timeout)).map_err(net_error)?;
    let mut resends = 0;
    loop {
        s.write_all(&req).map_err(net_error)?;
        let p = read_packet(s)?;
        match u32_at(&p, 4) {
            Some(INIT_CMD_ACK) => {
                // Ack: connection number, GUID, then the camera name in UTF-16LE.
                let name: Vec<u16> = p.get(28..).unwrap_or_default().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|c| *c != 0).collect();
                // libfuji: Fuji cameras need at least 50 ms before OpenSession.
                std::thread::sleep(Duration::from_millis(100));
                return Ok(String::from_utf16_lossy(&name));
            }
            Some(INIT_FAIL) => {
                let reason = u32_at(&p, 8).unwrap_or(0);
                if reason == INIT_BUSY && resends < cfg.busy_resends {
                    // Resend on the same socket; closing it makes the camera stop listening.
                    resends += 1;
                    std::thread::sleep(cfg.resend_wait);
                    continue;
                }
                return Err(TransportError::Io(format!("the camera refused the init (INIT_FAIL {reason:#06x})")));
            }
            t => return Err(TransportError::Io(format!("unexpected init answer type {t:?}"))),
        }
    }
}

impl Connector for WifiConnector {
    fn present(&mut self) -> bool {
        let now = Instant::now();
        let mut s = self.shared.lock().unwrap();
        s.wanted_until = Some(now + self.cfg.want_for);
        s.invite.as_ref().is_some_and(|(at, _)| now.duration_since(*at) < self.cfg.invite_fresh)
    }

    /// `reset` has no Wi-Fi meaning: the connector ignores it.
    fn open(&mut self, _reset: bool) -> Result<Box<dyn Transport>, TransportError> {
        let inv = {
            let mut s = self.shared.lock().unwrap();
            let fresh = s.invite.take().filter(|(at, _)| at.elapsed() < self.cfg.invite_fresh);
            fresh.map(|(_, i)| i).ok_or(TransportError::Gone)?
        };
        let addr = SocketAddr::V4(SocketAddrV4::new(inv.ip, inv.port));
        let mut s = TcpStream::connect_timeout(&addr, self.cfg.connect_timeout).map_err(net_error)?;
        let _ = s.set_nodelay(true);
        init(&mut s, &self.cfg, inv.ip)?;
        s.set_read_timeout(Some(self.cfg.op_timeout)).map_err(net_error)?;
        s.set_write_timeout(Some(self.cfg.op_timeout)).map_err(net_error)?;
        Ok(Box::new(WifiTransport { s, tid: 0 }))
    }

    fn kind(&self) -> TransportKind {
        TransportKind::Wifi
    }

    fn problem(&mut self) -> Option<String> {
        self.shared.lock().unwrap().problem.clone()
    }
}

/// An open wireless tether session.
pub struct WifiTransport {
    s: TcpStream,
    /// The next transaction id. The first operation (OpenSession) uses 0, as
    /// `fujiptpip.Session` does.
    tid: u32,
}

impl Transport for WifiTransport {
    fn op(&mut self, code: u16, params: &[u32], data: Option<&[u8]>) -> Result<Reply, TransportError> {
        let tid = self.tid;
        self.tid = self.tid.wrapping_add(1);
        self.s.write_all(&ptp::command(code, tid, params)).map_err(net_error)?;
        if let Some(d) = data {
            self.s.write_all(&ptp::data(code, tid, d)).map_err(net_error)?;
        }
        let mut payload = Vec::new();
        loop {
            let p = read_packet(&mut self.s)?;
            let h = ptp::header(&p).ok_or_else(|| TransportError::Io("short container".into()))?;
            match h.kind {
                KIND_DATA => payload.extend_from_slice(&p[12..]),
                KIND_RESPONSE => return Ok(Reply { rc: h.code, data: payload }),
                // Events (kind 4) can arrive between transactions; the owner polls instead.
                _ => {}
            }
        }
    }
}

impl Drop for WifiTransport {
    fn drop(&mut self) {
        let _ = self.s.set_write_timeout(Some(Duration::from_millis(500)));
        let _ = self.s.write_all(&GOODBYE);
        let _ = self.s.shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_and_init_match_the_python_tools() {
        // fujiptpip.fuji_guid("192.168.1.23") = prefix + bytes 23, 1, 168, 192.
        let g = fuji_guid(Ipv4Addr::new(192, 168, 1, 23));
        assert_eq!(&g[..12], &GUID_PREFIX);
        assert_eq!(&g[12..], &[23, 1, 168, 192]);
        let r = init_request("ukiyo-probe", &g);
        assert_eq!(r.len(), INIT_LEN);
        assert_eq!(&r[..12], &[82, 0, 0, 0, 1, 0, 0, 0, 0xF2, 0xE4, 0x53, 0x8F]);
        assert_eq!(&r[12..28], &g);
        assert_eq!(&r[28..30], &[b'u', 0]);
        assert!(r[28 + 22..].iter().all(|b| *b == 0), "name, then zero padding");
        let long = init_request(&"x".repeat(100), &g);
        assert_eq!(long.len(), INIT_LEN);
        assert_eq!(&long[INIT_LEN - 2..], &[0, 0], "the name keeps a terminator");
    }

    #[test]
    fn invites_parse() {
        let peer = Ipv4Addr::new(10, 0, 0, 9);
        let i = parse_invite("NOTIFY * HTTP/1.1\r\nDSC: 10.0.0.5\r\nCAMERANAME: X-T5\r\nDSCPORT: 15740\r\n\r\n", peer).unwrap();
        assert_eq!(i, Invite { ip: Ipv4Addr::new(10, 0, 0, 5), port: 15740, camera: Some("X-T5".into()) });
        assert_eq!(parse_invite("DSCPORT: 15740\r\n", peer).unwrap().ip, peer, "no DSC: the peer");
        assert!(parse_invite("NOTIFY * HTTP/1.1\r\nDSC: 10.0.0.5\r\n", peer).is_none());
        let d = String::from_utf8(pcss_datagram(Ipv4Addr::new(10, 0, 0, 2))).unwrap();
        assert!(d.starts_with("DISCOVERY * HTTP/1.1\r\nHOST: 10.0.0.2\r\n") && d.contains("SERVICE: PCSS/1.0"));
    }
}
