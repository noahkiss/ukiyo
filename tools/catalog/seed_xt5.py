# /// script
# requires-python = ">=3.11"
# ///
"""Seed catalog/x-t5.json and the fake camera's property shape from the local dumps.

Usage: seed_xt5.py LOCAL_DIR [--force]
  LOCAL_DIR holds props-tether.json, props-movie.json, props-backupmode.json and fuji-names.json.

Writes ../../catalog/x-t5.json (refuses to overwrite it without --force, since lab sessions
edit it) and ../../src-tauri/src/camera/fake_x-t5.json. Neither file carries a current value,
serial number or owner string: the catalog holds names and shapes, the fake shape holds
codes, types, forms and made-up values.
"""
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
TYPES = {"0x0000": "undefined", "0x0001": "int8", "0x0002": "uint8", "0x0003": "int16",
         "0x0004": "uint16", "0x0005": "int32", "0x0006": "uint32", "0x0007": "int64",
         "0x0008": "uint64", "0xffff": "string"}

PTP_NAMES = {
    0x5001: "BatteryLevel", 0x5002: "FunctionalMode", 0x5003: "ImageSize", 0x5004: "CompressionSetting",
    0x5005: "WhiteBalance", 0x5006: "RGBGain", 0x5007: "FNumber", 0x5008: "FocalLength",
    0x5009: "FocusDistance", 0x500A: "FocusMode", 0x500B: "ExposureMeteringMode", 0x500C: "FlashMode",
    0x500D: "ExposureTime", 0x500E: "ExposureProgramMode", 0x500F: "ExposureIndex",
    0x5010: "ExposureBiasCompensation", 0x5011: "DateTime", 0x5012: "CaptureDelay",
    0x5013: "StillCaptureMode", 0x5014: "Contrast", 0x5015: "Sharpness", 0x5016: "DigitalZoom",
    0x5017: "EffectMode", 0x5018: "BurstNumber", 0x5019: "BurstInterval", 0x501A: "TimelapseNumber",
    0x501B: "TimelapseInterval", 0x501C: "FocusMeteringMode", 0x501D: "UploadURL", 0x501E: "Artist",
    0x501F: "CopyrightInfo",
}

# RAW CONV. recipe-slot fields (read_presets.py, after FilmKit).
PRESET = {
    0xD18E: "PresetImageSize", 0xD18F: "PresetImageQuality", 0xD190: "PresetDynamicRange",
    0xD191: None, 0xD192: "PresetFilmSimulation", 0xD193: "PresetMonoWC", 0xD194: "PresetMonoMG",
    0xD195: "PresetGrainEffect", 0xD196: "PresetColorChrome", 0xD197: "PresetColorChromeFxBlue",
    0xD198: "PresetSmoothSkin", 0xD199: "PresetWhiteBalance", 0xD19A: "PresetWBShiftR",
    0xD19B: "PresetWBShiftB", 0xD19C: "PresetColorTemp", 0xD19D: "PresetHighlightTone",
    0xD19E: "PresetShadowTone", 0xD19F: "PresetColor", 0xD1A0: "PresetSharpness",
    0xD1A1: "PresetHighIsoNR", 0xD1A2: "PresetClarity", 0xD1A3: "PresetLongExpNR",
    0xD1A4: "PresetColorSpace", 0xD1A5: None,
}

FILM_SIMS = ["PROVIA/STANDARD", "Velvia/VIVID", "ASTIA/SOFT", "PRO Neg. Hi", "PRO Neg. Std",
             "MONOCHROME", "MONOCHROME+Ye", "MONOCHROME+R", "MONOCHROME+G", "SEPIA",
             "CLASSIC CHROME", "ACROS", "ACROS+Ye", "ACROS+R", "ACROS+G", "ETERNA/CINEMA",
             "CLASSIC Neg.", "ETERNA BLEACH BYPASS", "NOSTALGIC Neg.", "REALA ACE"]

def labels(pairs):
    return {str(k): v for k, v in pairs}

