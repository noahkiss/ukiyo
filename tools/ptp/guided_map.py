# /// script
# requires-python = ">=3.11"
# dependencies = ["pyusb>=1.2,<2"]
# ///
"""Guided property mapping: a dialog names one camera change, the script reads what moved.

Usage:
  guided_map.py STEPS [--answer done|skip|stop] [--rescan]
  guided_map.py STEPS --dry-run SNAPSHOT.json [--answer ...] [--fake-wrong ID,..] [--fake-noop ID,..]
                [--fake-menu-stuck ID,..] [--fake-write-menu ID,..] [--fake-timeout ID|open,..]

STEPS is a name in steps/ (x-t5-first) or a path. Run from tools/ptp with
`DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib uv run -q guided_map.py x-t5-first`.

Before you start:
- Quit Ukiyo (the script takes owner.lock) and run `killall PTPCamera`.
- Camera: NETWORK/USB SETTING > CONNECTION MODE: USB TETHER SHOOTING FIXED,
  STILL/MOVIE switch on STILL (MOVIE for a movie steps file), shooting screen showing.

The session:
1. A dialog explains the session. After Start, leave the camera alone for 5 s: the script reads
   every property again and again, and marks the codes that change on their own as noisy.
2. Per step: the script reads every property, shows a dialog with the change to make (Done, Skip,
   Stop), waits for the shooting screen (a menu that stays open gets a "close the menu" dialog),
   reads again and prints one line: expected codes moved or not, unexpected codes, old -> new.
3. At the end: a summary table, a list of settings that now differ from the start, and a
   proposed catalog patch. The script never edits catalog/*.json.

A steps file holds {"name", "model", "mode", "steps": [...]}. "mode" is "stills" (the default) or
"movie": the STILL/MOVIE switch must be on that position at the start, because movie-only codes
answer 0x2002 in stills and the list of codes is read once, when the session opens.
Each step has:
  id           short name, used in the log and with the --fake-* options
  instruction  the change in the camera's own menu words
  note         optional extra text for the dialog
  expect       [{"code": "0xd023", "value": 5, "label": "STRONG LARGE"}]; leave out "value" when
               it is unknown, and the step records whatever the code becomes
  also         codes that may move with the change without counting as unexpected
  desc         codes whose descriptor (enum, range) to compare before and after
  patch        catalog fields to propose per code when the step is confirmed (name, menu, scale,
               values)
  restore      true for a step that puts settings back. The script builds its expect list: every
               writable code an earlier step expected that now differs from the session start,
               newest change first, each with its start value. Its instruction lists them.
A step whose expected value already shows asks the owner to set another value first.

Output, in local/mapping/ (gitignored, it holds this camera's values):
  <date>-<name>.jsonl           one record per step
  <date>-<name>-summary.md      the summary table
  <date>-<name>-proposed.json   RFC 6902 JSON Patch against catalog/<model>.json, for review

Writes: only AUTO POWER OFF (D381). Every 10 s it goes to a neighbouring timeout (5 min, or 2 min
when the owner's is 5 min) and back to the owner's value; the write resets the power-off timer
(docs/protocol.md, "Camera state and power"), and a crash between the two writes never leaves the
camera at OFF. A finally block restores it, retrying 0x2019 and 0xA001 with a 250 ms backoff. If the owner's value is already
OFF, nothing is written. A write can answer 0xA001 while reads work and D22E reads 1: the owner
is in a menu. Outside a dialog the script then prints "a camera menu is open; close it".

If the camera stops answering (a USB timeout, at open or mid-session), the script tries once to
restore D381, writes the summary, and ends with one line naming what to check.

--dry-run reads a recorded dump (dump_props.py format) instead of USB. It fakes each Done as the
change the step expects, a menu that stays open about a second, histogram noise, and a busy
answer now and then. --answer answers every dialog without showing it, to test unattended.
--fake-wrong/--fake-noop/--fake-menu-stuck make named steps go wrong, to test the result logic.
--fake-write-menu makes writes answer 0xA001 for 12 s after a step while reads work.
--fake-timeout makes the camera stop answering once after a step (or at open, with "open").
"""
import argparse
import copy
import datetime
import json
import random
import signal
import struct
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
OUT_DIR = REPO / "local" / "mapping"

