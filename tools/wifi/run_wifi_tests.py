# /// script
# requires-python = ">=3.11"
# dependencies = ["bleak>=0.22"]
# ///
"""Guided Wi-Fi and Bluetooth test run for a Fujifilm X-T5. See tools/wifi/README.md.

Usage:
  uv run -q run_wifi_tests.py              # the real run, guided by macOS dialogs
  uv run -q run_wifi_tests.py --dry-run    # fake camera on localhost, scripted answers, no changes

Read-only, except one reversible AUTO POWER OFF (D381) toggle. Never captures, deletes or
formats. Logs go to local/wifi-test/<timestamp>/.
"""
import argparse
import datetime as dt
import hashlib
import json
import re
import socket
import struct
import subprocess
import sys
import threading
import time
from pathlib import Path

import fujiptpip as fp
import macnet

REPO = Path(__file__).resolve().parents[2]
CATALOG = REPO / "catalog" / "x-t5.json"
D381 = 0xD381
APO_LABEL = {1: "5 MIN", 2: "2 MIN", 3: "OFF", 4: "1 MIN", 5: "30 SEC", 6: "15 SEC"}
APO_SECONDS = {1: 300, 2: 120, 3: None, 4: 60, 5: 30, 6: 15}
BUSY, MENU = 0x2019, 0xA001
DRY_WIFI_KEY = "dry-run-only-key"  # what the scripted dry run types for the camera network

# Values that may hold owner or device identity: always hashed in logs.
SENSITIVE = {0x501E, 0x501F, 0xD100, 0xD145, 0xD146, 0xD167, 0xD20B, 0xD215, 0xD216, 0xD36D, 0xD500}
# String values safe to log in clear.
SAFE_STRINGS = {0x5003, 0xD153, 0xD184, 0xD186, 0xD187, 0xD1B1, 0xD233}
# Codes worth reading on Wi-Fi even when not advertised (libfuji fujiptp.h, protocol.md).
WIFI_CODES = [0xD16E, 0xD207, 0xD212, 0xD220, 0xD222, 0xD226, 0xD227, 0xD228, 0xD22B, 0xD22E,
              0xD244, 0xD307, 0xD381, 0xD400, 0xD401, 0xD620, 0xD621, 0xDF00, 0xDF01, 0xDF21,
              0xDF22, 0xDF23, 0xDF24, 0xDF25, 0xDF28, 0xDF2A, 0xDF31, 0xDF44]
DESCRIBE = [D381, 0xD22E, 0xD307, 0xD16E, 0xD207, 0x5005, 0xD001, 0xD212, 0xDF00, 0xDF01]

MENU_NET = "MENU/OK > NETWORK/USB SETTING"
MENU_APO = "MENU/OK > SET UP > POWER MANAGEMENT > AUTO POWER OFF"
CONNECTION_MODES = ["USB CARD READER", "USB WEBCAM", "USB TETHER SHOOTING AUTO",
                    "USB TETHER SHOOTING FIXED", "WIRELESS TETHER SHOOTING FIXED",
                    "USB RAW CONV./BACKUP RESTORE"]


QUIRKS = {"empty-deviceinfo": "GetDeviceInfo answers OK with no data (gp: the X-T4 does this)",
          "refuse": "the camera answers INIT_FAIL to every init",
          "short-size": "ObjectInfo reports 100 KB for every file (libfuji: D227 off)",
          "no-objects": "the object list is empty"}


class StopRun(Exception):
    pass


# ---------------------------------------------------------------- logging and redaction

class Redactor:
    MAC = re.compile(r"\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b")
    FUJI_NAME = re.compile(r"\b((?:FUJIFILM[-_ ])?X-?[A-Z0-9]{1,6}(?:\s?II)?)[-_]([0-9A-Fa-f]{4,12})\b")

    def __init__(self):
        self.secrets = {}

    @staticmethod
    def tag(s):
        return "<h:" + hashlib.sha256(s.encode("utf-8", "replace")).hexdigest()[:10] + ">"

    def add(self, s):
        if not isinstance(s, str) or len(s.strip()) < 3:
            return
        t = self.tag(s)
        for form in (s, s.encode("utf-16-le").hex(), s.encode().hex()):
            self.secrets[form] = t

    def text(self, line):
        for k in sorted(self.secrets, key=len, reverse=True):
            if k in line:
                line = line.replace(k, self.secrets[k])
        line = self.MAC.sub(lambda m: self.tag(m.group(0).lower()), line)
        return self.FUJI_NAME.sub(lambda m: m.group(1) + "-****", line)


class RunLog:
    def __init__(self, root, dry_run):
        stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S") + ("-dry" if dry_run else "")
        self.dir = root / stamp
        (self.dir / "files").mkdir(parents=True, exist_ok=True)
        self.jsonl = open(self.dir / "events.jsonl", "a", encoding="utf-8")
        self.red = Redactor()
        self.step = "start"
        self.t0 = time.monotonic()
        self.lock = threading.Lock()

    def __call__(self, kind, **fields):
        rec = {"t": round(time.monotonic() - self.t0, 3), "at": dt.datetime.now().isoformat(timespec="seconds"),
               "step": self.step, "kind": kind, **fields}
        line = self.red.text(json.dumps(rec, default=str, ensure_ascii=False))
        with self.lock:
            self.jsonl.write(line + "\n")
            self.jsonl.flush()
        if kind not in ("op", "prop"):
            print(f"[{rec['t']:8.1f}] {self.step:>16} {kind}: {line[line.find(chr(34) + 'kind'):][:160]}")

    def safe(self, obj):
        return json.loads(self.red.text(json.dumps(obj, default=str, ensure_ascii=False)))


# ---------------------------------------------------------------- dialogs

class Dialogs:
    """Blocking macOS dialogs, or scripted answers in a dry run."""

    def __init__(self, log, scripted=None):
        self.log, self.scripted = log, scripted

    def _osa(self, script, args):
        cmd = ["osascript"] + sum((["-e", ln] for ln in script), []) + list(args)
        p = subprocess.run(cmd, capture_output=True, text=True)
        return p.returncode, p.stdout.strip()

    def ask(self, key, title, lines, buttons=("Stop", "Skip", "Done")):
        text = "\n".join(lines)
        self.log("dialog", key=key, title=title, text=text, buttons=list(buttons))
        if self.scripted is not None:
            pick = self.scripted.get(key, buttons[-1])
            print(f"\n  [dialog {title}]\n  " + text.replace("\n", "\n  ") + f"\n  -> {pick}\n")
        else:
            script = ["on run argv", "set b to {}", "repeat with i from 3 to count of argv",
                      "set end of b to item i of argv", "end repeat",
                      "set r to display dialog (item 1 of argv) with title (item 2 of argv) buttons b "
                      "default button (count of b) giving up after 3600",
                      "if gave up of r then return \"(timeout)\"", "return button returned of r", "end run"]
            rc, out = self._osa(script, [text, title, *buttons])
            pick = out if rc == 0 and out in buttons else ("Skip" if out == "(timeout)" else "Stop")
        self.log("answer", key=key, answer=pick)
        if pick == "Stop":
            raise StopRun(key)
        return pick

    def text(self, key, title, lines, default="", hidden=False):
        """A text field. Returns the string, or None on Skip. Hidden answers are never logged."""
        text = "\n".join(lines)
        self.log("dialog", key=key, title=title, text=text, input=True)
        if self.scripted is not None:
            val = self.scripted.get(key, default)
            print(f"\n  [input {title}]\n  " + text.replace("\n", "\n  ") + f"\n  -> {'<hidden>' if hidden else val!r}\n")
        else:
            script = ["on run argv",
                      "set r to display dialog (item 1 of argv) with title (item 2 of argv) default answer "
                      "(item 3 of argv)" + (" with hidden answer" if hidden else "") +
                      " buttons {\"Stop\", \"Skip\", \"OK\"} default button 3 giving up after 3600",
                      "if gave up of r then return \"__ukiyo_skip__\"",
                      "if button returned of r is \"Skip\" then return \"__ukiyo_skip__\"",
                      "if button returned of r is \"Stop\" then return \"__ukiyo_stop__\"",
                      "return text returned of r", "end run"]
            rc, out = self._osa(script, [text, title, default])
            val = None if rc != 0 or out == "__ukiyo_skip__" else out
            if out == "__ukiyo_stop__" or rc != 0:
                self.log("answer", key=key, answer="Stop")
                raise StopRun(key)
        self.log("answer", key=key, answer=("<hidden>" if hidden else val) if val is not None else "Skip")
        return val

    def choose(self, key, title, prompt, options):
        self.log("dialog", key=key, title=title, text=prompt, options=options)
        if self.scripted is not None:
            val = self.scripted.get(key, options[0])
            print(f"\n  [choose {title}] {prompt}\n  -> {val}\n")
        else:
            script = ["on run argv", "set opts to items 3 thru -1 of argv",
                      "set r to choose from list opts with title (item 1 of argv) with prompt (item 2 of argv)",
                      "if r is false then return \"\"", "return item 1 of r", "end run"]
            rc, out = self._osa(script, [title, prompt, *options])
            val = out if out in options else None
        self.log("answer", key=key, answer=val)
        return val

    def background(self, key, title, lines, button="End now"):
        """A dialog that does not block. Returns a callable: True once the owner pressed the button."""
        self.log("dialog", key=key, title=title, text="\n".join(lines), background=True)
        if self.scripted is not None:
            print(f"\n  [dialog {title}] " + " / ".join(lines))
            return lambda: False, lambda: None
        script = ["on run argv", "display dialog (item 1 of argv) with title (item 2 of argv) buttons "
                  "{(item 3 of argv)} default button 1 giving up after 1200", "end run"]
        p = subprocess.Popen(["osascript"] + sum((["-e", ln] for ln in script), []) + ["\n".join(lines), title, button],
                             stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)

        def pressed():
            return p.poll() is not None and "gave up:true" not in (p.stdout.read() if p.stdout else "")

        return pressed, p.kill


