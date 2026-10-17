# ukiyo

A Mac app for managing a Fujifilm X-series camera over USB: a recipe library that writes the
C1-C7 custom slots, and camera settings edited from the computer instead of the on-camera menus.
Stage: v0 Lab (live property mapper) built; not yet run against hardware. `docs/roadmap.md` has
what comes next.

## Layout

- `docs/protocol.md` — what is known about the camera's PTP interface. Update it with every finding.
- `docs/catalog.md` — the property catalog format.
- `docs/menus.md` — menu sets compared across bodies; X-T5 menu items mapped to catalog codes.
  `docs/fuji-vendor-ptp.md` and `docs/mapping-sources.md` — what open sources and Fuji's own tools
  say about vendor codes.
- `docs/agent-interface.md` — the `ukiyo` CLI and MCP server (stages 1-5 built): the shared owner,
  tools, plans, write safety.
- `app/` — the window: React + TypeScript + Vite (pnpm, Node from `.node-version`).
  `src/App.tsx` the shell and hash routes (`#/hub`, `#/settings/appearance`), `src/shell/`
  (Sidebar with the brand-mark slot, page head, camera chip, icons, ModeNotice: the wrong-USB-mode notice, the menu, playback and MOVIE notices, and the switch walkthrough with each body's menu path), `src/pages/` (Hub, Recipes,
  Settings with Appearance, Connect, and Lab: Lab state and keys; it stays mounted while hidden),
  `src/views/` (the Lab's StatusBar, PropTable, ChangeFeed, LabelPanel with the write box,
  FakeBar), `src/lib/` (`camera.tsx` live status for every page, `live.ts` the camera's properties and
  catalog, `plans.ts` plan then apply, `prefs.tsx` theme, brand mark and app icon choices in
  `settings.json` (local storage in the browser preview), `decode.ts` value display, `links.ts` opens `target=_blank` links in the browser, `catalog.ts` the bundled recipe
  catalogue as library recipes), `src/fixtures/` sample data
  for the browser preview (see its README), `src/styles/` (`tokens.css` light and dark
  tokens, then base, shell, pages, lab), `src/assets/` (bundled OFL fonts with licences, camera
  art, brand marks and app icons as light and dark webp; `camera/index.ts` maps a model to its
  art, with a generic body for unknown models). `src/bindings.ts` generated (do not edit;
  `UKIYO_UPDATE_BINDINGS=1 cargo test bindings`). Dev server pinned to 4823 (strictPort). In a
  browser outside Tauri, `?camera=tether|rawconv|cardreader|movie|menu|playback` previews a camera state and
  `?model=X-H2S` another body.
- `src-tauri/core/` — `ukiyo-core`, the workspace member with no Tauri: `src/api.rs` (`Request`,
  `Response`, `Error`, `Event`, `call`, `dispatch`) and the modules below. Cargo commands in
  `src-tauri/` cover both crates.
- `src-tauri/core/src/camera/` — `ptp` (framing, datasets), `transport` (Connector/Transport seam),
  `usb` (nusb), `fake` (fake X-T5; shape in `fake_x-t5.json`), `clock`, `owner` (the one
  connection thread: queue, polling, pause, power-off, reconnect rules, keep-awake D381 toggle,
  menu-close half-press), `owner_tests`.
- `src-tauri/core/src/recipes/` — the recipe text parser (`mod`, `values`), recipe blocks in post
  HTML (`html`), name and sensor from a post title (`post`), the catalogue formats and the
  de-duplicating merge and popularity (`catalogue`), sensor families (`family`), one adapter
  per recipe site (`sources/`), `tests` on real sample blocks. `docs/recipes.md` covers it and the credits.
- `src-tauri/core/examples/recipes_refresh.rs` — reads each source's feed into
  `catalog/recipes/sources/<id>.json` and merges them into `catalog/recipes/catalog.json`;
  caches pages in `local/recipe-feeds/<id>/`.
