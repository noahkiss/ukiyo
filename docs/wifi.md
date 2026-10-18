# Wi-Fi and Bluetooth: what sources say, what the test run measures

Research and a test plan, 2026-10-17. Nothing here was tested on our camera yet;
`tools/wifi/run_wifi_tests.py` collects the first facts. Findings go to `docs/protocol.md`.

Owner goals: manage everything over Wi-Fi as FUJIFILM XApp does, and transfer images better
than Fujifilm's apps.

## Sources

| Key | Source | What it gives |
|---|---|---|
| gp | libgphoto2 `camlibs/ptp2/fujiptpip.c`, `ptp.c` (LGPL) | Framing, init packet, ports, 0x902B parser |
| lf | petabyt/libfuji `lib/fuji.c`, `fujiptp.h`, `discovery.c`, `bluetooth.c`, `docs/` (MIT) | Wi-Fi session setup, DF00/DF01 states, tether discovery, BLE UUIDs |
| hk | hkr/fuji-cam-wifi-tool `lib/` | Older Wi-Fi remote protocol, the same three ports |
| fb | gkoh/furble `lib/furble/Fujifilm.h` (MIT) | Bluetooth company ID 0x04D8, advert layout |
| man | X-T5 HTML manual, `connections/` pages | Menu routes for each mode |

## Wireless modes on the X-T5

| Mode | How it starts | Network | What a client gets |
|---|---|---|---|
| Smartphone (XApp) | XApp over Bluetooth, after PAIRING REGISTRATION | The camera's own hidden access point; XApp reads the SSID and password over Bluetooth | Remote shooting, live view, image import, settings backup (lf) |
| Wireless tether | CONNECTION MODE > WIRELESS TETHER SHOOTING FIXED | The camera joins an access point from NETWORK SETTING > ACCESS POINT SETTING | The USB tether feature set over TCP (lf `docs/dev.md`): settings, live view, capture |
| Frame.io | Frame.io Camera to Cloud | An access point, then the internet | Cloud upload only; no local protocol |
| PC AUTO SAVE | not on the X-T5 | — | Fujifilm removed it in 2022 (lf). The X-T5 menu has no such item |

The X-T5 manual lists no other wireless mode. AUTO POWER OFF also applies during tethered
shooting (man, CONNECTION MODE).

## Bodies with wireless tether

Capture One's camera list and its Fujifilm wireless tethering guide (support.captureone.com,
read 2026-10-17) name these bodies for Wi-Fi tether: X-T5, X-H2, X-H2S (from Capture One 16.2),
X-S20, X-M5, GFX100 II, GFX100S II, GFX100RF. Other tether bodies (X-T4, X-T3, X-H1, X-S10,
X-Pro3, GFX100S and older) are wired only. X-T50, X-E5 and the X100 series have no tethering.

- Capture One states no minimum Fujifilm firmware.
- The guide: camera and computer on the same network, a dedicated router preferred. The X-H2
  and X-H2S use CREATE/EDIT CONNECTION SETTING instead of ACCESS POINT SETTING.
- The guide: "WIRELESS TETHER SHOOTING FIXED - it is not possible to save to both card and your
  workstation."
- Capture One mobile (iPad) also tethers these bodies over Wi-Fi.

## Fuji PTP over TCP

- Three TCP ports on the camera: 55740 commands, 55741 events, 55742 live view JPEG. The last two
  listen only after InitiateOpenCapture (gp, hk).
- Only the first packet is PTP/IP. The client sends a fixed 82-byte init: u32 length 82, u32
  type 1, u32 version 0x8F53E4F2, a 16-byte GUID, the client name in UCS-2. The X-T4 closes the
  socket on any other length (gp).
- The camera answers type 2 (ack: u32 connection number, GUID, camera name) or type 5
  (INIT_FAIL). libfuji retries up to four times; the camera may show an accept prompt.
- After that, each packet is a USB-style container: u32 length, u16 type (1 command, 2 data,
  3 response, 4 event), u16 code, u32 transaction ID, then parameters or payload (gp).
- Wait at least 50 ms between init and OpenSession (lf).
- The X-T4 returns an empty GetDeviceInfo over Wi-Fi; gphoto2 synthesizes one (gp). Fuji
  operation 0x902B returns every property descriptor in one reply: u32 count, then per
  property a u32 size and a PropDesc (gp `ptp_fuji_getdeviceinfo`).
- Close: CloseSession, then the 8-byte goodbye `08 00 00 00 ff ff ff ff` (lf).
- Live view frames on 55742: u32 length, a 14-byte header, then a JPEG (lf, gp).

