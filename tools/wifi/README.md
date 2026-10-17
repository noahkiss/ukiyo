# tools/wifi

A guided test run that collects facts about the X-T5 over Wi-Fi and Bluetooth. `docs/wifi.md`
explains what it measures and why.

## Run it

Start the run while the Mac is still online, from this directory:

```sh
uv run -q run_wifi_tests.py
```

`uv` installs `bleak` (Bluetooth) at start, so the run needs no internet later. Each step opens a
macOS dialog. Done means you did the step; Skip leaves it out; Stop ends the run. Stop still
rejoins the original Wi-Fi and writes the summary.

Allow these macOS prompts for your terminal app when they appear:

- Bluetooth (the Bluetooth step).
- Local Network (macOS 15 and later block LAN traffic without it). The run checks at pre-flight.
- Incoming connections for Python (the camera connects back to the Mac during discovery).

Do not pair the Mac with the camera if macOS asks. Click Cancel.

## What it asks you to do

About 30-45 minutes. The camera needs a charged battery and a card with a few recent photos.

| Step | You do | The Mac does | Time |
|---|---|---|---|
| Pre-flight | Type the Wi-Fi name if macOS hides it | Records the network, other routes, internet | 1 min |
| Bluetooth | Bluetooth ON; close XApp; optional PAIRING REGISTRATION screen, then BACK | Scans, lists GATT services, reads safe values | 3-5 min |
| Wireless tether | Note CONNECTION MODE; set ACCESS POINT SETTING; AUTO POWER OFF 1 MIN; select WIRELESS TETHER SHOOTING FIXED | Discovers the camera, runs the PTP suite below | 15-25 min |
| Smartphone | Start XApp Remote on the phone; optional: type the camera network's password | Scans for the camera's network; read-only probe if it can join | 3-5 min |
| End | Set CONNECTION MODE and AUTO POWER OFF back | Rejoins the original Wi-Fi, checks the internet, shows the summary | 2 min |

The PTP suite, per mode: Fuji init, OpenSession, GetDeviceInfo and Fuji 0x902B, every
advertised property (value and descriptor), a sample of the USB tether codes from
`catalog/x-t5.json`, one D381 toggle, storage and object lists, a download of up to 5 recent
files with thumbnails, a live view frame, and a keep-alive watch.

## Safety

- Read-only, except one AUTO POWER OFF (D381) toggle to a neighbouring value and back. The
  restore runs in a `finally`. If it fails, the summary says which value to set by hand.
- `fujiptpip.Session` refuses capture, delete, format and object-write operations.
- No Bluetooth writes, subscriptions or pairing. Wi-Fi SSID, password and pairing
  characteristics are listed but never read (`ble.DENY`).
- Connection attempts are spaced at least 2 s apart, and give up after a few.
- The run rejoins the original Wi-Fi in a `finally`. If it cannot name the network, it turns
  Wi-Fi off and on, and macOS rejoins its preferred network.
- It never changes the camera's network settings. You make the camera menu changes yourself.

## Output

Everything goes to `local/wifi-test/<timestamp>/` (gitignored):

| File | Contents |
|---|---|
| `summary.md` | The findings in short |
| `events.jsonl` | Every dialog, answer, operation and result, timestamped |
| `props-<step>.json` | Every property read: answer code, latency, value, descriptor |
| `findings.json` | The data behind `summary.md` |
| `files/` | Downloaded photos, thumbnails, live view frames |

Logs hash or mask serial numbers, MAC addresses, network names, owner strings (copyright,
comments, lens serial) and any other string value not on a short allow-list. Downloaded photos
are not redacted: their EXIF carries the camera serial. `--keep-raw` also saves raw DeviceInfo
blobs, which are not redacted.

## Dry run

```sh
uv run -q run_wifi_tests.py --dry-run
```

It runs the whole flow against a fake X-T5 on localhost (`fakecam.py`), with scripted answers
printed in the terminal. It changes nothing: network commands are printed, not run, and it
uses no Bluetooth. Options for a dry run:

- `--real-dialogs` shows the real dialogs.
- `--answer KEY=BUTTON` answers one dialog (keys are in `events.jsonl`), e.g. `--answer tether_apo=Stop`.
- `--fake-quirk Q` makes the fake camera misbehave: `empty-deviceinfo`, `refuse`, `short-size`, `no-objects`.

## Files

| File | Role |
|---|---|
| `run_wifi_tests.py` | The entry point: flow, dialogs, logging, redaction, summary |
| `fujiptpip.py` | Fuji PTP over TCP: framing, datasets, session, wireless tether discovery |
| `macnet.py` | Wi-Fi state, join, rejoin, internet check, notifications |
| `ble.py` | Read-only Bluetooth survey (bleak) |
| `fakecam.py` | The fake camera for `--dry-run` |