# Curated entries: protocol.md (confirmed) and menu-map.md high/medium guesses (likely).
C, L = "confirmed", "likely"
LAB, PROTO, MAP, FK, GP = "lab session", "protocol.md", "menu map", "filmkit", "libgphoto2"
CURATED = {
    0xD22E: dict(name="CameraScreen", confidence=C, source=LAB,
                 values=labels([(1, "Shooting"), (2, "Playback"), (3, "Menu open")])),
    0xD381: dict(name="AutoPowerOff", menu="SET UP > POWER MANAGEMENT > AUTO POWER OFF", confidence=C, source=LAB,
                 values=labels([(1, "5 MIN"), (2, "2 MIN"), (3, "OFF"), (4, "1 MIN"), (5, "30 SEC"), (6, "15 SEC")]),
                 notes="Write 1 -> 3 tested. Movie mode keeps its own value."),
    0xD307: dict(name="StillMovie", menu="STILL/MOVIE switch", confidence=C, source=LAB,
                 values=labels([(1, "Movie"), (2, "Stills")])),
    0x500A: dict(menu="Focus mode selector", confidence=C, source=LAB,
                 values=labels([(0x8001, "S"), (0x8002, "C"), (1, "M")])),
    0xD201: dict(name="ReleaseMode", menu="Drive dial", confidence=C, source=LAB,
                 values=labels([(4, "S"), (2, "CL"), (3, "CH"), (10, "BKT"), (8, "Movie")]),
                 notes="CH = 3 is not confirmed."),
    0x500F: dict(menu="ISO dial", confidence=C, source=LAB, values=labels([(-3, "AUTO")]),
                 notes="AUTO is 0xFFFFFFFD."),
    0xD035: dict(name="IsoDialAuto", menu="ISO dial (A position)", confidence=L, source=LAB,
                 notes="Flips when the ISO dial moves to or from A."),
    0xD033: dict(name="ShutterDialAuto", menu="Shutter speed dial (A position)", confidence=L, source=LAB,
                 notes="Flips when the shutter dial moves to or from A."),
    0xD034: dict(name="ExposureCompDialC", menu="Exposure compensation dial (C position)", confidence=L, source=LAB,
                 notes="Flips when the dial moves to or from C."),
    0x5010: dict(menu="Exposure compensation dial", confidence=C, source=LAB, unit="EV", scale=1000),
    0x5007: dict(menu="Aperture ring", confidence=C, source=LAB, unit="f/", scale=100),
    0x500D: dict(menu="Shutter speed dial", confidence=C, source=LAB),
    0xD18C: dict(name="PresetSlot", confidence=C, source=FK,
                 values=labels([(i, f"C{i}") for i in range(1, 8)]),
                 notes="Selects which custom slot D18D-D1A5 read and write."),
    0xD18D: dict(name="PresetName", confidence=C, source=FK),
    0xD029: dict(name="ColorChromeEffect", menu="IMAGE QUALITY SETTING > COLOR CHROME EFFECT", confidence=C, source=PROTO,
                 values=labels([(1, "OFF"), (2, "WEAK"), (3, "STRONG")]),
                 notes="libgphoto2 calls it Shadowing."),
    0xD02E: dict(name="DRangePriority", menu="IMAGE QUALITY SETTING > D RANGE PRIORITY", confidence=C, source=PROTO,
                 values=labels([(0, "OFF"), (1, "WEAK"), (2, "STRONG"), (32768, "AUTO")]),
                 notes="libgphoto2 calls it WideDynamicRange."),
    0xD007: dict(menu="IMAGE QUALITY SETTING > DYNAMIC RANGE", confidence=L, source=MAP,
                 values=labels([(65535, "AUTO"), (100, "DR100"), (200, "DR200"), (400, "DR400")])),
    0xD023: dict(menu="IMAGE QUALITY SETTING > GRAIN EFFECT", confidence=L, source=MAP,
                 values=labels([(1, "OFF"), (2, "WEAK SMALL"), (3, "STRONG SMALL"), (4, "WEAK LARGE"), (5, "STRONG LARGE")])),
    0xD01C: dict(menu="IMAGE QUALITY SETTING > HIGH ISO NR", confidence=L, source=MAP,
                 values=labels([(0x5000, "+4"), (0x6000, "+3"), (0x0000, "+2"), (0x1000, "+1"), (0x2000, "0"),
                                (0x3000, "-1"), (0x4000, "-2"), (0x7000, "-3"), (0x8000, "-4")])),
    0xD001: dict(menu="IMAGE QUALITY SETTING > FILM SIMULATION", confidence=L, source=GP,
                 values=labels(enumerate(FILM_SIMS, 1)),
                 notes="19 and 20 are not in libgphoto2; inferred from the enum length."),
    0xD025: dict(menu="AF/MF SETTING > NUMBER OF FOCUS POINTS", confidence=L, source=MAP,
                 values=labels([(3, "117 POINTS"), (4, "425 POINTS")])),
    0xD026: dict(menu="AF/MF SETTING > MF ASSIST", confidence=L, source=MAP),
    0xD10A: dict(name="ReleaseFocusPriorityAFS", menu="AF/MF SETTING > RELEASE/FOCUS PRIORITY > AF-S PRIORITY SELECTION",
                 confidence=L, source=MAP, notes="libgphoto2 calls it ShutterPriorityMode1."),
    0xD10B: dict(name="ReleaseFocusPriorityAFC", menu="AF/MF SETTING > RELEASE/FOCUS PRIORITY > AF-C PRIORITY SELECTION",
                 confidence=L, source=MAP, notes="libgphoto2 calls it ShutterPriorityMode2."),
    0xD34E: dict(name="AeAfLockMode", menu="SET UP > BUTTON/DIAL SETTING > AE/AF-LOCK MODE", confidence=L, source=MAP),
    0xD15B: dict(menu="SET UP > SAVE DATA SET-UP > FRAME NO.", confidence=L, source=MAP),
    0xD031: dict(name="MonochromaticColorMG", menu="IMAGE QUALITY SETTING > MONOCHROMATIC COLOR", confidence=L, source=MAP,
                 notes="Range collapses to 0 with a colour film simulation."),
    0xD104: dict(name="MonochromaticColorWC", menu="IMAGE QUALITY SETTING > MONOCHROMATIC COLOR", confidence=L, source=MAP,
                 notes="libgphoto2 calls it BlackImageTone."),
    0xD156: dict(name="LensFirmwareVersion", confidence=L, source=MAP, notes="Hex BCD: 0x105 = 1.05."),
    0xD16D: dict(name="FrameNumber", menu="SET UP > SAVE DATA SET-UP > FRAME NO.", confidence=L, source=MAP,
                 notes="The current file number."),
    0xD16E: dict(name="UsbConnectionMode", menu="SET UP > CONNECTION SETTING > CONNECTION MODE", confidence=L, source=MAP,
                 values=labels([(5, "USB TETHER SHOOTING FIXED"), (6, "USB RAW CONV./BACKUP RESTORE")])),
    0xD1B3: dict(name="DigitalTeleConv", menu="SHOOTING SETTING > DIGITAL TELE-CONV.", confidence=L, source=MAP,
                 values=labels([(100, "OFF"), (140, "1.4x"), (200, "2.0x")])),
    0xD1BF: dict(name="ConstantSpeedFocus", menu="SET UP > BUTTON/DIAL SETTING > LENS ZOOM/FOCUS SETTING", confidence=L,
                 source=MAP, scale=10),
    0xD1C0: dict(name="ConstantSpeedZoom", menu="SET UP > BUTTON/DIAL SETTING > LENS ZOOM/FOCUS SETTING", confidence=L,
                 source=MAP, scale=10),
    0xD232: dict(name="AfcCustomSet", menu="AF/MF SETTING > AF-C CUSTOM SETTINGS", confidence=L, source=MAP,
                 values=labels([(i, f"SET {i}") for i in range(1, 7)])),
    0xD233: dict(name="AfcCustomValues", menu="AF/MF SETTING > AF-C CUSTOM SETTINGS", confidence=L, source=MAP,
                 notes="Tracking sensitivity, speed tracking sensitivity, zone area switching."),
    0xD237: dict(name="CopyrightAuthor", menu="SET UP > SAVE DATA SET-UP > COPYRIGHT INFO", confidence="guess", source=MAP),
}


