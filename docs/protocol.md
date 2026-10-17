# Fujifilm X-T5 over USB (PTP)

What we know about talking to an X-T5 (firmware 4.31) over USB. Everything here was observed on
one camera from a Mac. Read-only unless a row says a write was tested.

## USB connection modes

Set on the camera: SET UP > CONNECTION SETTING > CONNECTION MODE. The menu item is locked while
the cable is plugged in, so software cannot switch modes.

| Mode | What PTP exposes | Camera usable while connected |
|---|---|---|
| USB CARD READER | files only; 4 readable properties | yes |
| USB RAW CONV./BACKUP RESTORE | 61 properties: raw-conversion settings and the C1-C7 recipe slots | no (soft-locked) |
| USB TETHER SHOOTING FIXED | 264 properties: shooting, AF, controls, setup; 143 writable | yes |

Recipe slot *contents* (D18E-D1A5) read only in RAW CONV. mode. Tether mode returns
0x200A for them; it still reads slot names (D18D).

## Session basics

- Vendor 0x04CB, product 0x02FC ("USB PTP Camera"). macOS's PTPCamera agent grabs the device;
  `killall PTPCamera` before claiming it.
- Plain PTP: OpenSession 0x1002, GetDevicePropValue 0x1015, SetDevicePropValue 0x1016.
- GetDevicePropDesc 0x1014 works in tether mode (types, rw flag, enums, ranges) and fails
  (0x2002) in RAW CONV. mode, where only values can be read.
- The device advertises no PTP events, so state changes must be polled.
- Stills properties refuse reads in movie mode and vice versa. Stills: 181 readable; movie: 170.
  Together every one of the 264 properties is readable in one mode or the other.
- One process can hold the USB interface. Polling and writing must share one connection.
- Some descriptors use datatype 0x0000 (D180, D181, D1B4, D1B5, D212, D22F, D235): nothing
  after the read-only flag can be parsed, since the type gives no size. GetDevicePropValue still
  returns bytes for some of them (D212 and D22F change on their own). The app shows those bytes
  as hex.
- D1B1 (string) sends a truncated descriptor: default and current, but no form byte. Treat a
  missing form byte as "no form".
- ExposureIndex 0x500F is int32, so ISO AUTO (0xFFFFFFFD) decodes as -3.

## Camera state and power

| Code | Meaning | Values |
|---|---|---|
| D22E | camera screen state (ro) | 1 shooting, 2 playback, 3 menu open |
| D381 | AUTO POWER OFF (stills) | 1 = 5 min, 2 = 2 min, 3 = OFF, 4 = 1 min, 5 = 30 s, 6 = 15 s. **Write tested**: 1 -> 3 accepted and shown in the menu |
| D307 | STILL/MOVIE switch | 2 stills, 1 movie |

- While a menu is open, every property returns 0xA001. Poll again after D22E returns to 1.
  Observed 2026-10-17 (fw 4.31): opening the menu first gave one 0x2019 (busy), then D22E = 3.
- Writes can answer 0xA001 while reads work and D22E still reads 1 (owner in a menu, 2026-10-17).
  Treat 0xA001 on a write as menu-open and retry later.
- Polling does not reset the auto power off timer. With D381 = 2 min, the camera powered off
  2:00 after the session opened while properties were read continuously (2026-10-17). Keeping it
  awake needs the D381 write; reopening sessions to reset the timer is the reconnect pattern
  ruled out below.
- What does reset it (2026-10-17, D381 at 15 s, one action every 5 s): changing D381 to another
  value and back, and a half-press (D208 = 0x0200 + InitiateCapture, then D208 = 0x0004 +
  InitiateCapture; the lens refocuses). GetDeviceInfo, GetStorageIDs and rewriting D381 with its
  own value do not. After a half-press release, reads answer at once but writes answer 0x2019
  for 0.13-0.22 s (five trials); queue writes and retry with a ~100-250 ms backoff.
- PC priority (D207 = 2) does not keep it awake either (slept at 15 s). `docs/fuji-vendor-ptp.md`
  found no keep-alive operation in any source, Fuji's SDK included.
- D212 (the change-event list) stays empty over USB: count 0 at rest and while an ISO dial
  turn moved the shutter speed (0x500D) five times. Polling is the change source.
- D22F changes about three times a second at rest: a blob of 32-bit counts, likely the live
  histogram.
- Auto power off drops the camera from USB entirely; nothing can wake it over USB. Just before,
  every property returns 0x2019 (device busy) for about a second.
- Movie mode has its own AUTO POWER OFF value (D381 read 2 there while stills was 3).
- **Do not reconnect in a tight loop.** Rapid reconnects plus USB resets while the camera was
  powering off soft-locked it once; it needed a battery pull. Wait ~30 s after the device
  disappears, and reset the USB device only when a session open actually fails.

## How the app polls