OK, BUSY, NOT_SUPPORTED, MENU = 0x2001, 0x2019, 0x200A, 0xA001
SCREEN, STILL_MOVIE, AUTO_POWER_OFF = 0xD22E, 0xD307, 0xD381
APO_OFF = 3  # AUTO POWER OFF: OFF
APO_5MIN, APO_2MIN = 1, 2
KEEPALIVE_S = 10
WRITE_MENU_S = 12  # --fake-write-menu: how long writes answer 0xA001 after a step
BASELINE_S = 5
MENU_PATIENCE_S = 5
# Always noisy: histogram, focus position, the change-event list, and our own keep-alive writes.
NOISY = {0xD22F, 0xD171, 0xD212, AUTO_POWER_OFF}
SCALARS = {1: "b", 2: "B", 3: "h", 4: "H", 5: "i", 6: "I", 7: "q", 8: "Q"}


NO_ANSWER = ("the camera did not answer: check CONNECTION MODE is USB TETHER SHOOTING FIXED, "
             "close menus, unplug/replug once")


class NoAnswer(Exception):
    """The camera did not answer over USB: a timeout, or the device went away."""


def hexcode(c):
    return f"{c:#06x}"


def ptp_string(b):
    if not b:
        return ""
    return b[1:1 + 2 * b[0]].decode("utf-16-le", "replace").rstrip("\0")


def decode(dtype, raw):
    """A PTP value as JSON: number, string, list, or hex for anything without a known size."""
    try:
        if dtype == 0xFFFF:
            return ptp_string(raw)
        if dtype in SCALARS:
            return struct.unpack("<" + SCALARS[dtype], raw)[0]
        if dtype & 0x4000 and dtype & 0xFF in SCALARS:
            fmt = SCALARS[dtype & 0xFF]
            n = struct.unpack_from("<I", raw)[0]
            return list(struct.unpack_from(f"<{n}{fmt}", raw, 4))
    except struct.error:
        pass
    return raw.hex()


class RealCamera:
    """The USB camera through read_presets.PTP, which takes owner.lock first."""

    def __init__(self, rescan):
        from read_presets import PTP  # imported here so --dry-run never loads pyusb
        import dump_props
        import usb.core

        self.usb_error = usb.core.USBError
        try:
            self.p = PTP()
            rc, _ = self.p.open()
        except self.usb_error as e:
            raise NoAnswer(str(e)) from e
        if rc not in (OK, 0x201E):
            sys.exit(f"OpenSession failed: {rc:#x}")
        rc, raw = self._op(0x1001)
        info, props = dump_props.device_info(raw)
        self.model, self.firmware = info["model"], info["firmware"]
        rc, raw = self._op(0x1015, (STILL_MOVIE,))
        mode = "movie" if rc == OK and struct.unpack_from("<H", raw)[0] == 1 else "stills"
        cache = OUT_DIR / "cache" / f"{self.model}-{self.firmware}-{mode}.json".lower()
        if cache.exists() and not rescan:
            self.types = {int(k, 16): v for k, v in json.loads(cache.read_text()).items()}
        else:
            print(f"describing {len(props)} properties ({mode}) ...")
            self.types = {}
            for code in props:
                rc, raw = self._op(0x1014, (code,))
                if rc == OK:
                    self.types[code] = struct.unpack_from("<H", raw, 2)[0]
                elif self._op(0x1015, (code,))[0] == OK:
                    self.types[code] = 0
            cache.parent.mkdir(parents=True, exist_ok=True)
            cache.write_text(json.dumps({hexcode(k): v for k, v in sorted(self.types.items())}, indent=1))
        self.dump_props = dump_props

    def _op(self, code, params=(), data=None):
        try:
            return self.p.op(code, params, data)
        except self.usb_error as e:
            raise NoAnswer(str(e)) from e

    def drain(self):
        """Drop a late answer to an operation that timed out, so the next one reads its own."""
        try:
            while True:
                self.p.inp.read(1 << 20, timeout=200)
        except self.usb_error:
            pass

    def codes(self):
        return sorted(self.types)

    def value(self, code):
        rc, raw = self._op(0x1015, (code,))
        return rc, decode(self.types.get(code, 0), raw) if rc == OK else None

    def desc(self, code):
        rc, raw = self._op(0x1014, (code,))
        if rc != OK:
            return {"error": f"{rc:#x}"}
        try:
            d = self.dump_props.prop_desc(raw)[1]
        except (ValueError, struct.error):
            return {"raw": raw.hex()}
        return {k: v for k, v in d.items() if k in ("rw", "range", "enum")}

    def write_u16(self, code, value):
        return self._op(0x1016, (code,), struct.pack("<H", value))[0]

    def close(self):
        self._op(0x1003)


