# The property catalog

A catalog names a camera model's PTP properties: what each code is, where it lives in the
camera's menus, and what its values mean. There is one file per model in `catalog/`, named by
the model string the camera reports in GetDeviceInfo, lowercased, with anything other than a
letter or digit turned into `-`: `X-T5` is `catalog/x-t5.json`.

A catalog never holds a value read from one camera: no current setting, serial number, owner
or copyright string, or shot count. It describes the model, so it can be shared.

## Format

```json
{
  "schema": 1,
  "model": "X-T5",
  "manufacturer": "FUJIFILM",
  "usb": { "vendor": "0x04cb", "product": "0x02fc" },
  "firmware": ["4.31"],
  "properties": {
    "0xd381": {
      "name": "AutoPowerOff",
      "menu": "SET UP > POWER MANAGEMENT > AUTO POWER OFF",
      "type": "uint16",
      "values": { "1": "5 MIN", "2": "2 MIN", "3": "OFF", "4": "1 MIN", "5": "30 SEC", "6": "15 SEC" },
      "confidence": "confirmed",
      "source": "lab session",
      "modes": ["stills", "movie"],
      "notes": "Write 1 -> 3 tested. Movie mode keeps its own value."
    }
  }
}
```

| Field | Meaning |
|---|---|
| `schema` | The format version. The app refuses a file with another version. |
| `model`, `manufacturer` | As GetDeviceInfo reports them. |
| `usb` | USB vendor and product id, hex. |
| `firmware` | Body firmware versions the entries were mapped on. The Lab adds the connected version when it saves a label. |
| `properties` | Keyed by the code: `0x` and four lowercase hex digits. Sorted by code. |

Each property has only the fields it needs:

| Field | Meaning |
|---|---|
| `name` | A short identifier, `PascalCase`. libgphoto2's name where it is right. |
| `menu` | Where the setting lives: menu path as the camera prints it (`SECTION > ITEM`), or the dial or switch. |
| `type` | PTP datatype: `int8` to `uint64`, `string`, `array`, `undefined` (datatype 0). |
| `values` | Raw value to the label the camera shows. Numbers are keys in decimal (`"32769"`, not `"0x8001"`; signed types negative: ISO AUTO is `"-3"`); strings are their own text. Sorted by number. |
| `unit`, `scale` | The raw value divided by `scale` is the value in `unit`: exposure compensation is in 1/1000 EV, so `"scale": 1000, "unit": "EV"`. |
| `confidence` | `confirmed` (seen move on a camera, or a write read back), `likely` (strong evidence, not yet seen), `guess`. |
| `source` | Where the entry came from: `PTP` (the standard), `libgphoto2`, `filmkit`, `menu map` (a reading of the manual against the dumps), `protocol.md`, `lab session`. The Lab appends `+ lab session` when it edits an entry from another source. |
| `modes` | Where the code reads: `stills`, `movie` (tether mode with the STILL/MOVIE switch set), `rawconv` (USB RAW CONV./BACKUP RESTORE). |
| `notes` | Anything else: quirks, dependencies, open questions. |

An entry with no `name` is unnamed: the Lab lists it under the Unnamed filter.

## Mapping a new model

1. Connect the camera in USB TETHER SHOOTING FIXED mode and start the app. If no catalog
   exists for the model, the first label you save creates it.
2. Press **Export dump** in each mode you can reach (stills, movie, RAW CONV.). Dumps go to
   the app data dir; they carry your camera's serials, so keep them out of the repo.
3. Change one setting at a time on the camera. Each change group in the feed is usually one
   physical action. Label the code, both values, and set the confidence.
4. Commit the catalog file. Check it first: it should hold no value that identifies your camera.

The app writes the file whole, with two-space indentation and keys in the order above, so a
diff shows only what changed. `tools/catalog/seed_xt5.py` wrote the first X-T5 file from the
local dumps, libgphoto2's names and the menu map; it refuses to overwrite a catalog without
`--force`, since lab sessions edit it.
