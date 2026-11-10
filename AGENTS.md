# ukiyo

A Mac app for managing a Fujifilm X-series camera over USB: a recipe library that writes the
C1-C7 custom slots, and camera settings edited from the computer instead of the on-camera menus.
The v0 Lab (live property mapper) is built and has not yet run against hardware.
`docs/roadmap.md` has what comes next.

## Layout

`docs/layout.md` maps the source tree and the app's data folder. Read it before you add a module.
The main parts:

- `app/` — the window: React + TypeScript + Vite (pnpm, Node from `.node-version`). Dev server
  pinned to 4823 (strictPort). `src/bindings.ts` is generated; do not edit it.
- `src-tauri/core/` — `ukiyo-core`, no Tauri: camera transport and owner, catalog, plans,
  library, recipes, `ukiyo serve`. `src-tauri/src/` — the Tauri shell. `src-tauri/cli/` — the
  `ukiyo` CLI and MCP server (`docs/agent-interface.md`).
- `catalog/` — the shareable property catalog, menu sets and the bundled recipe catalogue.
- `tools/` — Python discovery scripts (`ptp/`, `wifi/`, `catalog/`), art and release scripts.
- `local/` — gitignored dumps and logs. They contain camera and lens serial numbers and owner
  strings.
- `docs/protocol.md` — what is known about the camera's PTP interface. Update it with every
  finding.

## Generated files

Regenerate these with the command; do not hand-edit them.

| File | Command |
|---|---|
| `app/src/bindings.ts` | `UKIYO_UPDATE_BINDINGS=1 cargo test bindings` (in `src-tauri/`) |
| `src-tauri/cli/tests/surface/*` (CLI and MCP surface guards) | `UKIYO_UPDATE_SURFACE=1 cargo test -p ukiyo-cli surface` (in `src-tauri/`) |
| `src-tauri/capabilities/links.json` | `UKIYO_UPDATE_LINK_HOSTS=1 cargo test link_hosts` (in `src-tauri/`) |
| `src-tauri/icons/` compiled icons | `tools/art/build_app_icon.sh`, after any `.icon` change |

## Build, test, run

- In `src-tauri/`: `cargo test`, `cargo clippy --all-targets -- -D warnings`.
- In `app/`: `pnpm build`; run with `pnpm tauri dev` from `app/` (not the repo root).
  `pnpm bundle` builds `Ukiyo.app` with the CLI inside (ad-hoc signed).
- Release build without upload: `tools/release/release.sh --dry-run` (Apple Silicon).
  `UKIYO_FAKE_CAMERA=1` uses the fake camera; `UKIYO_FAKE_MODE=rawconv` fakes RAW CONV. mode;
  `UKIYO_FAKE_AUTO=0` stops the fake's own setting changes (the CLI tests set it).
  `UKIYO_TRANSPORT=usb|wifi` picks the transport for one run (with the fake: a loopback
  Wi-Fi endpoint).
  `pnpm tauri dev` spawns `target/debug/ukiyo serve`, which outlives the window for 120 s
  (`UKIYO_IDLE_EXIT=S` shortens it); `UKIYO_DATA_DIR=DIR` uses another data folder.
  Anything that runs `ukiyo-cli` or `serve` in a test sets `UKIYO_FAKE_CAMERA=1` and a temp
  `UKIYO_DATA_DIR`, whose `ukiyo.sock` path must stay under 104 bytes.

## Rules for touching the camera

- Read before you write; write only properties whose meaning is confirmed, and read back.
- One process owns the USB connection: `ukiyo serve`, which holds `owner.lock`. Quit any old
  Ukiyo.app build that predates the lock (before 17e2c0c); it opens USB without the lock.
- `tools/ptp/` and `tools/wifi/` scripts take `owner.lock` too. Run them from their directory with
  `DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib uv run -q <script>.py`.
- Never reconnect in a tight loop or reset USB on every connect (see protocol.md, power).
- Never commit current values, serials or owner strings to `catalog/`.
- Every camera write from the window, CLI or MCP is a plan; there is no direct write method.
- Keep-awake never writes AUTO POWER OFF = OFF; a half-press is sent only to close a menu over
  the shooting screen, once per write.