class FakeCamera:
    """A recorded dump_props.py file standing in for the camera. No USB."""

    def __init__(self, path, wrong, noop, stuck, write_menu=frozenset(), timeout=frozenset()):
        if "open" in timeout:
            raise NoAnswer("fake timeout at open")
        d = json.loads(Path(path).read_text())
        self.model, self.firmware = d.get("model", "X-T5"), d.get("firmware", "?")
        self.props, self.vals = {}, {}
        for k, v in d["properties"].items():
            if "current" in v:
                self.props[int(k, 16)] = v
                self.vals[int(k, 16)] = v["current"]
            elif "raw" in v:
                self.props[int(k, 16)] = v
                self.vals[int(k, 16)] = v["raw"]
        self.vals.setdefault(SCREEN, 1)
        self.vals.setdefault(STILL_MOVIE, 2)
        self.wrong, self.noop, self.stuck = wrong, noop, stuck
        self.write_menu, self.timeout = write_menu, timeout
        self.menu_until = 0.0
        self.write_menu_until = 0.0
        self.dead = False
        self.writes = 0
        self.rng = random.Random(5)

    def codes(self):
        return sorted(self.vals)

    def menu_open(self):
        return time.monotonic() < self.menu_until

    def value(self, code):
        time.sleep(0.001)  # about the time a USB read takes
        if self.dead:
            self.dead = False  # one timeout; the camera answers again after it
            raise NoAnswer("fake timeout")
        if code not in self.vals:
            return NOT_SUPPORTED, None
        if self.menu_open():
            return (OK, 3) if code == SCREEN else (MENU, None)
        if code == 0xD22F:
            return OK, self.rng.randbytes(8).hex()
        if code == 0xD171:
            return OK, self.rng.randint(-50, 50)
        return OK, self.vals[code]

    def desc(self, code):
        p = self.props.get(code, {})
        return {k: v for k, v in p.items() if k in ("rw", "range", "enum")}

    def write_u16(self, code, value):
        if self.menu_open() or time.monotonic() < self.write_menu_until:
            return MENU
        self.writes += 1
        if self.writes % 5 == 0:  # a busy answer now and then, to exercise the retries
            return BUSY
        self.vals[code] = value
        return OK

    def close(self):
        pass

    def drain(self):
        pass

    def _other(self, code, avoid):
        p = self.props.get(code, {})
        options = p.get("enum") or []
        if not options and p.get("range"):
            lo, hi, step = p["range"]
            options = list(range(lo, hi + 1, step or 1))
        options = [o for o in options if o not in avoid]
        if options:
            return options[0]
        now = self.vals.get(code) or 0
        return now + 1 if isinstance(now, int) else f"{now}~"  # a string code, such as D226

    def act(self, step):
        """What the owner's Done would have done on a real camera."""
        sid = step["id"]
        self.menu_until = time.monotonic() + (MENU_PATIENCE_S + 2.5 if sid in self.stuck else 1.2)
        if sid in self.write_menu:
            self.write_menu_until = self.menu_until + WRITE_MENU_S
        if sid in self.timeout:
            self.dead = True
        if sid in self.noop:
            return
        for e in step["expect"]:
            code = int(e["code"], 16)
            if "value" in e and sid not in self.wrong:
                self.vals[code] = e["value"]
            else:
                self.vals[code] = self._other(code, {self.vals.get(code), e.get("value")})


