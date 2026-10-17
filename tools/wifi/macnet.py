"""macOS network helpers: read the Wi-Fi state, join a network, rejoin, check the internet.

Commands that change the Mac's network go through `Net.change`, which only prints them in
dry-run mode. Passwords never reach a log.
"""
import ipaddress
import json
import re
import subprocess
import time

REDACTED = "<redacted>"


def sh(cmd, timeout=30):
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout.strip(), p.stderr.strip()
    except (OSError, subprocess.TimeoutExpired) as e:
        return -1, "", str(e)


class Net:
    def __init__(self, log, dry_run):
        self.log, self.dry_run = log, dry_run
        self.device = self.wifi_device()
        self.original = None    # {"ssid": str|None, ...}; ssid kept in memory only
        self.changed = False

    # ------------------------------------------------------------ reading

    def wifi_device(self):
        rc, out, _ = sh(["networksetup", "-listallhardwareports"])
        m = re.search(r"Hardware Port: (?:Wi-Fi|AirPort)\nDevice: (\S+)", out)
        return m.group(1) if m else "en0"

    def current_ssid(self):
        """Try three sources. macOS 14+ redacts the SSID without Location Services for the terminal."""
        seen = {}
        rc, out, _ = sh(["networksetup", "-getairportnetwork", self.device])
        m = re.search(r"Current Wi-Fi Network: (.+)", out)
        seen["networksetup"] = m.group(1).strip() if m else out[:80]
        rc, out, _ = sh(["ipconfig", "getsummary", self.device])
        m = re.search(r"^\s*SSID : (.+)$", out, re.M)
        seen["ipconfig"] = m.group(1).strip() if m else None
        rc, out, _ = sh(["system_profiler", "SPAirPortDataType", "-json"], timeout=60)
        try:
            ifs = json.loads(out)["SPAirPortDataType"][0]["spairport_airport_interfaces"]
            cur = next(i for i in ifs if i.get("_name") == self.device).get("spairport_current_network_information")
            seen["system_profiler"] = cur.get("_name") if cur else None
        except (ValueError, KeyError, IndexError, StopIteration, TypeError):
            seen["system_profiler"] = None
        ssid = None
        for v in (seen["ipconfig"], seen["system_profiler"]):
            if v and v != REDACTED and "redacted" not in v.lower():
                ssid = v
                break
        if not ssid:
            v = seen["networksetup"]
            if v and "not associated" not in v.lower() and "redacted" not in v.lower() and "Error" not in v:
                ssid = v
        redacted = any(v and "redacted" in str(v).lower() for v in seen.values())
        return ssid, redacted, {k: ("<present>" if v else v) for k, v in seen.items()}

    def ipv4(self, device=None):
        device = device or self.device
        _, ip, _ = sh(["ipconfig", "getifaddr", device])
        _, mask, _ = sh(["ipconfig", "getoption", device, "subnet_mask"])
        _, router, _ = sh(["ipconfig", "getoption", device, "router"])
        out = {"ip": ip or None, "mask": mask or None, "router": router or None}
        if ip and mask:
            try:
                out["broadcast"] = str(ipaddress.IPv4Network(f"{ip}/{mask}", strict=False).broadcast_address)
                out["network"] = str(ipaddress.IPv4Network(f"{ip}/{mask}", strict=False))
            except ValueError:
                pass
        return out

    def other_routes(self):
        """Active non-Wi-Fi interfaces with IPv4 (Ethernet, iPhone USB, Thunderbolt bridge)."""
        _, out, _ = sh(["networksetup", "-listallhardwareports"])
        routes = []
        for port, dev in re.findall(r"Hardware Port: (.+)\nDevice: (\S+)", out):
            if dev == self.device:
                continue
            _, ip, _ = sh(["ipconfig", "getifaddr", dev])
            if ip:
                routes.append({"port": port, "device": dev, "ip": ip})
        _, out, _ = sh(["route", "-n", "get", "default"])
        m = re.search(r"interface: (\S+)", out)
        return routes, (m.group(1) if m else None)

    def internet(self, device=None):
        cmd = ["curl", "-s", "-o", "/dev/null", "-w", "%{http_code} %{time_total}", "--max-time", "6"]
        if device:
            cmd += ["--interface", device]
        rc, out, _ = sh(cmd + ["http://captive.apple.com/hotspot-detect.html"], timeout=10)
        parts = out.split()
        ok = rc == 0 and parts and parts[0] == "200"
        return {"ok": bool(ok), "http": parts[0] if parts else None,
                "s": float(parts[1]) if len(parts) > 1 else None}

    def scan_fuji_ssids(self):
        """Names of nearby networks that look like a Fujifilm camera's (system_profiler, ~10 s)."""
        rc, out, _ = sh(["system_profiler", "SPAirPortDataType", "-json"], timeout=90)
        found, total = [], 0
        try:
            ifs = json.loads(out)["SPAirPortDataType"][0]["spairport_airport_interfaces"]
            for i in ifs:
                for n in i.get("spairport_airport_other_local_wireless_networks", []) or []:
                    total += 1
                    name = n.get("_name", "")
                    if "FUJI" in name.upper() or re.match(r"X-[A-Z0-9]+", name):
                        found.append({"ssid": name, "channel": n.get("spairport_network_channel"),
                                      "security": n.get("spairport_security_mode")})
        except (ValueError, KeyError, IndexError, TypeError):
            pass
        return found, total

    def arp_neighbours(self):
        _, out, _ = sh(["arp", "-an"])
        return [ip for ip in re.findall(r"\((\d+\.\d+\.\d+\.\d+)\) at (?!\(incomplete\))", out)]

    # ------------------------------------------------------------ changing

    def change(self, cmd, shown=None):
        shown = shown or cmd
        if self.dry_run:
            self.log("net_change_dry_run", cmd=" ".join(shown))
            return 0, "", ""
        rc, out, err = sh(cmd, timeout=60)
        self.log("net_change", cmd=" ".join(shown), rc=rc, out=out[:200], err=err[:200])
        return rc, out, err

    def wait_ip(self, seconds=45):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            info = self.ipv4()
            if info["ip"] and not info["ip"].startswith("169.254."):
                return info
            time.sleep(2.0)
        return self.ipv4()

    def join(self, ssid, password=None):
        cmd = ["networksetup", "-setairportnetwork", self.device, ssid]
        shown = ["networksetup", "-setairportnetwork", self.device, "<ssid>"]
        if password:
            cmd.append(password)
            shown.append("<password>")
        self.changed = True
        rc, out, err = self.change(cmd, shown)
        # networksetup returns 0 and prints an error on failure.
        failed = rc != 0 or "Error" in out or "Could not" in out or "Failed" in out
        info = {"ip": "127.0.0.1", "mask": "255.0.0.0", "broadcast": "127.0.0.1"} if self.dry_run else self.wait_ip()
        return (not failed) and bool(info.get("ip")), info

    def rejoin_original(self):
        """Rejoin the network recorded at pre-flight, then verify the internet. Two fallbacks."""
        if not self.changed:
            return {"needed": False, "internet": self.internet()}
        ssid = (self.original or {}).get("ssid")
        tried = []
        if ssid:
            ok, info = self.join(ssid)  # known networks take the password from the keychain
            tried.append("networksetup join")
            if ok or self.dry_run:
                net = {"ok": True} if self.dry_run else self.internet()
                if net["ok"]:
                    self.changed = False
                    return {"needed": True, "tried": tried, "internet": net}
        # Fallback: power-cycle Wi-Fi; macOS then joins its preferred network.
        self.change(["networksetup", "-setairportpower", self.device, "off"])
        time.sleep(0 if self.dry_run else 3)
        self.change(["networksetup", "-setairportpower", self.device, "on"])
        tried.append("wifi power cycle")
        if not self.dry_run:
            self.wait_ip(60)
        net = {"ok": True} if self.dry_run else self.internet()
        self.changed = not net["ok"]
        return {"needed": True, "tried": tried, "internet": net}


def notify(title, text, dry_run=False):
    if dry_run:
        print(f"  [notification] {title}: {text}")
        return
    sh(["osascript", "-e", "on run argv", "-e",
        "display notification (item 2 of argv) with title (item 1 of argv) sound name \"Glass\"",
        "-e", "end run", title, text])
