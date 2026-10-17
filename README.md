# ukiyo

A Mac app for a Fujifilm X-series camera over USB. This first version is the **Lab**: a live
property mapper. Flip a setting on the camera, watch which PTP property moves, and label it.
Labels go into a model catalog (`catalog/<model>.json`) that anyone can extend for their camera.

## Run it

Requirements: Rust through rustup (the version in `rust-toolchain.toml`), Node 24
(`.node-version`), and pnpm (`packageManager` in `app/package.json`, through corepack).

```bash
cd app
pnpm install
UKIYO_FAKE_CAMERA=1 pnpm tauri dev   # no camera: a fake X-T5 with made-up values
pnpm tauri dev                       # the real camera over USB
```

The dev server runs on port 4823. `UKIYO_FAKE_MODE=rawconv` starts the fake camera in
RAW CONV. mode instead of tether.

With a real camera:

1. Set SET UP > CONNECTION SETTING > CONNECTION MODE to **USB TETHER SHOOTING FIXED**. The
   other modes work, with fewer properties (`docs/protocol.md`).
2. Plug the camera in and turn it on. The app stops macOS's PTPCamera agent before it claims
   the camera, and finds it within 3 seconds.
3. Change one setting on the camera. The change feed shows what moved. Type what you did into
   "What I just changed" first, and the note sticks to that change.
4. Label it: select the change (`j`/`k`) and press `l`, or double-click a row. `⌘↩` saves.

The camera's own menu blocks every read while it is open. The app pauses, shows "Camera in
menu", and resumes when the menu closes. When the camera powers itself off, the app waits 30
seconds before it reconnects. "Keep camera awake" sets AUTO POWER OFF to OFF while connected
and restores it on Disconnect or quit.

## Tests

```bash
cd src-tauri
cargo test                                  # unit tests, the owner on the fake camera
cargo clippy --all-targets -- -D warnings
UKIYO_UPDATE_BINDINGS=1 cargo test bindings # rewrite app/src/bindings.ts after a command change
cd ../app && pnpm build
```

## Files

| Path | Holds |
|---|---|
| `src-tauri/src/camera/` | The PTP core: `ptp` framing, `usb` (nusb), `fake`, `owner` (the one connection thread) |
| `src-tauri/src/catalog.rs` | Catalog load, label, save |
| `src-tauri/src/lab.rs` | The Lab: mirror for the window, session log, export, the fake camera's controls |
| `app/` | The window: React, TypeScript, Vite |
| `catalog/` | One JSON file per camera model (`docs/catalog.md`) |
| `catalog/recipes/` | Recipe settings from Fuji X Weekly, film.recipes and H-P Chavaz's recipe database, with a link to each post (`docs/recipes.md`) |
| `src-tauri/src/recipes/` | The recipe text parser, the source adapters and the de-duplicating merge |
| `docs/protocol.md` | What is known about the camera's PTP interface |
| `tools/ptp/` | The Python discovery scripts the Rust core was ported from |
| `tools/catalog/seed_xt5.py` | Seeds `catalog/x-t5.json` and the fake camera's shape from local dumps |

The app writes session logs (`sessions/*.jsonl`), exports (`dumps/`) and `settings.json` to
`~/Library/Application Support/com.nkmkdigital.ukiyo/`. In a dev build, labels go to the repo's
`catalog/` so they can be committed; `UKIYO_CATALOG_DIR` points them elsewhere.

## Credits

The bundled film simulation recipes are © their creators: Ritchie Roesch,
[Fuji X Weekly](https://fujixweekly.com); Justin Gould, [film.recipes](https://film.recipes);
and the creators compiled by Henri-Pierre Chavaz in his
[Fujifilm Film Simulation Recipes Database](https://docs.google.com/spreadsheets/d/11PvZ7rOnwmbz5VylVIvzOuWwEa83c0DweAlBwJ2pZJc).
ukiyo bundles their settings only, with a link to each post (`docs/recipes.md`).
