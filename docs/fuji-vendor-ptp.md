# Fujifilm vendor PTP: what open sources say

A reference compiled from open-source code and docs on 2026-10-17, plus one closed app read
statically (c1). Nothing here was tested on our camera; `protocol.md` holds what we observed. Each row names its source. Treat a row as a lead
until a lab session confirms it on the X-T5.

## Sources

| Key | Source | Licence | What it is |
|---|---|---|---|
| gp | [gphoto/libgphoto2](https://github.com/gphoto/libgphoto2/tree/92171a382f185f617e95cb9acecf2d43688b19ab/camlibs/ptp2) `camlibs/ptp2` at `92171a3` | LGPL | `ptp.h` names, `config.c` widgets, `library.c` capture and live view |
| lf | [petabyt/libfuji](https://github.com/petabyt/libfuji/tree/34af78ada37613b6dfa18a439900e49a54dde8e3) at `34af78a` (successor of camlib/fudge) | MIT | `lib/fujiptp.h`, `lib/fuji_usb.c`, `fp/src/d185.c` |
| mk | [mikefsq/ptp](https://github.com/mikefsq/ptp/tree/0fe3c68) at `0fe3c68`, `fuji/` | MIT | Go driver tested on an X-T5. Names from the X-T5's Fuji plugin binary (`names_xt5_gen.go`), 263 X-T5 descriptors (`props_xt5_gen.go`), capture notes (`capture.go`, `OPERATION.md`) |
| sdk | Fujifilm X SDK headers `XAPI.h`, `XAPIOpt.h`, `X-T5.h`, as redistributed in [NINA-Fujifilm-Native-Plugin/Headers](https://github.com/Scdouglas1999/NINA-Fujifilm-Native-Plugin/tree/8ad28da/Headers) | Fujifilm SDK | SDK constants. They give values, not PTP codes |
| fc | [po4yka/fuji-cli](https://github.com/po4yka/fuji-cli/tree/31c97b5/docs/internals) at `31c97b5` | MIT | X-T5 fw 4.31 device audits and a static analysis of the 4.31 firmware image |
| il | [ILFforever/fujifilm-ptp-recipes](https://github.com/ILFforever/fujifilm-ptp-recipes/tree/daba2d9/docs) at `daba2d9` | see repo | Recipe slots, live (C0) codes, decompiled X RAW STUDIO SDK (`XGFXAPI.dll`). Write tests on an X-H2 |
| ox | [antonyshyn/OpenXWebcam](https://github.com/antonyshyn/OpenXWebcam/blob/a198c33/CameraEngine/Sources/CameraEngine/FujiCamera.swift) at `a198c33` | MIT | macOS live view over PTP |
| tv | [brendan-mistly/tetherview](https://github.com/brendan-mistly/tetherview/blob/8b40f59/Sources/TetherviewCore/FujiLiveView.swift) at `8b40f59` | see repo | iOS live view over PTP |
| hk | [hkr/fuji-cam-wifi-tool](https://github.com/hkr/fuji-cam-wifi-tool/blob/c597fc4/lib/include/capabilities.hpp) at `c597fc4` | see repo | Older Wi-Fi remote protocol |
| js | [Junsudo/fuji-ptp-notes](https://github.com/Junsudo/fuji-ptp-notes/blob/f662d3c/README.md) at `f662d3c` | see repo | GFX100RF slot notes and value tables |
| c1 | Capture One 16.5.1.14, static bundle inspection on 2026-10-17 (report in `local/mapping/`, not in git) | Capture One EULA; facts only | Phase One's own Fuji PTP code, no Fuji SDK. Its property table names 164 codes with a datatype, UI label and value labels |

Plugin names (mk) cover most codes. Some are misattributed: mk calls D307 `CustomSettingAutoUpdate`,
but our lab confirmed D307 as the STILL/MOVIE switch. Use a plugin name as a lead, not as proof.

## Vendor operations

| Code | Name | Meaning and parameters | Transport | Source |
|---|---|---|---|---|
| 0x900C | SendObjectInfo (Fuji) | Creates a file. Used to upload a RAF for in-camera conversion (format 0xF802, name `FUP_FILE.dat`) | USB | lf `lib/fujiptp.h`, `lib/fuji_usb.c` |
| 0x900D | SendObject2 | Sends the data for 0x900C | USB | lf |
| 0x901D | SendObject (Fuji) | Writes to a file | USB | lf, `docs/dev.md` |
| 0x9020 / 0x9021 | Initiate / TerminateMovieCapture | Start and stop movie | Wi-Fi | gp `ptp.h:1058`, hk `message.hpp` |
| 0x9022 | GetCapturePreview | Last image | Wi-Fi | gp, hk |
| 0x9023-0x9025 | StepZoom / StartZoom / StopZoom | Power zoom | Wi-Fi | lf |
| 0x9026 / 0x9027 | LockS1Lock / UnlockS1Lock | Set focus point / unlock focus | Wi-Fi | gp `ptp.h:1061`, lf |
| 0x902B | GetDeviceInfo (Fuji) | All property descriptors in one reply: uint32 count, then size-prefixed descriptors | Wi-Fi; gp calls it when advertised | gp `ptp.c:5845`, `library.c:457` |
| 0x902C / 0x902D / 0x902E | StepShutterSpeed / StepFNumber / StepExposureBias | One parameter: 1 step up, 0 step down | Wi-Fi | lf, hk |
| 0x9030 | CancelInitiateCapture | | Wi-Fi | gp |
| 0x9040-0x9042 | FmSendObjectInfo / FmSendObject / FmSendPartialObject | | Wi-Fi | gp |
| 0x9050-0x9056, 0x9060 | Image import folders, dates, extension object info, thumb, partial object | Newer app transfer | Wi-Fi/BLE | lf |
| 0x9801-0x9805 | unknown | Advertised by the X-T5 in USB CARD READER mode and listed in fw 4.31. Meaning unknown | USB | fc `x-t5-firmware-4.31-static-analysis-2026-09-03.md` |

USB tether shooting uses only standard operations for control: InitiateCapture 0x100E,
InitiateOpenCapture 0x101C and TerminateOpenCapture 0x1018, plus property writes.

## Events

| Code | Name | Source |
|---|---|---|
| 0xC001 | PreviewAvailable | gp `ptp.h:1414` |
| 0xC004 | ObjectAdded (Fuji) | gp `ptp.h:1415`; gp also accepts standard 0x4002 |

The X-T5 advertises no events (protocol.md). The firmware lists standard events 4002-4009 for card
reader mode only (fc). In tether mode the event channel is D212 (below).

## Property codes

Codes in our catalog first. "Catalog" gives our current name; **bold** marks a conflict or a code
we have unnamed.

### Control, state and power

| Code | Catalog | Sources say | Values | Source |
|---|---|---|---|---|
| D207 | PriorityMode | Priority mode, SDK `XSDK_PriorityMode` | 1 camera, 2 PC (USB). PC locks the body's dials, buttons and shutter; the lens focus ring still works | gp `config.c:2004`; sdk `XAPI.h:270`; mk `capture.go:30-39`; tv `FujiLiveView.swift:13-16` |
| D208 | **unnamed** | Release / Camera Action: the shutter gesture that the next InitiateCapture performs | Bit field, see [D208](#d208-release-values) | gp `ptp.h:2995-3017`, `config.c:3431`; sdk `XAPI.h:273-345`; mk `capture.go:20-77` |
| D209 | AFStatus | AF status during capture | 1 busy, 2 OK, 3 failed (no focus) | gp `library.c:5420-5430`; mk `capture.go:80-84` |
| D212 | CurrentState | **Event list** | uint16 count, then count x (uint16 code, uint32 value). Lists properties that changed | gp `ptp.c:5879` `ptp_fuji_getevents`; lf `fujiptp.h` `PtpFujiEvents` |
| D224 | **unnamed** | ReleaseStatus (SDK `CapReleaseStatus`) | Likely the SDK bitmap: 0x100 shooting, 0x200 S1, 0x400 bulb, 0x800 AF, 0x2000 WB lock, 0x4000 AE lock, 0x8000 AF lock | il `xsdk-live-shooting-properties.md`; sdk `XAPI.h:368` |
| D22D | **unnamed** | RecordingStatus (movie) | | il `xsdk-live-shooting-properties.md` |
| D22E | CameraScreen | CameraStatus | Our values stand | mk names |
| D22F | **unnamed** | HistogramData | 4 channels x 256 bins of counts; channel order unconfirmed | mk `settings.go:193-232`, names |
| D230 | ForceMode | Force mode; the X Webcam app sets it to 1 | 1, 2 | gp `library.c:393`; il |
| D23E | **unnamed** | CameraOperationLock | 1, 2 | mk names, descriptors |
| D381 | AutoPowerOff | AutoPowerOffSetting | 1 5 min, 2 2 min, 3 OFF, 4 1 min, 5 30 s, 6 15 s. Same as our lab map | sdk `XAPIOpt.h:1531-1536`; mk `settings.go:166` |
| D364 | CustomAutoPowerOff | CustomAutoPowerOff | X-T5 advertises range 0..255 step 1. Meaning unknown | gp; mk descriptors; sdk `X-T5.h:221` (supported) |
| D369 | **unnamed** | USBPowerSupplyCommunication | 1 auto, 2 on, 3 off (X-T5 offers 1, 3) | mk names; sdk `XAPIOpt.h:1526` |
| D16E | UsbConnectionMode | USBMode, read only | 5 tether, 6 RAW CONV., 8 X WEBCAM; fails in card reader mode | lf `fuji_usb.c` `fujiusb_setup` |
| D38C | LensZoomPosSelect | **A zoom-position selector**, not a PC-mode flag. c1 calls it LensZoomPos: it reads D170 (the lens zoom position), writes that value to D38C, then reads D38D, the aperture list for that position. gp writes 1 to it at capture start, commented "PC Mode"; 1 is the position a prime lens reports. SDK `CapLensZoomPos` | X-T5 range 1..40 | c1; gp `config.c:510-515`, `library.c:384`; il; mk |
| D21C | **unnamed** | ApiPreProcess | 0 | mk names; gp adds it "needed for X-T2?" `library.c:385` |
| D1BC | **unnamed** | LiveViewMode | 1, 2 (SDK LIVEVIEW_MODE1/2) | mk; sdk `XAPIOpt.h:566` |
| D1BE | **unnamed** | LiveViewStatus, read only | | mk; sdk `API_CODE_GetLiveViewStatus` |
| D173 | LiveViewImageQuality | | 1 fine, 3 normal | ox `FujiCamera.swift` |
| D174 | LiveViewImageSize | | 1 XGA 1024x768, 2 VGA, 3 QVGA | gp `config.c:2594`; ox |
| D36A | BatteryInfo | c1 derives body battery 1 and 2 and grip battery 1 and 2 from it (its synthetic D900-D903) | uint32 as 4 bytes: body, grip, grip2, body2 | il `xsdk-live-shooting-properties.md`; c1 |
| D36B | BatteryRatioInfo | Remaining battery as ratios (c1) | String of percentages, e.g. `97,0,0`; field count varies | il; c1 |
| D20C | MediaRecord | Tethered frame also written to card | 1 RAW+JPEG, 2 RAW, 3 JPEG, 4 off (USB only) | mk `capture.go:122-127` |
| D20E | FreeSDRAMImages | Free slots in the volatile capture buffer | | gp; mk `capture.go:212` |

### Recipe and image settings

| Code | Catalog | Sources say | Values | Source |
|---|---|---|---|---|
| D02E | DRangePriority | gp `WideDynamicRange`; gp comment confirms DR Priority on an X-T5 | 0 off, 1 weak, 2 strong, 0x8000 auto (X-T5 offers 0, 1, 0x8000) | gp `config.c:12112`; js `README.md:238`; mk |
| D191 | **unnamed** | Slot D-Range Priority | as D02E | il `current-shooting-state.md`; js `README.md:161` |
| D1A5 | **unnamed** | Live Image Size, composite 1-27 (size L/M/S x aspect) | X-H2 write-tested 7 -> 8. fc saw 7 in RAW CONV.; 7 is L 3:2 in il's table, not a slot count | il `current-shooting-state.md`; fc audit 2026-08-31 |
| D104 | MonochromaticColorWC | gp `BlackImageTone`; il: live Mono WC | -180..180 step 10 | il; fc firmware table |
| D031 | MonochromaticColorMG | live Mono MG | -180..180 step 10 | il; fc |
| D189 | SmoothSkinEffect | | 1 off, 2 weak, 3 strong | il; mk |
| D34D | LMOMode | live Lens Modulation Optimiser | 1, 2 | il |
| D017 | ColorTemperature | Written only while 0x5005 = 0x8007 | Kelvin | il `current-shooting-state.md` |
| D007, D320, D321 | DRangeMode, HighLightTone, ShadowTone | Refused while D02E is not 0; D007 reads 65535 then | DR 100/200/400; tones -20..40 step 5 | il; fc |
| D183 | **unnamed** | StartRawConversion: write 0 to start | | lf `fuji_usb.c` `fuji_process_raf` |
| D184 | IOPCode | | string (hex) | lf; il |
| D185 | **unnamed** | RawConvProfile: the full conversion profile | Blob: uint16 count 0x1D, uint8 length + UTF-16 IOP code, padding to 0x201, then 29 x uint32 fields | lf `fp/src/d185.c:41-70,292-363` |
| D186 / D187 | TetherRaw…Code | Tether RAW condition / compatibility codes | | lf; il |
| D01C | NoiseReduction | | 0x0000-0x8000 in steps of 0x1000 (non-linear map) | mk descriptors; gp `config.c:1551` |
| D022 | RawCompression | | 1 uncompressed, 2 lossless, 3 compressed (checked on X-T5) | mk `capture.go:183-188` |
| D028 | CommandDialMode | **DOFScale** per the X-T5 plugin | 1, 2 | mk `fuji.go:153-155` |

### Codes whose catalog name is likely wrong for the X-T5

Our names for these come from the older Wi-Fi protocol (hk) or from gp guesses.

| Code | Catalog | Sources say | Evidence | Source |
|---|---|---|---|---|
| D229 | CaptureRemaining | **MovieAFMode** | gp's own comment says "Movie AF Mode?"; hk's Wi-Fi meaning is "image space" | gp `ptp.h:3033`; mk; hk |
| D22A | MovieRemainingTime | **MovieFocusArea** | gp comment "Movie Focus Area?"; X-T5 int16 range -6..6 | gp `ptp.h:3034`; mk |
| D240 | ShutterSpeed2 | **MovieAperture** | gp comment "Movie Aperture"; X-T5 enum is an f-number (400) | gp `ptp.h:3036`; il; mk |
| D242 | BatteryLevel | **MovieSensitivity** | int32 with the ISO value list | gp `ptp.h:3038`; il; mk |
| D201 | ReleaseMode | Drive mode (SDK `XSDK_DRIVE_MODE_*`) | **2 CH, 3 CL**, 4 single, 5 multi-exposure, 8 movie, 0xA AE bracket, 0xB ISO bracket, 0xF focus bracket, 0x10 pixel shift. Our notes say CL 2. gp's table (1 single, 2 CL, 3 CH) predates the SDK | mk `capture.go:643-659`; gp `config.c:1816` |
| D1BF / D1C0 | ConstantSpeedFocus / ConstantSpeedZoom | **swapped**: plugin calls D1BF ZoomSpeed and D1C0 FocusSpeed | | mk names |
| D306 / D36F / D370 | — / FunctionLockCategory1 / 2 | **off by one**: plugin D306 = Category1, D36F = Category2, D370 = Category3 | | mk names |

## D208 release values

D208 holds a gesture. The host writes it, then sends InitiateCapture(0, 0). The high byte presses,
the low byte releases.

| Value | SDK name | Meaning |
|---|---|---|
| 0x0100 | SHOOT | Shoot |
| 0x0200 | S1ON | Half-press: AF and metering |
| 0x0300 | S2 | Full press from the half-pressed state |
| 0x0400 | BULB_ON | Bulb start |
| 0x0500 | BULBS2_ON | Bulb start with S2 |
| 0x0800 | REC_START | Movie start |
| 0x4000 | PIXELSHIFT | Pixel shift |
| 0x8000 | CUSWB | Custom WB measure |
| 0x9000 / 0x9100 / 0x9200 / 0x9300 | AEON / AFON / AFAEON / AF | AE-L, AF-ON, both, AF |
| 0xA000 | INSTANTAF | Instant AF (used in MF) |
| 0x0001 / 0x0002 / 0x0003 / 0x0004 | N_AFOFF / N_AEOFF / N_AFAEOFF / N_S1OFF | Releases |
| 0x0008 | N_BULBOFF | Bulb end |
| 0x0010 / 0x0020 | N_INSTANTAF / N_AF | Releases |
| 0x0040 | N_WBLOFF | WB lock off |
| 0x0080 | REC_STOP | Movie stop |
| 0x000F | CANCEL | Cancel everything |

Common pairs: 0x0304 shoot and release S1 (the gphoto2 default, and the firmware default value);
0x000C bulb end and release S1; 0x0005 release AF and S1. The X-T5 advertises: 0x200, 0x4, 0x304,
0x500, 0xC, 0x8000, 0xA000, 0x6, 0x9000, 0x2, 0x9100, 0x1, 0x9300, 0x5, 0xE, 0x9200, 0x40 (mk
descriptors). Sources: sdk `XAPI.h:273-345`; mk `capture.go:42-77`; gp `config.c:3431-3468`.

## Answers to the open questions

### Keeping the camera awake

- **No source has a keep-alive operation.** Not libgphoto2, libfuji, mikefsq, the Fuji SDK, nor the
  live-view apps. The SDK has `XSDK_PowerOFF` but nothing to wake or hold the camera.
- **Polling does not help.** We showed this ourselves (protocol.md). X RAW STUDIO idles at two
  commands every 3 s (fc audit 2026-09-04) and still documents nothing about power.
- **D381 = 3 (OFF) is the known lever.** The SDK constants match our lab map exactly. mikefsq exposes
  `SetAutoPowerOff` for the same reason. Restore the old value when the session ends.
- **The live-view apps tell users to set AUTO POWER OFF to OFF**, or the camera drops off the bus
  mid-stream (ox README). So live view alone did not keep an X-T30 awake.
- **PC priority may hold the camera awake.** mikefsq says a host that exits without restoring D207
  leaves the body "inert ... and held awake" (`capture.go:32-36`). One source, not measured. It
  also locks the body's controls, so it does not suit a settings app the owner is holding.
- **D364 CustomAutoPowerOff** is a writable 0..255 range on the X-T5. Its meaning is unknown. A
  read in each mode is a cheap next probe.
- **D369 USB power supply / communication** (1 auto, 3 off on the X-T5) may change power behaviour
  on USB power. Unknown.

### Priority mode (D207)

- 1 = camera priority, 2 = PC priority (gp, sdk). gphoto2 sets 2 at capture start and 1 at exit.
- PC priority locks the dials, buttons and shutter; the lens manual-focus ring still works (mk,
  tv). Most remote capture is ignored without it (mk).
- Closing the PTP session does **not** restore 1. The host must write 1 before it leaves (mk).
- The X-T5 answers a redundant write (value already set) with vendor error 0xA002. Read first (mk).
- 0xA002 also answers every write, redundant or not, while the tether buffer holds captured
  images (observed 2026-10-18 over wireless tether). DeleteObject on the buffer's objects clears
  it. Capture One treats 0xA002 as a busy code.
- The camera refuses the change to 2 while undownloaded frames sit in the volatile store (mk).
- D207 reads 0xA001 while a menu or playback is open; a half-press returns the body (mk
  `OPERATION.md`). This matches our 0xA001-in-menu finding.
- Some bodies stream live view without priority; others refuse (tv).

### Half-press, focus and capture (D208, D209)

Sequence used by gphoto2 and mikefsq (gp `library.c:5360-5450`; mk `capture.go:290-330`):

1. D207 = 2.
2. D208 = 0x0200, then InitiateCapture(0, 0). This is the half-press.
3. Poll D209 while it reads 1. 2 = focused. 3 = failed: send D208 = 0x0004 + InitiateCapture to
   release S1, then D207 = 1.
4. D208 = 0x0304, then InitiateCapture(0, 0). Retry on 0x2019 DeviceBusy; it is back-pressure.
5. Wait for the new object: poll D212, or GetObjectHandles and compare.

- 0x0304 without a prior 0x0200 is accepted and fires nothing (mk, confirmed on an X-T5).
- Every failure path must release S1, or the camera answers DeviceBusy across sessions (mk).
- AF without a shot: 0x9300 then 0x0005 (AF modes); 0xA000 then 0x0006 in MF (gp
  `config.c:3482-3585`, from a Fuji webcam trace).
- Bulb (0x0400 / 0x0500 then 0x000C) did not work on the tested X-T5 (mk).
- Frames land in a volatile store (storage 0x10000001). They occupy buffer slots until deleted,
  even when also written to the card (mk `OPERATION.md`).

### Live view

| Step | Call | Source |
|---|---|---|
| 1 | TerminateOpenCapture, to clear a stale stream | ox |
| 2 | D207 = 2 if the body needs it (retry 0x2019 up to 10 x 300 ms) | ox, tv |
| 3 | Optional D174 size, D173 quality | ox, gp |
| 4 | InitiateOpenCapture(0, 0) | gp `library.c:3990`, ox, mk |
| 5 | Each frame: GetObject on handle 0x80000001 (gp, ox), or the newest handle in storage 0x10000002 (mk, tv). Plain JPEG | gp `library.c:4000-4030` |
| 6 | DeleteObject every preview handle after reading. Previews that pile up stop the camera answering | mk, tv |
| 7 | Stop: TerminateOpenCapture, then D207 = 1 | gp `config.c:627-640`, ox |

Polling faster than the camera refreshes can also stall it (tv). D22F carries a live histogram
without live view (mk).

## What vendor software does at session start

| Software | Observed sequence | Source |
|---|---|---|
| A Fuji timelapse app (trace quoted by gphoto2) | D38C = 1, then D207 = 2 | gp `config.c:509-511` |
| gphoto2 capture start / end | D38C = 1, D207 = 2 ... TerminateOpenCapture, D207 = 1 | gp `config.c:496-525`, `627-640` |
| FUJIFILM X Webcam (traces quoted by gphoto2) | D230 ForceMode = 1; AF: D208 = 0x9300, wait D209 1 -> 2, D208 = 0x0005; D208 = 0x9000 ... 0x0002 while polling D212 | gp `library.c:393`, `config.c:3463-3467` |
| X RAW STUDIO 1.28 (macOS) | Talks through ImageCaptureCore and `ptpcamerad`, not raw USB. Idle: two commands every 3 s. Its `FTLPTP.dylib` exports `FTL_PTP_SetDeviceBusyRetryInfo` | fc `x-t5-device-audit-2026-09-04.md` |
| X RAW STUDIO settings path | Reads slots via D18C, D18D, D18E-D1A4. Never reads back after a write. Never reads the live codes | il `current-shooting-state.md` |
| RAW CONV. backup / restore | Backup: GetObjectInfo(0) + GetObject(0). Restore: SendObjectInfo (format 0x5000, size) + SendObject, under 100 KB | lf `fuji_usb.c` |
| RAW CONV. convert | 0x900C / 0x900D upload a RAF; read D185, edit, write D185; D183 = 0; poll GetObjectHandles; GetObject the JPEG; DeleteObject | lf `fuji_usb.c` `fuji_process_raf` |
| Fuji Wi-Fi apps | Camera state DF00 and client state DF01 handshake (5 = remote mode), version props DF21-DF31. gphoto2's Wi-Fi path writes DF01 = 5, echoes DF24, then InitiateOpenCapture to start the event queue. Wi-Fi only | lf `fujiptp.h`, `lib/fuji.c`; gp `library.c:9900-9908` |

No open trace of X Acquire exists in these sources. Capture One's sequence comes from a static
reading of its code (c1), not a trace:

| Software | Sequence in the code | Source |
|---|---|---|
| Capture One 16.5 open | D208 = 0x0004 (S1 off); read D01B, write D01B = 1 | c1 |
| Capture One PC priority | Needs D01B = 1; D207 = 2, retrying busy codes 0x2019, 0xA001, 0xA002 for up to 10 s. Release writes D207 = 1 | c1 |
| Capture One property poll | About every 1 s: read D170, write it to D38C, read D38D | c1 |
| Capture One capture | D01B = 1, then D208 + InitiateCapture; GetObject, then DeleteObject to free the D20E buffer. Events: only 0x4002 and 0x400D; no D212 polling | c1 |

Session facts from the X-T5 (fc audit 2026-08-31): after `ptpcamerad` is killed, the camera answers
0x201E SessionAlreadyOpen. A retried OpenSession must carry TransactionID 0; a continued counter
gets 0x2006. ImageCaptureCore `requestSendPTPCommand` works alongside `ptpcamerad` (fc audit
2026-09-04).

## Firmware facts (X-T5 4.31)

From fc's static analysis of `FWUP0030.DAT`:

- The firmware's full property list is the same 264 codes as our tether-mode set.
- Its RAW CONV. list is the same 61 codes as ours.
- Card reader mode: properties 5001, D303, D406, D407.
- A descriptor table gives type, access, form and factory default for each code: D208 default
  0x0304; D031, D104 range -180..180 step 10; D032 -50..50 step 10; D320, D321 -20..40 step 5.
- D18E-D1A4 and D183-D187 have no static descriptor. That fits GetDevicePropDesc failing in RAW
  CONV. mode.

## Other codes that sources name

All codes our catalog leaves unnamed, with the X-T5 plugin name and the X-T5 descriptor from mk.
Types: `undef` is datatype 0x0000. Values are enum lists or `min..max/step`.

| Code | X-T5 plugin name (mk) | Type | Access | Values |
|---|---|---|---|---|
| D02D | — | uint16 | rw | 32769 |
| D036 | — (mk: did not move with any dial) | uint16 | ro | |
| D038 | — (il: ErrorDetails, inferred) | uint32 | ro | |
| D039 | Shoot1 (il: release capability) | uint32 | rw | bit mask list |
| D040 | — | uint32 | rw | 0..4294967295/1 |
| D17B | FirmwareVersionUp1 | uint16 | rw | 1 |
| D180 | FoldernameSuffix2 | undef | rw | |
| D181 | FoldernameSuffix3 | undef | ro | |
| D182 | FoldernameSuffix4 | int16 | rw | -1..999/1 |
| D18A | FaceFrame | string | rw | |
| D18B | CustomWBArea2 | string | rw | |
| D1B0 | PROMData1 | string | rw | |
| D1B1 | PROMData2 | string | rw | |
| D1B2 | ImageFormat | uint16 | rw | 7 |
| D1B4 | CustomWBGain2 | undef | rw | |
| D1B5 | CustomWBGain3 | undef | ro | |
| D1B6 | SummerTime | uint16 | rw | 1,2 |
| D1B8 | ZoomOperation1 | uint32 | ro | |
| D1B9 | ZoomOperation2 | uint16 | ro | 1 |
| D1BA | FocusOperation1 | uint32 | ro | |
| D1BB | FocusOperation2 | uint16 | ro | 1 |
| D1BD | SubjectDetectionMode | uint16 | rw | 1 |
| D1C5 | TrackingAfFrameInfo | — | — | |
| D20A | SetAutoWhiteBalance2 | uint16 | ro | |
| D21D | FirmwareVersionUp2 | uint16 | ro | |
| D225 | FocusLimiterPos | string | rw | |
| D226 | FocusLimiterIndicator (lf, Wi-Fi: CompressSmall) | string | ro | |
| D227 | FocusLimiterRange (lf, Wi-Fi: EnableCorrectFileSize) | string | ro | |
| D228 | FocusLimiterMode (lf, Wi-Fi: AutoSaveDatabaseStatus) | uint16 | rw | 1,2,3,4 |
| D22B | — | int16 | rw | -3..3/1 |
| D22C | — | uint16 | rw | 1..6/1 |
| D231 | PixelShiftSettings | uint16 | rw | 0,10,20,50,150 |
| D234 | HighFrequencyFlickerlessMode | uint16 | rw | 1,2 |
| D235 | CameraPropList12 (lf: CompressionCutOff) | undef | rw | |
| D236 | CameraRotation | uint16 | ro | |
| D238 | ElectronicLevelSetting | uint16 | rw | 1,2,3 |
| D239 | ApertureUnit (F or T stops) | uint16 | rw | 1,2 |
| D23A | CustomDispInfo2 | uint32 | rw | 0..116/1 |
| D23B | PlayBackMedia | uint16 | rw | 1,2 |
| D23C | LiveViewImageRatio | uint16 | rw | 2,1 |
| D23F | Format3 | uint32 | ro | |
| D243 | — (il: MovieExposureBias) | int16 | rw | |
| D246 | MovieResolution | uint16 | rw | 3..9 |
| D247 | MovieFrameRate | uint16 | rw | 3,4,5,6 |
| D248 | MovieBitRate | uint16 | rw | 3,4,5,6 |
| D249 | MovieFileFormat | uint16 | rw | 1..7 |
| D24A | HighSpeedRecMode | uint16 | rw | 1,2,3 |
| D24B | HighSpeedRecResolution | uint16 | rw | |
| D24C | HighSpeedRecFrameRate | uint16 | rw | |
| D24D | HighSpeedRecPlayBackFrameRate | uint16 | rw | |
| D24E | MovieMediaRecord | uint16 | rw | 1,4,7,8 |
| D251 | MovieHDMIOutputRAW | uint16 | rw | 1,2,3 |
| D252 | MovieHDMIOutputRAWResolution | uint16 | rw | |
| D253 | MovieHDMIOutputRAWFrameRate | uint16 | rw | |
| D254 | FlogRecording | uint16 | rw | 1,2,3,8 |
| D255 | MediaEjectWarning | uint16 | ro | |
| D256 | MovieCropMagnification | uint16 | rw | 2 |
| D257 | MovieHDMIOutputInfoDisplay | uint16 | rw | 1,2 |
| D258 | MovieHDMIRecControl | uint16 | rw | 1,2 |
| D259 | MovieIsMode | uint16 | rw | 1,3 |
| D25A | MovieIsModeBoost | uint16 | rw | 1,2 |
| D25B | MovieZebraSetting | uint16 | rw | 1,2,3 |
| D25C | MovieZebraLevel | uint16 | rw | 50..100 step 5 |
| D25D | MovieOptimizedControl | uint16 | rw | 1,2 |
| D25E | MovieTallyLight | uint16 | rw | 1..7 |
| D262 | CustomSetting3 | uint16 | ro | 257..1799/1 |
| D263 | MovieCustomName2 | string | rw | |
| D264 | MovieAFCCustom | string | rw | |
| D265 | MovieHighFrequencyFlickerlessMode | uint16 | rw | 1,2 |
| D266 | TimeCode | string | rw | |
| D267 | TimeCodeDisplay | uint16 | rw | 1,2 |
| D268 | TimeCodeStartSetting2 | uint16 | ro | 1,2 |
| D269 | TimeCodeCountUp | uint16 | rw | 1,2 |
| D26A | TimeCodeDropFrame | uint16 | rw | 1,2 |
| D26B | TimeCodeHDMIOutput | uint16 | rw | 1,2 |
| D26C | MovieCustomWBArea2 (il: movie WB mode; enum equals 0x5005's) | uint16 | rw | WB list |
| D26D | MovieWhiteBalanceTune2 | int16 | rw | -9..9/1 |
| D26E | MovieWhiteBalanceTune3 | int16 | rw | -9..9/1 |
| D26F | — (il: movie WB colour temperature) | uint16 | rw | |
| D270 | MovieFilmSimulationMode | uint16 | rw | 1..19 |
| D271 | — (il: movie dynamic range) | uint16 | rw | 100,200,400 |
| D272 | — (il: movie metering mode) | uint16 | rw | 1,2,3,4 |
| D273 | MovieMonochromaticColor | int16 | rw | |
| D274 | MovieMonochromaticColor2 | int16 | rw | |
| D275 | MovieDataLevelSetting | uint16 | rw | 1,2 |
| D276 | MovieHighLightTone | int16 | rw | -20..40/5 |
| D277 | MovieShadowTone | int16 | rw | -20..40/5 |
| D278 | MovieColorMode | int16 | rw | -40..40/10 |
| D279 | MovieSharpness | int16 | rw | -40..40/10 |
| D27A | MovieNoiseReduction | uint16 | rw | as D01C |
| D27B | InterFrameNR | uint16 | rw | 1,2 |
| D27C | MoviePeripheralLightCorrection | uint16 | rw | 1,2 |
| D27D | MovieFaceDetectionMode | uint16 | rw | 2 |
| D27E | MovieEyeAFMode | uint16 | rw | 1 |
| D27F | MovieSubjectDetectionMode | uint16 | rw | 1 |
| D280 | MicLevelIndicator | string | ro | |
| D281 | MovieMFAssistMode | uint16 | rw | list |
| D282 | MovieFocusCheckMode | uint16 | rw | 1,2 |
| D283 | MovieFocusCheckLock | uint16 | rw | 1,2 |
| D284 | MovieRecordingTime | string | ro | |
| D285 | MovieRemainingTime | string | ro | |
| D286 | MovieFocusGuide | string | ro | |
| D287 | InternalMicLevel | uint16 | rw | 1,2,3 |
| D288 | InternalMicLevelManual | int16 | rw | -300.. step 15 |
| D289 | ExternalMicLevel | uint16 | rw | 1,2,3 |
| D28A | ExternalMicLevelManual | int16 | rw | -300.. step 15 |
| D28B | MicLevelLimiter | uint16 | rw | 1,2 |
| D28C | WindFilter | uint16 | rw | 1,2 |
| D28D | LowCutFilter | uint16 | rw | 1,2 |
| D28E | HeadPhonesVolume | int16 | rw | 0..100/10 |
| D28F | XLRAdapterMicSource | uint16 | rw | 1,2 |
| D290 | XLRAdapterMoniteringSource | uint16 | rw | 1,2 |
| D291 | XLRAdapterHDMIOutputSource | uint16 | rw | 1,2 |
| D292 | TimeCodeCurrentValue | string | ro | |
| D293 | MovieCropMagnificationValue | uint16 | ro | |
| D294 | MovieFullTimeManual | uint16 | rw | 1,2 |
| D297 | MovieRecVolume | uint16 | rw | 4,3,2,1 |
| D304 | PerformanceSettings | uint16 | rw | 1,2,3,4,5,7 |
| D305 | MicLineSetting | uint16 | rw | 1,2 |
| D306 | FunctionLockCategory1 | uint32 | rw | 0..32716/1 |
| D38B | CheckLensInfo4 | uint16 | ro | |

## Proposed catalog labels

Each entry is a proposal for the main session. The Capture One 16.5 import (2026-10-17) settled
some of them in the catalog (D028 DOFScale, D208, D209, D20C, D022, D173 values); the catalog is the
record. Confidence follows `docs/catalog.md`: `likely` when
two sources agree or one source tested on hardware, `guess` otherwise. None is `confirmed`: we have
not observed any of them.

| Code | Proposed name | Confidence | Values / note | Source |
|---|---|---|---|---|
| D208 | ReleaseControl | likely | D208 table above. Writes act only with InitiateCapture | sdk `XAPI.h:273`; mk; gp |
| D212 | EventList | likely | uint16 count + (uint16 code, uint32 value) entries. Rename from CurrentState | gp `ptp.c:5879`; lf |
| D207 | PriorityMode | likely (keep) | add values 1 Camera, 2 PC; note "PC locks body controls; session close does not restore" | gp; sdk; mk |
| D209 | AFStatus | likely (keep) | add values 1 busy, 2 OK, 3 failed | gp; mk |
| D224 | ReleaseStatus | guess | SDK release-status bitmap | il; sdk |
| D22D | MovieRecordingStatus | guess | | il |
| D22F | Histogram | likely | 4 x 256 bins | mk; protocol.md (changes ~3/s) |
| D23E | CameraOperationLock | guess | 1, 2 | mk |
| D369 | UsbPowerSupplyCommunication | likely | 1 AUTO, 2 ON, 3 OFF | mk; sdk |
| D16E | UsbMode | likely | 5 tether, 6 RAW CONV., 8 X WEBCAM | lf |
| D1BC | LiveViewMode | guess | 1, 2 | mk; sdk |
| D1BE | LiveViewStatus | guess | | mk; sdk |
| D173 | LiveViewImageQuality | likely (keep) | add 1 FINE, 3 NORMAL | ox |
| D174 | LiveViewImageSize | likely (keep) | add 1 XGA, 2 VGA, 3 QVGA | gp; ox |
| D191 | PresetDRangePriority | likely | 0 OFF, 1 WEAK, 2 STRONG, 32768 AUTO | il; js |
| D02E | DRangePriority | confirmed (keep) | add the same values | js; mk |
| D1A5 | ImageSize (live, composite) | guess | 1-27 size x aspect; one X-H2 write test | il |
| D183 | StartRawConversion | likely | RAW CONV. only | lf; fc |
| D185 | RawConversionProfile | likely | blob, see above | lf; fc |
| D20C | MediaRecord | likely (keep) | add 1 RAW+JPEG, 2 RAW, 3 JPEG, 4 OFF | mk; sdk |
| D022 | RawCompression | likely (keep) | add 1 UNCOMPRESSED, 2 LOSSLESS, 3 COMPRESSED | mk; gp |
| D201 | ReleaseMode | confirmed (keep) | revise note: SDK says 2 = CH, 3 = CL. Recheck CL on the drive dial | mk; sdk |
| D229 | MovieAfMode | guess | rename from CaptureRemaining | gp comment; mk |
| D22A | MovieFocusArea | guess | rename from MovieRemainingTime | gp comment; mk |
| D240 | MovieAperture | likely | rename from ShutterSpeed2 | gp comment; il; mk |
| D242 | MovieSensitivity | likely | int32 ISO; rename from BatteryLevel | gp comment; il; mk |
| D243 | MovieExposureBias | guess | | il |
| D26C | MovieWhiteBalance | guess | same enum as 0x5005 | il; mk |
| D26F | MovieColorTemperature | guess | | il |
| D271 | MovieDRangeMode | guess | 100, 200, 400 | il; mk |
| D272 | MovieMeteringMode | guess | 1-4 | il; mk |
| D270 | MovieFilmSimulation | likely | 1-19 as D001 | mk |
| D276 / D277 / D278 / D279 / D27A | MovieHighlightTone / MovieShadowTone / MovieColor / MovieSharpness / MovieNoiseReduction | likely | ranges as the stills codes | mk |
| D028 | DofScale | guess | rename from CommandDialMode | mk (plugin name) |
| D1BF | ConstantSpeedZoom | guess | swap with D1C0; recheck in the menu | mk |
| D1C0 | ConstantSpeedFocus | guess | swap with D1BF | mk |
| D306 | FunctionLockCategory1 | guess | shift D36F to Category2, D370 to Category3 | mk |
| D231 | PixelShiftSettings | guess | | mk |
| D234 | FlickerReduction (high frequency) | guess | 1, 2 | mk; sdk |
| D239 | ApertureUnit | guess | 1, 2 (F / T stop) | mk |
| D238 | ElectronicLevel | guess | 1, 2, 3 | mk |
| D23C | LiveViewImageRatio | guess | 1, 2 | mk |
| D1B6 | SummerTime | guess | 1, 2 | mk |
| D1B2 | ImageFormat | guess | | mk |
| D17B | FirmwareUpdate1 | guess | | mk |
| D21C | ApiPreProcess | guess | | mk; gp |
| D246-D25E, D263-D297 | names from the table above | guess | movie-only codes; plugin names | mk |
| D304 | PerformanceSettings | guess | boost modes | mk |

Do not adopt plugin names that contradict our lab findings: D307 stays StillMovie, D22E stays
CameraScreen.

## Next lab probes

Read-only first, per the rules in AGENTS.md:

1. Read D212 while turning a dial. If it lists the changed code, the app can drop most polling.
2. Read D364 and D369 in stills and movie mode. Check the menus for a matching setting.
3. Read D207 and D23E at rest, in a menu, and in playback.
4. With the owner's consent: D207 = 2, wait past the AUTO POWER OFF time, then D207 = 1. This tests
   the "held awake" claim. Confirm the body controls unlock afterwards.