class Dialogs:
    """Blocking macOS dialogs. The keep-alive runs while one is up, since only this thread owns USB."""

    SCRIPT = [
        "on run argv",
        "set b to {}",
        "repeat with i from 3 to count of argv",
        "set end of b to item i of argv",
        "end repeat",
        'display dialog (item 1 of argv) with title (item 2 of argv) buttons b default button (count of b)',
        "return button returned of result",
        "end run",
    ]

    def __init__(self, answer, idle):
        self.answer, self.idle = answer, idle

    def ask(self, text, buttons, title="ukiyo mapping"):
        if self.answer:
            print("\n  [dialog] " + text.replace("\n", "\n  [dialog] "))
            pick = self.answer.capitalize()
            return pick if pick in buttons else buttons[-1]
        args = ["osascript"] + sum((["-e", line] for line in self.SCRIPT), []) + [text, title] + list(buttons)
        proc = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        try:
            while proc.poll() is None:
                self.idle(True)
                time.sleep(0.5)
        except BaseException:
            proc.kill()  # the camera stopped answering, or Ctrl-C: do not leave the dialog up
            raise
        out = proc.stdout.read().strip()
        return out if proc.returncode == 0 and out in buttons else "Stop"


class KeepAwake:
    """Resets the auto power off timer: D381 to a neighbouring timeout and back, every 10 s."""

    def __init__(self, cam):
        self.cam = cam
        rc, self.owner = cam.value(AUTO_POWER_OFF)
        if rc != OK:
            sys.exit(f"cannot read AUTO POWER OFF (D381): {rc:#x}")
        self.dirty = False
        self.last = time.monotonic()
        self.toggles = 0
        self.other = APO_2MIN if self.owner == APO_5MIN else APO_5MIN
        self.in_dialog = False  # the owner is expected to be in a menu while a dialog is up
        self.menu_told = False

    def menu(self, rc):
        """Tell the owner once per episode when a write answers 0xA001: D22E can read 1 meanwhile."""
        if rc == MENU and not self.in_dialog and not self.menu_told:
            print("! a camera menu is open; close it (writes answer 0xA001)", file=sys.stderr)
            self.menu_told = True
        elif rc == OK:
            self.menu_told = False

    def write(self, value, patience):
        """Write D381 and read it back, retrying busy (0x2019) and menu (0xA001) every 250 ms."""
        end = time.monotonic() + patience
        while True:
            rc = self.cam.write_u16(AUTO_POWER_OFF, value)
            self.menu(rc)
            if rc == OK:
                rc, now = self.cam.value(AUTO_POWER_OFF)
                if rc == OK and now == value:
                    return True
            if rc not in (BUSY, MENU, OK) or time.monotonic() > end:
                return False
            time.sleep(0.25)

    def tick(self):
        if self.dirty:
            self.restore(10)
        if self.owner == APO_OFF or time.monotonic() - self.last < KEEPALIVE_S:
            return
        self.last = time.monotonic()
        rc = self.cam.write_u16(AUTO_POWER_OFF, self.other)
        self.menu(rc)
        if rc != OK:
            # A menu is open (the owner's presses keep it awake) or the camera is busy: try in 1 s.
            self.last = time.monotonic() - KEEPALIVE_S + 1
            return
        self.dirty = True
        self.restore(10)
        self.toggles += 1

    def restore(self, patience):
        if not self.dirty:
            return True
        try:
            done = self.write(self.owner, patience)
        except NoAnswer:
            return False
        if done:
            self.dirty = False
            return True
        if not self.menu_told:
            print("! AUTO POWER OFF not back to your value yet; retrying", file=sys.stderr)
        return False


def load_catalog(model):
    name = "".join(ch if ch.isalnum() else "-" for ch in model.lower())
    path = REPO / "catalog" / f"{name}.json"
    return (json.loads(path.read_text()) if path.exists() else {"properties": {}}), path