### Wireless tether discovery

From lf `discovery.c`:

1. The client broadcasts a UDP datagram to port 51562:
   `DISCOVERY * HTTP/1.1`, `HOST: <client ip>`, `MX: 5`, `SERVICE: PCSS/1.0`.
2. The camera opens TCP to the client's port 51560 and sends `DSC: <camera ip>`,
   `CAMERANAME: <model>`, `DSCPORT: <port>`.
3. The client answers `HTTP/1.1 200 OK` and connects to the camera on that port.

Observed on our X-T5 (fw 4.31, 2026-10-17; `tools/wifi/tether_probe.py`, and a packet capture of
Capture One 16.5):

- Discovery found the camera in 5-7 s. Its message starts `NOTIFY * HTTP/1.1` and offers
  DSCPORT 15740, the standard PTP/IP port. One TCP connection carries everything; Capture One
  opened no event or live view socket.
- The init is the Fuji 82-byte request (version 0x8F53E4F2). The camera checks the GUID: it must
  be `ada5485d87b27f0bd3d5ded0` followed by the camera's IPv4 address as a little-endian u32.
  libfuji uses the same 12 bytes with a zero tail. Any other GUID, and the standard PTP/IP init,
  get INIT_FAIL with reason 0x201D (invalid parameter) at once.
- The first correct init gets INIT_FAIL 0x2019 (busy). Resend it on the same socket after about
  1 s; the camera then acks. Closing the socket instead makes the camera stop listening until
  CONNECTION MODE is set again.
- Then plain PTP: OpenSession, GetDeviceInfo (FUJIFILM, X-T5, 4.31, 23 operations, 264
  properties, the USB tether set), D16E reads 5 (tether). No on-camera prompt appeared.
- Storage is the tether buffer, not the card: two storages, "Still" and "Live", with all-ones
  capacity and free space, and zero objects. **Wireless tether gives no access to the card's
  photos**, so image transfer needs the smartphone mode or USB.
- Capture One writes D38C = 1 about every 1.5 s. A static look at its bundle shows this is not a
  heartbeat: D38C selects a zoom position (C1 copies D170 into it), and C1 then reads D38D, the
  aperture list for that position. Its property refresh runs about once a second.

What Capture One 16.5's bundle shows (static inspection, notes in `local/mapping/` on the dev Mac, gitignored):

- Its Fuji code is Phase One's own; Fuji's SDK is not embedded. Discovery and init match the
  above. On INIT_FAIL 0x2019 it waits 0.5 s and resends on the same socket, up to 3 times;
  0x201E and other reasons are fatal. The client name is the Mac's host name.
- It uses one socket: no event or live view socket (55741/55742). Live view is
  InitiateOpenCapture, then operation 0x9018 GetLiveViewImage per frame.
- Images: it waits for ObjectAdded (0x4002), downloads with GetObject, then DeleteObject from the
  tether buffer. It has no card browse and no card access over Wi-Fi.
- Busy answers it retries: 0x2019, 0xA001, 0xA002, in 200 ms steps.
- PC priority: D207 = 2 to take, 1 to release. It needs D01B (live zoom) = 1 first.

After the init, wireless tether is plain USB-tether PTP: OpenSession, GetDeviceInfo, and
D16E (USB mode) (lf `fuji_usb.c`).

### Smartphone mode session

XApp-style sessions need writes before the camera gives access (lf `fuji.c`):

- Read DF00 (camera state: 0 wait, 1 transfer, 2 full access, 6 remote) and the version
  properties DF21, DF22, DF24, DF25, DF28.
- Write DF01 (client state: 5 remote, 11 remote gallery, 20 XApp gallery, 22 XApp live view).
  The camera may show a Yes/No prompt.
- Write DF24 to 0x2000C, DF25 to 5, then GetObjectInfo before GetThumb, or GetThumb blocks.
- D227 = 1 makes ObjectInfo report the true size; with 0 it reports about 100 KB. D226 = 1
  makes the camera compress images to 400-800 KB.
- The X-T5 lists photos oldest first (lf: DF25 >= 5).
- Downloads use GetPartialObject in 1 MiB chunks (lf `FUJI_MAX_PARTIAL_OBJECT`).

## Bluetooth LE

From lf `bluetooth.c` (after fb):

- Adverts carry manufacturer data with company ID 0x04D8. Byte 0 is a type; type 0x02 adds a
  4-byte pairing token.
- The X-T5 uses secure pairing: a bond, then a status write and the client name.
- Wi-Fi hand-off: read the SSID (`bf6dc9cf-…`), write `04 00` to `600655e6-…`, read the password
  (`e809256a-…`), then join the hidden network. So the Mac cannot reach the smartphone-mode
  network without Bluetooth writes and a bond.
