# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Live view and a shutter button over wireless tether, in a browser page.

Usage (camera in CONNECTION MODE > WIRELESS TETHER SHOOTING FIXED, on the Mac's network):
  uv run -q liveview_demo.py [--port 4917]
then open http://127.0.0.1:4917/

A demo, not the app: one camera session, live view frames (object 0x80000001,
read then deleted) after InitiateOpenCapture, and a Take photo button that fires the shutter, downloads the shot from the
tether buffer, deletes the buffer copy (the camera refuses writes until it is empty), and shows
the JPEG. Shots are saved to local/wifi-test/demo/ (gitignored; EXIF carries the camera serial).
Ctrl-C ends live view, returns priority to the camera and closes the session.
"""
import argparse
import http.server
import json
import signal
import struct
import sys
import threading
import time
from pathlib import Path

import fujiptpip as fp
import tether_probe as tp

REPO = Path(__file__).resolve().parents[2]
OUT = REPO / "local" / "wifi-test" / "demo"
INITIATE_CAPTURE, DELETE_OBJECT = 0x100E, 0x100B
BUSY = (0x2019, 0xA001, 0xA002)
STILL = 0x10000001
JPEG_FORMAT = 0x3801

PAGE = """<!doctype html><html><head><meta charset="utf-8"><title>X-T5 live view</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
:root{--bg:#f6f6f4;--fg:#1d1d1b;--muted:#6b6b66;--line:#d9d9d4;--accent:#1d1d1b;--accent-fg:#fff}
@media (prefers-color-scheme:dark){:root{--bg:#141413;--fg:#ededea;--muted:#9a9a94;--line:#33332f;--accent:#ededea;--accent-fg:#141413}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.4 -apple-system,system-ui,sans-serif}
main{max-width:1100px;margin:0 auto;padding:16px;display:grid;gap:16px;grid-template-columns:1fr 1fr}
@media (max-width:800px){main{grid-template-columns:1fr}}
figure{margin:0;border:1px solid var(--line);border-radius:8px;overflow:hidden;background:#000;aspect-ratio:3/2;display:grid;place-items:center}
figure img{width:100%;height:100%;object-fit:contain}
#lvbox{position:relative}#lvbox img{object-fit:cover}
#focus{position:absolute;border:2px solid #fff;box-shadow:0 0 0 1px #0008;display:none}
#focus.ok{border-color:#3c3}#focus.fail{border-color:#e33}
#info{display:flex;flex-wrap:wrap;gap:4px 16px;margin-top:8px;color:var(--muted);font-variant-numeric:tabular-nums}
h2{font-size:13px;font-weight:600;color:var(--muted);margin:0 0 8px}
.bar{grid-column:1/-1;display:flex;gap:12px;align-items:center}
button{font:inherit;padding:10px 18px;border-radius:8px;border:0;background:var(--accent);color:var(--accent-fg);cursor:pointer}
button:disabled{opacity:.5;cursor:default}#status{color:var(--muted)}
</style></head><body><main>
<div class="bar"><button id="shoot">Take photo</button><span id="status"></span></div>
<section><h2>Live view</h2><figure id="lvbox"><img id="lv" src="/stream" alt="Live view"><div id="focus"></div></figure>
<div id="info"></div></section>
<section><h2>Last photo</h2><figure><img id="shot" alt=""></figure></section>
</main><script>
const b=document.getElementById('shoot'),st=document.getElementById('status');
b.onclick=async()=>{b.disabled=true;st.textContent='Shooting…';const t=performance.now();
 try{const r=await fetch('/shoot',{method:'POST'});const j=await r.json();
  if(j.error){st.textContent=j.error}else{document.getElementById('shot').src=j.url;
   st.textContent=`${j.name}: ${(j.bytes/1e6).toFixed(1)} MB in ${((performance.now()-t)/1000).toFixed(1)} s`}}
 catch(e){st.textContent=String(e)}b.disabled=false};
let shown='';setInterval(async()=>{try{const j=await (await fetch('/latest')).json();
 if(j.url&&j.url!==shown){shown=j.url;document.getElementById('shot').src=j.url;
  st.textContent=j.preview?`${j.name}: preview shown, full size downloading…`:`${j.name}: ${(j.bytes/1e6).toFixed(1)} MB in ${(j.ms/1000).toFixed(1)} s`}}catch(e){}},700);
const fb=document.getElementById('focus'),info=document.getElementById('info');
const ss=us=>!us?'':us>=1e6?`${(us/1e6).toFixed(us%1e6?1:0)}s`:`1/${Math.round(1e6/us)}`;
setInterval(async()=>{try{const d=await (await fetch('/status')).json();
 // Focus area "x,y,size": centre origin, x right, y up; 117 points = 13x9 grid, 425 = 25x17.
 const fa=(d.focus_area||'').split(',').map(Number);
 if(fa.length>=3&&!isNaN(fa[0])){const [cols,rows]=d.focus_points===4?[25,17]:[13,9];
  const w=Math.max(1,fa[2]/3)/cols*100,h=Math.max(1,fa[2]/3)/rows*100*1.0;
  const cx=(fa[0]+(cols-1)/2+.5)/cols*100,cy=((rows-1)/2-fa[1]+.5)/rows*100;
  Object.assign(fb.style,{display:'block',left:`${cx-w/2}%`,top:`${cy-h/2}%`,width:`${w}%`,height:`${h}%`});
  fb.className=d.af_status===2?'ok':d.af_status===3?'fail':''}
 const fd=(d.focus_distance||'').split(',').map(Number);
 const dist=fd[0]>=1024?'∞':fd[0]?`${(fd[0]/100).toFixed(2)} m`:'';
 const iso=d.iso>0x7fffffff?'ISO AUTO':d.iso?`ISO ${d.iso}`:'';
 const ev=d.exposure_comp!=null?`${(((d.exposure_comp<<16)>>16)/1000).toFixed(1)} EV`:'';
 const bat=(d.battery_ratio||'').split(',')[0];
 info.textContent=[d.focus_mode===1?'MF':d.focus_mode===32770?'AF-C':'AF-S',ss(d.shutter),
  d.aperture?`f/${(d.aperture/100).toFixed(1)}`:'',ev,iso,dist&&`Focus ${dist}`,bat&&`Battery ${bat}%`]
  .filter(Boolean).join('   ')}catch(e){}},1000);
</script></body></html>"""


class Camera:
    def __init__(self, wait):
        ip, bc = tp.lan_ip_and_broadcast()
        log = lambda kind, **kw: print(kind, kw.get("port", ""), kw.get("error", ""), flush=True) \
            if kind in ("tether_invite", "init_ack", "init_fail", "nc_relay", "tcp_connect_fail") else None
        fp.FORBIDDEN_OPS.discard(INITIATE_CAPTURE)  # this demo fires the shutter
        fp.FORBIDDEN_OPS.discard(DELETE_OBJECT)     # tether buffer only, after download
        self.lock = threading.Lock()
        # Right after another session closes, the camera sometimes drops the next init at once.
        # Three tries, 15 s apart, each with a fresh discovery.
        for attempt in range(3):
            print("Looking for the camera...", flush=True)
            found = fp.discover_tether(ip, bc, wait, log)
            if not found:
                sys.exit("No camera found. Set CONNECTION MODE > WIRELESS TETHER SHOOTING FIXED.")
            self.s = fp.Session(found["ip"], found["port"], name="ukiyo-demo", timeout=30.0, log=log)
            try:
                self.s.connect_and_init(attempts=1)
                break
            except (OSError, fp.PTPError) as e:
                print("Init failed:", e, flush=True)
                self.s.sock = None
                if attempt == 2:
                    sys.exit("The camera keeps closing the connection. Set CONNECTION MODE again.")
                time.sleep(15)
        print("OpenSession", fp.rc_name(self.s.open_session()), flush=True)
        self.prio = self.u16(0xD207)
        self.clear_buffer()
        self.set16(0xD01B, 1)
        # Live view runs under camera priority, so the camera's own buttons (half-press focus)
        # keep working. PC priority (D207 = 2) is taken only around a release, as Capture One does.
        self.set16(0xD207, 1)
        rc, _, _, _ = self.s.transaction(fp.OP["InitiateOpenCapture"], (0, 0), quiet=True)
        print("InitiateOpenCapture", fp.rc_name(rc), flush=True)
        self.frames = 0
        self.last_rc = None
        self.last_check = 0.0
        self.latest = None

    def u16(self, code):
        rc, d, _ = self.s.get_value(code)
        return struct.unpack("<H", d[:2])[0] if rc == fp.OK and len(d) >= 2 else None

    def set16(self, code, v, tries=12):
        for _ in range(tries):
            rc, _ = self.s.set_value(code, struct.pack("<H", v))
            if rc not in BUSY:
                return rc
            time.sleep(0.25)
        return rc

    def handles(self):
        rc, d, _, _ = self.s.transaction(fp.OP["GetObjectHandles"], (STILL, 0, 0), quiet=True)
        return fp.parse_u32_array(d) if rc == fp.OK else []

    def clear_buffer(self):
        for h in self.handles():
            self.s.transaction(DELETE_OBJECT, (h, 0), quiet=True)

    def frame(self):
        # Frames are object 0x80000001. The camera keeps returning the same frame until the client
        # deletes that object; then the next read is a fresh frame (tested 2026-10-18).
        # A shot taken on the camera waits in the tether buffer, and live view stops
        # (InvalidObjectHandle) until the client downloads and deletes it.
        with self.lock:
            if time.monotonic() - self.last_check > 2:
                self.last_check = time.monotonic()
                self.import_buffer()
            rc, d, _, _ = self.s.transaction(fp.OP["GetObject"], (0x80000001,), timeout=10, quiet=True)
            if rc == fp.OK:
                self.s.transaction(DELETE_OBJECT, (0x80000001, 0), quiet=True)
            elif self.import_buffer():
                self.s.transaction(fp.OP["TerminateOpenCapture"], (0,), quiet=True)
                self.s.transaction(fp.OP["InitiateOpenCapture"], (0, 0), quiet=True)
        if rc != self.last_rc:
            self.last_rc = rc
            print("live view frame:", fp.rc_name(rc), len(d), "bytes", flush=True)
        if rc != fp.OK or not d:
            time.sleep(0.2)
            return None
        i = d.find(b"\xff\xd8")
        self.frames += 1
        return d[i:] if i >= 0 else None

    STATUS = {0xD395: "focus_area", 0xD347: "focus_area_0", 0xD025: "focus_points", 0xD209: "af_status",
              0xD02A: "af_mode", 0x500D: "shutter", 0x5007: "aperture", 0x500F: "iso", 0xD36A: "battery",
              0x5010: "exposure_comp", 0xD36B: "battery_ratio", 0xD226: "focus_distance",
              0x500A: "focus_mode"}

    def status(self):
        """Raw values for the overlay. Strings decoded, numbers as ints; shown as they are until
        their formats are mapped."""
        out = {}
        with self.lock:
            for code, key in self.STATUS.items():
                rc, d, _ = self.s.get_value(code)
                if rc != fp.OK or not d:
                    out[key] = None
                elif len(d) >= 1 and d[0] * 2 + 1 == len(d):
                    out[key] = fp.Reader(d).string()
                elif len(d) == 2:
                    out[key] = struct.unpack("<H", d)[0]
                elif len(d) == 4:
                    out[key] = struct.unpack("<I", d)[0]
                else:
                    out[key] = d.hex()
        return out

    def import_buffer(self):
        """Downloads every shot waiting in the tether buffer (JPEG kept for the page, RAW saved),
        then deletes the buffer copies. Returns how many objects it took. Caller holds the lock."""
        hs = self.handles()
        if not hs:
            return 0
        time.sleep(0.5)  # RAW+JPEG arrive as two objects
        hs = self.handles()
        OUT.mkdir(parents=True, exist_ok=True)
        infos = {}
        for h in hs:
            rc, d, _, _ = self.s.transaction(fp.OP["GetObjectInfo"], (h,), quiet=True)
            infos[h] = fp.parse_object_info(d) if rc == fp.OK else {}
        # The JPEG first, and its small thumbnail before the full file, so the page shows the shot
        # within a second while the full-size files stream.
        hs.sort(key=lambda h: infos[h].get("format") != JPEG_FORMAT)
        for h in hs:
            info = infos[h]
            name = info.get("filename") or f"obj-{h:x}"
            if info.get("format") == JPEG_FORMAT:
                rc, thumb, _, st = self.s.transaction(fp.OP["GetThumb"], (h,), timeout=20, quiet=True)
                if rc == fp.OK and thumb:
                    tname = name.rsplit(".", 1)[0] + ".thumb.jpg"
                    (OUT / tname).write_bytes(thumb)
                    self.latest = {"name": name, "url": f"/photo/{tname}", "bytes": len(thumb),
                                   "ms": st["ms"], "preview": True}
                    print("Thumbnail", tname, f"{len(thumb) / 1e3:.0f} KB in {st['ms']:.0f} ms", flush=True)
            rc, data, _, st = self.s.transaction(fp.OP["GetObject"], (h,), timeout=180, quiet=True)
            if rc == fp.OK:
                (OUT / name).write_bytes(data)
                print("Downloaded", name, f"{len(data) / 1e6:.1f} MB in {st['ms'] / 1000:.1f} s", flush=True)
                if info.get("format") == JPEG_FORMAT:
                    self.latest = {"name": name, "url": f"/photo/{name}", "bytes": len(data), "ms": st["ms"]}
            self.s.transaction(DELETE_OBJECT, (h, 0), quiet=True)
        return len(hs)

    def shoot(self):
        with self.lock:
            self.import_buffer()  # pending shots block every write, priority included
            before = set(self.handles())
            self.set16(0xD207, 2)
            self.set16(0xD208, 0x0200)
            self.s.transaction(INITIATE_CAPTURE, (0, 0), quiet=True)
            time.sleep(0.8)
            self.set16(0xD208, 0x0304)
            rc, _, _, _ = self.s.transaction(INITIATE_CAPTURE, (0, 0), quiet=True)
            if rc != fp.OK:
                self.set16(0xD207, 1)
                return {"error": f"The camera did not fire ({fp.rc_name(rc)})."}
            for _ in range(40):
                time.sleep(0.25)
                if [h for h in self.handles() if h not in before]:
                    break
            else:
                self.set16(0xD207, 1)
                return {"error": "No photo arrived in the tether buffer."}
            self.latest = None
            self.import_buffer()
            self.set16(0xD207, 1)  # writes work again once the buffer is empty
            return self.latest or {"error": "Download failed."}

    def close(self):
        with self.lock:
            try:
                self.s.transaction(fp.OP["TerminateOpenCapture"], (0,), quiet=True)
                self.clear_buffer()
                if self.prio:
                    self.set16(0xD207, self.prio)
            except (OSError, fp.PTPError) as e:
                print("Clean-up error:", e, "- set PC/camera priority on the camera if needed")
            self.s.close()


def handler_for(cam):
    class H(http.server.BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def do_GET(self):
            if self.path == "/":
                body = PAGE.encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            elif self.path == "/stream":
                self.send_response(200)
                self.send_header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
                self.end_headers()
                try:
                    while True:
                        f = cam.frame()
                        if f:
                            self.wfile.write(b"--frame\r\nContent-Type: image/jpeg\r\nContent-Length: "
                                             + str(len(f)).encode() + b"\r\n\r\n" + f + b"\r\n")
                        else:
                            time.sleep(0.1)
                except (BrokenPipeError, ConnectionResetError):
                    pass
            elif self.path == "/status":
                body = json.dumps(cam.status()).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            elif self.path == "/dump":
                with cam.lock:
                    rc, d, _, _ = cam.s.transaction(fp.OP["GetDeviceInfo"], (), quiet=True)
                    codes = fp.parse_device_info(d)["properties"] if rc == fp.OK else []
                    vals = {}
                    for c in codes:
                        rc, v, _ = cam.s.get_value(c)
                        vals[f"{c:#06x}"] = v.hex() if rc == fp.OK else fp.rc_name(rc)
                body = json.dumps(vals).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            elif self.path == "/latest":
                body = json.dumps(cam.latest or {}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            elif self.path.startswith("/photo/"):
                p = OUT / Path(self.path[7:]).name
                if not p.is_file():
                    self.send_error(404)
                    return
                data = p.read_bytes()
                self.send_response(200)
                self.send_header("Content-Type", "image/jpeg")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
            else:
                self.send_error(404)

        def do_POST(self):
            if self.path != "/shoot":
                self.send_error(404)
                return
            try:
                res = cam.shoot()
            except (OSError, fp.PTPError) as e:
                res = {"error": f"{type(e).__name__}: {e}"}
            body = json.dumps(res).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    return H


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--port", type=int, default=4917)
    ap.add_argument("--wait", type=float, default=150.0, help="discovery time in seconds")
    args = ap.parse_args()
    sys.path.insert(0, str(REPO / "tools" / "ptp"))
    import owner_lock
    owner_lock.hold()
    # A background shell ignores SIGINT; let SIGTERM run the same clean-up as Ctrl-C.
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    cam = Camera(args.wait)
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), handler_for(cam))
    print(f"Open http://127.0.0.1:{args.port}/  (Ctrl-C to stop)", flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        srv.shutdown()
        cam.close()
        print("Closed.", flush=True)


if __name__ == "__main__":
    main()