# ---------------------------------------------------------------- the run

class Run:
    def __init__(self, args):
        self.args = args
        self.dry = args.dry_run
        self.log = RunLog(Path(args.out), self.dry)
        scripted = None
        if self.dry and not args.real_dialogs:
            scripted = {"ble_pairing_scan": "Done", "tether_network": "This network",
                        "connection_mode_before": "USB TETHER SHOOTING FIXED",
                        "phone_mode": "I have the password", "phone_net_ssid": "FUJIFILM-X-T5-9F3A",
                        "phone_net_password": DRY_WIFI_KEY, "phone_net_how": "Join", "camera_ip": ""}
            scripted.update(kv.split("=", 1) for kv in args.answer)
        self.dlg = Dialogs(self.log, scripted)
        self.net = macnet.Net(self.log, self.dry)
        self.findings = {"dry_run": self.dry, "started": dt.datetime.now().isoformat(timespec="seconds"),
                         "modes": {}, "errors": [], "warnings": []}
        self.catalog = self._catalog()
        self.fake = None
        self.ports = {"cmd": fp.CMD_PORT, "event": fp.EVENT_PORT, "jpeg": fp.JPEG_PORT,
                      "pcss": fp.PCSS_PORT, "tether": fp.TETHER_PORT}
        self.conn_mode_before = None
        self.apo_restore_needed = None
        self.stopped = False
        self.last_d381_write = None

    def _catalog(self):
        try:
            return {int(k, 16): v for k, v in json.loads(CATALOG.read_text())["properties"].items()}
        except (OSError, ValueError, KeyError):
            return {}

    def step(self, name):
        self.log.step = name
        self.log("step", name=name)

    def warn(self, text):
        self.findings["warnings"].append(text)
        self.log("warning", text=text)

    # ------------------------------------------------------------ main flow

    def main(self):
        try:
            if self.dry:
                import fakecam
                self.fake = fakecam.FakeCamera(time_scale=0.02, quirks=set(self.args.fake_quirk))
                self.ports.update(cmd=self.fake.ports["cmd"], jpeg=self.fake.ports["jpeg"], event=1,
                                  pcss=self.fake.ports["pcss"], tether=self._free_port())
                self.fake.start(self.ports["tether"])
                self.log("fake_camera", **self.fake.ports, tether=self.ports["tether"])
            self.intro()
            self.preflight()
            self.ble_step()
            self.tether_mode()
            self.phone_mode()
        except StopRun as e:
            self.stopped = True
            self.log("stopped_by_owner", at=str(e))
        except Exception as e:  # noqa: BLE001 - the run must always reach the clean-up below
            self.findings["errors"].append(f"{type(e).__name__}: {e}")
            self.log("crash", error=f"{type(e).__name__}: {e}")
            import traceback
            self.log("traceback", text=traceback.format_exc())
        finally:
            self.finish()

    @staticmethod
    def _free_port():
        s = socket.socket()
        s.bind(("127.0.0.1", 0))
        p = s.getsockname()[1]
        s.close()
        return p

    def intro(self):
        self.step("intro")
        self.dlg.ask("intro", "ukiyo Wi-Fi test", [
            "This run tests the X-T5 over Wi-Fi and Bluetooth.",
            "It takes about 30-45 minutes.",
            "Each step shows a dialog like this one.",
            "Done: you did the step. Skip: leave it out. Stop: end the run.",
            "",
            "Before you start:",
            "Charge the camera battery.",
            "Put a card with a few recent photos in slot 1.",
            "Keep this Mac on power.",
            "Disconnect the camera's USB cable.",
            "Close FUJIFILM XApp on your phone.",
            "",
            "If macOS asks to let this terminal use Bluetooth or",
            "find devices on the local network: Allow.",
        ], buttons=("Stop", "Start"))

    # ------------------------------------------------------------ pre-flight

    def preflight(self):
        self.step("preflight")
        ssid, redacted, sources = self.net.current_ssid()
        info = self.net.ipv4()
        routes, default_if = self.net.other_routes()
        internet = self.net.internet()
        lan = self.local_network_probe(info.get("router"))
        if not ssid and info.get("ip"):
            hint = "macOS hides the network name (Location Services)." if redacted else "No name found."
            got = self.dlg.text("original_ssid", "Current Wi-Fi", [
                hint,
                "Type the name of the Wi-Fi network this Mac is on now.",
                "The run rejoins it at the end.",
                "Skip: the run turns Wi-Fi off and on instead.",
            ])
            ssid = got or None
        if ssid:
            self.log.red.add(ssid)
        self.net.original = {"ssid": ssid}
        pf = {"wifi_device": self.net.device, "ssid_known": bool(ssid),
              "ssid_redacted_by_macos": redacted, "ssid_sources": sources, "ipv4": info,
              "other_routes": routes, "default_route_interface": default_if, "internet": internet,
              "local_network": lan}
        self.findings["preflight"] = self.log.safe(pf)
        self.log("preflight", **pf)
        if not routes:
            self.warn("No second route to the internet (Ethernet or iPhone USB). The Mac is offline "
                      "while it is on a camera network.")

    def local_network_probe(self, router):
        """macOS 15+ blocks LAN traffic until the terminal app has Local Network permission.
        A blocked app gets EHOSTUNREACH at once. Sending one datagram also raises the prompt."""
        if self.dry or not router:
            return {"tested": False}
        u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            u.sendto(b"ukiyo", (router, 9))
            res = {"tested": True, "ok": True}
        except OSError as e:
            res = {"tested": True, "ok": False, "error": str(e)}
        finally:
            u.close()
        if not res["ok"]:
            self.dlg.ask("local_network", "Local Network permission", [
                "macOS blocks this terminal from the local network.",
                "System Settings > Privacy & Security > Local Network.",
                "Turn it on for your terminal app, then press Done.",
            ])
        return res

    # ------------------------------------------------------------ Bluetooth

    def ble_step(self):
        self.step("ble")
        out = self.findings["ble"] = {"ran": False}
        if self.args.skip_ble:
            out["skipped"] = "--skip-ble"
            return
        ans = self.dlg.ask("ble_intro", "Bluetooth (read-only)", [
            "On the camera:",
            f"{MENU_NET} > Bluetooth/SMARTPHONE SETTING > Bluetooth ON/OFF > ON.",
            "On your phone: close XApp, or turn Bluetooth off.",
            "Leave the camera in shooting mode.",
            "",
            "If this Mac asks to pair or to allow Bluetooth:",
            "Allow Bluetooth for Terminal. Do NOT pair (Cancel).",
        ])
        if ans == "Skip":
            out["skipped"] = "owner"
            return
        out["ran"] = True
        try:
            if self.dry:
                total, fuji = 7, [(None, _fake_advert())]
            else:
                import ble
                total, fuji = ble.scan(self.args.ble_scan_s)
        except Exception as e:  # noqa: BLE001 - Bluetooth off, no permission, bleak missing
            out["error"] = f"scan: {type(e).__name__}: {e}"
            self.log("ble_error", error=out["error"])
            return
        out["devices_seen"] = total
        out["fuji_adverts"] = [self._ble_advert(rec) for _, rec in fuji]
        self.log("ble_scan", devices_seen=total, fuji=len(fuji))
        if fuji:
            dev, rec = max(fuji, key=lambda f: f[1].get("rssi") or -999)
            try:
                if self.dry:
                    gatt = _fake_gatt()
                else:
                    import ble
                    gatt = ble.survey(dev, read=not self.args.ble_no_read)
                out["gatt"] = self._ble_gatt(gatt)
            except Exception as e:  # noqa: BLE001
                out["gatt_error"] = f"{type(e).__name__}: {e}"[:300]
                self.log("ble_error", error=out["gatt_error"])
        ans = self.dlg.ask("ble_pairing_scan", "Bluetooth: pairing advert", [
            "Optional: see what the camera sends while it waits to pair.",
            f"On the camera: {MENU_NET} > Bluetooth/SMARTPHONE SETTING > PAIRING REGISTRATION.",
            "Do not pair from your phone.",
            "Press Done here. The Mac scans for 15 seconds.",
        ])
        if ans == "Done":
            try:
                if self.dry:
                    t, fj = 7, [(None, _fake_advert(pairing=True))]
                else:
                    import ble
                    t, fj = ble.scan(15)
                out["pairing_adverts"] = [self._ble_advert(r) for _, r in fj]
            except Exception as e:  # noqa: BLE001
                out["pairing_error"] = f"{type(e).__name__}: {e}"
            self.dlg.ask("ble_pairing_exit", "Bluetooth: leave pairing", [
                "On the camera: press BACK until PAIRING REGISTRATION closes.",
                "The camera must not pair with anything.",
            ], buttons=("Stop", "Done"))

    def _ble_advert(self, rec):
        rec = dict(rec)
        rec["address"] = Redactor.tag(str(rec.get("address")))
        if rec.get("name"):
            self.log.red.add(rec["name"]) if not Redactor.FUJI_NAME.search(rec["name"]) else None
        if rec.get("fuji_mfg", {}).get("token"):
            rec["fuji_mfg"]["token"] = Redactor.tag(rec["fuji_mfg"]["token"])
        self.log("ble_advert", **rec)
        return self.log.safe(rec)

    def _ble_gatt(self, gatt):
        n_chr = n_read = 0
        for s in gatt["services"]:
            for c in s["characteristics"]:
                n_chr += 1
                if "value" in c:
                    n_read += 1
                    c["value"] = _show_blob(self.log.red, c["value"], allow_text=c["uuid"].startswith(("00002a26", "00002a29", "00002a24", "68887a7a", "621a98d8", "c922ac69")))
        gatt["counts"] = {"services": len(gatt["services"]), "characteristics": n_chr, "read": n_read}
        self.log("ble_gatt", **gatt)
        return self.log.safe(gatt)

    # ------------------------------------------------------------ joining networks

    def join_network(self, key, why):
        ssid = self.dlg.text(key + "_ssid", "Join a network", [why, "Type the network name (SSID)."])
        if not ssid:
            return None
        self.log.red.add(ssid)
        pw = self.dlg.text(key + "_password", "Join a network", ["Password for that network.",
                                                                 "Leave empty for an open network."], hidden=True)
        if pw:
            self.log.red.add(pw)
        ans = self.dlg.ask(key + "_how", "Join a network", [
            "This Mac will now leave its Wi-Fi and join that network.",
            "It rejoins the original network at the end.",
            "Join: the Mac does it.",
            "Skip: you join it yourself from the Wi-Fi menu, then press Skip.",
        ], buttons=("Stop", "Skip", "Join"))
        if ans == "Join":
            ok, info = self.net.join(ssid, pw or None)
        else:
            self.net.changed = True
            info = self.net.wait_ip(60) if not self.dry else {"ip": "127.0.0.1"}
            ok = bool(info.get("ip"))
        self.log("joined", ok=ok, ipv4=info)
        return info if ok else None

    # ------------------------------------------------------------ mode B: wireless tether

    def tether_mode(self):
        self.step("tether")
        out = self.findings["modes"]["wireless_tether"] = {"ran": False}
        ans = self.dlg.ask("tether_intro", "Wireless tether", [
            "Test 1: WIRELESS TETHER SHOOTING FIXED.",
            "The camera joins a Wi-Fi network; this Mac talks to it there.",
            "This is the mode X Acquire and Lightroom use.",
            "It needs a network the camera can join (home Wi-Fi or a phone hotspot).",
            "About 15-25 minutes.",
        ], buttons=("Stop", "Skip", "Start"))
        if ans == "Skip":
            out["skipped"] = "owner"
            return
        mode = self.dlg.choose("connection_mode_before", "Connection mode", "\n".join([
            f"On the camera: {MENU_NET} > CONNECTION MODE.",
            "Which mode is selected now? (You set it back at the end.)"]), CONNECTION_MODES)
        self.conn_mode_before = mode
        out["connection_mode_before"] = mode
        where = self.dlg.ask("tether_network", "Wireless tether: network", [
            "Which network should the camera join?",
            "This network: the Wi-Fi this Mac is on now (simplest).",
            "Other network: a hotspot or another Wi-Fi; this Mac joins it too.",
            "The camera and this Mac must be on the same network.",
        ], buttons=("Skip", "Other network", "This network"))
        if where == "Skip":
            out["skipped"] = "owner (network)"
            return
        if where == "Other network":
            info = self.join_network("tether_net", "The camera and this Mac will both use this network.")
            if not info:
                out["skipped"] = "could not join the network"
                return
        info = self.net.ipv4() if not self.dry else {"ip": "127.0.0.1", "broadcast": "127.0.0.1",
                                                     "network": "127.0.0.0/8"}
        out["mac_ipv4"] = info
        self.dlg.ask("tether_ap", "Wireless tether: camera network", [
            "On the camera:",
            f"1. {MENU_NET} > NETWORK SETTING > ACCESS POINT SETTING.",
            "2. MANUAL SETUP > SELECT FROM NETWORK LIST, pick the network, enter its password.",
            "   (or SIMPLE SETUP if your router has a WPS button)",
            "3. NETWORK SETTING > IP ADDRESS SETTING > AUTO.",
            "Skip if the camera already has this network.",
        ])
        self.dlg.ask("tether_apo", "Wireless tether: auto power off", [
            "The camera must stay on during the tests.",
            f"On the camera: {MENU_APO}.",
            "Note the current value, then choose OFF.",
            "The keep-alive test at the end asks for 1 MIN.",
            "You set your usual value back at the end.",
        ])
        self.dlg.ask("tether_mode_on", "Wireless tether: start", [
            f"On the camera: {MENU_NET} > CONNECTION MODE > WIRELESS TETHER SHOOTING FIXED.",
            "Go back to shooting mode (half-press the shutter).",
            "Then press Done. The Mac searches for up to 2 minutes.",
            "If macOS asks to allow incoming connections for Python: Allow.",
        ], buttons=("Stop", "Done"))
        out["ran"] = True
        cam = self.discover_tether(info, out)
        if not cam:
            return
        self.ptp_suite(cam["ip"], cam["port"], out, full=True)

    def discover_tether(self, info, out):
        broadcasts = sorted({b for b in (info.get("broadcast"), "255.255.255.255") if b})
        if self.dry:
            broadcasts = ["127.0.0.1"]
        local_ip = info.get("ip") or "0.0.0.0"
        t0 = time.monotonic()
        try:
            found = fp.discover_tether(local_ip, broadcasts, self.args.discovery_s, self.log,
                                       pcss_port=self.ports["pcss"], tether_port=self.ports["tether"])
        except OSError as e:
            found = None
            self.log("discovery_error", error=str(e))
        out["discovery"] = {"method": "PCSS broadcast", "found": bool(found),
                            "seconds": round(time.monotonic() - t0, 1)}
        if found:
            out["discovery"]["invite"] = self.log.safe(found["invite"])
            return found
        # Fallback 1: neighbours in the ARP cache that answer on the command port.
        for ip in ([] if self.dry else self.net.arp_neighbours())[:64]:
            if not self._on_subnet(ip, info):
                continue
            try:
                socket.create_connection((ip, self.ports["cmd"]), timeout=0.7).close()
                self.log("port_probe_hit", ip=ip)
                out["discovery"]["method"] = "ARP + port 55740"
                time.sleep(2.0)
                return {"ip": ip, "port": self.ports["cmd"]}
            except OSError:
                continue
        # Fallback 2: the owner reads the IP from the camera.
        ip = self.dlg.text("camera_ip", "Camera IP address", [
            "The Mac did not find the camera.",
            f"On the camera: {MENU_NET} > INFORMATION shows its wireless IP address.",
            "Type it here, or Skip.",
        ])
        if ip and re.fullmatch(r"\d+\.\d+\.\d+\.\d+", ip.strip()):
            out["discovery"]["method"] = "typed IP"
            return {"ip": ip.strip(), "port": self.ports["cmd"]}
        out["discovery"]["method"] = "none"
        return None

    @staticmethod
    def _on_subnet(ip, info):
        import ipaddress
        try:
            return ipaddress.IPv4Address(ip) in ipaddress.IPv4Network(info.get("network") or "0.0.0.0/0")
        except ValueError:
            return False

    # ------------------------------------------------------------ the PTP suite

    def ptp_suite(self, host, port, out, full):
        s = fp.Session(host, port, name="ukiyo-wifi-test", timeout=10.0, log=self.log)
        try:
            ack = self.init_with_prompt(s)
            out["init"] = self.log.safe({"camera_name": ack["name"], "conn": ack["conn"]})
            rc = s.open_session()
            out["open_session"] = fp.rc_name(rc)
            if rc not in (fp.OK, 0x201E):
                return
            di = self.device_info(s, out)
            ops = set(di.get("operations", []))
            if not ops:
                # Empty DeviceInfo: try the operations this run uses and let the answers tell.
                ops = {fp.OP[k] for k in ("SetDevicePropValue", "GetThumb", "GetPartialObject", "GetObject",
                                          "InitiateOpenCapture")}
                out["ops_assumed"] = True
            props = self.read_properties(s, out, di)
            if not full:
                return
            self.d381_toggle(s, out, ops, props)
            self.storage_and_transfer(s, out, ops)
            self.live_view(s, out, ops)
            self.keep_alive(s, out, props)
        except (OSError, fp.PTPError) as e:
            out.setdefault("errors", []).append(f"{type(e).__name__}: {e}")
            self.log("ptp_error", error=f"{type(e).__name__}: {e}")
        finally:
            self.restore_d381(s, out)
            s.close()

    def init_with_prompt(self, s):
        try:
            return s.connect_and_init(attempts=2, spacing=3.0)
        except fp.PTPError as e:
            self.log("init_refused", error=str(e))
        self.dlg.ask("init_prompt", "Camera prompt", [
            "The camera did not accept the connection yet.",
            "Look at the camera screen.",
            "If it asks to allow a connection or register a PC: choose OK / YES.",
            "Then press Done.",
        ])
        return s.connect_and_init(attempts=3, spacing=3.0)

    def device_info(self, s, out):
        rc, data, _, st = s.transaction(fp.OP["GetDeviceInfo"], timeout=15)
        res = {"rc": fp.rc_name(rc), "bytes": len(data), "ms": st["ms"]}
        di = {}
        if rc == fp.OK and data:
            try:
                di = fp.parse_device_info(data)
                self.log.red.add(di.get("serial"))
                res.update({k: di[k] for k in ("manufacturer", "model", "device_version", "vendor_ext_desc",
                                                "functional_mode")})
                res["serial"] = Redactor.tag(di.get("serial") or "")
                for k in ("operations", "events", "properties", "capture_formats", "image_formats"):
                    res[k] = [f"{x:#06x}" for x in di[k]]
                    res["n_" + k] = len(di[k])
            except fp.PTPError as e:
                res["parse_error"] = str(e)
        elif rc == fp.OK:
            res["note"] = "empty DeviceInfo (gp: the X-T4 does this over Wi-Fi)"
        out["device_info"] = self.log.safe(res)
        self.log("device_info", **res)
        if self.args.keep_raw and data:
            (self.log.dir / "raw-deviceinfo.bin").write_bytes(data)
        ops = set(di.get("operations", []))
        if not data or fp.OP["FujiGetDeviceInfo"] in ops:
            rc, blob, _, st = s.transaction(fp.OP["FujiGetDeviceInfo"], timeout=30)
            fres = {"rc": fp.rc_name(rc), "bytes": len(blob), "ms": st["ms"]}
            if rc == fp.OK and blob:
                try:
                    parsed = fp.parse_fuji_device_info(blob)
                    fres.update(count=parsed["count"], parsed=len(parsed["descs"]), notes=parsed["notes"],
                                codes=[f"{d['code']:#06x}" for d in parsed["descs"]],
                                writable=sum(1 for d in parsed["descs"] if d.get("writable")))
                    di["fuji_descs"] = {d["code"]: d for d in parsed["descs"]}
                except fp.PTPError as e:
                    fres["parse_error"] = str(e)
                if self.args.keep_raw:
                    (self.log.dir / "raw-902b.bin").write_bytes(blob)
            out["fuji_device_info"] = fres
            self.log("fuji_device_info", **{k: v for k, v in fres.items() if k != "codes"})
        return di

    def read_properties(self, s, out, di):
        advertised = list(di.get("properties", []))
        for c in di.get("fuji_descs", {}):
            if c not in advertised:
                advertised.append(c)
        cat_tether = [c for c, v in self.catalog.items() if "stills" in (v.get("modes") or [])]
        extra = [c for c in WIFI_CODES + cat_tether if c not in advertised]
        results, descs = {}, {}
        # Describe first so values can be decoded by datatype.
        for code in advertised + [c for c in DESCRIBE if c not in advertised]:
            rc, data, _, st = s.transaction(fp.OP["GetDevicePropDesc"], (code,), quiet=True)
            if rc == fp.OK and data:
                try:
                    descs[code] = fp.parse_prop_desc(data)[0]
                except fp.PTPError as e:
                    descs[code] = {"code": code, "parse_note": str(e)}
            else:
                descs[code] = {"code": code, "rc": fp.rc_name(rc)}
        for d in di.get("fuji_descs", {}).values():
            descs.setdefault(d["code"], d)
        for code in advertised + extra:
            rc, data, st = s.get_value(code)
            if rc == BUSY:
                time.sleep(0.25)
                rc, data, st = s.get_value(code)
            desc = descs.get(code, {})
            rec = {"code": f"{code:#06x}", "rc": fp.rc_name(rc), "ms": st["ms"], "advertised": code in advertised,
                   "name": (self.catalog.get(code) or {}).get("name")}
            if rc == fp.OK:
                rec["value"] = _show_value(self.log.red, code, desc.get("datatype"), data)
            if desc.get("datatype") is not None:
                rec["datatype"] = f"{desc['datatype']:#06x}"
                rec["writable"] = desc.get("writable")
                rec["form"] = desc.get("form")
            results[code] = rec
            self.log("prop", **rec)
        ok = [c for c, r in results.items() if r["rc"] == "OK"]
        lat = sorted(r["ms"] for r in results.values() if r["rc"] == "OK")
        cat_codes = set(cat_tether)
        summ = {
            "advertised": len(advertised), "read_ok": len(ok),
            "advertised_ok": sum(1 for c in ok if c in advertised),
            "extra_tried": len(extra), "extra_ok": sum(1 for c in ok if c not in advertised),
            "writable_described": sum(1 for d in descs.values() if d.get("writable")),
            "usb_tether_catalog": len(cat_codes),
            "catalog_answering_on_wifi": len(cat_codes & set(ok)),
            "wifi_only_ok": [f"{c:#06x}" for c in ok if c not in cat_codes],
            "rc_counts": _count(r["rc"] for r in results.values()),
            "read_ms_median": lat[len(lat) // 2] if lat else None,
            "read_ms_p90": lat[int(len(lat) * 0.9)] if lat else None,
            "described": {f"{c:#06x}": _desc_view(self.log.red, descs[c]) for c in DESCRIBE if c in descs},
            "key_values": {f"{c:#06x}": results[c].get("value") for c in WIFI_CODES if c in results and "value" in results[c]},
        }
        out["properties"] = self.log.safe(summ)
        self.log("properties_summary", **{k: v for k, v in summ.items() if k not in ("described",)})
        (self.log.dir / f"props-{self.log.step}.json").write_text(
            json.dumps(self.log.safe({f"{c:#06x}": r for c, r in results.items()}), indent=1))
        return {c: r for c, r in results.items() if r["rc"] == "OK"}

    # ------------------------------------------------------------ D381

    def _write_d381(self, s, value, patience=3.0):
        end = time.monotonic() + patience
        while True:
            rc, st = s.set_value(D381, struct.pack("<H", value))
            if rc in (fp.OK, 0xA002):
                self.last_d381_write = time.monotonic()
                rrc, data, _ = s.get_value(D381)
                now = struct.unpack("<H", data[:2])[0] if rrc == fp.OK and len(data) >= 2 else None
                return rc, now
            if rc not in (BUSY, MENU) or time.monotonic() > end:
                return rc, None
            time.sleep(0.25)

    def d381_toggle(self, s, out, ops, props):
        res = out["d381_toggle"] = {"ran": False}
        if fp.OP["SetDevicePropValue"] not in ops:
            res["skipped"] = "SetDevicePropValue not advertised"
            return
        if D381 not in props or not props[D381].get("advertised"):
            res["skipped"] = "D381 not advertised or not readable"
            return
        rc, data, _ = s.get_value(D381)
        if rc != fp.OK or len(data) < 2:
            res["skipped"] = f"read failed {fp.rc_name(rc)}"
            return
        orig = struct.unpack("<H", data[:2])[0]
        other = 2 if orig == 1 else 1
        res.update(ran=True, before=APO_LABEL.get(orig, orig), target=APO_LABEL.get(other, other))
        self.apo_restore_needed = orig
        t0 = time.monotonic()
        try:
            wrc, now = self._write_d381(s, other)
            res.update(write_rc=fp.rc_name(wrc), read_back=APO_LABEL.get(now, now),
                       write_ms=round((time.monotonic() - t0) * 1000, 1), accepted=now == other)
            self.log("d381_toggle", **res)
        finally:
            self.restore_d381(s, out)

    def restore_d381(self, s, out):
        orig = self.apo_restore_needed
        if orig is None or s.sock is None:
            return
        try:
            rrc, data, _ = s.get_value(D381)
            now = struct.unpack("<H", data[:2])[0] if rrc == fp.OK and len(data) >= 2 else None
            if now != orig:
                wrc, now = self._write_d381(s, orig, patience=5.0)
            ok = now == orig
        except (OSError, fp.PTPError) as e:
            ok, now = False, f"error: {e}"
        out.setdefault("d381_toggle", {})["restored"] = ok
        self.log("d381_restore", ok=ok, value=APO_LABEL.get(now, now))
        if ok:
            self.apo_restore_needed = None
        else:
            self.warn(f"AUTO POWER OFF was not restored over Wi-Fi. Set {MENU_APO} to "
                      f"{APO_LABEL.get(orig, orig)} by hand.")

    # ------------------------------------------------------------ storage and transfer

    def storage_and_transfer(self, s, out, ops):
        res = out["transfer"] = {}
        rc, data, _, st = s.transaction(fp.OP["GetStorageIDs"])
        ids = fp.parse_u32_array(data) if rc == fp.OK and data else []
        res["storage_ids"] = {"rc": fp.rc_name(rc), "ids": [f"{i:#010x}" for i in ids]}
        res["storages"] = []
        for sid in ids:
            rc, data, _, _ = s.transaction(fp.OP["GetStorageInfo"], (sid,))
            if rc == fp.OK and data:
                try:
                    si = fp.parse_storage_info(data)
                    self.log.red.add(si.get("volume_label"))
                    res["storages"].append(self.log.safe({"id": f"{sid:#010x}", **si}))
                except fp.PTPError as e:
                    res["storages"].append({"id": f"{sid:#010x}", "parse_error": str(e)})
        rc, data, _, st = s.transaction(fp.OP["GetObjectHandles"], (0xFFFFFFFF, 0, 0), timeout=60)
        handles = fp.parse_u32_array(data) if rc == fp.OK and data else []
        res["handles"] = {"rc": fp.rc_name(rc), "count": len(handles), "ms": st["ms"]}
        if not handles:
            res["note"] = "no objects listed; the benchmark did not run"
            self.log("transfer_summary", **res)
            return
        infos = {}
        for h in sorted(handles)[:1] + sorted(handles)[-self.args.files:]:
            rc, data, _, st = s.transaction(fp.OP["GetObjectInfo"], (h,), timeout=20)
            if rc == fp.OK and data:
                try:
                    infos[h] = {**fp.parse_object_info(data), "info_ms": st["ms"]}
                except fp.PTPError as e:
                    infos[h] = {"parse_error": str(e)}
        first = sorted(handles)[0]
        res["order_hint"] = {"first_handle": infos.get(first, {}).get("capture_date"),
                             "last_handle": infos.get(sorted(handles)[-1], {}).get("capture_date")}
        picks = sorted(handles)[-self.args.files:]
        files_dir = self.log.dir / "files"
        budget_end = time.monotonic() + self.args.transfer_s
        total_bytes, total_s, per_file = 0, 0.0, []
        chunk = self.args.chunk_kib * 1024
        for h in reversed(picks):
            if time.monotonic() > budget_end or total_bytes > self.args.max_mb * 1e6:
                break
            oi = infos.get(h, {})
            rec = {"handle": h, "filename": oi.get("filename"), "format": f"{oi.get('format', 0):#06x}",
                   "reported_size": oi.get("size"), "info_ms": oi.get("info_ms")}
            if fp.OP["GetThumb"] in ops:
                rc, thumb, _, st = s.transaction(fp.OP["GetThumb"], (h,), timeout=20)
                rec["thumb"] = {"rc": fp.rc_name(rc), "bytes": len(thumb), "ms": st["ms"]}
                if rc == fp.OK and thumb:
                    (files_dir / f"thumb-{h:08x}.jpg").write_bytes(thumb)
            if fp.OP["GetPartialObject"] in ops:
                blob, stats = self._download_partial(s, h, oi.get("size") or 0, chunk)
            else:
                rc, blob, _, st = s.transaction(fp.OP["GetObject"], (h,), timeout=300)
                stats = {"method": "GetObject", "rc": fp.rc_name(rc), "ms": st["ms"], "first_ms": st["first_ms"]}
            rec.update(stats)
            rec["bytes"] = len(blob)
            secs = (stats.get("ms") or 0) / 1000
            rec["mb_s"] = round(len(blob) / 1e6 / secs, 2) if secs else None
            if blob:
                name = re.sub(r"[^A-Za-z0-9_.-]", "_", oi.get("filename") or f"obj-{h:08x}.bin")
                (files_dir / name).write_bytes(blob)
                rec["sha256"] = hashlib.sha256(blob).hexdigest()[:16]
            total_bytes += len(blob)
            total_s += secs
            per_file.append(rec)
            self.log("transfer_file", **rec)
            self._blobs = getattr(self, "_blobs", {})
            self._blobs[h] = blob
        res["files"] = self.log.safe(per_file)
        res["total_mb"] = round(total_bytes / 1e6, 2)
        res["mb_s_overall"] = round(total_bytes / 1e6 / total_s, 2) if total_s else None
        if per_file and fp.OP["GetPartialObject"] in ops:
            res["checks"] = self._transfer_checks(s, per_file, ops)
        self.log("transfer_summary", **{k: v for k, v in res.items() if k != "files"})

    def _download_partial(self, s, h, size, chunk):
        """Chunked download. The reported size can be wrong (libfuji: ~100 KB while D227 is 0),
        so after reaching it one more chunk is asked for; data there means read on to a short chunk."""
        buf = bytearray()
        t0 = time.monotonic()
        first_ms, chunks, rc = None, [], fp.OK
        target, probed, size_short = size, False, False
        while len(buf) < 400 * 1024 * 1024:
            if target and len(buf) >= target:
                if probed:
                    break
                probed, want = True, chunk
            else:
                want = min(chunk, target - len(buf)) if target else chunk
            rc, data, _, st = s.transaction(fp.OP["GetPartialObject"], (h, len(buf), want), timeout=60, quiet=True)
            if first_ms is None:
                first_ms = st["first_ms"]
            if rc != fp.OK or not data:
                break
            buf += data
            chunks.append(st["ms"])
            if probed and target and len(buf) > target:
                size_short, target = True, 0
            if len(data) < want:
                break
        ms = (time.monotonic() - t0) * 1000
        return bytes(buf), {"method": "GetPartialObject", "rc": fp.rc_name(rc), "chunk_kib": chunk // 1024,
                            "chunks": len(chunks), "ms": round(ms, 1), "first_ms": first_ms,
                            "reported_size_short": size_short,
                            "chunk_ms_median": sorted(chunks)[len(chunks) // 2] if chunks else None}

    def _transfer_checks(self, s, per_file, ops):
        checks = {}
        blobs = getattr(self, "_blobs", {})
        small = min((f for f in per_file if f["bytes"]), key=lambda f: f["bytes"], default=None)
        if small and small["bytes"] < 60e6 and fp.OP["GetObject"] in ops:
            rc, whole, _, st = s.transaction(fp.OP["GetObject"], (small["handle"],), timeout=300)
            checks["getobject_vs_partial"] = {
                "rc": fp.rc_name(rc), "ms": st["ms"], "bytes": len(whole),
                "mb_s": round(len(whole) / 1e6 / (st["ms"] / 1000), 2) if st["ms"] else None,
                "same_bytes": whole == blobs.get(small["handle"])}
        big = max(per_file, key=lambda f: f["bytes"])
        blob = blobs.get(big["handle"], b"")
        if len(blob) > 2 * 1024 * 1024:
            off = len(blob) // 2
            rc, part, _, st = s.transaction(fp.OP["GetPartialObject"], (big["handle"], off, 1024 * 1024), timeout=60)
            checks["resume_mid_file"] = {"rc": fp.rc_name(rc), "offset": off, "ms": st["ms"],
                                         "matches": part == blob[off:off + 1024 * 1024]}
            sweep = {}
            for kib in (256, 1024, 4096):
                rc, part, _, st = s.transaction(fp.OP["GetPartialObject"], (big["handle"], 0, kib * 1024), timeout=60)
                sweep[f"{kib}KiB"] = {"rc": fp.rc_name(rc), "bytes": len(part), "ms": st["ms"],
                                      "mb_s": round(len(part) / 1e6 / (st["ms"] / 1000), 2) if st["ms"] else None}
            checks["chunk_sweep"] = sweep
        return checks

    # ------------------------------------------------------------ live view

    def live_view(self, s, out, ops):
        res = out["live_view"] = {"ran": False}
        if fp.OP["InitiateOpenCapture"] not in ops:
            res["skipped"] = "InitiateOpenCapture not advertised"
            return
        res["ran"] = True
        tid = s.tid
        rc, _, _, st = s.transaction(fp.OP["InitiateOpenCapture"], (0, 0), timeout=15)
        res["initiate"] = {"rc": fp.rc_name(rc), "ms": st["ms"]}
        try:
            if rc != fp.OK:
                return
            time.sleep(0.3)
            res["event_pipe"] = self._event_pipe(s.host)
            res["jpeg_pipe"] = self._jpeg_pipe(s.host)
            if not res["jpeg_pipe"].get("frames"):
                rc, data, _, st = s.transaction(fp.OP["GetObject"], (0x80000001,), timeout=15)
                res["getobject_80000001"] = {"rc": fp.rc_name(rc), "bytes": len(data), "ms": st["ms"],
                                             "jpeg": data[:2] == b"\xff\xd8"}
                if rc == fp.OK and data[:2] == b"\xff\xd8":
                    (self.log.dir / "files" / "liveview-getobject.jpg").write_bytes(data)
        finally:
            trc, _, _, _ = s.transaction(fp.OP["TerminateOpenCapture"], (tid,), timeout=15)
            res["terminate"] = fp.rc_name(trc)
            self.log("live_view", **res)

    def _event_pipe(self, host):
        if self.dry:
            return {"skipped": "dry run"}
        try:
            sock = fp.connect(host, self.ports["event"], timeout=3, attempts=2, spacing=2.0, log=self.log)
        except fp.PTPError as e:
            return {"connected": False, "error": str(e)}
        events, end = [], time.monotonic() + 4
        sock.settimeout(1.0)
        try:
            while time.monotonic() < end and len(events) < 20:
                try:
                    pkt = fp.parse_container(fp.recv_packet(sock))
                    events.append({"type": pkt["type"], "code": f"{pkt['code']:#06x}",
                                   "params": [f"{p:#x}" for p in pkt.get("params", [])]})
                except socket.timeout:
                    continue
        except (OSError, fp.PTPError) as e:
            return {"connected": True, "events": events, "error": str(e)}
        finally:
            sock.close()
        return {"connected": True, "events": events}

    def _jpeg_pipe(self, host):
        try:
            sock = fp.connect(host, self.ports["jpeg"], timeout=3, attempts=2, spacing=2.0, log=self.log)
        except fp.PTPError as e:
            return {"connected": False, "error": str(e)}
        frames, t0 = [], time.monotonic()
        sock.settimeout(3.0)
        try:
            while len(frames) < 5 and time.monotonic() - t0 < 6:
                raw = fp.recv_packet(sock)
                soi = raw.find(b"\xff\xd8")
                frames.append({"bytes": len(raw), "header": raw[4:soi].hex() if soi > 0 else raw[4:18].hex(),
                               "t": round(time.monotonic() - t0, 3)})
                if len(frames) == 1 and soi > 0:
                    (self.log.dir / "files" / "liveview-frame.jpg").write_bytes(raw[soi:])
        except (OSError, fp.PTPError) as e:
            err = str(e)
        else:
            err = None
        finally:
            sock.close()
        span = frames[-1]["t"] - frames[0]["t"] if len(frames) > 1 else 0
        return {"connected": True, "frames": len(frames), "fps": round((len(frames) - 1) / span, 1) if span else None,
                "first": frames[0] if frames else None, "error": err}

    # ------------------------------------------------------------ keep-alive

    def keep_alive(self, s, out, props):
        res = out["keep_alive"] = {}
        ans = self.dlg.ask("keepalive_setup", "Keep-alive test", [
            "Last test: does the Wi-Fi connection keep the camera awake?",
            f"On the camera: {MENU_APO} > 1 MIN.",
            "Go back to shooting mode (half-press the shutter).",
            "Press Done, then do not touch the camera for about 3 minutes.",
        ])
        if ans == "Skip":
            res["skipped"] = "owner"
            return
        rc, data, _ = s.get_value(D381)
        apo = struct.unpack("<H", data[:2])[0] if rc == fp.OK and len(data) >= 2 else None
        secs = APO_SECONDS.get(apo)
        res["auto_power_off"] = APO_LABEL.get(apo, apo)
        if secs is None:
            res["note"] = "AUTO POWER OFF is OFF or unknown: watched for a fixed time only"
        scale = 0.02 if self.dry else 1.0
        limit = min(self.args.keepalive_max_s, (secs or 120) + 90) * scale
        poll = self.args.keepalive_poll_s * scale
        pressed, close = self.dlg.background("keepalive", "Keep-alive test", [
            f"Leave the camera alone for up to {int(limit / scale // 60) + 1} minutes.",
            "Do not touch any button or dial.",
            "The Mac checks every few seconds whether it is still connected.",
            "End now: stop this test early.",
        ])
        t0 = time.monotonic()
        res["polls"], res["dropped_at_s"] = 0, None
        try:
            while time.monotonic() - t0 < limit:
                if pressed():
                    res["ended_by_owner"] = True
                    break
                try:
                    rc, data, _ = s.get_value(0xD22E)
                    res["polls"] += 1
                    if rc == BUSY:
                        res.setdefault("busy_at_s", round(time.monotonic() - t0, 1))
                except (OSError, fp.PTPError) as e:
                    res["dropped_at_s"] = round((time.monotonic() - t0) / scale, 1)
                    if self.last_d381_write:
                        res["since_last_d381_write_s"] = round((time.monotonic() - self.last_d381_write) / scale, 1)
                    res["drop_error"] = str(e)
                    s.sock = None
                    break
                time.sleep(poll)
        finally:
            close()
        res["watched_s"] = round((time.monotonic() - t0) / scale, 1)
        if res["dropped_at_s"] is not None:
            res["verdict"] = "camera slept: the Wi-Fi session did not hold it awake"
        elif secs is not None and res["watched_s"] > secs + 30:
            res["verdict"] = "camera stayed awake past AUTO POWER OFF: the Wi-Fi session held it awake"
        else:
            res["verdict"] = "inconclusive"
        self.log("keep_alive", **res)
        if res["dropped_at_s"] is not None:
            self.dlg.ask("keepalive_wake", "Camera asleep", [
                "The camera went to sleep.",
                "Turn it off and on again, then press Done.",
            ], buttons=("Stop", "Done"))

    # ------------------------------------------------------------ mode A: smartphone

    def phone_mode(self):
        self.step("phone")
        out = self.findings["modes"]["smartphone"] = {"ran": False}
        ans = self.dlg.ask("phone_mode", "Smartphone mode", [
            "Test 2: the XApp connection (observe only).",
            "The X-T5 turns on its own Wi-Fi only when XApp asks over Bluetooth.",
            "That network is hidden; XApp gets its password over Bluetooth.",
            "Scan only: the Mac looks for the camera's network while XApp is connected.",
            "I have the password: the Mac joins it and reads, nothing more.",
        ], buttons=("Skip", "I have the password", "Scan only"))
        if ans == "Skip":
            out["skipped"] = "owner"
            return
        out["ran"] = True
        if self.conn_mode_before:
            self.dlg.ask("phone_restore_mode", "Smartphone mode: setup", [
                f"On the camera: {MENU_NET} > CONNECTION MODE > {self.conn_mode_before}.",
                f"{MENU_NET} > Bluetooth/SMARTPHONE SETTING > Bluetooth ON/OFF > ON.",
            ])
        self.dlg.ask("phone_xapp", "Smartphone mode: start XApp", [
            "On your phone: open FUJIFILM XApp and connect to the camera.",
            "Open Remote Shooting (or Image Import) so the camera starts its Wi-Fi.",
            "Leave it open, then press Done.",
        ])
        found, total = ([], 0) if self.dry else self.net.scan_fuji_ssids()
        for f in found:
            self.log.red.add(f["ssid"]) if not Redactor.FUJI_NAME.search(f["ssid"]) else None
        out["scan"] = self.log.safe({"networks_seen": total, "fuji_networks": found})
        self.log("phone_scan", **out["scan"])
        if ans == "I have the password":
            self.dlg.ask("phone_xapp_close", "Smartphone mode: free the camera", [
                "The camera serves one client at a time.",
                "Keep the camera's Wi-Fi on, but leave XApp's screen (do not disconnect if you can).",
                "Press Done when ready.",
            ])
            info = self.join_network("phone_net", "Join the camera's own network.")
            if info:
                host = info.get("router") or "192.168.0.1"
                if self.dry:
                    host = "127.0.0.1"
                out["camera_host"] = host
                self.ptp_suite(host, self.ports["cmd"], out, full=False)
        self.dlg.ask("phone_done", "Smartphone mode: done", ["On your phone: close XApp."], buttons=("Stop", "Done"))

    # ------------------------------------------------------------ finish

    def finish(self):
        self.step("finish")
        try:
            rj = self.net.rejoin_original()
        except Exception as e:  # noqa: BLE001
            rj = {"error": str(e)}
        self.findings["rejoin"] = self.log.safe(rj)
        self.log("rejoin", **rj)
        if self.apo_restore_needed is not None:
            self.warn(f"Check {MENU_APO}: it should read {APO_LABEL.get(self.apo_restore_needed)}.")
        if self.fake:
            self.fake.close()
        self.findings["ended"] = dt.datetime.now().isoformat(timespec="seconds")
        self.findings["stopped_by_owner"] = self.stopped
        (self.log.dir / "findings.json").write_text(json.dumps(self.log.safe(self.findings), indent=1))
        md = render_summary(self.log.safe(self.findings), self.log.dir)
        (self.log.dir / "summary.md").write_text(md)
        restore = []
        if self.conn_mode_before:
            restore.append(f"{MENU_NET} > CONNECTION MODE > {self.conn_mode_before}.")
        restore.append(f"{MENU_APO}: set it back to your usual value.")
        lines = ["The run is over. Logs:", str(self.log.dir), "", "On the camera:", *restore]
        if not (rj.get("internet") or {}).get("ok", True):
            lines += ["", "This Mac is not back online. Pick your Wi-Fi from the menu bar."]
        for w in self.findings["warnings"][:3]:
            lines += ["", w]
        macnet.notify("ukiyo Wi-Fi test", "Finished. Summary in local/wifi-test/.", self.dry)
        try:
            self.dlg.ask("summary", "ukiyo Wi-Fi test: finished", lines, buttons=("Done",))
        except StopRun:
            pass
        print(f"\nsummary: {self.log.dir / 'summary.md'}")


# ---------------------------------------------------------------- value display helpers

def _texty(b):
    ascii_run = re.search(rb"[\x20-\x7e]{6,}", b)
    utf16_run = re.search(rb"(?:[\x20-\x7e]\x00){6,}", b)
    return bool(ascii_run or utf16_run)


def _show_blob(red, b, allow_text=False):
    b = bytes(b)
    if allow_text:
        txt = b.decode("utf-8", "replace").rstrip("\x00")
        return {"text": red.text(txt), "len": len(b)}
    if len(b) <= 64 and not _texty(b):
        return {"hex": b.hex(), "len": len(b)}
    return {"hash": Redactor.tag(b.hex()), "len": len(b), "texty": _texty(b)}


def _show_value(red, code, dtype, data):
    if dtype is not None and dtype in fp.SCALAR:
        try:
            return fp.Reader(data).value(dtype)
        except fp.PTPError:
            return _show_blob(red, data)
    if dtype == 0xFFFF or (dtype is None and data[:1] and len(data) == 1 + data[0] * 2):
        try:
            s = fp.Reader(data).string()
        except fp.PTPError:
            return _show_blob(red, data)
        if code in SAFE_STRINGS and code not in SENSITIVE:
            return {"text": s}
        red.add(s)
        return {"string": Redactor.tag(s), "len": len(s)}
    if dtype is None and len(data) in (1, 2, 4):
        return {"u": int.from_bytes(data, "little"), "len": len(data)}
    if code == 0xD212:
        try:
            return {"events": [(f"{c:#06x}", v) for c, v in fp.parse_events_d212(data)]}
        except fp.PTPError:
            pass
    return _show_blob(red, data)


def _desc_view(red, d):
    v = dict(d)
    for k in ("default", "current"):
        if isinstance(v.get(k), str) and v.get("code") not in SAFE_STRINGS:
            red.add(v[k])
            v[k] = Redactor.tag(v[k])
    if "datatype" in v:
        v["datatype"] = f"{v['datatype']:#06x}"
    if "code" in v:
        v["code"] = f"{v['code']:#06x}"
    if isinstance(v.get("values"), list) and len(v["values"]) > 24:
        v["values"] = v["values"][:24] + [f"... {len(d['values'])} total"]
    return v


def _count(it):
    out = {}
    for x in it:
        out[x] = out.get(x, 0) + 1
    return out


# ---------------------------------------------------------------- dry-run BLE data

def _fake_advert(pairing=False):
    return {"address": "2C3F0E41-FAKE-4E0B-9C51-000000000001", "name": "X-T5-9F3A", "rssi": -48, "tx_power": None,
            "service_uuids": ["91f1de68-dff6-466e-8b65-ff13b0f16fb8"] if pairing else [],
            "service_data": {}, "fuji_mfg": {"type": 2 if pairing else 1, "len": 5 if pairing else 1,
                                             "token": "a1b2c3d4" if pairing else None},
            "other_mfg_ids": []}


def _fake_gatt():
    def ch(uuid, props, value=None, read=None):
        c = {"uuid": uuid, "handle": 0, "properties": props, "role": "", "descriptors": []}
        if value is not None:
            c["value"] = value
        if read:
            c["read"] = read
        return c
    return {"connected": True, "mtu": 185, "services": [
        {"uuid": "0000180a-0000-1000-8000-00805f9b34fb", "role": "Device Information", "characteristics": [
            ch("00002a29-0000-1000-8000-00805f9b34fb", ["read"], b"FUJIFILM"),
            ch("00002a26-0000-1000-8000-00805f9b34fb", ["read"], b"4.31")]},
        {"uuid": "4e941240-d01d-46b9-a5ea-67636806830b", "role": "Wi-Fi hand-off", "characteristics": [
            ch("bf6dc9cf-3606-4ec9-a4c8-d77576e93ea4", ["read"], read="skipped: camera Wi-Fi SSID"),
            ch("e809256a-915c-4967-92e8-53b7d4cad213", ["read"], read="skipped: camera Wi-Fi password")]},
        {"uuid": "15ca59fe-620c-464d-a987-223fab660cde", "role": "lens", "characteristics": [
            ch("68887a7a-10a7-4a4a-aadd-6f9db1abaa0e", ["read"], b"XF23mmF1.4 R LM WR"),
            ch("aaaaaaaa-0000-4000-8000-000000000000", ["read", "notify"], b"\x01\x00\x05")]},
    ]}


# ---------------------------------------------------------------- summary

def render_summary(f, run_dir):
    L = [f"# Wi-Fi test run {run_dir.name}", ""]
    L.append(f"- Started {f.get('started')}, ended {f.get('ended')}." + (" **Dry run (fake camera).**" if f.get("dry_run") else ""))
    if f.get("stopped_by_owner"):
        L.append("- The owner stopped the run early.")
    for e in f.get("errors", []):
        L.append(f"- Error: {e}")
    for w in f.get("warnings", []):
        L.append(f"- Warning: {w}")
    pf = f.get("preflight") or {}
    L += ["", "## Pre-flight", "",
          f"- Wi-Fi device {pf.get('wifi_device')}; network name known: {pf.get('ssid_known')} "
          f"(macOS redacted it: {pf.get('ssid_redacted_by_macos')}).",
          f"- Internet at start: {(pf.get('internet') or {}).get('ok')}. Default route: {pf.get('default_route_interface')}.",
          f"- Other routes: {', '.join(r['port'] for r in pf.get('other_routes') or []) or 'none'}."]
    b = f.get("ble") or {}
    L += ["", "## Bluetooth", ""]
    if not b.get("ran"):
        L.append(f"- Not run ({b.get('skipped') or b.get('error')}).")
    else:
        L.append(f"- Devices seen: {b.get('devices_seen')}; Fujifilm adverts: {len(b.get('fuji_adverts') or [])}.")
        for a in b.get("fuji_adverts") or []:
            L.append(f"  - {a.get('name')} rssi {a.get('rssi')}, mfg {a.get('fuji_mfg')}, services {a.get('service_uuids')}")
        for a in b.get("pairing_adverts") or []:
            L.append(f"  - pairing screen: {a.get('name')} mfg {a.get('fuji_mfg')}, services {a.get('service_uuids')}")
        g = b.get("gatt")
        if g:
            L.append(f"- GATT: {g['counts']}.")
            L += ["", "| Service | Characteristic | Properties | Value or note |", "|---|---|---|---|"]
            for s in g["services"]:
                for c in s["characteristics"]:
                    val = c.get("value") or c.get("read") or ""
                    L.append(f"| {s['uuid'][:8]} {s.get('role') or ''} | {c['uuid'][:8]} {c.get('role') or ''} | "
                             f"{','.join(c['properties'])} | {json.dumps(val)[:60] if val else ''} |")
        if b.get("gatt_error"):
            L.append(f"- GATT error: {b['gatt_error']}")
    for name, m in (f.get("modes") or {}).items():
        L += ["", f"## Mode: {name}", ""]
        if not m.get("ran"):
            L.append(f"- Not run ({m.get('skipped') or 'stopped by the owner'}).")
            continue
        if m.get("connection_mode_before"):
            L.append(f"- CONNECTION MODE before the test: {m['connection_mode_before']}.")
        if m.get("discovery"):
            d = m["discovery"]
            L.append(f"- Discovery: {d.get('method')}, found {d.get('found')} in {d.get('seconds')} s.")
        if m.get("scan"):
            L.append(f"- Wi-Fi scan: {m['scan'].get('networks_seen')} networks, Fujifilm: {m['scan'].get('fuji_networks')}.")
        if m.get("ops_assumed"):
            L.append("- DeviceInfo listed no operations; the run tried them anyway.")
        if m.get("init"):
            L.append(f"- Init ack: {m['init']}. OpenSession: {m.get('open_session')}.")
        di = m.get("device_info") or {}
        if di:
            L.append(f"- GetDeviceInfo: {di.get('rc')}, {di.get('bytes')} bytes, {di.get('model')} {di.get('device_version')}, "
                     f"{di.get('n_operations')} ops, {di.get('n_properties')} props, {di.get('n_events')} events. {di.get('note', '')}")
            if di.get("operations"):
                L.append(f"  - ops: {' '.join(di['operations'])}")
        if m.get("fuji_device_info"):
            L.append(f"- Fuji 0x902B: {m['fuji_device_info']}")
        p = m.get("properties")
        if p:
            L.append(f"- Properties: {p['advertised']} advertised, {p['read_ok']} read OK "
                     f"({p['advertised_ok']} advertised, {p['extra_ok']} of {p['extra_tried']} unadvertised).")
            L.append(f"  - USB tether catalog codes answering over Wi-Fi: {p['catalog_answering_on_wifi']} of {p['usb_tether_catalog']}.")
            L.append(f"  - Writable (described): {p['writable_described']}. Read latency median {p['read_ms_median']} ms, p90 {p['read_ms_p90']} ms.")
            L.append(f"  - Answer codes: {p['rc_counts']}.")
            L.append(f"  - Answering but not in the USB catalog: {' '.join(p['wifi_only_ok'][:40]) or 'none'}.")
            L.append(f"  - Key values: {json.dumps(p['key_values'])[:600]}")
        t = m.get("d381_toggle")
        if t:
            L.append(f"- D381 toggle: {t}")
        tr = m.get("transfer")
        if tr:
            L.append(f"- Storage: {tr.get('storage_ids')}; objects: {tr.get('handles')}. {tr.get('note', '')}")
            if tr.get("files"):
                L += ["", "| File | Bytes | Reported | Method | Chunks | First byte ms | Total ms | MB/s | Thumb ms |",
                      "|---|---|---|---|---|---|---|---|---|"]
                for x in tr["files"]:
                    L.append(f"| {x.get('filename')} | {x.get('bytes')} | {x.get('reported_size')} | {x.get('method')} | "
                             f"{x.get('chunks', '')} | {x.get('first_ms')} | {x.get('ms')} | {x.get('mb_s')} | "
                             f"{(x.get('thumb') or {}).get('ms', '')} |")
                L += ["", f"- Overall {tr.get('mb_s_overall')} MB/s over {tr.get('total_mb')} MB. Order: {tr.get('order_hint')}."]
            if tr.get("checks"):
                L.append(f"- Checks: {json.dumps(tr['checks'])}")
        lv = m.get("live_view")
        if lv:
            L.append(f"- Live view: {json.dumps(lv)[:500]}")
        ka = m.get("keep_alive")
        if ka and ka.get("skipped"):
            L.append(f"- Keep-alive: not run ({ka['skipped']}).")
        elif ka:
            L.append(f"- Keep-alive: {ka.get('verdict')} (AUTO POWER OFF {ka.get('auto_power_off')}, watched "
                     f"{ka.get('watched_s')} s, dropped at {ka.get('dropped_at_s')} s, "
                     f"{ka.get('since_last_d381_write_s')} s after the last D381 write, {ka.get('polls')} polls).")
        for e in m.get("errors", []):
            L.append(f"- Error: {e}")
    rj = f.get("rejoin") or {}
    L += ["", "## End", "", f"- Rejoin: {rj}."]
    L += ["", "Details: `events.jsonl` (every operation), `props-*.json` (every property), `findings.json`."]
    return "\n".join(L) + "\n"


def parse_args(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dry-run", action="store_true", help="fake camera on localhost; scripted answers; no changes")
    ap.add_argument("--out", default=str(REPO / "local" / "wifi-test"), help="log root (default local/wifi-test)")
    ap.add_argument("--real-dialogs", action="store_true", help="with --dry-run: show the real dialogs")
    ap.add_argument("--fake-quirk", action="append", default=[], choices=sorted(QUIRKS),
                    help="with --dry-run: make the fake camera misbehave (repeatable)")
    ap.add_argument("--answer", action="append", default=[], metavar="KEY=BUTTON",
                    help="with --dry-run: answer dialog KEY with BUTTON (repeatable)")
    ap.add_argument("--skip-ble", action="store_true", help="leave out the Bluetooth step")
    ap.add_argument("--ble-no-read", action="store_true", help="list GATT characteristics without reading them")
    ap.add_argument("--ble-scan-s", type=float, default=20.0)
    ap.add_argument("--discovery-s", type=float, default=120.0, help="wireless tether discovery time")
    ap.add_argument("--files", type=int, default=5, help="recent files to download")
    ap.add_argument("--max-mb", type=float, default=400.0, help="download budget in MB")
    ap.add_argument("--transfer-s", type=float, default=240.0, help="download budget in seconds")
    ap.add_argument("--chunk-kib", type=int, default=1024, help="GetPartialObject chunk size")
    ap.add_argument("--keepalive-max-s", type=float, default=420.0)
    ap.add_argument("--keepalive-poll-s", type=float, default=10.0)
    ap.add_argument("--keep-raw", action="store_true", help="also save raw DeviceInfo blobs (unredacted)")
    a = ap.parse_args(argv)
    if a.dry_run:
        a.discovery_s = min(a.discovery_s, 15.0)
    return a


if __name__ == "__main__":
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    Run(parse_args()).main()