class Session:
    def __init__(self, cam, steps, dialogs, fake, mode="stills"):
        self.cam, self.steps, self.ui, self.fake, self.mode = cam, steps, dialogs, fake, mode
        self.catalog, self.catalog_path = load_catalog(cam.model)
        self.noisy = set(NOISY)
        self.records = []
        self.start = None
        self.keep = None
        self.log = None

    # ----- reading
    def entry(self, code):
        return self.catalog["properties"].get(hexcode(code), {})

    def show(self, code, v):
        e = self.entry(code)
        label = (e.get("values") or {}).get(str(v))
        if label is None and e.get("scale") and isinstance(v, int):
            label = f"{v / e['scale']:+g}"
        return f"{v!r}" + (f" {label}" if label else "")

    def name(self, code):
        return f"{self.entry(code).get('name', '?')} ({hexcode(code)})"

    def idle(self, in_dialog=False):
        if self.keep:
            self.keep.in_dialog = in_dialog
            self.keep.tick()

    def snapshot(self):
        """Every readable property, or None while a menu is open (reads answer 0xA001)."""
        snap = {}
        for code in self.cam.codes():
            rc, v = self.cam.value(code)
            if rc == MENU:
                return None
            snap[code] = v if rc == OK else f"rc {rc:#x}"
        self.idle()
        return snap

    def wait_shooting(self):
        """Wait for D22E = 1 (shooting screen), asking the owner to close the menu if it stays open.
        Returns False when the owner stops."""
        asked = time.monotonic()
        while True:
            rc, screen = self.cam.value(SCREEN)
            if rc == OK and screen == 1:
                return True
            self.idle()
            if time.monotonic() - asked > MENU_PATIENCE_S:
                where = {2: "playback", 3: "a menu"}.get(screen, "a menu") if rc == OK else "a menu"
                print(f"  camera shows {where}; asking the owner to close it")
                pick = self.ui.ask(
                    f"The camera still shows {where}.\n\nPress BACK (or half-press the shutter) until the "
                    "shooting screen shows, then click OK.", ["Stop", "OK"])
                if pick == "Stop":
                    return False
                asked = time.monotonic()
            time.sleep(0.5)

    def stable_snapshot(self):
        while True:
            if not self.wait_shooting():
                return None
            snap = self.snapshot()
            if snap is not None:
                return snap

    def baseline(self):
        first = self.stable_snapshot()
        if first is None:
            return None
        end, n = time.monotonic() + BASELINE_S, 1
        while time.monotonic() < end or n < 2:
            snap = self.stable_snapshot()
            if snap is None:
                return None
            n += 1
            self.noisy |= {c for c in snap if snap[c] != first.get(c)}
        print(f"baseline: {n} reads in {BASELINE_S} s; noisy: {', '.join(map(hexcode, sorted(self.noisy)))}")
        return first

    # ----- one step
    def restore_step(self, i, step, before):
        """The step with its expect list built: earlier expected codes that differ from the start."""
        expect, lines, seen = [], [], set()
        for j in range(i - 2, -1, -1):
            for e in self.steps[j].get("expect", []):
                c = int(e["code"], 16)
                if c in seen or c not in self.start or before.get(c) == self.start[c]:
                    continue
                if self.cam.desc(c).get("rw") is False:  # a read-out, such as D226; nothing to set
                    continue
                seen.add(c)
                was = self.show(c, self.start[c])
                expect.append({"code": hexcode(c), "value": self.start[c], "label": was})
                what = self.steps[j]["instruction"].split(". ")[0].rstrip(".")
                lines.append(f"- {self.name(c)}: undo step {j + 1} ({what}). Back to {was}.")
        text = "\n".join(lines) or "Nothing to put back."
        return {**step, "expect": expect, "instruction": f"{step['instruction']}\n\n{text}"}

    def run_step(self, i, step):
        n = len(self.steps)
        if step.get("restore"):
            before = self.stable_snapshot()
            if before is None:
                return "Stop"
            step = self.restore_step(i, step, before)
        expect = [(int(e["code"], 16), e) for e in step["expect"]]
        before = self.stable_snapshot()
        if before is None:
            return "Stop"
        already = [(c, e) for c, e in expect if "value" in e and before.get(c) == e["value"]]
        if already:
            shows = "\n".join(f"{self.name(c)} = {self.show(c, e['value'])}" for c, e in already)
            pick = self.ui.ask(
                f"Step {i} of {n}: {step['id']}\n\n{step['instruction']}\n\nThe camera already shows "
                f"this value, so the step could not see it move:\n{shows}\n\nFirst set that item to any "
                "other value and close the menu, then click Done. The step follows.",
                ["Stop", "Skip", "Done"])
            if pick != "Done":
                return self.record(i, step, pick, before, before)
            if self.fake:
                self.fake.menu_until = time.monotonic() + 1.2
                for c, e in already:
                    self.fake.vals[c] = self.fake._other(c, {e["value"]})
            before = self.stable_snapshot()
            if before is None:
                return "Stop"
        now = "" if step.get("restore") else "\n".join(
            f"Now: {self.name(c)} = {self.show(c, before.get(c))}" for c, _ in expect)
        note = f"\n\n{step['note']}" if step.get("note") else ""
        desc_before = {c: self.cam.desc(int(c, 16)) for c in step.get("desc", [])}
        pick = self.ui.ask(
            f"Step {i} of {n}: {step['id']}\n\n{step['instruction']}{note}\n\n{now}\n\n"
            + ("Close the menu, then click Done." if step.get("restore")
               else "Make only this change, close the menu, then click Done."), ["Stop", "Skip", "Done"])
        if pick != "Done":
            return self.record(i, step, pick, before, before)
        if self.fake:
            self.fake.act(step)
        after = self.stable_snapshot()
        if after is None:
            return "Stop"
        desc_after = {c: self.cam.desc(int(c, 16)) for c in step.get("desc", [])}
        desc = {c: [desc_before[c], desc_after[c]] for c in desc_before if desc_before[c] != desc_after[c]}
        return self.record(i, step, pick, before, after, desc)

    def record(self, i, step, pick, before, after, desc=None):
        expect = [(int(e["code"], 16), e) for e in step["expect"]]
        allowed = {c for c, _ in expect} | {int(c, 16) for c in step.get("also", [])}
        changed = {c: [before.get(c), after.get(c)] for c in sorted(set(before) | set(after))
                   if before.get(c) != after.get(c)}
        quiet = {c: v for c, v in changed.items() if c not in self.noisy or c in allowed}
        unexpected = [c for c in quiet if c not in allowed]
        checks = []
        for c, e in expect:
            if c not in changed:
                checks.append("same")
            elif "value" in e and changed[c][1] != e["value"]:
                checks.append("wrong")
            else:
                checks.append("ok")
        if pick == "Skip":
            result = "skipped"
        elif pick == "Stop":
            result = "stopped"
        elif all(x == "ok" for x in checks):
            result = "confirmed"
        elif "wrong" in checks or (all(x == "same" for x in checks) and unexpected):
            result = "contradicted"
        else:
            result = "unclear"
        rec = {
            "t": datetime.datetime.now().isoformat(timespec="seconds"),
            "step": i, "id": step["id"], "instruction": step["instruction"], "answer": pick,
            "result": result,
            "expect": [{**e, "check": x} for (_, e), x in zip(expect, checks)],
            "changed": {hexcode(c): v for c, v in quiet.items()},
            "unexpected": [hexcode(c) for c in unexpected],
            "noisy_changed": [hexcode(c) for c in changed if c not in quiet],
            "desc_changed": desc or {},
        }
        self.records.append(rec)
        self.log.write(json.dumps(rec) + "\n")
        self.log.flush()
        moves = "; ".join(f"{hexcode(c)} {self.show(c, o)} -> {self.show(c, nv)}" for c, (o, nv) in quiet.items())
        exp = ", ".join(f"{hexcode(c)} {x}" for (c, _), x in zip(expect, checks))
        extra = f" | unexpected: {', '.join(map(hexcode, unexpected))}" if unexpected else ""
        dtxt = f" | descriptor moved: {', '.join(desc)}" if desc else ""
        print(f"[{i}/{len(self.steps)}] {step['id']}: {result.upper()} (expected {exp}){extra}{dtxt}"
              + (f"\n      {moves}" if moves else ""))
        return pick

    # ----- the whole session
    def run(self, name, stem):
        intro = (
            f"Guided mapping: {name}, {len(self.steps)} steps, {self.cam.model}.\n\n"
            "Before you start:\n"
            f"- CONNECTION MODE: USB TETHER SHOOTING FIXED, STILL/MOVIE switch on "
            f"{'MOVIE' if self.mode == 'movie' else 'STILL'}.\n"
            "- After you click Start, do not touch the camera for 5 seconds: the script learns which "
            "values change on their own.\n"
            "- Each step names one change. Make only that change, close the menu so the shooting "
            "screen shows, then click Done. Skip a step you cannot do; Stop ends the session.\n"
            "- While a menu is open the camera answers no reads, so the script waits for the "
            "shooting screen.\n"
            "- The script keeps the camera awake by switching AUTO POWER OFF to another timeout and back every "
            "10 seconds, and puts your value back at the end.")
        if self.ui.ask(intro, ["Stop", "Start"]) != "Start":
            return
        rc, sm = self.cam.value(STILL_MOVIE)
        if rc == OK and (sm == 1) != (self.mode == "movie"):
            want, now = ("MOVIE", "STILL") if self.mode == "movie" else ("STILL", "MOVIE")
            sys.exit(f"The STILL/MOVIE switch is on {now}; these steps need {want}. Set it and run again.")
        self.keep = KeepAwake(self.cam)
        print(f"AUTO POWER OFF is {self.show(AUTO_POWER_OFF, self.keep.owner)}"
              + ("; no keep-alive needed" if self.keep.owner == APO_OFF else "; keep-alive every 10 s"))
        self.log = open(OUT_DIR / f"{stem}.jsonl", "a")
        try:
            self.start = self.baseline()
            if self.start is None:
                return
            for i, step in enumerate(self.steps, 1):
                if self.run_step(i, step) == "Stop":
                    break
        except NoAnswer:
            print("the camera stopped answering; trying once to restore AUTO POWER OFF", file=sys.stderr)
            self.cam.drain()
            raise
        finally:
            # A second Ctrl-C or SIGTERM must not cut the restore short.
            signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM, signal.SIGHUP})
            ok = self.keep.restore(60)
            print(f"AUTO POWER OFF {'restored to' if ok else 'NOT restored; set it by hand to'} "
                  f"{self.show(AUTO_POWER_OFF, self.keep.owner)} ({self.keep.toggles} keep-alive toggles)")
            try:
                end = self.snapshot() if self.start else None
            except NoAnswer:
                end = None
            self.log.close()
            self.finish(stem, end)

    def finish(self, stem, end):
        rows = ["| # | step | expected | result | codes changed |", "|---|---|---|---|---|"]
        for r in self.records:
            exp = ", ".join(f"{e['code']}={e.get('value', '?')}" for e in r["expect"])
            moved = ", ".join(f"{c} {v[0]!r}->{v[1]!r}" for c, v in r["changed"].items()) or "none"
            rows.append(f"| {r['step']} | {r['id']} | {exp} | {r['result']} | {moved} |")
        lines = [f"# Guided mapping {stem}", "", f"{self.cam.model} firmware {self.cam.firmware}", ""] + rows
        if end:
            diff = [c for c in sorted(self.start) if c not in self.noisy and self.start[c] != end.get(c)]
            if diff:
                lines += ["", "## Settings that differ from the session start", ""]
                for c in diff:
                    menu = self.entry(c).get("menu", "")
                    lines.append(f"- {self.name(c)} {menu}: was {self.show(c, self.start[c])}, "
                                 f"now {self.show(c, end.get(c))}")
        text = "\n".join(lines) + "\n"
        print("\n" + text)
        (OUT_DIR / f"{stem}-summary.md").write_text(text)
        patch = self.propose()
        (OUT_DIR / f"{stem}-proposed.json").write_text(json.dumps(patch, indent=2) + "\n")
        print(f"log:     {OUT_DIR / (stem + '.jsonl')}\nsummary: {OUT_DIR / (stem + '-summary.md')}\n"
              f"patch:   {OUT_DIR / (stem + '-proposed.json')} ({len(patch)} ops against "
              f"{self.catalog_path.relative_to(REPO)}; review it, then apply by hand or in the Lab)")

    def propose(self):
        """JSON Patch ops for the confirmed steps, built against a working copy so ops stack."""
        work = copy.deepcopy(self.catalog)
        props = work.setdefault("properties", {})
        ops = []

        def put(path, value, why):
            obj = work
            keys = path.strip("/").split("/")
            for k in keys[:-1]:
                obj = obj[k]
            if obj.get(keys[-1]) == value:
                return
            ops.append({"op": "replace" if keys[-1] in obj else "add", "path": path,
                        "value": copy.deepcopy(value), "why": why})
            obj[keys[-1]] = value

        for r in self.records:
            if r["result"] != "confirmed":
                continue
            step = next(s for s in self.steps if s["id"] == r["id"])
            if step.get("restore"):
                continue
            why = f"guided step {r['step']} {r['id']}"
            for e in r["expect"]:
                code = hexcode(int(e["code"], 16))
                new = r["changed"][code][1]
                if code not in props:
                    put(f"/properties/{code}", {"confidence": "guess", "source": "lab session",
                                                 "modes": [self.mode]}, why)
                extra = {hexcode(int(k, 16)): v for k, v in step.get("patch", {}).items()}.get(code, {})
                for k in ("name", "menu", "scale"):
                    if k in extra:
                        put(f"/properties/{code}/{k}", extra[k], why)
                if "values" in extra:
                    for v, label in extra["values"].items():
                        if "values" not in props[code]:
                            put(f"/properties/{code}/values", {}, why)
                        put(f"/properties/{code}/values/{v}", label, why)
                elif e.get("label") and not props[code].get("scale") and isinstance(new, int):
                    if "values" not in props[code]:
                        put(f"/properties/{code}/values", {}, why)
                    put(f"/properties/{code}/values/{new}", e["label"], why)
                put(f"/properties/{code}/confidence", "confirmed", why)
                src = props[code].get("source", "")
                if "lab session" not in src:
                    put(f"/properties/{code}/source", f"{src} + lab session" if src else "lab session", why)
        return ops


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("steps", help="a name in steps/ or a path to a steps file")
    ap.add_argument("--dry-run", metavar="SNAPSHOT", help="use a recorded dump_props.py file, no USB")
    ap.add_argument("--answer", choices=["done", "skip", "stop"], help="answer every dialog without showing it")
    ap.add_argument("--rescan", action="store_true", help="describe every property again, ignoring the cache")
    ap.add_argument("--fake-wrong", default="", help="dry run: these step ids set another value")
    ap.add_argument("--fake-noop", default="", help="dry run: these step ids change nothing")
    ap.add_argument("--fake-menu-stuck", default="", help="dry run: the menu stays open past the patience")
    ap.add_argument("--fake-write-menu", default="", help="dry run: after these steps writes answer 0xA001")
    ap.add_argument("--fake-timeout", default="", help="dry run: the camera stops answering after these steps, or at open")
    a = ap.parse_args()

    path = Path(a.steps)
    if not path.exists():
        path = HERE / "steps" / f"{a.steps}.json"
    spec = json.loads(path.read_text())
    name = spec.get("name", path.stem)
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    day = datetime.date.today().isoformat()
    stem = f"{day}-{name}" + ("-dry" if a.dry_run else "")
    k = 2
    while (OUT_DIR / f"{stem}.jsonl").exists():
        stem = f"{day}-{name}" + ("-dry" if a.dry_run else "") + f"-{k}"
        k += 1

    def stop(signum, frame):
        raise SystemExit(f"signal {signum}")  # so the finally blocks restore D381 and close
    for s in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(s, stop)

    ids = lambda s: {x for x in s.split(",") if x}
    try:
        if a.dry_run:
            cam = FakeCamera(a.dry_run, ids(a.fake_wrong), ids(a.fake_noop), ids(a.fake_menu_stuck),
                             ids(a.fake_write_menu), ids(a.fake_timeout))
            fake = cam
        else:
            cam = RealCamera(a.rescan)
            fake = None
    except NoAnswer:
        sys.exit(NO_ANSWER)
    session = Session(cam, spec["steps"], None, fake, spec.get("mode", "stills"))
    session.ui = Dialogs(a.answer, session.idle)
    if cam.model != spec.get("model", cam.model):
        print(f"warning: steps are for {spec['model']}, camera is {cam.model}", file=sys.stderr)
    gone = False
    try:
        session.run(name, stem)
    except KeyboardInterrupt:
        print("stopped", file=sys.stderr)
    except NoAnswer:
        gone = True
    finally:
        try:
            cam.close()
        except NoAnswer:
            gone = True
    if gone:
        sys.exit(NO_ANSWER)


if __name__ == "__main__":
    main()
