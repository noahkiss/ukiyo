# ukiyo

A Mac app for managing a Fujifilm X-series camera over USB: a recipe library that writes the
C1-C7 custom slots, and camera settings edited from the computer instead of the on-camera menus.
Stage: v0 Lab (live property mapper) built; not yet run against hardware. `docs/roadmap.md` has
what comes next.

## Layout

- `docs/protocol.md` — what is known about the camera's PTP interface. Update it with every finding.
- `docs/catalog.md` — the property catalog format.
- `app/` — the window: React + TypeScript + Vite (pnpm, Node from `.node-version`).
  `src/App.tsx` the shell and hash routes (`#/hub`, `#/settings/appearance`), `src/shell/`
  (Sidebar with the brand-mark slot, page head, camera chip, icons), `src/pages/` (Hub, Recipes,
  Settings with Appearance, Connect, and Lab: Lab state and keys; it stays mounted while hidden),
  `src/views/` (the Lab's StatusBar, PropTable, ChangeFeed, LabelPanel with the write box,
  FakeBar), `src/lib/` (`camera.tsx` live status for every page, `prefs.tsx` theme, brand mark
  and app icon choices in local storage, `decode.ts` value display), `src/fixtures/` sample data
  for pages without a backend (see its README), `src/styles/` (`tokens.css` light and dark
  tokens, then base, shell, pages, lab), `src/assets/` (bundled OFL fonts with licences, camera
  art, brand marks and app icons as light and dark webp; `camera/index.ts` maps a model to its
  art, with a generic body for unknown models). `src/bindings.ts` generated (do not edit;
  `UKIYO_UPDATE_BINDINGS=1 cargo test bindings`). Dev server pinned to 4823 (strictPort). In a
  browser outside Tauri, `?camera=tether|rawconv|cardreader` previews a camera state and
  `?model=X-H2S` another body.
- `src-tauri/src/camera/` — `ptp` (framing, datasets), `transport` (Connector/Transport seam),
  `usb` (nusb), `fake` (fake X-T5; shape in `fake_x-t5.json`), `clock`, `owner` (the one
  connection thread: queue, polling, pause, power-off, reconnect rules), `owner_tests`.
- `src-tauri/src/catalog.rs` — catalog load, label, save. `src-tauri/src/lab.rs` — the Lab
  (mirror, session log, export, settings, fake controls). `src-tauri/src/lib.rs` — Tauri commands
  and `specta_builder`.
- `catalog/<model>.json` — the shareable property catalog. Dev builds write labels here;
  `UKIYO_CATALOG_DIR` overrides.
- `tools/catalog/seed_xt5.py LOCAL_DIR [--force]` — seeds `catalog/x-t5.json` and the fake shape
  from `local/` dumps.
- `tools/art/camera_art.py [SRC_ROOT]` — builds the Hub camera art (`app/src/assets/camera/`)
  from the generated drawings in `image-output.local/` (not in git). `uv run -q camera_art.py`.
- `tools/ptp/` — Python discovery scripts (uv inline deps, pyusb). Run from that directory with
  `DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib uv run -q <script>.py`.
  - `read_presets.py` — read C1-C7 (RAW CONV. mode).
  - `dump_props.py OUT.json` — every property descriptor or value.
  - `watch_props.py PROPS.json NAMES.json LOG` — poll and log changes.
  - `set_prop.py CODE VALUE` — write one uint16 property, with read before and after.
- `local/` — gitignored. Raw dumps, logs and the working property map. They contain camera and
  lens serial numbers and owner strings.
- App data (`~/Library/Application Support/app.ukiyo/`): `sessions/*.jsonl`, `dumps/`,
  `settings.json`.

## Build, test, run

- In `src-tauri/`: `cargo test`, `cargo clippy --all-targets -- -D warnings`.
- In `app/`: `pnpm build`; run with `pnpm tauri dev` from `app/` (not the repo root).
  `UKIYO_FAKE_CAMERA=1` uses the fake camera; `UKIYO_FAKE_MODE=rawconv` fakes RAW CONV. mode.

## Rules for touching the camera

- Read before you write; write only properties whose meaning is confirmed, and read back.
- One process owns the USB connection.
- Never reconnect in a tight loop or reset USB on every connect (see protocol.md, power).
- Never commit current values, serials or owner strings to `catalog/`.
