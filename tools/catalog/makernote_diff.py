# /// script
# requires-python = ">=3.11"
# ///
"""Diff the FujiFilm MakerNote of photos shot one setting change apart. No camera access.

Usage:
  makernote_diff.py PHOTO [PHOTO ...] [--steps STEPS] [--log SESSION.jsonl] [--no-photo ID,..]
                    [--all] [--json OUT.json]

Photos are sorted by DateTimeOriginal and SubSecTimeOriginal, then by file name. The first photo
is the baseline; each later photo is compared with the one before it.

--steps names a guided_map steps file (a name in tools/ptp/steps/ or a path). Photo N+1 then
belongs to step N of that file, so take one photo after every step. A step that got no photo
goes in --no-photo; the restore step is left out on its own.
--log adds the PTP side: for each step, the codes that moved in a guided_map session log
(local/mapping/<date>-<name>.jsonl), matched by step id.
--all also prints tags that change on every shot (counters, focus and face data).

Output is the step, its PTP changes, then each MakerNote tag that changed: id, name, old -> new
in raw numbers (exiftool -n). The internal serial number tag is never printed or saved.
Keep photos and --json output in local/: photos carry the owner strings and lens serial.
Needs exiftool on PATH (brew install exiftool).
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
STEPS_DIR = HERE.parent / "ptp" / "steps"
GROUP = "FujiFilm"
NEVER = {"InternalSerialNumber"}  # 0x0010: the body serial
# Tags that move between any two shots, whatever the setting.
VOLATILE = {
    "ImageCount", "ImageGeneration", "FacesDetected", "FacePositions", "NumFaceElements",
    "FaceElementTypes", "FaceElementPositions", "FocusPixel", "FrameNumber", "AFAreaPoint",
    "AFAreaZoneSize", "FocusWarning", "BlurWarning", "ExposureWarning", "SequenceNumber",
}


def read(photos):
    cmd = ["exiftool", "-j", "-a", "-u", "-H", "-n", "-G1", f"-{GROUP}:all",
           "-DateTimeOriginal", "-SubSecTimeOriginal", "-FileName", *map(str, photos)]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, check=True).stdout
    except FileNotFoundError:
        sys.exit("exiftool not found; brew install exiftool")
    except subprocess.CalledProcessError as e:
        sys.exit(e.stderr.strip() or "exiftool failed")
    shots = []
    for rec in json.loads(out):
        tags = {}
        for key, v in rec.items():
            group, _, name = key.partition(":")
            if group == GROUP and name not in NEVER and isinstance(v, dict):
                tags[name] = (v.get("id", "?"), v.get("val"))
        when = (rec.get("ExifIFD:DateTimeOriginal", {}).get("val", ""),
                str(rec.get("ExifIFD:SubSecTimeOriginal", {}).get("val", "")))
        shots.append({"file": Path(rec["SourceFile"]).name, "when": when, "tags": tags})
    if not any(s["tags"] for s in shots):
        sys.exit("no FujiFilm MakerNote in these files")
    return sorted(shots, key=lambda s: (s["when"], s["file"]))


def load_steps(name, no_photo):
    path = Path(name)
    if not path.exists():
        path = STEPS_DIR / f"{name}.json"
    steps = json.loads(path.read_text())["steps"]
    return [s for s in steps if not s.get("restore") and s["id"] not in no_photo]


def load_log(path):
    """The last record per step id: its PTP changes and result."""
    recs = {}
    for line in Path(path).read_text().splitlines():
        if line.strip():
            r = json.loads(line)
            recs[r["id"]] = r
    return recs


def diff(a, b, show_all):
    rows = []
    for name in sorted(set(a) | set(b), key=lambda n: (a.get(n) or b.get(n))[0]):
        if not show_all and name in VOLATILE:
            continue
        old, new = a.get(name, ("", None)), b.get(name, ("", None))
        if old[1] != new[1]:
            rows.append({"id": (old[0] or new[0]), "tag": name, "old": old[1], "new": new[1]})
    return rows


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("photos", nargs="+", type=Path)
    ap.add_argument("--steps", help="guided_map steps file the photos follow, one photo per step")
    ap.add_argument("--log", help="guided_map session log (.jsonl) for the PTP side")
    ap.add_argument("--no-photo", default="", help="step ids that got no photo")
    ap.add_argument("--all", action="store_true", help="also print tags that change on every shot")
    ap.add_argument("--json", type=Path, help="write the pairs here too (keep it in local/)")
    a = ap.parse_args()

    shots = read(a.photos)
    steps = load_steps(a.steps, {x for x in a.no_photo.split(",") if x}) if a.steps else []
    log = load_log(a.log) if a.log else {}
    if steps and len(shots) != len(steps) + 1:
        print(f"warning: {len(shots)} photos for {len(steps)} steps (want one more photo than "
              "steps); pairs past the first gap are off by one", file=sys.stderr)
    pairs = []
    print(f"baseline: {shots[0]['file']}")
    for n in range(1, len(shots)):
        step = steps[n - 1] if n - 1 < len(steps) else None
        rows = diff(shots[n - 1]["tags"], shots[n]["tags"], a.all)
        rec = log.get(step["id"]) if step else None
        pairs.append({"photo": shots[n]["file"], "step": step and step["id"],
                      "instruction": step and step["instruction"],
                      "ptp": rec and rec["changed"], "ptp_result": rec and rec["result"],
                      "makernote": rows})
        head = f"{shots[n]['file']}" + (f"  step {step['id']}: {step['instruction']}" if step else "")
        print(f"\n{head}")
        if rec:
            moved = "; ".join(f"{c} {v[0]!r} -> {v[1]!r}" for c, v in rec["changed"].items()) or "none"
            print(f"  PTP ({rec['result']}): {moved}")
        for r in rows:
            print(f"  {r['id']} {r['tag']}: {r['old']!r} -> {r['new']!r}")
        if not rows:
            print("  MakerNote: no change")
    if a.json:
        a.json.write_text(json.dumps({"baseline": shots[0]["file"], "pairs": pairs}, indent=1) + "\n")
        print(f"\nwrote {a.json}")


if __name__ == "__main__":
    main()
