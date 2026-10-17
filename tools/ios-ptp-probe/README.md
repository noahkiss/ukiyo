# PTP Probe (iOS / iPadOS)

A one-screen test app. It answers one question: can an iPhone or iPad, over a USB-C cable,
read and write Fujifilm X-T5 PTP properties? It tests two USB modes: USB TETHER SHOOTING FIXED
and USB RAW CONV./BACKUP RESTORE.

It uses ImageCaptureCore: `ICDeviceBrowser` finds the camera, the app opens one session, and
`ICCameraDevice.requestSendPTPCommand(_:outData:completion:)` sends PTP command containers
that the app builds itself. Protocol facts come from `docs/protocol.md`.

## Install from TestFlight

The probe ships through TestFlight as the first builds of Ukiyo (bundle ID
`com.nkmkdigital.ukiyo`). On the device it is named PTP Probe.

1. Install the TestFlight app from the App Store on the iPhone or iPad.
2. Accept the TestFlight invitation for Ukiyo, from the email or from TestFlight itself. Members
   of the internal group get every build.
3. In TestFlight, tap **Install** on the newest build.

No cable install, Developer Mode or profile trust is needed.

## Upload a build

Signing settings live in two gitignored files. Never commit them.

```sh
cd tools/ios-ptp-probe
cp Local.xcconfig.example Local.xcconfig   # team ID, Apple Distribution, the App Store profile
cp Local.example.env Local.env             # App Store Connect API key commands
./testflight.sh
```

`testflight.sh` generates the project, archives a Release build and uploads it. The build number
is the UTC time (`YYYYMMDDHHMM`); raise `MARKETING_VERSION` in `project.yml` by hand. App Store
Connect takes a few minutes to process a build, then the internal group gets it.

The Mac that uploads needs the Apple Distribution identity in its keychain and the profile
`match AppStore com.nkmkdigital.ukiyo` installed.

Simulator and unsigned builds (compile checks only; the simulator finds no camera). Generate
the project first (`xcodegen generate`; the `.xcodeproj` and `Info.plist` are gitignored):

```sh
xcodebuild build -project PTPProbe.xcodeproj -scheme PTPProbe -sdk iphonesimulator -derivedDataPath build
xcodebuild build -project PTPProbe.xcodeproj -scheme PTPProbe -destination 'generic/platform=iOS' \
  -derivedDataPath build CODE_SIGNING_ALLOWED=NO
```

The icon (`Icon/AppIcon.icon`) is a placeholder until Ukiyo has its own.

## Set up the camera

Set the mode before you plug in the cable. The camera locks the menu item while the cable is in.

| Test | Camera menu |
|---|---|
| Tether | NETWORK/USB SETTING > CONNECTION MODE > USB TETHER SHOOTING FIXED |
| Recipe read | NETWORK/USB SETTING > CONNECTION MODE > USB RAW CONV./BACKUP RESTORE |

Then:

1. Turn the camera on and close every menu (a menu makes every property answer 0xA001).
2. Connect the camera's USB-C port to the iPhone or iPad with a USB-C data cable.
3. Open PTP Probe. When the camera name shows, tap **Connect** and allow the prompts.
4. Tap **Run all**, or the single steps. Then tap **Copy log** or **Share log**.

To test the other mode: unplug, change CONNECTION MODE on the camera, plug in, tap Connect.
The app never reconnects on its own.

## What the buttons do

| Button | PTP | Writes |
|---|---|---|
| Device info | GetDeviceInfo 0x1001. Shows model, firmware, vendor extension and the operation list. The serial number is zeroed in the dump and never shown. | none |
| Read state | GetDevicePropValue 0x1015 for D22E (screen), D307 (still/movie), D381 (AUTO POWER OFF), then D18E as a mode probe | none |
| Describe D381 | GetDevicePropDesc 0x1014 for D381 | none |
| Write test | Reads D381, sets it to 3 (OFF), reads back, restores the original, reads back. If D381 is already 3, it tests with 1 (5 min). | D381, restored |
| Recipe read | Reads D18C, sets D18C = 1, reads D18D (slot name) and D18E (first field), restores D18C | D18C, restored |
| Run all | All of the above. It skips the write test unless D381 answered, and the recipe read unless D18E answered. | as above |

The **USB mode** line updates as answers come in. It resets when a session opens or the camera
goes, and follows the latest OK or 0x200A answer for D381 and D18E:

- **USB TETHER SHOOTING FIXED**: D381 answers.
- **USB RAW CONV./BACKUP RESTORE**: D18E answers. Descriptors are refused (0x2002 on the Mac,
  0x200A on an iPad).
- **unknown**: neither answered. The camera may be in USB CARD READER mode, or iOS blocked the
  commands.

## Reading the results

Each step logs the response code, the raw response container, and the data bytes. Write and
recipe steps also log `PASS` or `FAIL` per step. A step logs `SKIP` when its property is not
available in this USB mode (0x200A): the write test in RAW CONV. mode, the recipe read or its
D18E field in tether mode.

| Result | Meaning |
|---|---|
| `response 0x2001 OK` and the expected values | iOS passes raw PTP for this mode. A write test with every step PASS means iOS can read and write. |
| error `-21249` | ICReturnPTPNotAuthorizedToSendCommand. iOS refused the raw command. Check that camera control was allowed (Settings > PTP Probe), then unplug, replug and tap Connect. If it persists with control allowed, iOS does not allow raw PTP to this camera. |
| `0x2019 DeviceBusy` | The camera is about to power off (AUTO POWER OFF). Wait. Do not retry in a loop. |
| `0xA001` | A menu is open on the camera. Close it and run the step again. |
| `0x2002 GeneralError` or `0x200A` on Describe D381 | Expected in RAW CONV. mode. Values still read. |
| `0x200A DevicePropNotSupported` | The property does not exist in this mode. Tether mode gives this for D18E. |
| `0x201D` or `0x201C` on a write | The camera rejected the value or its format. Check how the payload went out (below). |

`WARNING: D381 is not back to N` means the restore failed. Set AUTO POWER OFF on the camera by
hand.

## Known unknowns

- The app passes the SetDevicePropValue payload to `outData` as bare value bytes, with no data
  container header. It assumes ImageCaptureCore adds the header. A 0x201C or 0x201D answer may
  mean that assumption is wrong.
- If the data phase comes back with a container header, the app strips it before decoding.
