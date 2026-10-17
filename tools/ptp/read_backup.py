# /// script
# requires-python = ">=3.11"
# dependencies = ["pyusb"]
# ///
"""Read the camera's settings backup (PTP object handle 0) to a file. Read-only.

Usage: read_backup.py LABEL

Needs CONNECTION MODE: USB RAW CONV./BACKUP RESTORE. Sends GetObjectInfo(0) and GetObject(0),
nothing else, and never restores. Saves local/backup/<date>-<LABEL>.bin and a .json with the
object info. The blob holds the body serial at 0x34, so the script writes only to local/backup/.
Run from tools/ptp with `DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib uv run -q read_backup.py LABEL`.
"""
import datetime
import json
import struct
import sys
from pathlib import Path

from read_presets import PTP

OK = 0x2001
REPO = Path(__file__).resolve().parents[2]
OUT_DIR = REPO / "local" / "backup"


def object_info(raw):
    """The fixed head of an ObjectInfo dataset: storage, format, size."""
    storage, fmt, protection, size = struct.unpack_from("<IHHI", raw)
    return {"storage": f"{storage:#010x}", "format": f"{fmt:#06x}", "protection": protection,
            "size": size, "raw": raw.hex()}


def main():
    if len(sys.argv) != 2 or not sys.argv[1].replace("-", "").isalnum():
        sys.exit("usage: read_backup.py LABEL  (letters, digits and dashes)")
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    stem = OUT_DIR / f"{datetime.date.today().isoformat()}-{sys.argv[1]}"
    if stem.with_suffix(".bin").exists():
        sys.exit(f"{stem.with_suffix('.bin')} exists; pick another label")
    p = PTP()
    rc, _ = p.open()
    if rc not in (OK, 0x201E):
        sys.exit(f"OpenSession failed: {rc:#x}")
    try:
        rc, raw = p.op(0x1008, (0,))  # GetObjectInfo
        if rc != OK:
            sys.exit(f"GetObjectInfo(0) answered {rc:#x}; is CONNECTION MODE USB RAW CONV./BACKUP RESTORE?")
        info = object_info(raw)
        rc, blob = p.op(0x1009, (0,))  # GetObject
        if rc != OK:
            sys.exit(f"GetObject(0) answered {rc:#x}")
    finally:
        p.op(0x1003)  # CloseSession
    stem.with_suffix(".bin").write_bytes(blob)
    stem.with_suffix(".json").write_text(json.dumps({**info, "read": len(blob)}, indent=1) + "\n")
    print(f"{len(blob)} bytes (object info says {info['size']}, format {info['format']}) -> "
          f"{stem.with_suffix('.bin').relative_to(REPO)}")


if __name__ == "__main__":
    main()