def main():
    local = pathlib.Path(sys.argv[1])
    force = "--force" in sys.argv
    load = lambda n: json.load(open(local / n))
    tether, movie = load("props-tether.json")["properties"], load("props-movie.json")["properties"]
    backup = load("props-backupmode.json")["properties"]
    names = load("fuji-names.json")

    codes = sorted({int(k, 16) for k in (*tether, *movie, *backup)})
    catalog_props, shape = {}, []
    for code in codes:
        key = f"{code:#06x}"
        t, m = tether.get(key, {"error": 1}), movie.get(key, {"error": 1})
        desc = t if "type" in t else m if "type" in m else None
        modes = [n for n, d in (("stills", t), ("movie", m)) if "error" not in d] + (["rawconv"] if key in backup else [])
        name = names.get(key) or PTP_NAMES.get(code) or PRESET.get(code)
        e = {"name": name, "menu": None, "type": TYPES.get(desc["type"], desc["type"]) if desc else None,
             "values": {}, "unit": None, "scale": None,
             "confidence": "confirmed" if code in PTP_NAMES else ("likely" if name else None),
             "source": "PTP" if code in PTP_NAMES else GP if key in names else FK if code in PRESET and name else None,
             "modes": modes, "notes": None}
        e.update(CURATED.get(code, {}))
        catalog_props[key] = {k: v for k, v in e.items() if v not in (None, {}, [])}
        shape.append(fake_shape(code, t, m, key in backup, backup.get(key)))

    meta = {k: load("props-tether.json")[k] for k in ("manufacturer", "model")}
    catalog = {
        "schema": 1,
        "model": meta["model"],
        "manufacturer": meta["manufacturer"],
        "usb": {"vendor": "0x04cb", "product": "0x02fc"},
        "firmware": ["4.31"],
        "properties": catalog_props,
    }
    out = ROOT / "catalog" / "x-t5.json"
    if out.exists() and not force:
        sys.exit(f"{out} exists; pass --force to replace it (it may hold lab labels)")
    out.write_text(json.dumps(catalog, indent=2, ensure_ascii=False) + "\n")
    fake = {"model": meta["model"], "manufacturer": meta["manufacturer"], "firmware": "9.99", "properties": shape}
    (ROOT / "src-tauri" / "src" / "camera" / "fake_x-t5.json").write_text(json.dumps(fake, indent=1) + "\n")
    print(f"{len(codes)} codes; {sum(1 for p in catalog_props.values() if p.get('name'))} named")


def made_up(d):
    """A plausible value with no tie to a real camera: the factory default, or the form's first value."""
    if "enum" in d and d["enum"]:
        return d["default"] if d["default"] in d["enum"] else d["enum"][0]
    if "range" in d:
        lo, hi, _ = d["range"]
        return d["default"] if isinstance(lo, int) and lo <= d["default"] <= hi else lo
    if d["type"] == "0xffff":
        return ""
    return d.get("default", 0) if isinstance(d.get("default"), int) else 0


def fake_shape(code, t, m, in_backup, b):
    desc = t if "type" in t else m if "type" in m else None
    s = {"code": f"{code:#06x}", "stills": "error" not in t, "movie": "error" not in m, "rawconv": in_backup}
    if desc:
        s.update(type=desc["type"], rw=desc["rw"], value=made_up(desc))
        for f in ("enum", "range"):
            if f in desc:
                s[f] = desc[f]
        if desc["type"] == "0xffff" and "enum" not in desc and "range" not in desc:
            s["value"] = ""
    else:
        s.update(type="0x0000", rw=False, value=None)
    if b and "bytes" in b:
        s["rawconv_bytes"] = b["bytes"]
    return s


if __name__ == "__main__":
    main()
