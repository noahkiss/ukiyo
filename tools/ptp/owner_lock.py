"""Ukiyo's camera owner lock, for the discovery scripts (docs/agent-interface.md, "One USB owner").

`ukiyo serve` holds an exclusive flock on owner.lock in Ukiyo's data folder while it owns the
camera. A script here takes the same lock before it touches USB, and keeps it until it exits, so
`serve` cannot start in the meantime. When Ukiyo holds it, the script stops with a message.

The data folder is ~/Library/Application Support/com.nkmkdigital.ukiyo, or UKIYO_DATA_DIR.
"""
import fcntl
import os
import sys
from pathlib import Path

BUNDLE_ID = "com.nkmkdigital.ukiyo"
OLD_DATA_DIR = "app.ukiyo"

_held = None  # the open lock file, kept for the life of the process


def data_dir() -> Path:
    d = os.environ.get("UKIYO_DATA_DIR")
    if d:
        return Path(d)
    return Path.home() / "Library" / "Application Support" / BUNDLE_ID


def hold() -> None:
    """Takes owner.lock for the rest of the process, or exits with a message if Ukiyo holds it.
    Calling it again is a no-op."""
    global _held
    if _held is not None:
        return
    d = data_dir()
    if not d.exists() and (d.parent / OLD_DATA_DIR).is_dir():
        # Creating the folder now would stop Ukiyo moving its old one here.
        sys.exit(f"Start Ukiyo once first: it moves its data folder to {d}.")
    d.mkdir(mode=0o700, parents=True, exist_ok=True)
    f = open(d / "owner.lock", "a+")
    try:
        fcntl.flock(f.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        f.close()
        sys.exit(
            "Ukiyo owns the camera (owner.lock is held). Quit the Ukiyo app and any `ukiyo` command;\n"
            "ukiyo serve exits 2 minutes after its last client leaves. Then run this script again."
        )
    f.truncate(0)
    f.write(f"{os.getpid()}\n")
    f.flush()
    _held = f
