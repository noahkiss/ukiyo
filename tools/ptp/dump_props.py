# /// script
# requires-python = ">=3.11"
# dependencies = ["pyusb"]
# ///
"""Dump every PTP device property descriptor a Fujifilm camera advertises. Read-only.

Usage: dump_props.py OUT.json
"""
import json
import struct
import sys

from read_presets import PTP

SCALARS = {1: "b", 2: "B", 3: "h", 4: "H", 5: "i", 6: "I", 7: "q", 8: "Q"}


class Reader:
    def __init__(self, buf):
        self.b, self.i = buf, 0

    def take(self, fmt):
        v = struct.unpack_from("<" + fmt, self.b, self.i)
        self.i += struct.calcsize("<" + fmt)
        return v[0]

    def string(self):
        n = self.take("B")
        s = self.b[self.i:self.i + 2 * n].decode("utf-16-le", "replace").rstrip("\0")
        self.i += 2 * n
        return s

    def array(self, fmt):
        return [self.take(fmt) for _ in range(self.take("I"))]

    def value(self, dtype):
        if dtype == 0xFFFF:
            return self.string()
        if dtype & 0x4000 and dtype & 0xFF in SCALARS:
            return self.array(SCALARS[dtype & 0xFF])
        if dtype in SCALARS:
            return self.take(SCALARS[dtype])
        raise ValueError(f"type {dtype:#x}")


def device_info(raw):
    r = Reader(raw)
    r.take("H"), r.take("I"), r.take("H")
    info = {"vendor_ext": r.string(), "functional_mode": r.take("H")}
    info["operations"] = [f"{x:#06x}" for x in r.array("H")]
    info["events"] = [f"{x:#06x}" for x in r.array("H")]
    props = r.array("H")
    r.array("H"), r.array("H")
    info.update(manufacturer=r.string(), model=r.string(), firmware=r.string())
    return info, props


def prop_desc(raw):
    r = Reader(raw)
    code, dtype, getset = r.take("H"), r.take("H"), r.take("B")
    d = {"type": f"{dtype:#06x}", "rw": bool(getset)}
    d["default"] = r.value(dtype)
    d["current"] = r.value(dtype)
    form = r.take("B")
    if form == 1:
        d["range"] = [r.value(dtype) for _ in range(3)]
    elif form == 2:
        d["enum"] = [r.value(dtype) for _ in range(r.take("H"))]
    return code, d


def main():
    p = PTP()
    rc, _ = p.open()
    rc, raw = p.op(0x1001)  # GetDeviceInfo
    info, props = device_info(raw)
    out = {k: v for k, v in info.items() if k != "serial"}
    out["properties"] = {}
    try:
        for prop in props:
            rc, raw = p.op(0x1014, (prop,))  # GetDevicePropDesc
            key = f"{prop:#06x}"
            if rc != 0x2001:
                rc2, val = p.op(0x1015, (prop,))  # GetDevicePropValue fallback
                if rc2 != 0x2001:
                    out["properties"][key] = {"error": f"desc {rc:#x}, value {rc2:#x}"}
                else:
                    out["properties"][key] = {"bytes": len(val), "raw": val.hex()[:256]}
                continue
            try:
                _, d = prop_desc(raw)
            except (ValueError, struct.error) as e:
                d = {"parse_error": str(e), "raw": raw.hex()}
            out["properties"][key] = d
    finally:
        p.op(0x1003)
    json.dump(out, open(sys.argv[1], "w"), indent=1)
    ok = sum(1 for v in out["properties"].values() if "error" not in v)
    rw = sum(1 for v in out["properties"].values() if v.get("rw"))
    print(f"{info['model']} fw {info['firmware']}: {len(props)} props, {ok} described, {rw} writable, {len(info['operations'])} ops")


if __name__ == "__main__":
    main()
