# Sources that can label the X-T5 PTP properties

Fujifilm's own software, file formats and adjacent public work that can name property codes and
value encodings. Open tethering code (libgphoto2, petabyt's camlib, fudge, libfuji, fujihack) is
covered in `docs/fuji-vendor-ptp.md`; this file does not repeat it.

Research only, 2026-10-17. Nothing here was tested on the camera. Every label below is a
candidate until a hands-on test confirms it on the X-T5 (`docs/protocol.md` rules apply).

## Sources at a glance

| Source | What it can label | Cost / effort | Licence notes | Link |
|---|---|---|---|---|
| fujifilm-ptp-recipes (ILFforever) | All 23 recipe codes, slot and live; value tables from X RAW STUDIO; XSDK live codes; MakerNote crosswalk | Read only, done | Docs CC BY 4.0: reuse with credit | [repo](https://github.com/ILFforever/fujifilm-ptp-recipes) |
| FUJIFILM Camera Control SDK (XSDK) headers | Value constants for ~60 settings; the constants equal the PTP wire values | Headers already public (see below); official copy needs EULA click-through | Fujifilm copyright; EULA bans reverse engineering and "tracing" | [SDK page](https://www.fujifilm-x.com/global/special/camera-control-sdk/) |
| ExifTool FujiFilm.pm | MakerNote tag IDs and encodings for every image setting | Free, public; test needs owner and camera | Perl licence; tag facts reusable | [FujiFilm.pm](https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/FujiFilm.pm) |
| grawji (p5k369) | Settings backup blob (handle 0) layout, checksum, restore; D185 capture method | Read only | GPL-3.0: cite facts, do not copy code | [repo](https://github.com/p5k369/grawji) |
| xt30-recipe-manager | Backup blob: X-T30 layout, checksum formula, Tether App restore | Read only (French) | MIT | [repo](https://github.com/xt30-recipe-lab/xt30-recipe-manager) |
| rawji (pinpox) | D185 RAW-conversion profile: 29 parameters and their order | Read only | No licence: cite only | [repo](https://github.com/pinpox/rawji) |
| X-T5 settings backup (our own read) | Every menu setting, by diffing blob offsets | Owner switches USB mode; one GetObject; diff per change | Our data; blob holds the serial, keep it in `local/` | — |
| Capture One 16.5 (static bundle inspection) | 164 codes: short name, datatype, UI label, value labels; tether call order | Read only, done 2026-10-17 | Capture One EULA; facts only, no code | report in `local/mapping/` (not in git) |
| FUJIFILM Tether App / Tether Plug-in PRO / Capture One (USB capture) | Codes behind each control the app exposes | Windows VM on a Linux host with usbmon; licences for the paid apps | App EULAs not read; check before capture | [Tether App](https://www.fujifilm-x.com/global/products/software/tether-app/) |
| XApp / Camera Remote over Wi-Fi | Wi-Fi remote codes only (a small set) | iPhone capture with `rvictl` on the Mac | Observation of our own traffic | — |
| Wireshark PTP/IP dissector | Nothing Fuji-specific | — | — | [packet-ptpip.c](https://gitlab.com/wireshark/wireshark/-/blob/master/epan/dissectors/packet-ptpip.c) |
| libfuji Lua dissector | ~35 Fuji Wi-Fi property names (D212, D22x, DFxx) | Read only | MIT | [wireshark_dissector.lua](https://github.com/petabyt/libfuji/blob/master/docs/wireshark_dissector.lua) |
| hkr/fuji-cam-wifi-tool | Wi-Fi remote codes on older bodies | Read only | Unlicense | [repo](https://github.com/hkr/fuji-cam-wifi-tool) |
| fffw (tiredboffin) | Firmware structures; backup parser `ffbak` (unpublished) | Ask the author | MIT | [repo](https://github.com/tiredboffin/fffw) |

## Candidate labels this research produces

Codes in `catalog/x-t5.json` that these sources name or correct. Proposals only; the catalog is
not edited.

| Code | Catalog today | Candidate | Source | Strength |
|---|---|---|---|---|
| D191 | unnamed | PresetDRangePriority (slot). 0 OFF, 1 WEAK, 2 STRONG, 32768 AUTO | ptp-recipes, write-tested on X-H2 | high |
| D18E | PresetImageSize | composite size × aspect: 1-27 = S/M/L × 9 aspects, 28-30 INSTAX (table in ptp-recipes `xrfc-value-tables.md` §6) | X RAW STUDIO label table | high |
| D1A5 | unnamed ("image format?" in FilmKit) | live Image Size, same composite as D18E. It is not a slot field | ptp-recipes; X-H2 write 7→8 | high |
| D1A3 | PresetLongExpNR | **conflict**: X RAW STUDIO's block builder sends `lLMOMode` here (live twin D34D) | ptp-recipes §15, decompiled | medium; test |
| D1A4 | PresetColorSpace | agrees (live twin D00A; 1 sRGB, 2 AdobeRGB) | ptp-recipes; SDK constants | high |
| D1B2 | unnamed | File type: 7 JPEG, 18 HEIF (also 9 TIFF, 11 TIFF 16-bit in RAW CONV.) | XRFC label table | high |
| D183 | unnamed | StartRawConversion (XSDK_ConvertRAWImage) | XSDK decompile; xt30 probe | high |
| D185 | unnamed | RAW conversion profile (D185 profile below) | rawji, grawji captures | high |
| D038 | unnamed | error detail (XSDK_GetErrorDetails) | XSDK decompile only | low |
| D039 | unnamed (menu-map: face/eye bitfield) | **conflict**: release capability (XSDK_CapReleaseEx) | XSDK decompile only | low; test |
| D224 | unnamed | release status (XSDK_GetReleaseStatus) | XSDK decompile only | low |
| D22D | unnamed | movie recording status | XSDK decompile only | low |
| D235 | unnamed (datatype 0) | dormant multi-property read buffer (set a code list, get the values) | XSDK decompile; X-H2 returns 0x200A | medium |
| D231 | unnamed (enum 0,10,20,50,150) | PIXEL SHIFT MULTI SHOT interval: shortest, 1 s, 2 s, 5 s, 15 s | SDK constants match the enum exactly | medium |
| D1BD | unnamed (enum 1-7) | SUBJECT DETECTION: 1 OFF, 2 ANIMAL, 3 BIRD, 4 AUTOMOBILE, 5 MOTORCYCLE & BIKE, 6 AIRPLANE, 7 TRAIN | SDK constants | medium |
| D026 | MFAssistMode (no values) | 1 STANDARD, 2 split B&W, 3 split colour, 4-11 peaking white/red/blue/yellow × low/high, 12 microprism | SDK constants | medium |
| D233 | AfcCustomValues | zone switching 0 FRONT, 1 AUTO, 2 CENTER confirmed | ExifTool AFCSettings presets (Set 3 = 2,2,Auto; Set 4 = 0,1,Front) | high |
| D238 | unnamed (enum 1-3) | PERFORMANCE: 1 NORMAL, 2 ECONOMY, 3 BOOST (one of several candidates) | SDK constants list 1-7 | low; test |
| every 1/2 item | "1 = ON probably" | 1 = ON, 2 = OFF is the SDK's generic constant | SDK constants | medium |

The XSDK live codes also name D037 (shooting mode) and movie codes D240, D242, D243, D26C, D26F,
D271, D272. Older Wi-Fi tools give D240 = shutter speed and D242 = battery, so these codes mean
different things on different bodies. Check each in movie mode.

## FUJIFILM Camera Control SDK (XSDK)

- **Contents.** C/C++ libraries for Windows, macOS, Linux (Ubuntu x64, Raspberry Pi OS 32/64-bit)
  and Android (USB). Headers `XAPI.h`, `XAPIOpt.h`, one `<model>.h` and `<model>_MOV.h` per
  camera, an "SDK manual", sample code. Latest Ver. 1.34 (2025-11-12). The X-T5 is supported.
- **Access.** Individuals accept a click-through EULA on the
  [agreement page](https://www.fujifilm-x.com/global/camera-control-sdk/agreement/); the page
  names no account. Businesses use a contact form. The agreement takes effect at download.
- **EULA terms that matter** (read 2026-10-17):
  - 2(a), 3.3: use the SDK only to develop and test a camera-control app.
  - 2(b): ship the library only as object code inside that app.
  - 3.5: no reverse engineering, and no "method to trace, decompile, or disassemble the SDK".
  - 3.9: do not put the SDK under an open-source licence (GPL, LGPL named).
  - 4: Fujifilm keeps "all proprietary information or trade secrets contained in it".
  - No confidentiality clause. Nothing about publishing facts learned from it. Japanese law.
  - Using the SDK with a camera voids its warranty (closing notice).
- **What it can label.** The headers give names and numbers for each setting's values. The API
  maps calls to property codes inside the DLL, not in the headers.
- **The constants are the PTP wire values.** Checked against codes we already confirmed:
  AUTO POWER OFF 1-6 equals D381 exactly; NR 0x0000-0x8000 equals D01C; drive S = 4 (D201);
  AF-S 0x8001, AF-C 0x8002 (0x500A); focus points 3 = 13×9, 4 = 25×17 (D025); tones ×10;
  DR AUTO 0xFFFF; WB codes equal the D199 table.
- **Already public.** [Scdouglas1999/Fujicom](https://github.com/Scdouglas1999/Fujicom) (an
  MIT-labelled ASCOM driver) commits the Fujifilm-copyright headers, `XAPI.dll` and `XSDK.DAT`.
  Reading them binds us to no EULA, but they are copyrighted and likely redistributed against
  3.4. Use facts only, cite, never copy the files.
- **Projects that call the SDK** (none map APIs to PTP codes): NINA-Fujifilm-Native-Plugin,
  Fujibooth, Fujifilm-SDK-HotFolder, fujiset, fujixcam on PyPI.
- **API → PTP code mapping** exists publicly only for ~60 exports, recovered by decompiling
  `XGFXAPI.dll` inside X RAW STUDIO (ptp-recipes `xsdk-live-shooting-properties.md`).

**Owner decision.** Publishing derived facts (a code's name, a value's meaning) is not addressed
by the EULA. Accepting it and then capturing what the SDK sends over USB would likely count as
"tracing". Recommendation: do not accept the EULA. Use the public constants as hints and confirm
each label with a camera test, so every catalog entry rests on our own observation.

## ExifTool crosswalk

MakerNote tags from ExifTool `FujiFilm.pm` v2.02, matched to the live (tether) and slot (RAW
CONV.) codes. "Relation" says how to turn one encoding into the other. ExifTool lacks two tags;
ptp-recipes supplies them from X-H2 test shots (marked ‡).

| Setting | MakerNote tag | MakerNote encoding | Live | Slot | PTP encoding | Relation |
|---|---|---|---|---|---|---|
| Film simulation | 0x1401 FilmMode; B&W, ACROS, Sepia in 0x1003 | hex: 0x000 PROVIA … 0x600 CLASSIC CHROME, 0x700 ETERNA, 0x800 CLASSIC Neg., 0x900 BLEACH BYPASS, 0xA00 NOSTALGIC Neg., 0xB00 REALA ACE; mono 0x300-0x310, ACROS 0x500-0x503 | D001 | D192 | enum 1-20 | lookup table |
| Grain | 0x1047 roughness, 0x104C size | 0/32/64; size 0/16/32 | D023 | D195 | 1 OFF, 2-5 weak/strong × small/large | lookup: (32,16)=2, (64,16)=3, (32,32)=4, (64,32)=5 |
| Color Chrome | 0x1048 | 0/32/64 | D029 | D196 | 1/2/3 | MN = (PTP − 1) × 32 |
| Color Chrome FX Blue | 0x104E | 0/32/64 | D030 | D197 | 1/2/3 | MN = (PTP − 1) × 32 |
| Smooth Skin | 0x104A ‡ | 0/32/64 | D189 | D198 | 1/2/3 | MN = (PTP − 1) × 32 |
| White balance | 0x1002 | 0 Auto, 1 white priority, 2 ambience, 0x100 Daylight, 0x200 Shade ("Cloudy"), 0x300-0x302 Fluorescent 1-3, 0x400 Incandescent, 0x600 Underwater, 0xF00+ Custom, 0xFF0 Kelvin | 0x5005 | D199 | 2, 0x8020, 0x8021, 4, 0x8006, 0x8001-3, 6, 8, 0x8008+, 0x8007 | lookup table; Custom unverified |
| Colour temperature | 0x1005 | Kelvin | D017 | D19C | Kelvin | **identical** |
| WB shift R, B | 0x100A (int32s × 2) | dial × 20 | D00B, D00C | D19A, D19B | dial, −9..9 | MN = PTP × 20 |
| Highlight tone | 0x1041 | −16 per step | D320 | D19D | ×10, half steps | MN = −PTP × 1.6 |
| Shadow tone | 0x1040 | −16 per step | D321 | D19E | ×10, half steps | MN = −PTP × 1.6 |
| Color | 0x1003 Saturation | 0 → 0, +1 0x080, +2 0x100, +3 0x0C0, +4 0x0E0, −1 0x180, −2 0x400, −3 0x4C0, −4 0x4E0 | D008 | D19F | ×10 | lookup table |
| Sharpness | 0x1001 | −4 0x00, −3 0x01, −2 0x02, −1 0x82, 0 0x03, +1 0x84, +2 0x04, +3 0x05, +4 0x06 | 0x5015 | D1A0 | ×10 | lookup table; see note |
| High ISO NR | 0x100E | 0 0x000, +1 0x180, +2 0x100, +3 0x1C0, +4 0x1E0, −1 0x280, −2 0x200, −3 0x2C0, −4 0x2E0 | D01C | D1A1 | 0x0000-0x8000, non-monotonic | lookup table |
| Clarity | 0x100F | dial × 1000 | D032 | D1A2 | ×10 | MN = PTP × 100 |
| Dynamic range | 0x1402 setting, 0x1403 development DR, 0x140B auto DR % | 0 auto / 1 manual; 100/200/400 | D007 | D190 | 100/200/400, 65535 AUTO (slot: 0 AUTO) | 0x1403 **identical** to PTP when manual |
| D Range Priority | 0x1443 (0 auto, 1 fixed), 0x1444 auto result, 0x1445 fixed level | 1 weak, 2 strong (0x1444 adds 3 plus) | D02E | D191 | 0, 1, 2, 0x8000 | fixed: 0x1445 **identical**; 0x1443 = 0 ↔ 0x8000 |
| Mono colour WC | 0x1049 (int8) | dial direct | D104 | D193 | ×10, −180..180 | MN = PTP / 10 |
| Mono colour MG | 0x104B (int8) | dial direct | D031 | D194 | ×10 | MN = PTP / 10 |
| LMO | 0x1045 | 0 off, 1 on | D34D | D1A3? | 1 ON, 2 OFF | MN = 2 − PTP (unverified) |
| AF-C custom | 0x102E AFCSettings | nibbles: tracking, speed, zone | — | — | D232 set, D233 "tracking,speed,zone" | same numbers; D233 order matches |
| Release/focus priority | 0x102B | AF-S, AF-C nibbles: 1 release, 2 focus | D10A, D10B | — | 1/2 | **identical** |
| Focus / Pre-AF / AF area | 0x102D | FocusMode2 0 M, 1 S, 2 C; PreAF 0/1; area 0 single, 1 zone, 2 wide | 0x500A, D34B, ? | — | 0x0001 M, 0x8001 S, 0x8002 C; PreAF 1/2 | lookup |
| Drive | 0x1103 DriveSettings | 0 S, 1 CL, 2 CH; byte 3 = fps | D201 | — | 4 S, 2 CL, 3 CH | lookup |
| Shutter type | 0x1050 | 0 mechanical, 1 electronic, 2 ES long, 3 EFC | D304? | — | enum 1,2,3,4,5,7 | test |
| Crop | 0x104D CropMode, 0x1044 DigitalZoom (/8) | bits: 2 sports finder, 4 ES 1.25×, 8 digital tele-conv | D16F, D1B3 | — | — | test |
| Image stabilisation | 0x1422 [type, mode] | mode 1 continuous, 2 shooting only | D351 | — | SDK: 1 continuous, 2 shoot, 3 off | mode **identical** (expected) |
| Flicker reduction | 0x1446 | on when bits 8-11 = 1 | D238? | — | — | test |
| Pixel shift | 0x1100 AutoBracketing (6), 0x1105 PixelShiftShots | — | D1B8-D1BB, D231 | — | — | test |
| Long exposure NR | not in ExifTool | — | D322 | D1A3? | 1/2 | — |

Notes:

- **Five settings share one encoding** across MakerNote and PTP: colour temperature,
  development DR, DRP fixed level, AF priority and (expected) IS mode. Every other one needs a
  lookup or a scale.
- **Compare raw values (`-n`), not labels.** ptp-recipes' X-H2 shots used another EXIF
  library. Its sharpness and B&W filter strings differ from ExifTool's, and it calls them
  wrong. Where it gives raw numbers (sharpness 0 and 6 at −4 and +4, NR, Sepia 0x310, ACROS
  0x500-0x503), they agree with ExifTool's table.
- ptp-recipes claims MakerNote Color and Sharpness include a film-sim offset. The X-H2 shots in
  that same document show plain dial positions. Test it.

## Capture One 16.5 (static bundle inspection)

Capture One 16.5.1.14 carries Phase One's own Fuji PTP code; it does not embed the Fuji SDK. A
static reading on 2026-10-17 (strings, symbols and disassembly; nothing run or attached) recovered
its property table: 164 codes with a short name, a PTP datatype, a UI label, a description and
value labels, plus the order of its tether calls. The owner rules its names authoritative unless
they contradict something we confirmed on the camera. The catalog import (source `Capture One
16.5`) found no such contradiction. It added 46 codes, renamed 20, named 13 unnamed ones and added
value labels (D208 capture types, D21C error codes, D17F hardware control). Its datatypes are not
taken: every text field is uint16 in its table, so the X-T5's own descriptors win. The main
correction is D38C: a zoom-position selector (Capture One writes D170's value into it and reads
D38D, the aperture list for that position), not a "PC Mode" flag. Capture One also shows
synthetic codes that are not on the wire (D900-D904 and 0x5892, derived from D36A and D36D; the
0x5802 MF Drive alias, which writes D171). The report and its working notes stay in
`local/mapping/`.

## Settings backup (BACKUP RESTORE)

What public work shows about X Acquire's file, which the Tether App and XApp (over BLE) also use:

- The blob is PTP object handle 0, format 0x5000. It exists only in USB RAW CONV./BACKUP RESTORE
  mode; card-reader mode answers 0x2009.
- Read: GetObjectInfo(0), then GetObject(0). Restore: SendObjectInfo (format 0x5000, size; X
  Acquire sends a 1076-byte dataset) and SendObject. A restore rewrites every setting, not a slot.
- Header: ASCII `FUJIFILM`, `X-BACKUP`, a version, the model at 0x14, the serial at 0x34.
- An additive u16 LE checksum sits at offset 176. A wrong checksum gets 0x200F on restore. The
  camera rewrites a few counter bytes itself.
- Custom banks C1-C7 are 256-byte records with their own codes, not PTP encodings (tones stored as
  4 − dial, Kelvin as an index into a descending preset list).
- Sizes: 5660 bytes (X-Pro2 generation), 33404 (X-T3), 5628 (X-T30). The X-T5 size and layout
  are not public. fffw's unpublished `ffbak` reports the C1-C7 record "more or less stable across
  models".
- Our X-T5 advertises 0x900C, 0x900D and 0x901D in backup mode (libfuji: Fuji SendObjectInfo,
  SendObject2, SendObject). rawji uses 0x900C/0x900D to send a RAF for conversion.

The RAW CONV. property set in `local/camera/x-t5/props-backupmode.json` is this mode's other
half. D183-D187 drive X RAW STUDIO:

| Code | Use |
|---|---|
| D183 | start RAW conversion |
| D184 | IOP code: processor id string, two comma-separated hex words |
| D185 | conversion profile; readable only after a RAF is sent with 0x900C/0x900D (format 0xF802) |
| D186, D187 | `<model>_<firmware>` check codes; X RAW STUDIO looks them up in `XRFC.DAT` |

**D185 profile** (rawji, grawji): u16 parameter count, the IOP code as a PTP string, then 29
int32 slots from offset 0x201. Slot order: ShootingCondition, FileType, ImageSize, ImageQuality,
ExposureBias, DynamicRange, WideDRange, FilmSimulation, GrainEffect, ColorChromeEffect,
WBShootCond, WhiteBalance, WBShiftR, WBShiftB, WBColorTemp, HighlightTone, ShadowTone, Color,
Sharpness, NoiseReduction, (reserved), ColorSpace, BlackImageTone, SmoothSkinEffect,
ColorChromeBlue, MonochromaticColorRG, Clarity, (reserved), (reserved). Values use the PTP
encodings (×10 tones, 0x1000-step NR). grawji found offsets by capturing X RAW STUDIO in a Windows
guest under QEMU/KVM, with usbmon on the Linux host, and diffing one toggle at a time.

## Other apps worth capturing

| App | What a capture could reveal | Effort | Needs |
|---|---|---|---|
| FUJIFILM Tether App (official) | Backup/restore on the X-T5; tether controls; the X-T5 blob size | 1 day | Windows VM with USB pass-through on a Linux host (usbmon) |
| FUJIFILM Tether Plug-in PRO (Lightroom Classic) | The widest remote settings surface Fujifilm ships | 2 days | Lightroom licence, paid plug-in, same VM |
| Capture One (Fujifilm tether) | Exposure, WB, film sim, drive. The static reading (below) already gives its names and call order; a capture would add only the order on the wire | 1-2 days | Capture One licence, same VM |
| X RAW STUDIO | Already decompiled and captured by ptp-recipes and grawji | none | — |
| X Acquire | Backup transfer; the Tether App covers it | low value | same VM |
| XApp / Camera Remote | Wi-Fi codes, BLE backup | half a day | iPhone; `rvictl` on the Mac for Wi-Fi traffic |
| Fuji X Weekly app | Recipe writes; a subset of the 23 known codes | low value | — |
| Cascable | Fuji Wi-Fi control | low value | licence |

macOS cannot capture USB traffic on Apple silicon, so every USB capture needs a Linux host.

## Proposed next steps

Ranked by labels gained per hour.

1. **Adopt ptp-recipes facts** (no camera). Bring D191, D18E, D1A5, D1B2, D183 and D185 into
   `catalog/x-t5.json` with source "fujifilm-ptp-recipes (CC BY 4.0)", and credit it in
   `docs/recipes.md`. Resolve the D1A3 conflict with step 3.
2. **MakerNote diff test** (owner and camera, 1 hour). Set one setting, shoot a JPEG, read the
   live codes, run `exiftool -a -u -H -n -FujiFilm:all` on the file, change the setting, repeat.
   Order: LMO, shutter type, flicker reduction, IS mode, drive CL/CH, AF area, sports finder,
   digital tele-conv, D Range Priority AUTO, then one pass over Color and Sharpness to check the
   film-sim offset claim. `tools/catalog/makernote_diff.py` pairs the photos with a guided
   session's steps; the procedure is in the work tracker.
3. **Slot field check** (owner, 15 min). In RAW CONV. mode, change one slot's LONG EXPOSURE NR
   and then its LMO on the camera; read D1A3 each time.
4. **SDK-hint checks** (owner, 30 min, tether mode). Step PIXEL SHIFT interval (D231), SUBJECT
   DETECTION type (D1BD), MF ASSIST (D026), PERFORMANCE (D238), and flip FACE/EYE detection to
   see whether D039 moves at all.
5. **Read the X-T5 backup blob** (owner switches the USB mode). One GetObjectInfo(0) and
   GetObject(0), read only, saved under `local/`. Then change one menu item per read and diff.
   This maps settings that have no PTP property. Never restore without a separate decision.
   `tools/ptp/read_backup.py` reads it; `tools/catalog/backup_diff.py` diffs the reads.
6. **Movie-mode dump** (owner flips the STILL/MOVIE switch). Check the XSDK movie codes D240,
   D242, D243, D26C, D26F, D271, D272. `tools/ptp/steps/x-t5-second-movie.json` steps through
   them.
7. **USB capture of the Tether App** (needs the VM). Gives the X-T5 blob size and confirms the
   backup path. Read the app's EULA first.
8. **Ask tiredboffin** whether `ffbak` covers the X-Processor 5 record layout.

Do not accept the XSDK EULA unless the owner decides the official manual is worth its terms.

## Owner actions

- Decide on the XSDK EULA (recommendation: decline).
- Run tests 2-6 with the camera.
- For step 7: a Linux host with usbmon, a Windows VM with USB pass-through, and app licences.