The app's connection owner (`src-tauri/core/src/camera/owner.rs`) follows these rules; they are the
facts above turned into behaviour. Not yet run against a camera.

- One thread owns the session. Writes go ahead of every other request, and each write is
  followed at once by GetDevicePropValue of the same code.
- A write answered 0x2019 is sent again on the same session after 100, 200, 400, 800 and
  1600 ms (3.1 s), then reported busy; the half-press busy window above is 1-2 answers.
  Reads are not retried.
- Each poll cycle reads the hot set (D22E, D307, anything changed in the last 10 s, pinned
  codes) plus the next 8 codes of a rotating sweep over every readable property. No sleeps.
- A code answering 0x2002 when described is skipped by the sweep. A D307 change describes
  every property again, since stills and movie read different sets.
- D22E = 3 or 2 or any 0xA001 pauses the sweep; D22E is then read every 500 ms until it shows
  another screen. Whether D22E itself answers 0xA001 in a menu is not yet known; either works.
- Three 0x2019 answers in a row mean busy: D22E every 500 ms. If the device then leaves the
  bus within 5 s, the app reports auto power off.
- After the device leaves or a session fails: presence check every 3 s, no open for 30 s.
- Checked on the X-T5 (2026-10-17): auto power off is detected (0x2019, then the device
  leaves); a cable pull mid-session gives a transfer error (IOKit 0xe00002ed, not responding) and
  the same 30 s hold; the camera reconnects once after the hold. Quit (idle exit) and SIGTERM
  both close the session cleanly and release `owner.lock`.
  OpenSession failing on a fresh claim allows one USB reset per connect attempt.
- Keep-awake (setting `keep_awake` in `settings.json`, on by default): while Ukiyo is in use,
  the owner reads D381, sets it to a neighbouring timeout (5 min, or 2 min when the owner's
  value is 5 min), and straight back to the owner's value, then reads it back. It repeats
  before the owner's timeout runs out: every 10 s at 15 s, 20 s at 30 s, 45 s at 1 min, 90 s
  at 2 min, 4 min at 5 min. "In use" is any client request (app, CLI, MCP) or window input
  in the last 5 minutes (`in_use_minutes`). Tether and stills only; nothing at OFF, in movie
  mode or in RAW CONV. It never writes OFF: a loss between the two writes leaves the
  neighbouring timeout, and the owner writes its value back after the reconnect. A toggle
  answered 0xA001 is skipped (a person in a menu keeps the camera awake), with no half-press.
  Not yet run against the camera in the app; the toggle itself kept an X-T5 awake (above).
- Screens: D22E 1 with writes answering 0xA001 is a shooting-side menu tab or the Q menu; a
  half-press (D208 = 0x0200 + InitiateCapture(0, 0), 0.5 s, D208 = 0x0004 + InitiateCapture)
  closes it. D22E 3 (SET UP, NETWORK/USB SETTING, any depth) and D22E 2 (playback) answer
  0xA001 to everything else, the half-press included. The owner pauses for both 2 and 3, and
  half-presses only at 1, once per write (`close_menu_to_write`, on by default).

## Recipe slots (RAW CONV. mode)

Write D18C (slot 1-7) to select a slot, then read D18D (name, PTP string) and D18E-D1A5
(settings). Restore D18C afterwards. Field names and encodings follow FilmKit
(github.com/eggricesoy/filmkit, MIT): film sim enum, tones ×10, Color Chrome 1-indexed
(1 off, 2 weak, 3 strong), WB 0x8007 = Kelvin.

## Hardware controls (tether mode)

| Control | Property | Values |
|---|---|---|
| Focus mode selector | 0x500A | S 0x8001, C 0x8002, M 0x0001 |
| Drive | D201 ReleaseMode | S 4, CL 2, BKT 10; CH probably 3 (unconfirmed); movie 8 |
| ISO | 0x500F | 0xFFFFFFFD auto; D035 flips with the dial's A position |
| Shutter speed | 0x500D exposure time, 0x500E program mode | D033 flips with the dial's A position |
| Exposure compensation | 0x5010 | D034 flips with the dial's C position |
| Aperture ring | 0x5007 | reports the chosen aperture, also in A |
| DISP button | none seen | |

The camera enforces dependencies itself: ISO 125 forced D-Range 400 -> 100; CL/CH zero Clarity.

## Names

- libgphoto2 `camlibs/ptp2/ptp.h` names 103 of the codes (`PTP_DPC_FUJI_*`). Some are wrong
  for the X-T5: D029 "Shadowing" is Color Chrome Effect; D02E is D Range Priority.
- The working map of unnamed codes, with confidence levels, lives in `local/` (not tracked:
  it carries this camera's serials and owner strings).
- The shareable map is `catalog/x-t5.json` (`docs/catalog.md`): names, menu paths and value
  labels with a confidence each, and no values from this camera.
