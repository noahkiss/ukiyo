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
    rc, val = p.op(0x1015, (code,))
    return f"{struct.unpack('<H', val)[0]}" if rc == 0x2001 and len(val) == 2 else f"rc {rc:#x} {val.hex()}"


def main():
    code, value = int(sys.argv[1], 16), int(sys.argv[2])
    p = PTP()
    p.open()
    try:
        print("before:", read(p, code))
        rc, _ = p.op(0x1016, (code,), struct.pack("<H", value))
        print(f"write {value}: rc {rc:#x}")
        print("after:", read(p, code))
    finally:
        p.op(0x1003)


if __name__ == "__main__":
    main()
