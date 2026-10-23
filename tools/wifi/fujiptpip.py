"""Fujifilm PTP over TCP: framing, datasets and a small client. Stdlib only.

Sources (see docs/wifi.md): libgphoto2 camlibs/ptp2/fujiptpip.c and ptp.c, petabyt/libfuji
lib/fuji.c, lib/fujiptp.h and lib/discovery.c.

Only the first packet is PTP/IP. After it, every packet is a USB-style container:
u32 length, u16 type (1 command, 2 data, 3 response, 4 event), u16 code, u32 transaction id,
then parameters or payload.
"""
import errno
import os
import socket
import struct
import time
import uuid

CMD_PORT = 55740
EVENT_PORT = 55741
JPEG_PORT = 55742
# Wireless tether discovery (libfuji discovery.c).
PCSS_PORT = 51562       # the client broadcasts DISCOVERY here
TETHER_PORT = 51560     # the camera connects back to the client here

FUJI_INIT_VERSION = 0x8F53E4F2
INIT_LEN = 82           # X-T4 closes the socket on any other length (gp fujiptpip.c)
INIT_CMD_REQ, INIT_CMD_ACK, INIT_FAIL = 1, 2, 5
T_CMD, T_DATA, T_RESP, T_EVENT = 1, 2, 3, 4
GOODBYE = struct.pack("<II", 8, 0xFFFFFFFF)

OK = 0x2001
RC_NAMES = {
    0x2001: "OK", 0x2002: "GeneralError", 0x2003: "SessionNotOpen", 0x2004: "InvalidTransactionID",
    0x2005: "OperationNotSupported", 0x2006: "ParameterNotSupported", 0x2009: "InvalidObjectHandle",
    0x200A: "DevicePropNotSupported", 0x200F: "AccessDenied", 0x2019: "DeviceBusy",
    0x201E: "SessionAlreadyOpen", 0xA001: "Fuji: menu open", 0xA002: "Fuji: busy (tether buffer holds images)",
}

OP = {
    "GetDeviceInfo": 0x1001, "OpenSession": 0x1002, "CloseSession": 0x1003,
    "GetStorageIDs": 0x1004, "GetStorageInfo": 0x1005, "GetNumObjects": 0x1006,
    "GetObjectHandles": 0x1007, "GetObjectInfo": 0x1008, "GetObject": 0x1009, "GetThumb": 0x100A,
    "GetDevicePropDesc": 0x1014, "GetDevicePropValue": 0x1015, "SetDevicePropValue": 0x1016,
    "TerminateOpenCapture": 0x1018, "GetPartialObject": 0x101B, "InitiateOpenCapture": 0x101C,
    "FujiGetDeviceInfo": 0x902B,
}
# Operations this tool never sends: capture, delete, format, writes to objects.
FORBIDDEN_OPS = {0x100B, 0x100C, 0x100D, 0x100E, 0x100F, 0x1010, 0x1011, 0x1012, 0x1013, 0x1019,
                 0x101A, 0x900C, 0x900D, 0x901D, 0x9020, 0x9026, 0x9040, 0x9041, 0x9042}

# PTP datatype codes -> struct format.
SCALAR = {1: "b", 2: "B", 3: "h", 4: "H", 5: "i", 6: "I", 7: "q", 8: "Q"}


class PTPError(Exception):
    pass


# ---------------------------------------------------------------- reading datasets

class Reader:
    def __init__(self, data, offset=0):
        self.d, self.o = bytes(data), offset

    def left(self):
        return len(self.d) - self.o

    def take(self, fmt):
        size = struct.calcsize("<" + fmt)
        if self.left() < size:
            raise PTPError(f"short read: need {size} at {self.o}, have {self.left()}")
        (v,) = struct.unpack_from("<" + fmt, self.d, self.o)
        self.o += size
        return v

    def string(self):
        n = self.take("B")
        if n == 0:
            return ""
        raw = self.d[self.o:self.o + n * 2]
        if len(raw) < n * 2:
            raise PTPError("short string")
        self.o += n * 2
        return raw.decode("utf-16-le", "replace").rstrip("\x00")

    def array16(self):
        n = self.take("I")
        if n > 10000:
            raise PTPError(f"absurd array length {n}")
        return [self.take("H") for _ in range(n)]

    def value(self, dtype):
        if dtype in SCALAR:
            return self.take(SCALAR[dtype])
        if dtype == 0xFFFF:
            return self.string()
        if dtype & 0x4000 and (dtype & 0xFF) in SCALAR:
            n = self.take("I")
            return [self.take(SCALAR[dtype & 0xFF]) for _ in range(min(n, 4096))]
        raise PTPError(f"datatype {dtype:#06x} has no known size")


