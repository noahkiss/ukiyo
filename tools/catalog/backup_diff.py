# /// script
# requires-python = ">=3.11"
# ///
"""Diff settings backups (read_backup.py output) taken one menu change apart. No camera access.

Usage: backup_diff.py BLOB BLOB [BLOB ...] [--header]

Each blob is compared with the one before it. Changed bytes are grouped into runs; each run
prints its offset and old -> new bytes, plus u16 and u32 little-endian readings when the run fits.
The header (offsets below 0x60: model, serial) is skipped unless --header, and the serial run at
0x34 is never printed. The checksum at offset 176 (0xB0) moves with every change; it is labelled.
"""
import argparse
import struct
import sys
from pathlib import Path

HEADER_END = 0x60
SERIAL = range(0x34, 0x54)
CHECKSUM = range(176, 178)


def runs(a, b, start):
    """Runs of changed offsets; bytes within 2 of each other join one run."""
    out, cur = [], None
    for i in range(start, max(len(a), len(b))):
        x = a[i] if i < len(a) else None
        y = b[i] if i < len(b) else None
        if x == y:
            continue
        if cur and i - cur[1] <= 2:
            cur[1] = i
        else:
            cur = [i, i]
            out.append(cur)
    return [(s, e + 1) for s, e in out]


def reading(buf, s, e):
    n = e - s
    if n <= 2 and e <= len(buf) and s + 2 <= len(buf):
        return f"u16 {struct.unpack_from('<H', buf, s)[0]}"
    if n <= 4 and s + 4 <= len(buf):
        return f"u32 {struct.unpack_from('<I', buf, s)[0]}"
    return ""


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("blobs", nargs="+", type=Path)
    ap.add_argument("--header", action="store_true", help="also diff the header (never the serial)")
    a = ap.parse_args()
    if len(a.blobs) < 2:
        sys.exit("need two or more blobs")
    data = [p.read_bytes() for p in a.blobs]
    start = 0 if a.header else HEADER_END
    for (pa, da), (pb, db) in zip(zip(a.blobs, data), zip(a.blobs[1:], data[1:])):
        print(f"\n{pa.name} -> {pb.name}" + (f" (size {len(da)} -> {len(db)})" if len(da) != len(db) else ""))
        shown = 0
        for s, e in runs(da, db, start):
            if any(i in SERIAL for i in range(s, e)):
                continue
            tag = " checksum" if s in CHECKSUM else ""
            ra, rb = reading(da, s, e), reading(db, s, e)
            extra = f"  ({ra} -> {rb})" if ra else ""
            print(f"  {s:#07x} +{e - s}: {da[s:e].hex(' ')} -> {db[s:e].hex(' ')}{extra}{tag}")
            shown += 1
        if not shown:
            print("  no change")


if __name__ == "__main__":
    main()
