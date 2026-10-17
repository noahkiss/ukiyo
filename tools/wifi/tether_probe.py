# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Read-only wireless tether probe: discover the X-T5 on the LAN, connect, and read what it shows.

Usage (camera in CONNECTION MODE > WIRELESS TETHER SHOOTING FIXED, on the Mac's network):
  uv run -q tether_probe.py [--wait 120] [--out DIR]

No dialogs, so it runs from a shell that has Local Network permission but no GUI. It tries
standard PTP/IP first (the camera offers port 15740), then the Fuji 82-byte init. Operations:
OpenSession, GetDeviceInfo, GetStorageIDs, GetStorageInfo, GetObjectHandles, and a few
GetDevicePropValue reads. No writes. Logs to local/wifi-test/probe-<ts>/ (serials masked).
"""
import argparse
import datetime as dt
import json
import socket
import struct
import sys
import time
import uuid
from pathlib import Path

import fujiptpip as fp

REPO = Path(__file__).resolve().parents[2]
NAME = "ukiyo-probe"
PROPS = {0xD381: "AUTO POWER OFF", 0xD22E: "screen state", 0xD16E: "USB mode", 0xD207: "priority",
         0x5001: "battery", 0xD242: "battery (Fuji)"}

# Standard PTP/IP packet types
INIT_CMD_REQ, INIT_CMD_ACK, INIT_EVT_REQ, INIT_EVT_ACK, INIT_FAIL = 1, 2, 3, 4, 5
OP_REQ, OP_RESP, EVENT, START_DATA, DATA, CANCEL, END_DATA = 6, 7, 8, 9, 10, 11, 12


class Log:
    def __init__(self, out):
        self.out = out
        out.mkdir(parents=True, exist_ok=True)
        self.f = open(out / "events.jsonl", "a")
        self.t0 = time.monotonic()

    def __call__(self, kind, **kw):
        rec = {"t": round(time.monotonic() - self.t0, 3), "kind": kind, **kw}
        self.f.write(json.dumps(rec, default=str) + "\n")
        self.f.flush()
        print(json.dumps(rec, default=str)[:400], flush=True)


def recv_pkt(sock):
    head = fp.recv_exact(sock, 8)
    length, ptype = struct.unpack("<II", head)
    return ptype, fp.recv_exact(sock, length - 8)


class PtpIp:
    """Standard PTP/IP (ISO 15740 annex): a command and an event connection."""

    def __init__(self, host, port, log, timeout=8.0):
        self.host, self.port, self.log, self.timeout = host, port, log, timeout
        self.cmd = self.evt = None
        self.tid = 0

    def init(self):
        self.cmd = fp.connect(self.host, self.port, self.timeout, attempts=2, spacing=2.0, log=self.log)
        self.cmd.settimeout(self.timeout)
        body = fp.fuji_guid(self.host) + (NAME + "\x00").encode("utf-16-le") + struct.pack("<I", 0x00010000)
        self.cmd.sendall(struct.pack("<II", 8 + len(body), INIT_CMD_REQ) + body)
        ptype, p = recv_pkt(self.cmd)
        if ptype != INIT_CMD_ACK:
            self.log("ptpip_init_fail", type=ptype, payload=p.hex()[:64])
            raise fp.PTPError(f"PTP/IP init answered type {ptype}")
        conn = struct.unpack_from("<I", p, 0)[0]
        name = p[20:-4].decode("utf-16-le", "replace").split("\x00")[0] if len(p) > 24 else ""
        ver = struct.unpack_from("<I", p, len(p) - 4)[0] if len(p) >= 24 else None
        self.log("ptpip_init_ack", conn=conn, camera_name=name, version=f"{ver:#x}" if ver else None)
        time.sleep(0.1)
        self.evt = fp.connect(self.host, self.port, self.timeout, attempts=2, spacing=2.0, log=self.log)
        self.evt.settimeout(self.timeout)
        self.evt.sendall(struct.pack("<III", 12, INIT_EVT_REQ, conn))
        ptype, p = recv_pkt(self.evt)
        self.log("ptpip_event_init", type=ptype, ok=ptype == INIT_EVT_ACK)
        return name

    def op(self, code, params=()):
        if code in fp.FORBIDDEN_OPS:
            raise fp.PTPError(f"operation {code:#06x} is forbidden here")
        tid = self.tid
        self.tid += 1
        body = struct.pack("<IHI", 1, code, tid) + b"".join(struct.pack("<I", x) for x in params)
        t0 = time.monotonic()
        self.cmd.sendall(struct.pack("<II", 8 + len(body), OP_REQ) + body)
        data = b""
        while True:
            ptype, p = recv_pkt(self.cmd)
            if ptype in (DATA, END_DATA):
                data += p[4:]
            elif ptype == START_DATA:
                pass
            elif ptype == OP_RESP:
                rc = struct.unpack_from("<H", p, 0)[0]
                break
            else:
                self.log("ptpip_unexpected", type=ptype)
        ms = round((time.monotonic() - t0) * 1000, 1)
        self.log("op", op=f"{code:#06x}", params=[f"{x:#x}" for x in params], rc=fp.rc_name(rc),
                 bytes=len(data), ms=ms)
        return rc, data

    def close(self):
        if self.cmd is None:
            return
        try:
            self.op(fp.OP["CloseSession"])
        except (OSError, fp.PTPError):
            pass
        for s in (self.evt, self.cmd):
            if s:
                s.close()


class FujiInit:
    """The Fuji 82-byte init on one socket (fujiptpip.Session), with the same op() shape."""

    def __init__(self, host, port, log):
        # 30 s: the camera may wait for OK on its screen before it answers the init (libfuji).
        self.s = fp.Session(host, port, name=NAME, timeout=30.0, log=log)

    def init(self):
        return self.s.connect_and_init(attempts=2)["name"]

    def op(self, code, params=()):
        rc, data, _, _ = self.s.transaction(code, params)
        return rc, data

    def close(self):
        self.s.close()


def local_ip():
    u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        u.connect(("192.0.2.1", 9))  # no packet is sent; it picks the interface
        return u.getsockname()[0]
    finally:
        u.close()


def lan_ip_and_broadcast():
    import subprocess
    ip = subprocess.run(["ipconfig", "getifaddr", "en0"], capture_output=True, text=True).stdout.strip()
    mask = subprocess.run(["ipconfig", "getoption", "en0", "subnet_mask"], capture_output=True,
                          text=True).stdout.strip()
    if not ip:
        return local_ip(), ["255.255.255.255"]
    a = struct.unpack(">I", socket.inet_aton(ip))[0]
    m = struct.unpack(">I", socket.inet_aton(mask or "255.255.255.0"))[0]
    return ip, [socket.inet_ntoa(struct.pack(">I", a | (~m & 0xFFFFFFFF))), "255.255.255.255"]


def survey(cam, log, findings):
    rc = cam.op(fp.OP["OpenSession"], (1,))[0]
    findings["open_session"] = fp.rc_name(rc)
    if rc not in (fp.OK, 0x201E):
        return
    rc, data = cam.op(fp.OP["GetDeviceInfo"])
    if rc == fp.OK and data:
        info = fp.parse_device_info(data)
        info["serial"] = "<masked>" if info.get("serial") else ""
        findings["device_info"] = {k: info[k] for k in ("manufacturer", "model", "device_version",
                                                        "vendor_ext_desc", "functional_mode")}
        findings["counts"] = {k: len(info[k]) for k in ("operations", "events", "properties")}
        findings["operations"] = [f"{x:#06x}" for x in info["operations"]]
        findings["properties"] = [f"{x:#06x}" for x in info["properties"]]
        log("device_info", **findings["device_info"], **findings["counts"])
    else:
        findings["device_info"] = {"rc": fp.rc_name(rc), "bytes": len(data)}
    findings["props"] = {}
    for code, label in PROPS.items():
        rc, data = cam.op(fp.OP["GetDevicePropValue"], (code,))
        findings["props"][f"{code:#06x}"] = {"label": label, "rc": fp.rc_name(rc), "hex": data.hex()[:32]}
        time.sleep(0.05)
    rc, data = cam.op(fp.OP["GetStorageIDs"])
    ids = fp.parse_u32_array(data) if rc == fp.OK else []
    findings["storage_ids"] = {"rc": fp.rc_name(rc), "ids": [f"{i:#x}" for i in ids]}
    for sid in ids[:2]:
        rc, data = cam.op(fp.OP["GetStorageInfo"], (sid,))
        if rc == fp.OK:
            si = fp.parse_storage_info(data)
            findings.setdefault("storage", {})[f"{sid:#x}"] = {k: v for k, v in si.items() if "label" not in k}
        rc, data = cam.op(fp.OP["GetObjectHandles"], (sid, 0, 0))
        n = len(fp.parse_u32_array(data)) if rc == fp.OK else None
        findings.setdefault("objects", {})[f"{sid:#x}"] = {"rc": fp.rc_name(rc), "count": n}
        log("objects", storage=f"{sid:#x}", rc=fp.rc_name(rc), count=n)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--wait", type=float, default=120.0, help="discovery time in seconds")
    ap.add_argument("--host", help="skip discovery: the camera's IP")
    ap.add_argument("--port", type=int, default=15740)
    ap.add_argument("--style", choices=("ptpip", "fuji"), default="fuji", help="init to try first")
    ap.add_argument("--out", default=str(REPO / "local" / "wifi-test" / f"probe-{dt.datetime.now():%Y%m%d-%H%M%S}"))
    args = ap.parse_args()
    sys.path.insert(0, str(REPO / "tools" / "ptp"))
    import owner_lock
    owner_lock.hold()
    log = Log(Path(args.out))
    findings = {}
    host, port = args.host, args.port
    if not host:
        ip, bcasts = lan_ip_and_broadcast()
        log("discovery_start", local_ip=ip, broadcasts=bcasts, wait_s=args.wait)
        found = fp.discover_tether(ip, bcasts, args.wait, log)
        if not found:
            findings["discovery"] = "timeout"
            print(json.dumps(findings))
            return 1
        host, port = found["ip"], found["port"]
        findings["discovery"] = {"port": port, "first_line": found["invite"]["first_line"],
                                 "keys": sorted(found["invite"])}
        time.sleep(1.0)
    for kind in ((FujiInit, PtpIp) if args.style == "fuji" else (PtpIp, FujiInit)):
        cam = kind(host, port, log) if kind is FujiInit else kind(host, port, log)
        try:
            findings["init"] = {"style": kind.__name__, "camera_name": cam.init()}
            survey(cam, log, findings)
            break
        except (OSError, fp.PTPError) as e:
            findings.setdefault("init_errors", []).append(f"{kind.__name__}: {type(e).__name__}: {e}")
            log("init_error", style=kind.__name__, error=f"{type(e).__name__}: {e}")
            time.sleep(3.0)
        finally:
            cam.close()
    (Path(args.out) / "findings.json").write_text(json.dumps(findings, indent=2, default=str))
    print("\nFINDINGS " + json.dumps({k: v for k, v in findings.items() if k not in ("operations", "properties")},
                                     default=str))
    return 0


if __name__ == "__main__":
    sys.exit(main())