- `src-tauri/core/src/catalog.rs` — catalog load, label, save. `src-tauri/core/src/lab.rs` — the
  Lab (mirror, session log with actors, export, settings, stored plans and their apply, fake
  controls). `src-tauri/core/src/plan.rs` — plans: build and gate, stale check; tests in
  `plan/tests.rs`. `src-tauri/core/src/library/` — presets, snapshots, favourites, the trash (`mod`), `prefs`
  (`settings.json`), `snapshot`, `history` (per camera, by a salted serial digest); tests in
  `library/tests.rs`. `src-tauri/core/src/evidence.rs` — the rules on a label from an agent (no
  `confirmed` without a change this session, no camera strings). `src-tauri/src/lib.rs` — Tauri
  commands (all but `recipe_parse` and `prefs_get` forward to `ukiyo serve` over `ukiyo.sock`), `specta_builder`;
  `main.rs` runs `serve` when argv[1] is `serve`; `src-tauri/src/events.rs` the four Tauri events.
- `src-tauri/core/src/serve/` — `lock` (`owner.lock`; `Lab::start` takes it), `wire` (the socket
  protocol), `server` (`ukiyo serve`: socket, events, idle exit), `client` (connect, spawn on
  demand, reconnect), `mod` (`spawn_detached`, the `serve` subcommand).
  `src-tauri/core/src/paths.rs` — the data folder, its move from `app.ukiyo`, `UKIYO_DATA_DIR`.
- `src-tauri/cli/` — the `ukiyo` CLI (bin `ukiyo-cli`): `main.rs` (clap), `cmds.rs`, `writes.rs` (the write commands), `library.rs` (presets, snapshots, favourites, prefs,
  trash), `mcp.rs` (`ukiyo mcp`, the stdio MCP server, actor
  `mcp:<client>`), `mcp_tools.rs` (the tool listing; surface guard against
  `tests/surface/mcp_tools.json`, update with `UKIYO_UPDATE_SURFACE=1 cargo test -p ukiyo-cli
  surface`), `conn.rs` (the
  socket client; spawns `serve`, never USB), `out.rs` (`--json` envelope, exit codes);
  `tests/cli.rs` and `tests/mcp.rs` on the fake camera. Its read views live in core `read.rs`, `modes.rs`,
  `session.rs`. Built so far: `serve`, `status`, `settings`, `camera watch`, `catalog show`,
  `log show`, `help modes`, `settings set`, `owner`, `plan`,
  `camera connect|disconnect|export`, `log note`, `mcp`, `presets`, `snapshots`,
  `settings fav`, `prefs`, `trash`, `status --history`.
- `src-tauri/capabilities/links.json` — the hosts the window may open links on; generated by
  `UKIYO_UPDATE_LINK_HOSTS=1 cargo test link_hosts`.
- `catalog/recipes/` — the bundled recipe catalogue: `sources/<id>.json` per site, the
  merged `catalog.json`, and `reddit.json` (Reddit counts for the popularity score). Settings and links only, no photos or post text.
- `catalog/menus/<model>.json` — each body's menus, options, ranges and defaults from the owner's
  manual; format in `docs/catalog.md` (Menu sets). `tools/catalog/menus_fetch.py [SLUG ...]
  [--offline]` rebuilds them from fujifilm-dsc.com (caches in `local/manuals/`; needs pdftotext).
- `catalog/<model>.json` — the shareable property catalog. Dev builds write labels here;
  `UKIYO_CATALOG_DIR` overrides.
- `tools/catalog/seed_xt5.py LOCAL_DIR [--force]` — seeds `catalog/x-t5.json` and the fake shape
  from `local/` dumps.
- `tools/catalog/makernote_diff.py PHOTO... [--steps S] [--log L]` — FujiFilm MakerNote changes
  between photos shot one step apart, paired with a guided session's PTP changes (needs exiftool).
  `backup_diff.py BLOB...` — changed byte runs between backups. Neither touches the camera.
- `tools/art/app_icon_layers.py` and `app_icon_alternates.py` build the Liquid Glass `.icon`
  sources from the finals; `tools/art/build_app_icon.sh` compiles `src-tauri/icons/AppIcon.icon`
  into `Assets.car`, `icon.icns` and the PNG fallbacks (all committed). Rerun it after any `.icon`
  change.
- `tools/art/camera_art.py [SRC_ROOT]` — builds the Hub camera art (`app/src/assets/camera/`)
  from the generated drawings in `image-output.local/` (not in git). `uv run -q camera_art.py`.
