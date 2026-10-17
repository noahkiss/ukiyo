# Ukiyo

Ukiyo is a Mac app for a Fujifilm X-series camera, connected over USB. It manages film
simulation recipes and the camera's settings from the computer, so you do not dig through the
on-camera menus.

- **Recipe library.** Browse a bundled catalogue of film simulation recipes and keep your own.
  Ukiyo will read and write the camera's custom slots C1-C7.
- **Settings editor.** Read and change camera settings from the Mac, laid out like Fujifilm's
  own menus.
- **Lab.** A live property mapper. Change a setting on the camera, watch which PTP property
  moves, and label it. Labels go into a per-model catalog that anyone can extend.
- **`ukiyo` CLI and MCP server.** The command line tool and the MCP server reach what the
  window reaches, with the same safety rules. An agent can read the camera, plan a change, and
  apply it after a read-back check.

## Status

Ukiyo is pre-release. There is no signed build or installer yet; build it from source.

- Built: the USB connection core, the Lab, the CLI with gated setting writes, the MCP
  server, and the library: presets, snapshots, favourite settings and camera history, in the
  app, the CLI and MCP. The Python tools in `tools/ptp/` read C1-C7.
- In progress: the recipe library, C1-C7 reads and writes in the app, and packaging.
  `docs/roadmap.md` lists what comes next.

Ukiyo writes only properties whose meaning is confirmed, and reads each one back after the
write. Expect rough edges.

## Cameras

| Camera | State |
|---|---|
| X-T5 | Tested. The property catalog (`catalog/x-t5.json`) maps its settings. |
| Other X-series bodies | Not tested. Ukiyo connects and the Lab maps their properties. `catalog/menus/` holds the menus of 21 bodies from their owner's manuals. |

Contributions of a catalog for another body are welcome. `docs/catalog.md` describes the format.

## USB modes

Set the mode on the camera: SET UP > CONNECTION SETTING > CONNECTION MODE.

| Mode | What Ukiyo can do |
|---|---|
| USB TETHER SHOOTING FIXED | Read and change settings, the Hub, the Lab. The camera keeps working. |
| USB RAW CONV./BACKUP RESTORE | Read and write recipe slots C1-C7 (in progress). The camera is locked until you unplug it. |
| STILL/MOVIE switch at MOVIE (in tether mode) | Movie settings. Stills settings need the switch at STILL. |

`ukiyo help modes` prints the steps. `docs/protocol.md` has the detail.

## Build from source

Requirements:

- macOS 13 or later
- Rust through rustup (the version in `rust-toolchain.toml`)
- Node 24 (`.node-version`)
- pnpm (`packageManager` in `app/package.json`, through corepack)

Run the app:

```bash
cd app
pnpm install
UKIYO_FAKE_CAMERA=1 pnpm tauri dev   # no camera: a fake X-T5 with made-up values
pnpm tauri dev                       # the real camera over USB
```

`UKIYO_FAKE_MODE=rawconv` starts the fake camera in RAW CONV. mode. The dev server uses port
4823.

Build the CLI and add the MCP server to a client:

```bash
cd src-tauri
cargo build -p ukiyo-cli
claude mcp add ukiyo -- "$PWD/target/debug/ukiyo-cli" mcp
```

Run the tests:

```bash
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
cd ../app && pnpm build
```

`AGENTS.md` maps the source tree. `docs/agent-interface.md` covers the CLI and the MCP server.

## How the connection works

One process owns the USB connection: `ukiyo serve`. The window, the CLI and the MCP server all
talk to it over a local socket, and it starts on demand. Ukiyo stops macOS's PTPCamera agent
before it claims the camera.

The camera blocks reads while its own menu is open. Ukiyo pauses and resumes when the menu
closes. "Keep the camera awake while you use Ukiyo" (on by default) resets the auto power off
timer while you use the app. It never sets AUTO POWER OFF to OFF.

Ukiyo keeps session logs, exports and settings in
`~/Library/Application Support/com.nkmkdigital.ukiyo/`.

## Credits

The bundled film simulation recipes are © their creators. Ukiyo bundles their settings only,
with a link to each post (`docs/recipes.md`).

- Ritchie Roesch, [Fuji X Weekly](https://fujixweekly.com)
- Justin Gould, [film.recipes](https://film.recipes)
- The creators compiled by Henri-Pierre Chavaz in the
  [Fujifilm Film Simulation Recipes Database](https://docs.google.com/spreadsheets/d/11PvZ7rOnwmbz5VylVIvzOuWwEa83c0DweAlBwJ2pZJc)

Property mapping draws on the documentation of
[fujifilm-ptp-recipes](https://github.com/ILFforever/fujifilm-ptp-recipes) by ILFforever,
used under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
`docs/mapping-sources.md` lists every other source and its licence.

## Disclaimer

Ukiyo is an independent project. It is not affiliated with, endorsed by, or supported by
FUJIFILM Corporation. Fujifilm, X-series and film simulation names are trademarks of their
owners. Use Ukiyo at your own risk.
