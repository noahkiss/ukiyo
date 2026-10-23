# /// script
# requires-python = ">=3.11"
# dependencies = ["pyusb>=1.2,<2"]
# ///
"""Poll every readable PTP property about once a second and log the ones that change. Read-only.

Usage: watch_props.py PROPS.json NAMES.json LOG
"""
import json
import sys
import time

from read_presets import PTP

# Counters and live state that change on their own; logged once a minute at most.
NOISY = {0xD36A, 0xD36B, 0xD212, 0xD242, 0x5001, 0xD171, 0xD170, 0xD347, 0x5011, 0xD22F}


def main():
    props = json.load(open(sys.argv[1]))["properties"]
    names = json.load(open(sys.argv[2]))
    log = open(sys.argv[3], "a", buffering=1)
    codes = [int(k, 16) for k, v in props.items() if "error" not in v]
    p = PTP()
    p.open()
    last, quiet_until = {}, {}
    log.write(f"{time.strftime('%H:%M:%S')} watching {len(codes)} properties\n")
    try:
        while True:
            for c in codes:
                rc, val = p.op(0x1015, (c,))
                v = val.hex() if rc == 0x2001 else f"err {rc:#x}"
                old = last.get(c)
                last[c] = v
                if old is None or old == v:
                    continue
                if c in NOISY and time.time() < quiet_until.get(c, 0):
                    continue
                quiet_until[c] = time.time() + 60
                name = names.get(f"{c:#06x}", "?")
                log.write(f"{time.strftime('%H:%M:%S')} {c:#06x} {name}: {old[:48]} -> {v[:48]}\n")
            time.sleep(0.5)
    finally:
        p.op(0x1003)


if __name__ == "__main__":
    main()
