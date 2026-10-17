# /// script
# requires-python = ">=3.11"
# dependencies = ["pyusb"]
# ///
"""Read Fujifilm custom presets C1-C7 over PTP (USB RAW CONV./BACKUP RESTORE mode).

Only write: the slot selector D18C, which is restored to its original value at the end.
"""
import json
import struct
import sys

import usb.core
import usb.util

import owner_lock

VENDOR = 0x04CB
NAMES = {
    0xD18D: "Name", 0xD18E: "ImageSize", 0xD18F: "ImageQuality", 0xD190: "DynamicRange%",
    0xD191: "?D191", 0xD192: "FilmSimulation", 0xD193: "MonoWC", 0xD194: "MonoMG",
    0xD195: "GrainEffect", 0xD196: "ColorChrome", 0xD197: "ColorChromeFxBlue",
    0xD198: "SmoothSkin", 0xD199: "WhiteBalance", 0xD19A: "WBShiftR", 0xD19B: "WBShiftB",
    0xD19C: "ColorTemp", 0xD19D: "HighlightTone", 0xD19E: "ShadowTone", 0xD19F: "Color",
    0xD1A0: "Sharpness", 0xD1A1: "HighIsoNR", 0xD1A2: "Clarity", 0xD1A3: "LongExpNR",
    0xD1A4: "ColorSpace", 0xD1A5: "?D1A5",
}


class PTP:
    def __init__(self):
        owner_lock.hold()  # before any USB access; refuses while Ukiyo owns the camera
        self.dev = usb.core.find(idVendor=VENDOR)
        if self.dev is None:
            sys.exit("no Fujifilm camera found")
        self.dev.set_configuration()
        intf = self.dev.get_active_configuration()[(0, 0)]
        eps = [e for e in intf if usb.util.endpoint_type(e.bmAttributes) == usb.util.ENDPOINT_TYPE_BULK]
        self.out = next(e for e in eps if usb.util.endpoint_direction(e.bEndpointAddress) == usb.util.ENDPOINT_OUT)
        self.inp = next(e for e in eps if usb.util.endpoint_direction(e.bEndpointAddress) == usb.util.ENDPOINT_IN)
        self.tid = 0

    def open(self):
        """OpenSession; only if the endpoints are stalled from a crashed session, reset once and retry."""
        try:
            return self.op(0x1002, (1,))
        except usb.core.USBError:
            self.dev.reset()
            self.__init__()
            return self.op(0x1002, (1,))

    def _recv(self):
        buf = bytes(self.inp.read(1 << 20, timeout=5000))
        length = struct.unpack_from("<I", buf)[0]
        while len(buf) < length:
            buf += bytes(self.inp.read(1 << 20, timeout=5000))
        return struct.unpack_from("<HH", buf, 4), buf[12:length]

    def op(self, code, params=(), data=None):
        self.tid += 1
        self.out.write(struct.pack("<IHHI", 12 + 4 * len(params), 1, code, self.tid) + b"".join(struct.pack("<I", p) for p in params))
        if data is not None:
            self.out.write(struct.pack("<IHHI", 12 + len(data), 2, code, self.tid) + data)
        payload = b""
        while True:
            (kind, rc), body = self._recv()
            if kind == 2:
                payload = body
            elif kind == 3:
                return rc, payload


def ptp_string(b):
    if not b:
        return ""
    n = b[0]
    return b[1:1 + 2 * n].decode("utf-16-le").rstrip("\0")


def main():
    p = PTP()
    rc, _ = p.open()
    if rc not in (0x2001, 0x201E):
        sys.exit(f"OpenSession failed: {rc:#x}")
    rc, raw = p.op(0x1015, (0xD18C,))
    if rc != 0x2001:
        sys.exit(f"cannot read slot selector D18C: {rc:#x}")
    orig = struct.unpack_from("<H", raw)[0]
    slots = []
    try:
        for slot in range(1, 8):
            rc, _ = p.op(0x1016, (0xD18C,), struct.pack("<H", slot))
            if rc != 0x2001:
                slots.append({"slot": slot, "error": f"select {rc:#x}"})
                continue
            entry = {"slot": slot}
            for prop, name in NAMES.items():
                rc, val = p.op(0x1015, (prop,))
                if rc != 0x2001:
                    entry[name] = f"err {rc:#x}"
                elif prop == 0xD18D:
                    entry[name] = ptp_string(val)
                elif len(val) == 2:
                    entry[name] = struct.unpack("<h", val)[0]
                elif len(val) == 4:
                    entry[name] = struct.unpack("<i", val)[0]
                else:
                    entry[name] = val.hex()
            slots.append(entry)
    finally:
        p.op(0x1016, (0xD18C,), struct.pack("<H", orig))
        p.op(0x1003)  # CloseSession
    print(json.dumps({"original_slot": orig, "slots": slots}, indent=1))


if __name__ == "__main__":
    main()
