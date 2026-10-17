"""A fake X-T5 on localhost for --dry-run: Fuji PTP over TCP, wireless tether discovery, live view.

Every packet and value here is synthetic, shaped after the sources in docs/wifi.md. Nothing is
copied from a real camera. Strings that look like serials or owner names exist to test redaction.
"""
import functools
import hashlib
import socket
import struct
import threading
import time

import fujiptpip as fp

MODEL = "X-T5"
CAMERA_NAME = "X-T5-9F3A"            # the suffix is fake; the run must mask it
SERIAL = "FAKE0123456789ABCDEF"
OWNER = "Fake Owner Name"

U16, U32, STR = 4, 6, 0xFFFF

# code: (datatype, writable, value, form)  form: None | ("enum", [..]) | ("range", lo, hi, step)
PROPS = {
    0x5005: (U16, True, 2, ("enum", [2, 4, 6, 0x8001, 0x8006, 0x8007])),
    0x500F: (5, True, -3, None),
    0xD001: (U16, True, 1, ("enum", list(range(1, 21)))),
    0xD16E: (U16, False, 5, None),
    0xD207: (U16, True, 1, ("enum", [1, 2])),
    0xD20B: (STR, False, CAMERA_NAME, None),
    0xD212: (0, False, b"\x01\x00\x0e\xd2\x01\x00\x00\x00", None),
    0xD215: (STR, True, OWNER, None),
    0xD22E: (U16, False, 1, None),
    0xD307: (U16, False, 2, None),
    0xD36D: (STR, False, "XF23mmF1.4 R LM WR SN 2AB01234", None),
    0xD381: (U16, True, 1, ("enum", [1, 2, 3, 4, 5, 6])),
    0xDF00: (U16, True, 2, None),
    0xDF24: (U32, True, 0x2000C, None),
}
ADVERTISED_OPS = [0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1007, 0x1008, 0x1009, 0x100A, 0x1014,
                  0x1015, 0x1016, 0x1018, 0x101B, 0x101C, 0x902B]
APO_SECONDS = {1: 300, 2: 120, 3: None, 4: 60, 5: 30, 6: 15}


@functools.lru_cache(maxsize=16)
def jpeg(seed, size):
    """A fake JPEG: SOI, filler from a hash chain, EOI."""
    out = bytearray(b"\xff\xd8\xff\xe0")
    h = hashlib.sha256(seed.encode()).digest()
    while len(out) < size - 2:
        h = hashlib.sha256(h).digest()
        out += h * 64
    return bytes(out[:size - 2]) + b"\xff\xd9"


OBJECTS = {h: (f"DSCF{1000 + h:04d}.{'RAF' if h % 2 else 'JPG'}", 3_000_000 + h * 250_000)
           for h in range(1, 9)}


def device_info():
    w = struct.pack
    def arr(xs):
        return w("<I", len(xs)) + b"".join(w("<H", x) for x in xs)
    return (w("<HIH", 100, 0x0E, 100) + fp.ptp_string("fujifilm.co.jp: 1.0;") + w("<H", 0)
            + arr(ADVERTISED_OPS) + arr([0x4002, 0xC004]) + arr(sorted(PROPS)) + arr([0x3801]) + arr([0x3801, 0xB103])
            + fp.ptp_string("FUJIFILM") + fp.ptp_string(MODEL) + fp.ptp_string("4.31") + fp.ptp_string(SERIAL))


def prop_desc(code, cur=None):
    dtype, rw, val, form = PROPS[code]
    val = val if cur is None else cur
    head = struct.pack("<HHB", code, dtype, int(rw))
    if dtype == 0:
        return head  # datatype 0: nothing after the flag (protocol.md)
    body = fp.pack_value(dtype, val) * 2
    if form is None:
        return head + body + b"\x00"
    if form[0] == "enum":
        return head + body + b"\x02" + struct.pack("<H", len(form[1])) + b"".join(fp.pack_value(dtype, v) for v in form[1])
    return head + body + b"\x01" + b"".join(fp.pack_value(dtype, v) for v in form[1:])