- Other services: shutter (`6514eb81-…`), geotag (`3b46ec2b-…`), time sync, lens name and
  firmware (`15ca59fe-…`), a file service for the settings backup and equipment status
  (`af854c2e-…`), configuration indications (`4c0020fe-…`).

## What the test run measures

`tools/wifi/README.md` has the steps. Per mode:

| Question | How the run answers it |
|---|---|
| How does the camera announce itself? | PCSS discovery, then ARP neighbours on port 55740, then the IP typed from NETWORK/USB SETTING > INFORMATION |
| Does the Fuji init work as described? Does the camera prompt? | Init attempts, ack contents, INIT_FAIL count |
| What does Wi-Fi expose? | GetDeviceInfo (ops, events, props), 0x902B, every advertised property's value and descriptor |
| How much of the USB tether set answers over Wi-Fi? | The catalog's stills codes read one by one; answer codes counted |
| Can we write settings over Wi-Fi? | One D381 toggle to a neighbouring value and back, with read-back |
| Does Wi-Fi tether see the card? | GetStorageIDs, GetStorageInfo, GetObjectHandles |
| How fast is transfer? | Up to 5 recent files: time to first byte, MB/s, chunk timing; GetObject vs GetPartialObject; a mid-file resume; a 256 KiB / 1 MiB / 4 MiB chunk sweep |
| Is ObjectInfo's size honest? | One chunk is asked for past the reported end |
| Live view? | InitiateOpenCapture, one frame from 55742 (or GetObject 0x80000001), events on 55741, TerminateOpenCapture |
| Does a Wi-Fi session keep the camera awake? | A read every 10 s with AUTO POWER OFF at 1 MIN; the time of the drop |
| What does Bluetooth expose without pairing? | Adverts (normal and on the pairing screen), GATT services, readable values |

The smartphone mode is observe-only in this run: a scan for the camera's network while XApp is
connected, and a read-only probe (no DF01 write) if the owner can supply the network password.

## Open questions

1. Answered 2026-10-17: wireless tether lists only the tether buffer, not the card.
2. Answered 2026-10-17: GetDeviceInfo answers in full over wireless tether.
3. Which of the 264 USB tether properties answer over Wi-Fi, and are they writable?
4. Does a Wi-Fi session hold the camera awake, unlike USB?
5. What transfer rate does each band give (WIRELESS COM. FREQUENCY SETTING: 2.4 GHz vs 5 GHz)?
6. Smartphone mode: can a Mac, once bonded, run the Bluetooth hand-off and the DF01 session?
   That needs Bluetooth writes and a pairing registration, so it needs its own owner decision.
7. Does wireless tether need PC priority (D207 = 2) for reads, or only for capture?
8. Is the BLE settings backup the same blob as the USB RAW CONV. backup (handle 0)?

## How a Wi-Fi transport fits the core

The core's seam (`src-tauri/core/src/camera/transport.rs`) already fits:

- `Transport::op(code, params, data) -> Reply { rc, data }` is one PTP transaction. A
  `WifiTransport` sends the Fuji container on the command socket and reads data and response
  packets. The framing is the USB container with a length prefix, so the dataset parsers stay
  as they are.
- `Connector::present()` and `open(reset)`: a `WifiConnector` answers `present` from discovery
  (a PCSS invitation, or a remembered IP that accepts TCP on 55740). `open` does the init and
  returns the transport. `reset` has no Wi-Fi meaning; the connector ignores it.
- `TransportError` maps as: socket closed or host unreachable -> `Gone`, read timeout ->
  `Timeout`, a bad container -> `Io`.
- Live view and events need two more sockets. They belong to the transport, opened after
  InitiateOpenCapture and closed with TerminateOpenCapture. The owner sees frames as data, not
  as a new connection.

The one-owner rule stays. `ukiyo serve` owns every camera session, USB and Wi-Fi alike, so the
window, CLI and MCP server never open a second one. The owner picks one connector at a time
per camera; a camera reachable both ways uses one of them. The reconnect rules carry over:
no tight loops, presence checks every few seconds, a hold after the camera leaves. Wi-Fi adds
one rule: the init can wait on an on-camera prompt, so the owner must not count that wait as
a failure.

Smartphone mode adds a Bluetooth step before the Wi-Fi connector: bond, hand-off, join the
camera's network. Joining a network changes the Mac's own Wi-Fi, so `serve` must ask the owner
first and rejoin afterwards. That needs a design of its own.
