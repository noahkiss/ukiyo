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

## Camera state and power

| Code | Meaning | Values |
|---|---|---|
| D22E | camera screen state (ro) | 1 shooting, 2 playback, 3 menu open |
| D381 | AUTO POWER OFF (stills) | 1 = 5 min, 2 = 2 min, 3 = OFF, 4 = 1 min, 5 = 30 s, 6 = 15 s. **Write tested**: 1 -> 3 accepted and shown in the menu |
| D307 | STILL/MOVIE switch | 2 stills, 1 movie |

- While a menu is open, every property returns 0xA001. Poll again after D22E returns to 1.
- Auto power off drops the camera from USB entirely; nothing can wake it over USB. Just before,
  every property returns 0x2019 (device busy) for about a second.
- Movie mode has its own AUTO POWER OFF value (D381 read 2 there while stills was 3).
- **Do not reconnect in a tight loop.** Rapid reconnects plus USB resets while the camera was
  powering off soft-locked it once; it needed a battery pull. Wait ~30 s after the device
  disappears, and reset the USB device only when a session open actually fails.

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