def ptp_string(s):
    if not s:
        return b"\x00"
    enc = (s + "\x00").encode("utf-16-le")
    return bytes([len(enc) // 2]) + enc


def pack_value(dtype, v):
    if dtype in SCALAR:
        return struct.pack("<" + SCALAR[dtype], v)
    if dtype == 0xFFFF:
        # Never a zero-length string on the wire: the X-T5 left junk in D20B after one
        # (docs/wifi.md). An empty value goes as the terminator alone, which clears the property.
        return ptp_string(v) if v else b"\x01\x00\x00"
    raise PTPError(f"cannot pack datatype {dtype:#x}")


def parse_device_info(data):
    r = Reader(data)
    info = {
        "standard_version": r.take("H"), "vendor_ext_id": r.take("I"),
        "vendor_ext_version": r.take("H"), "vendor_ext_desc": r.string(),
        "functional_mode": r.take("H"), "operations": r.array16(), "events": r.array16(),
        "properties": r.array16(), "capture_formats": r.array16(), "image_formats": r.array16(),
    }
    for key in ("manufacturer", "model", "device_version", "serial"):
        info[key] = r.string() if r.left() else ""
    return info


def parse_prop_desc(data, offset=0):
    """GetDevicePropDesc dataset. Returns (desc, end offset). Tolerates datatype 0 and a missing form."""
    r = Reader(data, offset)
    d = {"code": r.take("H"), "datatype": r.take("H"), "writable": bool(r.take("B"))}
    try:
        d["default"] = r.value(d["datatype"])
        d["current"] = r.value(d["datatype"])
    except PTPError as e:
        d["parse_note"] = str(e)
        return d, len(r.d)
    d["form"] = "none"
    if r.left():
        form = r.take("B")
        if form == 1:
            d["form"] = "range"
            d["min"], d["max"], d["step"] = (r.value(d["datatype"]) for _ in range(3))
        elif form == 2:
            d["form"] = "enum"
            n = r.take("H")
            d["values"] = [r.value(d["datatype"]) for _ in range(n)]
    return d, r.o


def parse_fuji_device_info(data):
    """0x902B: u32 count, then per property u32 size + a PropDesc (gp ptp.c ptp_fuji_getdeviceinfo)."""
    r = Reader(data)
    count = r.take("I")
    descs, notes = [], []
    for i in range(count):
        if r.left() < 4:
            notes.append(f"stopped at {i} of {count}: no size")
            break
        size = r.take("I")
        start = r.o
        try:
            desc, end = parse_prop_desc(r.d[:start + max(size, 0)] if size else r.d, start)
        except PTPError as e:
            notes.append(f"entry {i}: {e}")
            break
        descs.append(desc)
        # The size most likely covers only the PropDesc; fall back to the parsed end if not.
        r.o = start + size if size and start + size <= len(r.d) else end
    return {"count": count, "descs": descs, "notes": notes}


def parse_storage_info(data):
    r = Reader(data)
    return {"type": r.take("H"), "fs_type": r.take("H"), "access": r.take("H"),
            "max_capacity": r.take("Q"), "free_bytes": r.take("Q"), "free_images": r.take("I"),
            "description": r.string(), "volume_label": r.string() if r.left() else ""}


def parse_object_info(data):
    r = Reader(data)
    keys = [("storage_id", "I"), ("format", "H"), ("protection", "H"), ("size", "I"),
            ("thumb_format", "H"), ("thumb_size", "I"), ("thumb_w", "I"), ("thumb_h", "I"),
            ("image_w", "I"), ("image_h", "I"), ("bit_depth", "I"), ("parent", "I"),
            ("assoc_type", "H"), ("assoc_desc", "I"), ("sequence", "I")]
    oi = {k: r.take(f) for k, f in keys}
    for k in ("filename", "capture_date", "modification_date", "keywords"):
        oi[k] = r.string() if r.left() else ""
    return oi


def parse_u32_array(data):
    r = Reader(data)
    n = r.take("I")
    return [r.take("I") for _ in range(min(n, r.left() // 4))]


def parse_events_d212(data):
    """D212 change list: u16 count, then {u16 code, u32 value} (libfuji fujiptp.h PtpFujiEvents)."""
    r = Reader(data)
    n = r.take("H")
    out = []
    for _ in range(n):
        if r.left() < 6:
            break
        out.append((r.take("H"), r.take("I")))
    return out


def parse_init_ack(payload):
    """Init ack after the 8-byte header: u32 connection number, 16-byte GUID, UCS-2 name."""
    if len(payload) < 20:
        return {"conn": None, "guid": payload.hex(), "name": ""}
    conn = struct.unpack_from("<I", payload, 0)[0]
    name = payload[20:].decode("utf-16-le", "replace").split("\x00")[0]
    return {"conn": conn, "guid": payload[4:20].hex(), "name": name}


# The X-T5 refuses an init whose GUID it does not expect (INIT_FAIL 0x201D). Capture One 16.5 sends
# these 12 bytes, then the camera's IPv4 address as a little-endian u32 (observed 2026-10-17);
# libfuji uses the same 12 bytes with a zero last word.
FUJI_GUID_PREFIX = bytes.fromhex("ada5485d87b27f0bd3d5ded0")


def fuji_guid(host):
    try:
        tail = socket.inet_aton(host)[::-1]
    except OSError:
        tail = b"\x00" * 4
    return FUJI_GUID_PREFIX + tail


def build_init(name, guid):
    """The fixed 82-byte Fuji init command request."""
    body = struct.pack("<I", FUJI_INIT_VERSION) + guid
    raw = name.encode("utf-16-le")[:INIT_LEN - 8 - len(body) - 2]
    body += raw
    pkt = struct.pack("<II", INIT_LEN, INIT_CMD_REQ) + body
    return pkt + b"\x00" * (INIT_LEN - len(pkt))


def container(ptype, code, tid, params=(), payload=b""):
    body = struct.pack("<HHI", ptype, code, tid) + b"".join(struct.pack("<I", p) for p in params) + payload
    return struct.pack("<I", 4 + len(body)) + body


def parse_container(raw):
    """raw = full packet including the length. Returns dict(type, code, tid, params|payload)."""
    length, ptype, code, tid = struct.unpack_from("<IHHI", raw, 0)
    rest = raw[12:length]
    out = {"length": length, "type": ptype, "code": code, "tid": tid}
    if ptype in (T_RESP, T_EVENT, T_CMD):
        out["params"] = [struct.unpack_from("<I", rest, i)[0] for i in range(0, len(rest) - len(rest) % 4, 4)]
    else:
        out["payload"] = rest
    return out


def rc_name(rc):
    return RC_NAMES.get(rc, f"{rc:#06x}")


# ---------------------------------------------------------------- sockets

def recv_exact(sock, n):
    buf = bytearray()
    while len(buf) < n:
        chunk = sock.recv(min(n - len(buf), 1 << 20))
        if not chunk:
            raise PTPError(f"connection closed after {len(buf)} of {n} bytes")
        buf += chunk
    return bytes(buf)


def recv_packet(sock):
    head = recv_exact(sock, 4)
    (length,) = struct.unpack("<I", head)
    if length < 4 or length > 512 * 1024 * 1024:
        raise PTPError(f"bad packet length {length}")
    return head + recv_exact(sock, length - 4)


class NcSock:
    """A TCP connection through /usr/bin/nc, with the socket calls this module uses.

    macOS Local Network privacy can deny a Python process outgoing LAN connections
    (EHOSTUNREACH) with no prompt, while Apple's own nc is allowed. Talking to nc over pipes
    keeps the PTP code unchanged."""

    def __init__(self, host, port, timeout):
        import subprocess
        self.timeout = timeout
        self.p = subprocess.Popen(["/usr/bin/nc", "-v", "-G", str(max(int(timeout), 1)), host, str(port)],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  bufsize=0)
        line = self._stderr_line(timeout + 2)
        if "succeeded" not in line:
            self.close()
            raise OSError(f"nc: {line.strip() or 'no answer'}")

    def _stderr_line(self, timeout):
        import select
        buf = b""
        end = time.monotonic() + timeout
        while not buf.endswith(b"\n") and time.monotonic() < end:
            r, _, _ = select.select([self.p.stderr], [], [], max(end - time.monotonic(), 0))
            if not r:
                break
            c = os.read(self.p.stderr.fileno(), 1)
            if not c:
                break
            buf += c
        return buf.decode("utf-8", "replace")

    def settimeout(self, t):
        self.timeout = t

    def setsockopt(self, *a):
        pass

    def sendall(self, data):
        try:
            self.p.stdin.write(data)
        except BrokenPipeError as e:
            raise OSError("nc closed") from e

    def recv(self, n):
        import select
        r, _, _ = select.select([self.p.stdout], [], [], self.timeout)
        if not r:
            raise socket.timeout("timed out")
        return os.read(self.p.stdout.fileno(), n)

    def close(self):
        for f in (self.p.stdin, self.p.stdout, self.p.stderr):
            try:
                f.close()
            except OSError:
                pass
        try:
            self.p.terminate()
            self.p.wait(2)
        except Exception:  # noqa: BLE001 - best effort
            self.p.kill()


def connect(host, port, timeout, attempts=3, spacing=2.0, log=None):
    """TCP connect with a few spaced attempts. Never a tight loop. Falls back to nc when macOS
    denies this process the local network (EHOSTUNREACH at once)."""
    last = None
    for i in range(attempts):
        if i:
            time.sleep(max(spacing, 2.0))
        try:
            s = socket.create_connection((host, port), timeout=timeout)
            s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            return s
        except OSError as e:
            last = e
            if log:
                log("tcp_connect_fail", host=host, port=port, attempt=i + 1, error=str(e))
            if e.errno == errno.EHOSTUNREACH:
                try:
                    s = NcSock(host, port, timeout)
                    if log:
                        log("nc_relay", host=host, port=port)
                    return s
                except OSError as e2:
                    last = e2
                    if log:
                        log("nc_relay_fail", host=host, port=port, error=str(e2))
    raise PTPError(f"connect {host}:{port} failed: {last}")


class Session:
    """One command connection to the camera. Every transaction is logged through `log`."""

    def __init__(self, host, port=CMD_PORT, name="ukiyo-wifi-test", timeout=8.0, log=None, guid=None):
        self.host, self.port, self.name, self.timeout = host, port, name, timeout
        self.log = log or (lambda *a, **k: None)
        self.guid = guid or fuji_guid(host)
        self.sock = None
        self.tid = 0
        self.open = False

    def connect_and_init(self, attempts=3, spacing=3.0):
        """TCP connect plus the Fuji init request. Returns the ack dict; raises on INIT_FAIL each time."""
        for i in range(attempts):
            if i:
                time.sleep(max(spacing, 2.0))
            self.sock = connect(self.host, self.port, self.timeout, attempts=2, spacing=2.0, log=self.log)
            self.sock.settimeout(self.timeout)
            t0 = time.monotonic()
            self.sock.sendall(build_init(self.name, self.guid))
            raw = recv_packet(self.sock)
            length, ptype = struct.unpack_from("<II", raw, 0)
            ms = (time.monotonic() - t0) * 1000
            if ptype == INIT_CMD_ACK:
                ack = parse_init_ack(raw[8:])
                self.log("init_ack", attempt=i + 1, ms=round(ms, 1), length=length, conn=ack["conn"],
                         camera_name=ack["name"], raw_len=len(raw))
                time.sleep(0.1)  # libfuji: Fuji cameras need >= 50 ms before OpenSession
                return ack
            self.log("init_fail", attempt=i + 1, ms=round(ms, 1), type=ptype, raw=raw.hex())
            reason = struct.unpack_from("<I", raw, 8)[0] if len(raw) >= 12 else None
            if reason == 0x2019:
                # Busy: Capture One resends the init on the same socket, and the camera then accepts.
                # Closing instead makes the camera stop listening.
                for _ in range(4):
                    time.sleep(1.0)
                    self.sock.sendall(build_init(self.name, self.guid))
                    raw = recv_packet(self.sock)
                    length, ptype = struct.unpack_from("<II", raw, 0)
                    if ptype == INIT_CMD_ACK:
                        ack = parse_init_ack(raw[8:])
                        self.log("init_ack", attempt=i + 1, resent=True, length=length, conn=ack["conn"],
                                 camera_name=ack["name"])
                        time.sleep(0.1)
                        return ack
                    self.log("init_fail", attempt=i + 1, resent=True, type=ptype, raw=raw.hex())
            self.sock.close()
            self.sock = None
        raise PTPError("camera refused the init request (INIT_FAIL) on every attempt")

    def transaction(self, code, params=(), data_out=None, timeout=None, quiet=False):
        """Send one operation. Returns (rc, payload bytes, response params, stats)."""
        if code in FORBIDDEN_OPS:
            raise PTPError(f"operation {code:#06x} is forbidden in this tool")
        if self.sock is None:
            raise PTPError("not connected")
        tid = self.tid
        self.tid += 1
        self.sock.settimeout(timeout or self.timeout)
        t0 = time.monotonic()
        self.sock.sendall(container(T_CMD, code, tid, params))
        if data_out is not None:
            self.sock.sendall(container(T_DATA, code, tid, payload=data_out))
        payload, first = b"", None
        while True:
            pkt = parse_container(recv_packet(self.sock))
            if first is None:
                first = time.monotonic() - t0
            if pkt["type"] == T_DATA:
                payload += pkt["payload"]
            elif pkt["type"] == T_RESP:
                break
            else:
                self.log("unexpected_packet", op=f"{code:#06x}", type=pkt["type"], code=f"{pkt['code']:#06x}")
        total = time.monotonic() - t0
        stats = {"ms": round(total * 1000, 1), "first_ms": round(first * 1000, 1), "bytes": len(payload)}
        rc = pkt["code"]
        if not quiet:
            self.log("op", op=f"{code:#06x}", params=[f"{p:#x}" for p in params], tid=tid, rc=rc_name(rc),
                     **stats)
        return rc, payload, pkt.get("params", []), stats

    def open_session(self):
        """OpenSession(1). Tries transaction id 0 (our USB finding), then 1 (libfuji) once."""
        for tid in (0, 1):
            self.tid = tid
            rc, _, _, _ = self.transaction(OP["OpenSession"], (1,))
            if rc in (OK, 0x201E):
                self.open = True
                return rc
            time.sleep(2.0)
        return rc

    def get_value(self, code, quiet=True):
        rc, data, _, st = self.transaction(OP["GetDevicePropValue"], (code,), quiet=quiet)
        return rc, data, st

    def set_value(self, code, raw):
        rc, _, _, st = self.transaction(OP["SetDevicePropValue"], (code,), data_out=raw)
        return rc, st

    def close(self):
        if self.sock is None:
            return
        try:
            if self.open:
                self.transaction(OP["CloseSession"], timeout=3)
        except (OSError, PTPError):
            pass
        try:
            self.sock.sendall(GOODBYE)
        except OSError:
            pass
        self.sock.close()
        self.sock = None
        self.open = False


# ---------------------------------------------------------------- wireless tether discovery

def pcss_datagram(local_ip):
    return ("DISCOVERY * HTTP/1.1\r\n"
            f"HOST: {local_ip}\r\n"
            "MX: 5\r\n"
            "SERVICE: PCSS/1.0\r\n").encode()


def parse_invite(text):
    """The camera's message on the tether port: 'DSC: ip', 'CAMERANAME: model', 'DSCPORT: port'."""
    out = {"first_line": text.splitlines()[0] if text else ""}
    for line in text.splitlines():
        if ":" in line:
            k, v = line.split(":", 1)
            out[k.strip().upper()] = v.strip()
    return out


def discover_tether(local_ip, broadcasts, wait_s, log, pcss_port=PCSS_PORT, tether_port=TETHER_PORT,
                    stop=lambda: False):
    """Broadcast PCSS every 2 s and wait for the camera to connect to tether_port.

    Returns {"ip", "port", "invite"} or None.
    """
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("0.0.0.0", tether_port))
    srv.listen(2)
    srv.settimeout(2.0)
    udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    udp.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    msg = pcss_datagram(local_ip)
    end = time.monotonic() + wait_s
    sent, failed = 0, set()
    try:
        while time.monotonic() < end and not stop():
            for b in broadcasts:
                try:
                    udp.sendto(msg, (b, pcss_port))
                    sent += 1
                except OSError as e:
                    if b not in failed:
                        failed.add(b)
                        log("pcss_send_fail", dest=b, error=str(e))
            try:
                conn, addr = srv.accept()
            except socket.timeout:
                continue
            conn.settimeout(5.0)
            raw = b""
            try:
                # Read until the blank line that ends the message: one recv can return nothing yet.
                t_end = time.monotonic() + 5.0
                while b"\r\n\r\n" not in raw and time.monotonic() < t_end:
                    try:
                        chunk = conn.recv(2048)
                    except socket.timeout:
                        break
                    if not chunk:
                        break
                    raw += chunk
                inv = parse_invite(raw.decode("utf-8", "replace"))
                if "DSCPORT" in inv:
                    conn.sendall(b"HTTP/1.1 200 OK\r\n")
            finally:
                conn.close()
            if "DSCPORT" not in inv:
                log("tether_invite_incomplete", from_ip=addr[0], bytes=len(raw), first_line=inv["first_line"])
                continue
            ip = inv.get("DSC") or addr[0]
            port = int(inv.get("DSCPORT") or CMD_PORT)
            log("tether_invite", from_ip=addr[0], dsc_ip=ip, port=port, camera=inv.get("CAMERANAME", ""),
                first_line=inv["first_line"], datagrams_sent=sent)
            return {"ip": ip, "port": port, "invite": inv}
    finally:
        srv.close()
        udp.close()
    log("tether_discovery_timeout", seconds=wait_s, datagrams_sent=sent)
    return None