- `tools/ptp/` — Python discovery scripts (uv inline deps, pyusb). Every script takes Ukiyo's
  `owner.lock` first (`owner_lock.py`, through `PTP()`) and refuses while the app, `ukiyo serve`
  or any `ukiyo` command owns the camera. Run from that directory with
  `DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib uv run -q <script>.py`.
  - `read_presets.py` — read C1-C7 (RAW CONV. mode).
  - `dump_props.py OUT.json` — every property descriptor or value.
  - `watch_props.py PROPS.json NAMES.json LOG` — poll and log changes.
  - `set_prop.py CODE VALUE` — write one uint16 property, with read before and after.
  - `guided_map.py STEPS [--dry-run DUMP.json]` — guided mapping session: a dialog names one
    change per step (`steps/<name>.json`); the script diffs every property, keeps the camera awake
    with the D381 toggle, and writes a log, summary and proposed catalog patch to `local/mapping/`.
    A steps file with `"mode": "movie"` runs with the STILL/MOVIE switch on MOVIE; a
    `"restore": true` step lists what changed since the start.
  - `read_backup.py` — read the camera's settings backup blob into `local/backup/`.
- `tools/catalog/makernote_diff.py` and `backup_diff.py` — diff ExifTool MakerNote fields and
  backup blobs between two shots or two backups, to map settings PTP does not name.
    A steps file's `"mode": "movie"` runs with the STILL/MOVIE switch on MOVIE; a `"restore": true`
    step lists every changed item with its start value.
  - `read_backup.py LABEL` — the settings backup (object handle 0, RAW CONV. mode) to
    `local/backup/`. Read-only.
- `tools/wifi/` — guided Wi-Fi and Bluetooth test run (uv inline deps): `run_wifi_tests.py
  [--dry-run]`, `fujiptpip.py` (Fuji PTP over TCP), `macnet.py`, `ble.py`, `fakecam.py`. Logs to
  `local/wifi-test/`. `docs/wifi.md` covers what sources say and the Wi-Fi transport sketch.
- `tools/ios-ptp-probe/` — iOS/iPadOS test app (XcodeGen, ImageCaptureCore): can a phone read
  and write the camera's PTP properties over USB-C? Signing from gitignored `Local.xcconfig`; see
  its README.
- `local/` — gitignored. Raw dumps, logs and the working property map. They contain camera and
  lens serial numbers and owner strings.
- App data (`~/Library/Application Support/com.nkmkdigital.ukiyo/`): `sessions/*.jsonl`, `dumps/`,
  `settings.json` (theme, brand mark, app icon, `keep_awake`, `in_use_minutes`,
  `close_menu_to_write`), `library/` (presets, snapshots, favourites, `.trash/`, `log.jsonl`),
  `history/` (per-camera history and its salt), `owner.lock`, `ukiyo.sock`, `serve.log`.

## Build, test, run

- In `src-tauri/`: `cargo test`, `cargo clippy --all-targets -- -D warnings`.
- In `app/`: `pnpm build`; run with `pnpm tauri dev` from `app/` (not the repo root).
  `UKIYO_FAKE_CAMERA=1` uses the fake camera; `UKIYO_FAKE_MODE=rawconv` fakes RAW CONV. mode.
  `pnpm tauri dev` spawns `target/debug/ukiyo serve`, which outlives the window for 120 s
  (`UKIYO_IDLE_EXIT=S` shortens it); `UKIYO_DATA_DIR=DIR` uses another data folder.
  Anything that runs `ukiyo-cli` or `serve` in a test sets `UKIYO_FAKE_CAMERA=1` and a temp
  `UKIYO_DATA_DIR`, whose `ukiyo.sock` path must stay under 104 bytes.

## Rules for touching the camera

- Read before you write; write only properties whose meaning is confirmed, and read back.
- One process owns the USB connection: `ukiyo serve`, which holds `owner.lock`. Quit any Ukiyo.app
  built before stage 2 (17e2c0c) first; it opens USB without the lock.
- Never reconnect in a tight loop or reset USB on every connect (see protocol.md, power).
- Never commit current values, serials or owner strings to `catalog/`.
