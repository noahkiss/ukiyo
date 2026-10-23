# /// script
# requires-python = ">=3.11"
# dependencies = ["pyusb"]
# ///
"""Write one uint16 PTP property, reading it before and after.

Usage: set_prop.py 0xd381 3
"""
import struct
import sys

from read_presets import PTP


def read(p, code):
    """(value, shown): the uint16 value, or None with the answer code and raw bytes."""
    rc, val = p.op(0x1015, (code,))
    if rc == 0x2001 and len(val) == 2:
        v = struct.unpack("<H", val)[0]
        return v, str(v)
    return None, f"rc {rc:#x} {val.hex()}"


def main():
    code, value = int(sys.argv[1], 16), int(sys.argv[2])
    p = PTP()
    rc, _ = p.open()
    if rc not in (0x2001, 0x201E):
        sys.exit(f"OpenSession failed: {rc:#x}")
    try:
        before, shown = read(p, code)
        print("before:", shown)
        if before is None:
            sys.exit(f"not written: {code:#06x} did not read back as a uint16")
        rc, _ = p.op(0x1016, (code,), struct.pack("<H", value))
        print(f"write {value}: rc {rc:#x}")
        print("after:", read(p, code)[1])
    finally:
        p.op(0x1003)


if __name__ == "__main__":
    main()
