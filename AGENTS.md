# ukiyo

A Mac app for managing a Fujifilm X-series camera over USB: a recipe library that writes the
C1-C7 custom slots, and camera settings edited from the computer instead of the on-camera menus.
Stage: protocol discovery. No app code yet.

## Layout

- `docs/protocol.md` — what is known about the camera's PTP interface. Update it with every finding.
- `tools/ptp/` — Python discovery scripts (uv inline deps, pyusb). Run from that directory with
  `DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib uv run -q <script>.py`.
  - `read_presets.py` — read C1-C7 (RAW CONV. mode).
  - `dump_props.py OUT.json` — every property descriptor or value.
  - `watch_props.py PROPS.json NAMES.json LOG` — poll and log changes.
  - `set_prop.py CODE VALUE` — write one uint16 property, with read before and after.
- `local/` — gitignored. Raw dumps, logs and the working property map. They contain camera and
  lens serial numbers and owner strings.