class FakeCamera:
    def __init__(self, time_scale=0.02, quirks=()):
        self.scale = time_scale
        self.quirks = set(quirks)
        self.values = {c: p[2] for c, p in PROPS.items()}
        self.cmd = self._listen()
        self.jpeg = self._listen()
        self.pcss = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.pcss.bind(("127.0.0.1", 0))
        self.ports = {"cmd": self.cmd.getsockname()[1], "jpeg": self.jpeg.getsockname()[1],
                      "pcss": self.pcss.getsockname()[1]}
        self.tether_port = None
        self.refused_once = False
        self.busy_once = False
        self.last_reset = time.monotonic()
        self.stop = threading.Event()
        self.powered_off = False

    @staticmethod
    def _listen():
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind(("127.0.0.1", 0))
        s.listen(4)
        return s

    def start(self, tether_port):
        self.tether_port = tether_port
        for fn in (self._serve_cmd, self._serve_pcss, self._serve_jpeg):
            threading.Thread(target=fn, daemon=True).start()
        return self

    def close(self):
        self.stop.set()
        for s in (self.cmd, self.jpeg, self.pcss):
            try:
                s.close()
            except OSError:
                pass

    # ------------------------------------------------------------ discovery

    def _serve_pcss(self):
        self.pcss.settimeout(0.5)
        while not self.stop.is_set():
            try:
                data, addr = self.pcss.recvfrom(2048)
            except (socket.timeout, OSError):
                continue
            if b"SERVICE: PCSS/1.0" not in data:
                continue
            time.sleep(0.3)
            try:
                c = socket.create_connection(("127.0.0.1", self.tether_port), timeout=2)
                c.sendall((f"NOTIFY * HTTP/1.1\r\nDSC: 127.0.0.1\r\nCAMERANAME: {MODEL}\r\n"
                           f"DSCPORT: {self.ports['cmd']}\r\n\r\n").encode())
                c.recv(64)
                c.close()
                return
            except OSError:
                continue

    # ------------------------------------------------------------ live view pipe

    def _serve_jpeg(self):
        self.jpeg.settimeout(0.5)
        while not self.stop.is_set():
            try:
                c, _ = self.jpeg.accept()
            except (socket.timeout, OSError):
                continue
            frame = jpeg("liveview", 60_000)
            c.sendall(struct.pack("<I", 4 + 14 + len(frame)) + b"\x00" * 12 + b"\xfe\x7f" + frame)
            time.sleep(0.5)
            c.close()

    # ------------------------------------------------------------ command pipe

    def _apo_expired(self):
        secs = APO_SECONDS.get(self.values[0xD381])
        return secs is not None and time.monotonic() - self.last_reset > secs * self.scale

    def _serve_cmd(self):
        self.cmd.settimeout(0.5)
        while not self.stop.is_set():
            try:
                c, _ = self.cmd.accept()
            except (socket.timeout, OSError):
                continue
            threading.Thread(target=self._client, args=(c,), daemon=True).start()

    def _client(self, c):
        c.settimeout(0.5)
        try:
            init = fp.recv_packet(c)
            if len(init) != fp.INIT_LEN or struct.unpack_from("<I", init, 8)[0] != fp.FUJI_INIT_VERSION:
                c.close()
                return
            if not self.refused_once or "refuse" in self.quirks:  # the camera has not accepted yet
                self.refused_once = True
                c.sendall(struct.pack("<II", 12, fp.INIT_FAIL) + struct.pack("<I", 0x2019))
                c.close()
                return
            name = CAMERA_NAME.encode("utf-16-le") + b"\x00\x00"
            body = struct.pack("<I", 1) + b"\x11" * 16 + name
            c.sendall(struct.pack("<II", 8 + len(body), fp.INIT_CMD_ACK) + body)
            self.last_reset = time.monotonic()
            while not self.stop.is_set():
                if self._apo_expired():
                    self.powered_off = True
                    c.close()       # auto power off: the camera leaves the network
                    return
                try:
                    pkt = fp.recv_packet(c)
                except socket.timeout:
                    continue
                if pkt == fp.GOODBYE:
                    c.close()
                    return
                req = fp.parse_container(pkt)
                data_in = None
                if req["code"] == 0x1016:
                    c.settimeout(5)
                    data_in = fp.parse_container(fp.recv_packet(c))["payload"]
                    c.settimeout(0.5)
                rc, payload, params = self._handle(req["code"], req["params"], data_in)
                if payload is not None:
                    c.sendall(fp.container(fp.T_DATA, req["code"], req["tid"], payload=payload))
                c.sendall(fp.container(fp.T_RESP, rc, req["tid"], params))
        except (OSError, fp.PTPError):
            pass
        finally:
            c.close()

    def _handle(self, code, params, data_in):
        p = params + [0, 0, 0]
        if code == 0x1001:
            return fp.OK, (b"" if "empty-deviceinfo" in self.quirks else device_info()), []
        if code in (0x1002, 0x1003, 0x1018):
            return fp.OK, None, []
        if code == 0x101C:
            return fp.OK, None, []
        if code == 0x902B:
            entries = [prop_desc(cd, self.values[cd]) for cd in sorted(PROPS)]
            return fp.OK, struct.pack("<I", len(entries)) + b"".join(struct.pack("<I", len(e)) + e for e in entries), []
        if code == 0x1015:
            cd = p[0]
            if cd not in PROPS:
                return 0x200A, None, []
            dtype = PROPS[cd][0]
            v = self.values[cd]
            return fp.OK, (v if dtype == 0 else fp.pack_value(dtype, v)), []
        if code == 0x1014:
            return (fp.OK, prop_desc(p[0], self.values[p[0]]), []) if p[0] in PROPS else (0x200A, None, [])
        if code == 0x1016:
            cd = p[0]
            if cd != 0xD381:
                return 0x200F, None, []
            if not self.busy_once:
                self.busy_once = True
                return 0x2019, None, []
            new = struct.unpack("<H", data_in)[0]
            if new == self.values[cd]:
                return 0xA002, None, []
            self.values[cd] = new
            self.last_reset = time.monotonic()
            return fp.OK, None, []
        if code == 0x1004:
            return fp.OK, struct.pack("<II", 1, 0x10000001), []
        if code == 0x1005:
            return fp.OK, struct.pack("<HHHQQI", 4, 2, 0, 64 << 30, 20 << 30, 900) + fp.ptp_string("SD1") + fp.ptp_string(""), []
        if code == 0x1007:
            hs = [] if "no-objects" in self.quirks else sorted(OBJECTS)
            return fp.OK, struct.pack("<I", len(hs)) + b"".join(struct.pack("<I", h) for h in hs), []
        if code == 0x1008:
            if p[0] not in OBJECTS:
                return 0x2009, None, []
            fn, size = OBJECTS[p[0]]
            fmt = 0xB103 if fn.endswith("RAF") else 0x3801
            if "short-size" in self.quirks:
                size = 100_000
            head = struct.pack("<IHHIHIIIIIIIHII", 0x10000001, fmt, 0, size, 0x3808, 9000, 160, 120,
                               7728, 5152, 24, 0, 0, 0, 0)
            return fp.OK, head + fp.ptp_string(fn) + fp.ptp_string("20261017T120000") + fp.ptp_string("") + fp.ptp_string(""), []
        if code == 0x100A:
            return (fp.OK, jpeg(f"thumb{p[0]}", 9000), []) if p[0] in OBJECTS else (0x2009, None, [])
        if code == 0x1009:
            if p[0] not in OBJECTS:
                return 0x2009, None, []
            fn, size = OBJECTS[p[0]]
            return fp.OK, jpeg(fn, size), []
        if code == 0x101B:
            if p[0] not in OBJECTS:
                return 0x2009, None, []
            fn, size = OBJECTS[p[0]]
            blob = jpeg(fn, size)
            return fp.OK, blob[p[1]:p[1] + p[2]], [min(p[2], max(0, size - p[1]))]
        return 0x2005, None, []
