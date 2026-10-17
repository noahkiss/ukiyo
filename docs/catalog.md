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
| `requires` | Write conditions: `[{ "code": "0xd001", "values": ["6", "12"], "temp": "6" }]`. The camera takes a write of this code only while `code` holds one of `values`. A plan sets `temp` (else the first value) for the time of the write, then puts the old value back. |
| `resets` | Camera rules: `[{ "when": ["18"], "code": "0xd032", "to": "0" }]`. Writing one of `when` (any value when absent) sets `code` to `to`. A plan writes `code` after this one, and puts it back when only a temporary step changed it. |

`requires` and `resets` order every plan (`plan::sequence`, docs/agent-interface.md, "The
recipes as built"). They are model facts, so other bodies' catalogs carry their own.

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

## Menu sets

A menu set lists a body's menus as its owner's manual prints them: tabs, pages, items, options,
defaults and ranges. It tells the app what a body offers before anyone maps its PTP codes. There
is one file per body in `catalog/menus/`, named like a catalog (`X-T5` is
`catalog/menus/x-t5.json`).

`tools/catalog/menus_fetch.py` builds every file from Fujifilm's English HTML manuals at
`fujifilm-dsc.com/en/manual/<slug>/`. It caches each page in `local/manuals/<slug>/`, so a rerun
fetches only what is missing. A menu set holds structured facts only: names, option labels,
defaults and ranges. It never copies the manual's prose or images.

```sh
cd tools/catalog
uv run -q menus_fetch.py              # every body
uv run -q menus_fetch.py x-t5 x-h2s   # some bodies, by manual slug
uv run -q menus_fetch.py --offline    # cache only, no network
```

The script needs `pdftotext` (poppler) to read the firmware version from the New Features Guide.

### Format

```json
{
  "schema": 1,
  "model": "X-T5",
  "manual": {
    "url": "https://fujifilm-dsc.com/en/manual/x-t5/",
    "language": "en",
    "firmware": "4.30",
    "firmware_source": "x-t5_nfg_en_s_f.pdf",
    "layout": "menu list",
    "fetched": "2026-10-17"
  },
  "generation": "X-Trans V",
  "sensor": "X-Trans CMOS 5 HR",
  "processor": "X-Processor 5",
  "stats": { "items": 242, "described": 241 },
  "tabs": [
    {
      "menu": "shooting",
      "mode": "stills",
      "name": "IMAGE QUALITY SETTING",
      "pages": [
        {
          "page": 1,
          "items": [
            {
              "name": "GRAIN EFFECT",
              "sub": [
                { "name": "ROUGHNESS", "options": ["STRONG", "WEAK", "OFF"] },
                { "name": "SIZE", "options": ["LARGE", "SMALL"] }
              ],
              "ref": "menu_shooting/image_quality_setting/#grain_effect"
            }
          ]
        }
      ]
    }
  ],
  "firmware_changes": [{ "version": "4.30", "heading": "SELECT JPEG/HEIF" }]
}
```

| Field | Meaning |
|---|---|
| `schema` | The format version. |
| `model` | The model string GetDeviceInfo reports. |
| `manual.url` | The manual the file was built from. |
| `manual.firmware` | The body firmware the manual covers: the version on the New Features Guide's cover. `null` if the guide is missing. |
| `manual.firmware_source` | The guide's file name. |
| `manual.layout` | `menu list`: tabs and pages come from the manual's Menu List page. `section index`: older manuals without one; tabs come from the menu index and the factory-defaults tables, and pages are unknown. |
| `manual.fetched` | The date the cached index page was fetched. |
| `generation`, `sensor`, `processor` | Sensor generation for cross-model comparison: `X-Trans V`, `X-Trans IV + X-Processor 5`, `X-Trans IV`, `X-Trans III`. Set in the script, not read from the manual. |
| `stats` | Items in the menu tree, and how many of them the script found a description for. |
| `tabs` | The menu tabs in camera order. |
| `firmware_changes` | Each change the New Features Guide documents: the firmware version and the heading it is filed under. |

Each tab:

| Field | Meaning |
|---|---|
| `menu` | `shooting`, `playback`, `setup` or `network`. |
| `mode` | `stills` (photo menus) or `movie` (movie menus) for shooting tabs; `null` otherwise. |
| `name` | The tab name as the camera prints it. |
| `pages` | Menu pages in order. `page` is the page number, or `null` for a one-page tab or an unknown page. |

Each item has only the fields it needs:

| Field | Meaning |
|---|---|
| `name` | The item as the camera prints it. |
| `options` | Option labels as the camera prints them, in manual order. Absent when the manual shows them only as a picture. |
| `option_header` | The option column's header when it says more than "Option" (`Option (Image size)`). |
| `range` | Numeric options: `min`, `max`, and `step` and `unit` when known. |
| `range_from_text` | `true` when the range comes from a sentence in the manual, not an option table. Check these by hand. |
| `default` | The factory default. Only older manuals (`section index` layout) print a defaults table, so newer bodies have none. |
| `modes` | From the Menu List icons: `["stills", "movie"]` marks an item shared by the photo and movie menus (a change in one applies to both); `["movie"]` or `["stills"]` marks an item that applies to one only. Absent: the item follows its tab. |
| `changed_in` | Firmware versions whose New Features Guide documents this item (added or changed). |
| `sub` | Sub-items, each in this same shape. |
| `ref` | The manual page and anchor the item was read from, relative to `manual.url`. Absent when no description was found. |

Known gaps: some options appear only as screenshots (MONOCHROMATIC COLOR, the white-balance
shift grid, older manuals' grain and tone pages), so those items carry no options or range. The
manuals give no step for TONE CURVE. Defaults appear only where a manual prints a defaults table.

`docs/menus.md` compares the menu sets across bodies and maps X-T5 menu items to catalog codes.
