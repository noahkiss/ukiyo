# Agent interface: CLI and MCP

A design for the `ukiyo` command line tool and its MCP server. Stage 1 (the core crate and
`api`), stage 2 (the shared owner, `ukiyo serve`), stage 3 (the read-only CLI), stage 4
(plans and gated writes), stage 5 (the MCP server), stage 6 (the library), stage 7
(recipes) and stage 8 (packaging) are built (section 8). No release exists yet.
The tool shapes follow the `mcp-tool-design` guidance: a small tool surface,
search then read, and plan then apply for every camera write.

## 1. Goals

- An agent can configure everything the app can:
  - camera settings, presets and favourite settings
  - snapshots of the camera, their diffs, and restores
  - the C1-C7 recipe slots, the recipe library, and the bundled catalogue (search and import)
  - owner strings (copyright, author, file-name prefix, device name)
  - the Lab's property mapping: names, menu paths, value labels, confidence, notes
  - app appearance: theme, sidebar brand mark, app icon, keep-awake
- Agents get the same safety rules as the window, enforced in the core, not in each front end.
- One behaviour per feature. The window, the CLI and the MCP server never disagree.

Out of scope: shooting (no capture or live view), firmware, and anything the app itself cannot do.

## 2. Architecture

### One core, three front ends

```
                 ukiyo-core (library crate)
   camera owner · catalog · recipes · library · plans · session log
        |                      |                       |
   Tauri app            ukiyo CLI (clap)        ukiyo mcp (stdio)
   (window)             human + --json          MCP tools
```

- `ukiyo-core` lives in `src-tauri/core/`, a member of the Cargo workspace rooted at
  `src-tauri/Cargo.toml` (QuadCam keeps its sim crate the same way). `cargo test` and
  `cargo clippy` in `src-tauri/` cover both crates. The core has no Tauri dependency; it keeps
  `specta` so its types still export to `app/src/bindings.ts`.
- The core holds `camera/`, `catalog.rs`, `recipes/` (with `examples/recipes_refresh.rs`),
  `lab.rs`, `paths.rs` (the data folder, its move from `app.ukiyo`, `owner.lock`, `ukiyo.sock`)
  and `serve/` (the shared owner, its socket protocol and the client; "One USB owner" below).
  Stage 3 added `read.rs` (the read-only views: status, settings rows, menus, change groups,
  and why the camera cannot answer), `modes.rs` (the USB mode walkthrough) and `session.rs`
  (reading the session logs back). The CLI prints them; the MCP server returns the same
  shapes. Stage 5 added `evidence.rs`, the two rules on a label from an agent (section 4,
  `catalog_label`). Three more modules are planned or built:
  - `library` (stage 6): presets, favourites, snapshots, user recipes, camera history, and
    preferences.
  - `plan` (built, stage 4): builds and gates a plan, checks it for staleness, and shapes the
    applied result. `lab.rs` stores plans and applies them through the owner.
  - `api` (built): every front end speaks only this.
    - `ukiyo_api_table!`, the method table (stage 8, QuadCam's `api!`): one row per method
      with its variant, params, result and the body `dispatch` runs. The core expands it into
      `Request`, `Response`, `METHODS`, `Request::method` and `dispatch`; the app crate
      expands the same table into its Tauri commands (`src-tauri/src/lib.rs`,
      `tauri_commands!`). Rows sit in three groups: `result` and `quiet` rows become
      forwarding Tauri commands (with and without a result), `own` rows have a hand-written
      command or none. A new method is one row.
    - `Request`, one variant per method: `{"method": "plan_create", "params": {...}}`.
    - `Response`, the variant of the same name: `{"method": "plan_create", "result": {...}}`.
    - `Error`: a `code`, a `message`, and `hint`, `walkthrough` and `mode` when they help.
      The codes are section 5's, plus `failed`, `no_fake_camera`, `bad_request` (a socket
      line or tool call that is not a request) and `unavailable` (no `serve` to answer).
    - `Event`: `status`, `props`, `changes`, `catalog`, as `{"event": ..., "data": ...}`.
      `serve` sends these to every client that called `subscribe`.
    - `call::<method>`, one typed function per method, and `dispatch(&Lab, actor, Request)`.
      The socket calls `dispatch` with the connection's `hello` actor. The CLI and the MCP
      server send their requests over the socket.
    - Method names are the window's command names (`lab_snapshot`, `lab_pin`,
      `lab_keep_awake`, `lab_connect`, `lab_export`, `lab_label`, `lab_note`, `lab_fake`,
      `recipe_parse`, `plan_create`, `plan_show`, `plan_apply`, `plan_discard`, `activity`).
      `activity` is the window's input ping; every request counts as use for keep-awake. New methods
      for the CLI and MCP join the same enums. Stage 4 removed `lab_write`: a camera write
      exists only as a plan.
- `src-tauri/src/` keeps the window glue: Tauri commands, the four Tauri events
  (`events.rs`) and specta bindings. Each command sends its `api::Request` to `serve` over the
  socket, and the client turns `serve`'s events into the Tauri events. `recipe_parse` is the
  one exception: pure parsing with no shared state, so it runs in the app and works without
  `serve`. A command that waits on `serve` runs off the main thread. When `serve` is
  unreachable, `lab_snapshot` returns an empty snapshot whose status `reason` says why, and
  the other commands return the error's message.
- The CLI and the MCP server are one binary, `ukiyo` on PATH. Its cargo crate is
  `src-tauri/cli/` (`ukiyo-cli`), and the binary is `ukiyo-cli`, because the app's own binary is
  already `ukiyo` in the same target folder. `ukiyo mcp` starts the MCP server.

### One USB owner

AGENTS.md says one process owns the USB connection. The design keeps that rule with a lock file
and a socket.

| File (in `~/Library/Application Support/com.nkmkdigital.ukiyo/`) | Purpose |
|---|---|
| `owner.lock` | An exclusive `flock`. The process that holds it is the owner. |
| `ukiyo.sock` | The owner's Unix socket, mode 0600. Newline-delimited JSON-RPC carrying `api` requests. |
| `serve.log` | The stderr of a `serve` that a client spawned. |

The folder is mode 0700. `UKIYO_DATA_DIR` overrides it (tests, a second dev setup); a spawned
`serve` gets the spawner's folder through that variable, so both always agree.

Only one kind of process ever owns the camera: `ukiyo serve`, the shared layer. The app, the
CLI and the MCP server are all clients of its socket, and none of them claims USB itself. So
the app and an agent's CLI work side by side: both see the same camera, the same plans and the
same live changes, and nothing ever hands the USB connection over. (Owner decision
2026-10-17. An earlier draft had the app own USB and take over from `serve`; that meant a USB
release and re-claim on every app start, which is the reconnect pattern protocol.md warns
about.)

Rules:

1. `serve` takes `owner.lock` before it claims the USB device, and keeps it until it has
   closed the session cleanly. The kernel drops a `flock` when a process dies, so a crash
   leaves no stale lock.
2. `serve` listens on `ukiyo.sock`. A client connects, sends requests, reads replies, and may
   `subscribe` to events (property changes, mode changes, connect and disconnect, plan
   applied, log lines). Every client gets every event, so the app's window shows a change an
   agent made, live, and the reverse.
3. A client of any kind (app, CLI, MCP) finds no owner when the connect fails and `owner.lock`
   is free. It then spawns `ukiyo serve` detached (`setsid`, so it outlives the client that
   started it, and no terminal signal reaches it). It waits up to 10 s for the socket, and
   never spawns a second time in one connect. While the lock is held but the socket does not
   answer (an owner starting, or shutting down), it only waits. `serve`, not the client,
   removes a stale socket, after it holds the lock: a client that removed it could delete
   the socket of an owner that had just bound it.
   - The probe takes and drops the lock at once. A `serve` that starts at that instant
     retries the lock for 1 s instead of giving up.
   - Spawns are spaced out per client: after one, the next is allowed after 1 s, then 2, 4
     ... up to 60 s. The spacing resets after a connection that lasted 60 s. A `serve` that
     dies at start therefore never becomes a loop of USB claims.
   - The app spawns its own executable with the `serve` argument (`main.rs` branches before
     any Tauri code), in development and in the bundle. The owner is then always the app's own
     build, with no second binary to keep in step. The CLI spawns itself the same way.
4. `serve` runs the owner thread (`owner.rs`): polling, the 3 s presence check, the 30 s
   reopen delay, one USB reset per connect attempt. All requests from all clients go through
   its one queue, writes first, so two clients can never interleave on the wire.
5. Lifetime: `serve` stays up while any client is connected. The app counts as a client for as
   long as it is open: it holds one connection, subscribed, and reconnects after a drop (1 s
   later, then backing off to 30 s, spawning under rule 3). `serve` exits 120 s after the last
   client leaves (`--idle-exit S` or `UKIYO_IDLE_EXIT` change it). SIGINT, SIGTERM and SIGHUP
   stop it the same way. At exit it removes the socket first, closes its clients, then
   disconnects cleanly (AUTO POWER OFF restored, session closed). It releases the lock only
   after the owner thread has ended. Plans live in `serve`, so a plan made in the CLI can be
   applied from the app's window, and the reverse.
6. Each connection names its `actor` (`app`, `cli`, `mcp:<client name>`) in `hello`. The
   session log carries it on every write, plan apply, label, note, export, connect switch and
   keep-awake line, and a write's read-back change group carries it as `actor`, so the
   window can show "changed by an agent". A connection without `hello` is `unknown`.
7. Nothing else touches USB. `tools/ptp/` scripts take `owner.lock` (`tools/ptp/owner_lock.py`,
   called before any USB access) and refuse to run while it is held. A script keeps the lock
   until it exits, so `serve` cannot start meanwhile; a client then reports that the lock is
   held but nothing answers.

In code, `Lab::start` takes the held `OwnerLock`, so no path opens USB without the lock.

### The socket protocol

One JSON-RPC 2.0 message per line (`core/src/serve/wire.rs`):

```text
-> {"jsonrpc":"2.0","id":1,"method":"hello","params":{"actor":"app","protocol":1}}
<- {"jsonrpc":"2.0","id":1,"result":{"protocol":1,"version":"0.1.0","pid":4242,"fake":true}}
-> {"jsonrpc":"2.0","id":2,"method":"subscribe"}
<- {"jsonrpc":"2.0","id":2,"result":null}
<- {"jsonrpc":"2.0","method":"event","params":{"event":"status","data":{...}}}
-> {"jsonrpc":"2.0","id":3,"method":"plan_create","params":{"set":{"AutoPowerOff":"OFF"}}}
<- {"jsonrpc":"2.0","id":3,"result":{"plan_id":"p-7f3a","steps":[...],...}}
<- {"jsonrpc":"2.0","id":4,"error":{"code":"bad_request","message":"..."}}
```

- `method` and `params` are an `api::Request`. `result` is the matching `api::Response`'s
  `result` (`null` for a method with none). `error` is an `api::Error`.
- `hello` and `subscribe` are the only methods outside `api`. `hello` with another `protocol`
  number is refused. After `subscribe`, the connection gets every `api::Event`.
- Replies can come out of order: `plan_apply` and `lab_export` wait on the camera on their own
  thread. Match replies by `id`.
- A client that falls 4096 lines behind is disconnected rather than allowed to stall the
  owner thread. It reconnects and reads a fresh snapshot.
- A request is sent once. If the connection drops before the reply, it fails with
  `unavailable` and is not retried, since it may have reached the camera.

Why `serve` and not "the CLI opens the camera itself" (the bm proposal): ten CLI calls in a
script would mean ten USB claims and ten session opens. That is close to the reconnect loop
that soft-locked the camera once (docs/protocol.md, power). One short-lived owner holds one
session for all of them.

### Where the binary lives

- The app bundle ships the binary as a Tauri `externalBin`:
  `Ukiyo.app/Contents/MacOS/ukiyo-cli` (stage 8), signed with the app. The app needs it for
  nothing: the app spawns its own executable as `serve` (rule 3 above), and the bundled CLI
  spawns itself. In a source checkout, `cargo build -p ukiyo-cli` builds it.
- The Homebrew cask links it onto PATH as `ukiyo` (`binary ... target: "ukiyo"`). The link
  follows app updates, since it points into the bundle.
- Planned, not built: a Settings button and `ukiyo install --prefix DIR` that symlink
  `~/.local/bin/ukiyo` to the bundled binary, for an install without Homebrew.
- MCP client config. With the cask, `ukiyo` is on PATH. From a source checkout, give the
  built binary's full path (`src-tauri/target/debug/ukiyo-cli` after `cargo build -p
  ukiyo-cli`):

  ```bash
  claude mcp add ukiyo -- ukiyo mcp
  claude mcp add ukiyo -- /path/to/ukiyo/src-tauri/target/debug/ukiyo-cli mcp
  ```

  ```json
  { "mcpServers": { "ukiyo": { "command": "ukiyo", "args": ["mcp"] } } }
  ```

  Add `"env": { "UKIYO_FAKE_CAMERA": "1" }` to work against the fake camera. A fake
  `serve` and a real one must not share a data folder, so give that setup its own
  `UKIYO_DATA_DIR` too.

## 3. CLI

### Command tree

```
ukiyo status                                   link, mode, model, battery, card, what works now
ukiyo camera  watch [--code C]... [--for 60s]  the change feed, live
              connect | disconnect             the Lab's connect switch
              export                           write a full dump to dumps/
ukiyo settings [QUERY] [--menu PATH] [--code C]... [--favourites] [--unnamed] [--unconfirmed] [--menus]
              set NAME=VALUE...                one or more settings, through a plan
              fav add|rm NAME...
ukiyo presets ls | show ID | save NAME [--from-snapshot ID | NAME...] | apply ID | rm ID
ukiyo snapshots ls | save [NAME] | diff A [B] | restore ID | rm ID
ukiyo recipes ls [QUERY] [--sensor S] [--sim F]       the library
              search [QUERY] [--sensor S] [--sim F] [--limit N]   the bundled catalogue
              show ID | edit ID FIELD=VALUE... | rm ID
              import (--text FILE|- | --url URL | cat:ID) [--name N] [--save]
              read [SLOT] [--compare ID]       read C1-C7 (RAW CONV. mode)
              write SLOT ID                    write a recipe to a slot, through a plan
ukiyo owner   get | set FIELD=VALUE...         copyright, author, prefix, device-name
ukiyo catalog show [CODE] [--model M] | label CODE [--name N] [--menu M] [--value K=LABEL]...
                                [--confidence C] [--notes T]
ukiyo prefs   get | set KEY=VALUE...           theme, brand-mark, app-icon, keep-awake
ukiyo plan    show ID | apply ID | discard ID
ukiyo log     show [--since T] | note TEXT [--group N]
ukiyo help modes [tether|rawconv|movie]        the USB mode walkthrough
ukiyo mcp                                      the MCP server on stdio
ukiyo serve [--idle-exit S]                    the headless owner (spawned on demand)
```

11 top-level commands for daily use, plus `mcp`, `serve`, `install` and `help`.
`ukiyo dev fake ACTION` exists only in a fake-camera build.

### Flags

| Flag | Effect |
|---|---|
| `--json` | Every command. One JSON document on stdout, nothing else. Errors too. `camera watch` is the exception: one JSON object per line. |
| `--quiet` | Every command. No progress lines on stderr. |
| `--dry-run` | Commands that write (`settings set`, `owner set`, `plan apply`). Build and print the plan; apply nothing. |
| `--yes`, `-y` | Commands that write. Apply a plan without the y/N prompt. Without a TTY and without `--yes`, a write prints the plan and exits 6. `--json` never prompts. |
| `--allow-unconfirmed` | `settings set`, `owner set`. Lets a plan include properties whose catalog confidence is not `confirmed`. |

Values: `NAME` is a catalog name (`AutoPowerOff`), a menu label (`"AUTO POWER OFF"`), or a code
(`0xd381`). `VALUE` is a value label (`OFF`) or a raw number (`3`). The core resolves both and
refuses an ambiguous match with the candidates.

### `--json` shapes

Every command returns one envelope:

```json
{ "ok": true, "data": { } }
{ "ok": false, "error": { "code": "needs_rawconv", "message": "Recipe slots read only in USB RAW CONV./BACKUP RESTORE mode.", "hint": "Unplug the cable, set NETWORK/USB SETTING > CONNECTION MODE to USB RAW CONV./BACKUP RESTORE, plug back in.", "walkthrough": "ukiyo help modes rawconv", "mode": "tether" } }
```

`data` uses the same shapes as the MCP `structuredContent` in section 4. Key shapes:

```jsonc
// status (link, mode, capture and screen use the window's names: "rawConv", "cardReader")
{ "link": "connected", "reason": null, "model": "X-T5", "firmware": "x.yy", "mode": "tether",
  "capture": "stills", "screen": "shooting", "battery": null, "card": null, "lens": null,
  "owner": "serve", "pid": 4242, "fake": false, "keep_awake": true, "keep_awake_active": true,
  "available": { "settings": true, "slots": false, "movie_props": false },
  "notices": [] }
// notices, in tether mode only: what the person must do at the camera first. Each clears on
// its own once D22E reads 1 again or D307 reads STILL.
//   { "id": "menu", "text": "Close the camera menu: half-press the shutter button on the camera." }
//   { "id": "playback", "text": "Leave playback: press the playback button or half-press the shutter button on the camera." }
//   { "id": "movie", "text": "Set the STILL/MOVIE switch to STILL for stills settings and recipes." }

// settings
{ "rows": [ { "code": "0xd381", "name": "AutoPowerOff", "menu": "SET UP > POWER MANAGEMENT > AUTO POWER OFF",
              "value": 1, "value_label": "5 MIN", "options": [ { "value": 1, "label": "5 MIN" } ],
              "writable": true, "confidence": "confirmed", "available": true } ],
  "total": 1, "has_more": false }
// settings --menus
{ "menus": [ { "path": "SET UP > POWER MANAGEMENT", "depth": 1, "count": 1 } ] }

// camera watch: one object per line, a status first, then every change group
{ "event": "status", "data": { ...the status shape } }
{ "event": "changes", "data": { "id": 12, "at": 1792266821137.0, "source": "camera",
  "changes": [ { "code": "0xd381", "name": "AutoPowerOff", "old": 1, "new": 3,
                 "old_label": "5 MIN", "new_label": "OFF" } ] } }

// a plan (settings set, presets apply, snapshots restore, recipes write, owner set)
{ "plan_id": "p-7f3a", "expires_in_s": 300, "requires": { "mode": "tether", "capture": "stills" },
  "steps": [ { "code": "0xd381", "name": "AutoPowerOff", "menu": "SET UP > POWER MANAGEMENT > AUTO POWER OFF",
               "before": "5 MIN", "after": "OFF", "before_value": 1, "after_value": 3,
               "confidence": "confirmed", "gate": "ok" } ],
  "skipped": [ { "code": "0x....", "name": "...", "value": "...", "reason": "needs_movie", "message": "..." } ],
  "blocked": [ { "code": "0x....", "name": "...", "value": "...", "reason": "unconfirmed_property", "message": "..." } ],
  "allow_unconfirmed": false }

// an applied plan
{ "plan_id": "p-7f3a", "applied": 1, "failed": 0, "not_attempted": 0, "undo_plan_id": "p-91c0",
  "steps": [ { "code": "0xd381", "name": "AutoPowerOff", "wanted": "OFF", "readback": "OFF",
               "wanted_value": 3, "readback_value": 3, "ok": true, "result": "OK", "error": null,
               "attempts": 1 } ],
  "side_effects": [ { "code": "0x....", "name": "...", "old": 1, "new": 2, "old_label": "...", "new_label": "..." } ] }
```

The CLI's write commands wrap these as `data: { "plan": {...}, "result": {...} }`, with
`result: null` when nothing was applied (`--dry-run`, no `--yes`, nothing to change).

### Exit codes

The error codes are section 5's, plus `usage` (exit 2), `unavailable` (no `serve` answered,
exit 1) and `not_built` (a flag for a later stage, exit 1).

| Code | Meaning |
|---|---|
| 0 | Done. |
| 1 | Internal or I/O error. |
| 2 | Usage error: bad flag, unknown name, ambiguous value. |
| 3 | No camera: not connected, asleep, unplugged. |
| 4 | Wrong camera state: `needs_*`, `menu_open`, `in_playback`, `busy`. |
| 5 | Refused by a gate: unconfirmed, read-only, `never_write`, out of range, a label without evidence (`no_evidence`) or with camera text (`private_text`). |
| 6 | Not applied: declined at the prompt, `--dry-run`, no TTY without `--yes`, plan stale or expired. A `--dry-run` with `--json` prints `ok: true` and the plan, and still exits 6. |
| 7 | Applied in part: at least one step failed (`readback_mismatch`, `busy` ...). The output lists each step. |

### Example sessions

```console
$ ukiyo status
X-T5 · firmware 9.99 · tether, stills, shooting · owner: serve (pid 4242)
Works now: settings, presets, snapshots, owner strings, Lab
Needs RAW CONV.: recipe slots (ukiyo help modes rawconv)
Needs MOVIE: movie settings (ukiyo help modes movie)

$ ukiyo status          # the camera in playback
X-T5 · firmware 9.99 · tether, stills, playback · owner: serve (pid 4242)
Leave playback: press the playback button or half-press the shutter button on the camera.
Works now: settings, presets, snapshots, owner strings, Lab
...

$ ukiyo settings power off
0xd364  CustomAutoPowerOff  0      likely
    range: 0 to 255, step 1
0xd381  AutoPowerOff        2 MIN  confirmed  SET UP > POWER MANAGEMENT > AUTO POWER OFF
    options: 5 MIN (1), 2 MIN (2), OFF (3), 1 MIN (4), 30 SEC (5), 15 SEC (6)

$ ukiyo camera watch
15:53:35  status  connected, X-T5, tether, stills, shooting
15:53:36  camera #15  0xd171 FocusPosition  119 -> 126

$ ukiyo settings set "AUTO POWER OFF=OFF" --dry-run
Plan p-7bfb (expires in 5 min), needs tether + stills
  AutoPowerOff  5 MIN -> OFF  confirmed
Not applied (--dry-run). Apply it: ukiyo plan apply p-7bfb --yes

$ ukiyo plan apply p-7bfb --yes
Plan p-7bfb (expires in 5 min), needs tether + stills
  AutoPowerOff  5 MIN -> OFF  confirmed
AutoPowerOff  OFF  read back OFF  OK
Applied 1 of 1. Undo: ukiyo plan apply p-cd29

$ ukiyo settings set GrainEffect=2
ukiyo: 0xd023 GrainEffect is not confirmed in the catalog (likely); its meaning may be wrong. Pass allow_unconfirmed (--allow-unconfirmed) to write it
$ echo $?
5

$ ukiyo recipes write C3 cat:<recipe-id> --json
{"ok":false,"error":{"code":"needs_rawconv", ...}}
$ echo $?
4

$ ukiyo snapshots save before-trip && ukiyo snapshots diff before-trip
12 settings differ from the camera ...
```

### The read-only commands as built (stage 3)

`cli/src/` in `src-tauri/`: `main.rs` (clap), `cmds.rs`, `conn.rs` (the socket client),
`out.rs` (envelope, errors, exit codes). Tests: `cli/tests/cli.rs`.

- **Camera reads go through `serve`.** `status`, `settings` and `camera watch` connect to
  `ukiyo.sock` as actor `cli` and spawn `serve` (this binary) when none runs, so they work
  with the app open or closed. A command that just started `serve`, or finds a connect in
  progress, waits up to 8 s for the link to settle. Then it disconnects, and `serve` idles out
  120 s later. The CLI refuses a data folder whose socket path is longer than macOS allows
  (103 bytes), with exit 2.
- **`status` exits 0** whenever `serve` answered, camera or not; `link` and `reason` say where
  the camera stands. `battery` and `card` show the catalog's BatteryLevel and CaptureRemaining
  rows when the camera reports them (raw values; both are `likely`, and the X-T5 reads them
  in movie mode only). `lens` stays `null`: D36D carries the lens name with its serial.
- **`settings` needs tether mode** (`needs_tether`, exit 4), except with `--code`, which reads
  any connected mode, like the Lab's table. No camera is exit 3. `QUERY` words must all appear
  in the code, the name (CamelCase split into words), the menu path or the notes; keywords
  ("eye focus" finding FACE/EYE DETECTION) need catalog keywords, which do not exist yet.
  `--menu` is a path prefix. `options` appear when 10 rows or fewer match. `--favourites`
  fails with `not_built` until the library (stage 6).
- **`camera watch`** subscribes and prints a status line, then every change group, until
  Ctrl-C (a clean exit 0) or `--for`. A status line repeats when the link, mode, capture or
  screen changes. `--code` filters the output; it does not pin, because the owner's pinned set
  is shared and would replace the window's pins. Per-client pins come with stage 4.
- **Local reads need no `serve`.** `catalog show` reads the catalog folder (as `serve` does:
  `UKIYO_CATALOG_DIR`, the repo's `catalog/` in a dev build, else the data folder). Its model
  is `--model`, else the camera's when a `serve` already runs (it never spawns one), else the
  only catalog there. `CODE` is a code or a catalog name. `log show` reads
  `sessions/*.jsonl`: the newest session, or with `--since` (`10m`, `2h`, `2026-10-17`,
  `"2026-10-17 14:30"`, RFC 3339) every line since then; `--json` gives each line as written,
  plus `session`. `help modes` prints `modes.rs`.

### The write commands as built (stage 4)

`cli/src/writes.rs`. Tests: `cli/tests/cli.rs` (fake camera), `core/src/plan/tests.rs`.

- **`settings set NAME=VALUE...`** makes a plan in `serve`, prints it, and applies it after the
  y/N prompt or `--yes`. `NAME` is a catalog name, a menu path or its last part (`"AUTO
  POWER OFF"`), a `0x` code, or four hex digits; any case. `VALUE` is a value label (any
  case) or a raw number (`0x` hex too); a string property takes the text as given. An
  unknown or ambiguous name or value fails the whole plan (exit 2) with the valid values in
  the `hint`. `set` is the one word `settings QUERY` cannot search for.
- **A plan with no step** fails with its first blocked or skipped reason (`unconfirmed_property`
  exit 5, `needs_tether` exit 4 ...). When every value is already set, it prints "Nothing to
  change." and exits 0.
- **`plan show|apply|discard ID`** find plans in a running `serve` and never start one; with no
  `serve` the answer is `plan_expired`. A plan applies once.
- **`owner get`** reads the four owner strings in tether mode. **`owner set FIELD=VALUE`** is
  `settings set` on their catalog names. None of the four is confirmed, so `owner set` needs
  `--allow-unconfirmed`:

  | Field | Catalog name | Code | Confidence |
  |---|---|---|---|
  | `copyright` | Copyright | 0xd215 | likely |
  | `author` | CopyrightAuthor | 0xd237 | guess |
  | `prefix` | FileNamePrefix1 | 0xd365 | likely |
  | `device-name` | DeviceName | 0xd20b | likely |

- **`camera disconnect`** sends the Lab's switch and waits (up to 5 s) until the owner reports
  `stopped`; with no `serve` running it does nothing. **`camera connect`** spawns `serve` when
  needed and turns the switch on; the owner then follows its 3 s and 30 s rules.
  **`camera export`** writes the Lab dump and prints its path.
- **`log note TEXT [--group N]`** writes into the running session's log. It never starts
  `serve` (a note is no reason to claim the camera), so with none running it fails with
  `unavailable`.
- Plans live in `serve`, which exits 120 s after its last client. A plan not applied by then is
  gone (`plan_expired`), even inside its 5 minutes.
- The window's Lab write box makes a one-step plan with `allow_unconfirmed` (it is the mapping
  tool), shows its step, and applies it on the second click. Its writes log as actor `app`.

## 4. MCP

Tools only. No resources and no prompts: camera state is live and polled, so it is not stable
addressable context, and the catalogue is too large to read whole. The server is stdio; stdout
carries only MCP messages and logs go to stderr.

11 tools. A fake-camera build adds a 12th, `fake_camera`.

| # | Tool | readOnly | destructive | idempotent | openWorld |
|---|---|---|---|---|---|
| 1 | `camera_status` | yes | no | yes | no |
| 2 | `camera_watch` | yes | no | no | no |
| 3 | `settings_read` | yes | no | yes | no |
| 4 | `camera_plan` | yes | no | no | no |
| 5 | `camera_apply` | no | yes | no | no |
| 6 | `library_search` | yes | no | yes | no |
| 7 | `library_read` | yes | no | yes | no |
| 8 | `library_save` | no | no | yes | no |
| 9 | `library_delete` | no | yes | yes | no |
| 10 | `recipe_parse` | yes | no | yes | yes |
| 11 | `catalog_label` | no | no | yes | no |

Stable ids used across tools: `preset:<slug>`, `snapshot:<slug>`, `recipe:<slug>` (the user
library), `cat:<id>` (the bundled catalogue, `docs/recipes.md`), `slot:C1` to `slot:C7`,
`favourites`, `prefs`, `history`. Property codes are `0x` plus four lowercase hex digits.

### 1. `camera_status`

Reports the connection, the USB mode, and which features work in that mode.

- Best for: the first call in any camera task, and after any `needs_*` error.
- Input: `{ history?: boolean }`.
- Returns: link, reason, model, firmware, mode, capture, screen, battery, card space, lens,
  the owner (`serve`, its pid, fake or real camera), and `available` per feature with the missing requirement.
  With `history: true`: shot counts per day and month, lenses seen, the connect log.
- Not for property values: use `settings_read`.

### 2. `camera_watch`

Returns the change groups the camera reported since a cursor. Waits for new ones up to a limit.

- Best for: Lab mapping ("turn the dial now"), and checking a person's change on the body.
- Input: `{ since?: number, codes?: string[], wait_s?: number /* 0-60, default 10 */ }`.
  `codes` are pinned (read every cycle) for the duration of the call.
- Returns: `{ cursor, groups: [{ id, at, source: "camera"|"write", changes: [{ code, name?, old, new }] }], has_more }`.
- Follow up with `catalog_label` to name what moved.

### 3. `settings_read`

Reads live settings with their catalog names, menu paths and value labels.

- Best for: "what is X set to", finding a setting by name or meaning, browsing a menu.
- Input: `{ query?: string, codes?: string[], menu?: string, filter?: "all"|"favourites"|"unnamed"|"unconfirmed", view?: "rows"|"menus", limit?: number /* default 25 */ }`.
- Returns: rows `{ code, name, menu, value, value_label, options?, writable, confidence, available, error? }`,
  a `total` and `has_more`. `view: "menus"` returns the menu tree with row counts instead.
- Query tips: `query` matches names, menu paths and keywords ("eye focus" finds FACE/EYE
  DETECTION SETTING). `options` appear when the result has 10 rows or fewer.
- Not for recipe slots: use `library_read` with `slot:C1`.
- Follow up with `camera_plan` to change a value.

### 4. `camera_plan`

Builds a change plan against the live camera. Writes nothing.

- Best for: every camera write. One property, a preset, a snapshot restore, owner strings, a
  recipe into a slot.
- Input, exactly one source:
  - `{ set: { "<name|code>": "<label|raw>" } }`
  - `{ preset: "preset:<slug>" }`
  - `{ snapshot: "snapshot:<slug>" }`
  - `{ recipe: "recipe:<slug>"|"cat:<id>", slot: "C1".."C7" }`

  plus `allow_unconfirmed?: boolean` (default false).
- Returns: `{ plan_id, expires_in_s, requires, steps, skipped, blocked }` (shape in section 3).
  A plan with blocked steps can still be applied; blocked steps are not written.
- A plan of a snapshot is also its diff against the camera.
- Follow up with `camera_apply` and the `plan_id`, after the user agrees to the steps.

### 5. `camera_apply`

Applies a plan made by `camera_plan`. Writes to the camera, reads every value back, and logs it.

- Input: `{ plan_id: string }`.
- Returns: the applied-plan shape in section 3, with `undo_plan_id`.
- Refuses with `plan_stale` when any planned code changed since the plan, and `plan_expired`
  after 5 minutes or a restart of `serve`. Make a new plan.
- Stops at the first failed step; later steps are `not_attempted`. Apply `undo_plan_id` to
  revert what was written.

### 6. `library_search`

Searches what ukiyo stores on this Mac and the bundled recipe catalogue.

- Best for: finding a preset, snapshot or recipe by name, film simulation or sensor.
- Input: `{ kind: "recipe"|"catalogue"|"preset"|"snapshot"|"favourite", query?: string, sensor?: string, film_sim?: string, limit?: number /* default 20 */ }`.
- Returns: `{ items: [{ id, kind, name, summary, source?, credit? }], total, has_more }`.
  Catalogue items carry their credit and post link.
- Follow up with `library_read` for the full item.

### 7. `library_read`

Reads one stored item in full, or the diff between two.

- Input: `{ id: string, compare_to?: string }`. `id` takes every id form above, including
  `slot:C3`, `prefs` and `history`.
- Returns: the item: recipe settings, notes and warnings; preset properties; snapshot values
  with mode coverage; slot contents; preferences. With `compare_to`: `{ added, removed, changed: [{ code, name, a, b }] }`.
- A `slot:` read needs RAW CONV. mode. It selects the slot through D18C and restores D18C
  after (docs/protocol.md, "Recipe slots"); both writes go to the session log.
- For a diff against the live camera, use `camera_plan` with `snapshot`.

### 8. `library_save`

Creates or replaces one item on this Mac. Never writes to the camera.

- Input: `{ kind: "recipe"|"preset"|"snapshot"|"favourite"|"prefs"|"note"|"dump", name?: string, item?: object, from?: string }`.
  - `snapshot` without `item` captures the camera now; `dump` writes a full Lab dump.
  - `recipe` with `from: "cat:<id>"` copies a catalogue recipe into the library; `item` holds
    the editor's fields, checked against the slot ranges.
  - `favourite` adds a setting by name or code; `prefs` takes `{ theme, brand_mark, app_icon, keep_awake }`.
  - `note` adds a line to the session log, with an optional change group.
- Returns: `{ id, kind, replaced: boolean, warnings }`.
- A replaced item keeps its previous version in `library/.trash/`.

### 9. `library_delete`

Deletes one stored item. It moves to `library/.trash/` for 30 days.

- Input: `{ id: string }`. Refuses `cat:`, `slot:`, `history` and `prefs` ids.
- Returns: `{ id, trashed_at }`.

### 10. `recipe_parse`

Parses recipe text or a recipe post's URL into slot settings. Stores nothing.

- Best for: importing a recipe someone pasted or linked.
- Input: `{ text?: string, url?: string }`, exactly one.
- Returns: `{ name?, source_url?, settings, notes, warnings }`, the parser's shape
  (`recipes::Parsed`). One review step before anything is kept.
- Follow up with `library_save` (`kind: "recipe"`) to keep it, or `camera_plan` with the saved id
  to write it to a slot.

### 11. `catalog_label`

Edits one property's entry in the model's catalog: name, menu, value labels, confidence, notes.

- Best for: Lab mapping after `camera_watch` showed a code move.
- Input: `{ code: string, name?: string, menu?: string, values?: { "<raw>": "<label>" }, confidence?: "confirmed"|"likely"|"guess", notes?: string }`.
  An empty string clears a field; an empty label removes that value.
- Returns: the updated entry and the catalog path.
- Refuses `confidence: "confirmed"` unless this session's log shows the code changing (a
  `camera` change group or a read-back write). The catalog drives write-gating, so the evidence
  rule keeps an agent from un-gating a property by labelling it.
- Refuses text that matches a value read from this camera (serials, owner strings), so the
  catalog stays shareable.

### `fake_camera` (fake builds only)

Drives the fake camera: `{ action: "openMenu"|"closeMenu"|"movie"|"stills"|"powerOff"|"powerOn"|"unplug"|"nudge"|"autoOn"|"autoOff" }`.
The server lists it only when `UKIYO_FAKE_CAMERA=1`.

### The MCP server as built (stage 5)

`cli/src/mcp.rs` (the server and one handler per tool), `cli/src/mcp_tools.rs` (the listing).
Tests: `cli/tests/mcp.rs` drives `ukiyo-cli mcp` over stdio against a fake-camera `serve`;
the surface guard in `mcp_tools.rs` diffs the listing against
`cli/tests/surface/mcp_tools.json` (after a deliberate change:
`UKIYO_UPDATE_SURFACE=1 cargo test -p ukiyo-cli surface`).

- **No SDK.** JSON-RPC 2.0 over stdio, one message per line, as QuadCam's server does:
  `initialize`, `ping`, `tools/list`, `tools/call`. Protocol versions 2024-11-05 to
  2025-11-25. One request at a time; a `camera_watch` holds the line for its `wait_s`.
- **A client of `serve`, actor `mcp:<client name>`** (the `clientInfo.name` from
  `initialize`; `mcp` without one). It never opens USB, and spawns `serve` when none runs,
  under the CLI's rules. The connection stays open between calls, so a plan lives its full 5
  minutes while the user decides. It drops after 300 s with no call (`UKIYO_MCP_IDLE`
  changes it), so `serve` can idle out; the next call connects again.
- **Results:** a text summary in `content`, the section 3 shape in `structuredContent`. A
  failure is `isError: true` with `structuredContent.error` = `{ code, message, hint?,
  walkthrough?, mode? }`, and the same in the text. An unknown tool is JSON-RPC error
  -32602; bad arguments are `bad_request`.
- `fake_camera` is listed when the server's own environment has `UKIYO_FAKE_CAMERA=1`.
- **`camera_watch`:** without `since`, it returns only groups that arrive during the call.
  It returns at most 50 groups; `has_more` and `cursor` page through the rest. A cursor past
  the newest group means `serve` restarted: the result has `reset: true` and starts from
  now. `codes` filters; it does not pin, for the same reason as `camera watch --code`
  (section 3). It also returns `link`.
- **`catalog_label`** returns `{ model, path, code, entry }`. Its two rules live in the
  core (`evidence.rs`) and apply to every actor except `app`:
  - `no_evidence`: `confirmed` needs a change group for the code in this session, from
    the Lab's feed or the session log.
  - `private_text`: refuses a name, menu, note or value label that contains a string
    the camera reports. That is each whole string value, plus each word of 5 or more
    characters with a digit in it.

  The window's label panel is the person mapping by hand, so these rules do not apply to it.
- `camera_apply` reports a partial failure as a normal result (`failed` > 0), not
  `isError`: the steps say what was written.

### The library as built (stage 6)

`core/src/library/` (`mod` the store, `prefs`, `snapshot`, `history`), `cli/src/library.rs`,
the `library_*` handlers in `cli/src/mcp.rs`, and the window's Hub and Settings pages.

- **Storage**, in the data folder only (never in the repo):

  | Path | Holds |
  |---|---|
  | `settings.json` | preferences: theme, brand mark, app icon, keep-awake, in-use minutes, menu close |
  | `library/presets/<slug>.json` | a preset: settings by name or code, to labels or raw values |
  | `library/snapshots/<slug>.json` | a snapshot: model, firmware, mode, every readable value |
  | `library/favourites.json` | favourite settings, by code |
  | `library/.trash/` | deleted and replaced presets and snapshots, 30 days |
  | `library/log.jsonl` | every save, delete and restore, with its actor |
  | `history/<camera id>.json`, `history/.salt` | per-camera connects, shot counters, lenses |

  Every file has a `schema`. Writes hold `library/.lock` and rename a temp file into place.
  `settings.json` is read-modify-write and keeps keys it does not know.
- **Camera id:** the SHA-256 of the serial's digest with a per-Mac random salt, cut to 16
  hex digits. The serial is never stored or sent; `Status.camera_id` carries the id.
- **History:** shot counts come from `TotalShotCount` (catalog `likely`), read in tether
  stills mode only. `lenses` stays empty until the lens name is split from its serial (D36D).
- **CLI:** `presets ls|show|save|apply|rm`, `snapshots ls|show|save|diff|restore|rm`,
  `settings fav add|rm`, `settings --favourites`, `prefs get|set`, `trash ls|restore`,
  `status --history`. `presets save NAME SETTING...` takes the settings' values now;
  `--set NAME=VALUE` and `--from-snapshot ID` are the other sources. `apply` and `restore`
  are plans, with the usual `--dry-run`, `--yes` and `--allow-unconfirmed`.
- **MCP:** `library_search` (kinds `preset`, `snapshot`, `favourite`, `trash`),
  `library_read`, `library_save` (kinds `preset`, `snapshot`, `favourite`, `prefs`, `note`,
  `dump`; `from: trash:<name>` restores), `library_delete`; `camera_status {history}`,
  `settings_read {filter: favourites}`, `camera_plan {preset}` and `{snapshot}`.
- **The window:** Tauri commands `library_search`, `library_read`, `library_save`,
  `library_delete`, `prefs_set` forward to `serve`; `prefs_get` reads `settings.json` in the
  app, so the theme is ready before `serve`. The Hub reads the camera's properties and
  `history`; Settings builds its menu tree from the catalog's menu paths, writes one setting
  as a plan of confirmed codes, and shows a preset or snapshot plan before it writes. The
  browser preview (`?camera=`) keeps the fixtures.
- **Preferences migration:** at its first start, the app sends the old local-storage
  `ukiyo.appearance` values to `prefs_set`, then removes the key.

### The recipes as built (stage 7)

`core/src/recipes/` (`slot` the slot fields and their encoding, `fetch` a post by URL,
`bundle` the catalogue compiled in), `core/src/library/recipes.rs`, the slot parts of
`lab.rs` and `plan.rs`, `cli/src/recipes.rs`, the recipe handlers in `cli/src/mcp.rs`, and
the window's Recipes page (`app/src/lib/recipes.ts`, `app/src/views/PlanCard.tsx`).

- **Library recipes:** `library/recipes/<slug>.json`, id `recipe:<slug>`: `name`,
  `settings` (the parser's, in menu units), `notes`, `sensor`, `source` (`from: cat:<id>`,
  `url`, `credit`), `comment`. They trash and restore like presets. A save never refuses a
  setting a slot cannot hold; it returns a warning for it, and a slot plan of that recipe
  fails with `value_out_of_range`.
- **The catalogue** (`catalog/recipes/catalog.json`, about 1 MB) is compiled into the core,
  so the CLI, MCP and `serve` search it with no file to find. `library_search {kind:
  catalogue}` sorts by popularity; `sensor` and `film_sim` filter both recipes and the
  catalogue.
- **`recipe_parse {url}`** fetches one page (20 s limit, 8 MB, a User-Agent naming ukiyo).
  A page on a catalogue site goes through that site's adapter; any other host gets the generic
  reading: the recipe blocks inside its `<article>`, or the whole page. Further recipes in the
  post come back as `others`. The window runs it in its own process, off the main thread.
- **Slot reads:** `library_read {id: slot:C3}` or `{id: slots}` (CLI `recipes read [SLOT]`)
  go through `serve`. The owner selects each slot through D18C, reads D18D and D18E-D1A4,
  and restores D18C in one request, so the poll never sees another slot selected. The result
  is the slot's name, its settings in recipe form, warnings for values no setting means,
  each raw field, and `matches`: library recipes with the same normalised settings.
  `compare_to` with a recipe id gives a field-by-field `recipediff`. Outside RAW CONV. mode
  it fails with `needs_rawconv`.
- **Slot write plans:** `camera_plan {recipe, slot}` (CLI `recipes write SLOT ID`) encodes
  the recipe (`recipes::slot::encode`), reads the slot for every step's `before`, and gates
  each field. A slot gets the whole recipe or nothing: with any field blocked, every step is
  blocked. Absent settings take the camera default. Fields that do not apply are left out:
  dynamic range and the tones while D-range priority is on, Color under a monochrome film
  simulation, Monochromatic Color under a colour one. D18E, D18F, D1A3 and D1A4 are never
  written; D1A5 belongs to the live block and is never touched. The name goes last, cut to
  25 characters. Apply reads the slot again for the stale check, then one owner request
  selects the slot, writes and reads back each step, stops at the first failure, and restores
  D18C. The undo plan is a slot plan back to the values before (grain 6, a default slot's
  OFF, is written as 1).
- **Gated and dependent settings** are catalog data, not code:
  `requires` and `resets` per code (docs/catalog.md). Every plan, live or slot, is ordered by
  `plan::sequence`: a write that needs another code at some value comes after that code's
  write when the plan sets such a value, and before it otherwise; a code the camera resets
  with another write comes after it. When a prerequisite does not hold at a write's turn,
  the plan sets it for that write (`purpose: prerequisite`), puts it back at the end
  (`restore`), and also puts back anything the prerequisite made the camera reset. Each step
  lists `resets`, what the camera changes by itself. On the X-T5:

  | Rule | Codes | Seen |
  |---|---|---|
  | Monochromatic Color needs a monochrome film simulation (temporarily MONOCHROME) | D104, D031 on D001 | 0x201C on the X-T5 |
  | A monochrome film simulation sets Color to 0 | D001 resets D008 | ACROS on the X-T5; assumed for the others |
  | HEIF sets Clarity to 0 | D1B2 = 18 resets D032 | the X-T5 |
  | The slot's Monochromatic Color needs a monochrome slot film simulation | D193, D194 on D192 | ptp-recipes, X-H2 |
  | The slot's dynamic range and tones need D-range priority off | D190, D19D, D19E on D191 = 0 | ptp-recipes, X-H2 |

  The undo plan is planned the same way, toward the values before the plan, so it carries its
  own prerequisites. The stale check compares each code with its value before the plan.
- **The window:** the Recipes page reads the library (`library_search`, `library_read`),
  reads all seven slots when the camera arrives in RAW CONV. mode or on "Read all slots",
  saves through `recipe_save`, imports a link through `recipe_parse {url}`, and shows a slot
  plan in a plan card before "Write to camera". The card offers to plan again with
  unconfirmed settings included, since no slot field is confirmed yet. An edited, imported or
  slot-read recipe is saved to the library before it is planned. The browser preview keeps
  the fixtures.
- **Fake camera:** in RAW CONV. mode the fake keeps seven slots (C1 and C2 hold made-up
  recipes), and answers 0x201C as above, plus a slot grain write of 6. In tether mode a
  monochrome film simulation sets Color to 0 and opens Monochromatic Color's range; HEIF
  sets Clarity to 0.
- **Left for later:** confirming the slot fields on the X-T5 (until then slot plans need
  `allow_unconfirmed`), the slot name's real length limit, slot sets, recipes from a photo,
  and running the Recipes page against the camera.



| bm proposal | This design | Reason (mcp-tool-design) |
|---|---|---|
| Resources for camera state and the catalogue | Tools `camera_status` and `library_search` | Tools first; live state is not stable context; a large corpus wants search then read. |
| The CLI's verbs as MCP tools (about 25) | 11 tools | Shrink the surface; tools that return the same thing with other arguments are one tool. |
| `props set`, `presets apply`, `snapshot restore`, `recipes write`, `owner set` | One `camera_plan` + `camera_apply` pair | Inspect, render, validate, apply for high-safety writes; one gate for every write. |
| `props ls` and `props get` | One `settings_read` | Same result, different arguments. |
| `presets/snapshots/recipes ls|show` | `library_search` + `library_read` with stable ids | Search then read; ids the caller already has. |
| `snapshot diff` | `library_read` with `compare_to`, or `camera_plan` | No separate tool for a view of the same data. |
| `props watch` (streaming) | `camera_watch` with a cursor and a bounded wait | A tool call must return; the cursor allows follow-up reads. |
| `owner get|set` | CLI sugar only; MCP uses `settings_read` and `camera_plan` | Owner strings are properties; no sibling tool. |
| The CLI opens the camera itself when the app is closed | The CLI spawns `ukiyo serve`, one owner for many calls | Not from the skill: the reconnect rule in docs/protocol.md. |

## 5. Safety

The core enforces every rule here. The front ends only show them.

### Write-gating

- A plan step is writable only when all hold:
  - the descriptor says writable, and the code reads in the current mode (owner.rs `write`)
  - the catalog entry is `confirmed`
  - the value is in the descriptor's enum or range, and in the catalog's `values` if it has any
- `allow_unconfirmed` (CLI `--allow-unconfirmed`) lifts only the `confirmed` rule. MCP takes
  the same per-plan argument, with no start flag.
- **Never write.** A catalog entry with `never_write` (docs/catalog.md) is a command or status
  code: format, resets, save and power off, the release gesture, the PTP error. A plan that
  names one fails as a whole with `never_write`, before any other gate; a prerequisite or
  restore on one is blocked with that reason; apply checks again before the first write. No
  flag lifts it, `allow_unconfirmed` included, and the Lab's write box obeys it too. The owner's
  own half-press (D208 + InitiateCapture, to close a menu or keep the camera awake) is not a
  plan and still runs. The X-T5 marks D17B, D17F, D208, D21C, D21D and D230.
- The catalog's `values` gate applies to `confirmed` entries only. An unconfirmed entry's value
  list is itself a guess, and the Lab must be able to write values nobody has labelled yet.
- A step whose value is already set is `skipped` with reason `unchanged`. A code the camera
  does not read now is `skipped` with the mode or switch position it needs.
- The Lab's write box in the window is the one place that writes unconfirmed codes without a
  flag. That is the mapping tool; agents use the flag instead.
- Recipe slot fields (D18E-D1A5) are not yet confirmed by a write. Until they are, slot writes
  need `allow_unconfirmed`. A slot plan with any field blocked writes nothing.

### Plan, then apply

- Every camera write is a plan first. That includes one property, presets, snapshot restores,
  owner strings and recipe slot writes.
- A plan holds each step's `before`. Apply re-reads them and refuses with `plan_stale` if any
  changed.
- Plans live in the owner for 5 minutes and die with it. A plan applies once. Apply on a camera
  that cannot take writes now (`not_connected`, `menu_open` ...) keeps the plan for a retry;
  a stale plan is dropped.
- `--dry-run` stops after the plan. A TTY prompt or `--yes` is needed to apply.
- Steps run in plan order, which follows the catalog's `requires` and `resets`: a
  prerequisite first, set for the time of a write and put back after; a setting the camera
  resets after the write that resets it ("The recipes as built", section 4). A slot write is:
  select the slot (D18C), write the fields and name (D18D), restore D18C, in one owner
  request. Apply stops at the first failure and returns an undo plan. The undo plan puts back
  every code a step wrote that the camera answered OK, a read-back mismatch included, and is
  ordered the same way.
- A write the camera answers 0x2019 (busy) is sent again on the same session after 100, 200,
  400, 800 and 1600 ms (3.1 s in all), then fails with `busy`. After a half-press release the
  X-T5 answers writes busy for about 0.2 s (docs/protocol.md). The owner never reconnects to
  retry.
- A write the camera answers 0xA001 makes the owner read D22E. At 1 (a shooting-side menu
  tab or the Q menu) it sends one half-press, which closes the menu, and sends the write once
  more; the step and its change group carry `closed_menu`, and the window says "Closed the
  camera menu to apply the change". A second 0xA001 fails with `menu_open`. At 3 (SET UP or
  NETWORK/USB SETTING) it fails with `menu_open`; at 2 (playback), with `in_playback`. Both
  lock every PTP operation, the half-press included, so there is no half-press and no retry.
  `close_menu_to_write` in `settings.json` (default on) turns the half-press off.

### Read-back and the session log

- The owner reads back every write at once (owner.rs). Apply reports `wanted` and `readback`
  per step, and flags a step whose read-back differs (the camera enforces its own rules: ISO
  125 forces D-Range 100).
- After the last step, apply reports `side_effects`: other codes that changed during the apply
  that the poll had seen by the time the last step was read back. A camera rule that lands a
  cycle later is not in it.
- Every request that writes goes to `sessions/*.jsonl`, with `actor`: `app`, `cli` or
  `mcp:<client name>`. That includes plans applied, library saves, labels and notes. Lines:
  `write` (`actor`, `plan`, the owner's `outcome` with `rc` and `attempts`), `apply`
  (`applied`, `failed`, `not_attempted`, `undo`; or `refused` with the code), `label`,
  `note`, `export`, `connect`, `keep_awake`.

### Typed errors

Each error has `code`, `message`, `hint`, and `walkthrough` where a mode switch fixes it. The
walkthrough is the USB mode guidance: `ukiyo help modes <mode>` in the CLI, and
the Connect page's guide in the window.

| Code | When | Hint |
|---|---|---|
| `not_connected` | No camera on the bus | Plug in the cable and turn the camera on. |
| `camera_asleep` | Auto power off took the camera off USB | Turn the camera on. Keep-awake (on in Settings) resets its timer while Ukiyo is in use. |
| `busy` | The camera answers 0x2019 | Wait; try again in a few seconds. |
| `menu_open` | D22E = 3, or 0xA001 after the one half-press | Close the SET UP or NETWORK/USB SETTING menu on the camera. |
| `in_playback` | D22E = 2 | Leave playback on the camera: press the playback button. |
| `needs_tether` | Live settings in another mode | Switch to USB TETHER SHOOTING FIXED. |
| `needs_rawconv` | Slot contents in another mode | Switch to USB RAW CONV./BACKUP RESTORE. |
| `needs_stills`, `needs_movie` | A property of the other capture mode | Set the STILL/MOVIE switch. |
| `owner_conflict` | A process outside ukiyo holds the USB device | Quit that process (PTPCamera is handled by the owner). |
| `unconfirmed_property` | Gate | Confirm the property in the Lab, or pass `allow_unconfirmed`. |
| `never_write` | Gate: the catalog marks the code a command or status code | Nothing: no flag lifts it. Use the camera's own menu for the action. |
| `read_only`, `value_out_of_range`, `not_found`, `ambiguous` | Gate or lookup | Lists the valid values or candidates. |
| `plan_stale`, `plan_expired` | Apply | Make a new plan. |
| `readback_mismatch` | Apply: the camera took the write but holds another value | Read the setting; a camera rule may tie it to another. Apply the undo plan to revert. |
| `no_evidence` | A label from an agent marks a code `confirmed` that has not changed this session | Change it on the camera while `camera_watch` runs, or write it through a plan. |
| `private_text` | A label from an agent holds a string the camera reports | Describe the property; the catalog is shared. |

### No power cycling, no reconnect loops

- No tool and no command turns the camera off, resets USB on demand, or retries a connect.
- `camera connect` only lifts the Lab's stopped state; the owner then follows its 3 s and 30 s
  rules.
- A client never retries a failed request in a loop. It reconnects to the socket once if
  `serve` restarted, and spawns `serve` at most once per command.
- Keep-awake writes D381 only through the owner: a neighbouring timeout and straight back to
  the owner's value, never OFF. The camera holds the owner's value except during that round
  trip, so nothing needs restoring on disconnect.

## 6. Mapping: every UI feature

| Page | Feature | CLI | MCP |
|---|---|---|---|
| Hub | Overview: model, firmware, battery, card, lens | `status` | `camera_status` |
| Hub | History: shots per day and month, lenses seen, connect log | `status --history` | `camera_status {history}` |
| Hub | Owner details: read | `owner get` | `settings_read {query: "owner"}` |
| Hub | Owner details: edit | `owner set` | `camera_plan {set}` + `camera_apply` |
| Recipes | Library list, search, filter | `recipes ls` | `library_search {kind: recipe}` |
| Recipes | Recipe detail | `recipes show` | `library_read` |
| Recipes | Editor: edit fields | `recipes edit` | `library_save {kind: recipe, item}` |
| Recipes | Delete a recipe | `recipes rm` | `library_delete` |
| Recipes | Read C1-C7 | `recipes read [SLOT]` | `library_read {id: slot:Cn}` or `{id: slots}` |
| Recipes | Slot state (matches, edited, new) | `recipes read SLOT --compare ID` | `library_read {id: slot:Cn, compare_to}` |
| Recipes | Write a recipe to a slot | `recipes write` | `camera_plan {recipe, slot}` + `camera_apply` |
| Recipes | Import: paste text | `recipes import --text` | `recipe_parse {text}` + `library_save` |
| Recipes | Import: paste a link | `recipes import --url` | `recipe_parse {url}` + `library_save` |
| Recipes | Import: search the catalogue | `recipes search` | `library_search {kind: catalogue}` |
| Recipes | Import: add from the catalogue | `recipes import cat:<id> --save` | `library_save {kind: recipe, from}` |
| Settings | Menu tree | `settings --menus` | `settings_read {view: menus}` |
| Settings | Search by name and meaning | `settings QUERY` | `settings_read {query}` |
| Settings | Edit a setting | `settings set` | `camera_plan {set}` + `camera_apply` |
| Settings | Favourites: view | `settings --favourites` | `settings_read {filter: favourites}` |
| Settings | Favourites: add, remove | `settings fav add|rm` | `library_save {kind: favourite}`, `library_delete` |
| Settings | Presets: list, show | `presets ls|show` | `library_search`, `library_read` |
| Settings | Presets: save | `presets save` | `library_save {kind: preset}` |
| Settings | Presets: apply | `presets apply` | `camera_plan {preset}` + `camera_apply` |
| Settings | Presets: delete | `presets rm` | `library_delete` |
| Settings | Snapshots: save | `snapshots save` | `library_save {kind: snapshot}` |
| Settings | Snapshots: diff two | `snapshots diff A B` | `library_read {compare_to}` |
| Settings | Snapshots: diff against the camera | `snapshots diff A` | `camera_plan {snapshot}` |
| Settings | Snapshots: restore | `snapshots restore` | `camera_plan {snapshot}` + `camera_apply` |
| Settings | Snapshots: delete | `snapshots rm` | `library_delete` |
| Settings | Appearance: theme, brand mark, app icon | `prefs set` | `library_save {kind: prefs}` |
| Connect | Mode guidance and walkthrough | `help modes` | `walkthrough` in every `needs_*` error |
| Lab | Status bar: link, mode, timings | `status` | `camera_status` |
| Lab | Property table | `settings --code`, `settings --unnamed` | `settings_read {codes, filter}` |
| Lab | Change feed | `camera watch` | `camera_watch` |
| Lab | Pin codes | `camera watch --code` | `camera_watch {codes}` |
| Lab | Label panel | `catalog label` | `catalog_label` |
| Lab | Write box | `settings set --allow-unconfirmed` | `camera_plan {set, allow_unconfirmed}` + `camera_apply` |
| Lab | Note on a change group | `log note --group` | `library_save {kind: note}` |
| Lab | Export dump | `camera export` | `library_save {kind: dump}` |
| Lab | Keep awake | `prefs set keep-awake=on` | `library_save {kind: prefs}` |
| Lab | Connect, disconnect | `camera connect|disconnect` | none: the owner connects on demand |
| Lab | Fake camera bar | `dev fake` | `fake_camera` (fake builds) |
| Roadmap | Rebuild a recipe from a photo | later: `recipes import --photo` | later: `recipe_parse {file}` |

## 7. Open questions

1. ~~Hand-off.~~ Settled 2026-10-17: there is no hand-off. `serve` is the only owner, and the
   app is a client (section 2, "One USB owner").
2. ~~Confirming by agents.~~ Settled 2026-10-17: yes, under the evidence rule.
3. ~~Unconfirmed writes over MCP.~~ Settled 2026-10-17: the MCP server is open, with the same
   reach as the CLI and no start flag. A plan may still set `allow_unconfirmed`. Every write
   is still read back and logged, as protection against mistakes rather than abuse.
4. ~~App icon from the CLI.~~ Settled 2026-10-17: `prefs set app-icon` stores the choice, and
   the app applies it at its next start.
5. ~~Install path.~~ Settled 2026-10-17: a Homebrew cask in the owner's tap installs the app,
   and its `binary` stanza links the bundled `ukiyo` onto PATH, as QuadCam's cask does.

## 8. Implementation stages

Each stage ships on its own and keeps `cargo test` and clippy green. Start after the catalogue
work lands, since both touch `src-tauri/src/lib.rs`.

1. **Core crate.** Move the camera, catalog, recipes and Lab logic into `ukiyo-core`. Add the
   `api` request and response enums. The app calls `api`; no behaviour changes. Done
   2026-10-17: `src-tauri/core/`, section 2.
2. **Shared owner.** `ukiyo serve` with `owner.lock`, `ukiyo.sock`, events, spawn on demand
   and idle exit. The app becomes a client: its Tauri commands forward to the socket, and its
   events come from `subscribe`. No behaviour change in the window. Done 2026-10-17:
   `core/src/serve/`, `src-tauri/cli/`, section 2. The `tools/ptp/` lock check stays in stage 3.
3. **Read-only CLI.** `status`, `settings` (read), `camera watch`, `catalog show`, `log show`,
   `help modes`, `--json` and exit codes, working with the app open or closed. `tools/ptp/`
   scripts check the lock. Done 2026-10-17: section 3, "The read-only commands as built".
   Left for later: `settings --favourites` (stage 6), per-client pins for `camera watch
   --code` (stage 4), `lens` in `status`, and search keywords in the catalog.
4. **Plans and gated writes.** The `plan` module, typed errors, the `actor` field in the
   session log. CLI: `settings set`, `owner`, `plan`, `camera connect|disconnect|export`,
   `log note`. The window's Lab write box moves onto plans too. Done 2026-10-17: section 3,
   "The write commands as built", and section 5. Left for later: per-client pins for `camera
   watch --code`, a `plan applied` event to subscribers (the read-back change group carries
   the actor instead), and an idle exit that waits for open plans.
5. **MCP server.** `ukiyo mcp` with `camera_status`, `camera_watch`, `settings_read`,
   `camera_plan`, `camera_apply`, `catalog_label`, and `fake_camera` in fake builds. Done
   2026-10-17: section 4, "The MCP server as built". Left for later: the inputs listed
   there for stages 6 and 7, per-client pins for `camera_watch` `codes`, and progress
   notifications during a long `camera_watch`.
6. **Library.** Presets, favourites, snapshots, history and preferences in `ukiyo-core`, with
   the trash. Preferences move out of the window's local storage into `settings.json`. CLI:
   `presets`, `snapshots`, `settings fav`, `prefs`. MCP: `library_search`, `library_read`,
   `library_save`, `library_delete`. The Settings and Hub pages switch from fixtures. Done
   2026-10-17: section 4, "The library as built". Left for later: the lens name in history
   and `status`.
7. **Recipes.** The user recipe library, catalogue search, `recipe_parse` with URLs, slot reads,
   and slot write plans (gated until the slot fields are confirmed). CLI: `recipes`. Done
   2026-10-17, with gated and dependent settings in every plan: section 4,
   "The recipes as built". Left for later: listed there.
8. **Packaging,** on the pattern of [QuadCam](https://github.com/noahkiss/quadcam) (its cask in the owner's tap):
   - `ukiyo-cli` is an `externalBin` in `Contents/MacOS/`, beside the app's own binary
     (`ukiyo`). The app may then spawn it as `serve` instead of itself; either works.
   - The cask in the tap installs the whole thing: `app "Ukiyo.app"`, `binary
     "#{appdir}/Ukiyo.app/Contents/MacOS/ukiyo-cli", target: "ukiyo"`, `depends_on arch: :arm64`, and a `zap`
     covering `~/Library/Application Support/com.nkmkdigital.ukiyo` and the app's caches.
   - Self-updates through tauri-plugin-updater, so the cask sets `auto_updates true`.
   - GitHub releases ship an arm64 zip, and livecheck follows them.
   - Also from QuadCam: one `api` method table that generates the socket/MCP dispatch and the
     typed Tauri commands (`api!`), and a schema version with a surface guard test over the
     MCP and CLI listings.
   - The README carries the MCP config snippet (`ukiyo mcp`).

   Done 2026-10-17, "Packaging as built" below. Left for later: the first release, the cask in
   the tap, Developer ID signing and notarization (CI, as QuadCam does), and the
   no-Homebrew install button.

### Packaging as built (stage 8)

- **The CLI in the bundle.** `src-tauri/tauri.bundle.conf.json` is an overlay config that
  adds `bundle.externalBin: ["binaries/ukiyo-cli"]` and a `beforeBuildCommand` that also runs
  `tools/release/cli_sidecar.sh`. That script builds `ukiyo-cli` (release, or debug under
  `tauri build --debug`) and copies it to `src-tauri/binaries/ukiyo-cli-<target triple>`
  (gitignored); the bundler puts it in `Contents/MacOS/ukiyo-cli`. `pnpm bundle` (in `app/`)
  runs `tauri build --bundles app --config tauri.bundle.conf.json`.
  - Why an overlay and not `tauri.conf.json`: tauri-build copies each `externalBin` into
    `target/<profile>/` under its short name. In debug builds that would replace the real
    `target/debug/ukiyo-cli`, which the CLI tests run.
  - The app still spawns itself as `serve`; the bundled CLI spawns itself. Both are the same
    build.
- **The updater** (`src-tauri/src/updater.rs`). tauri-plugin-updater reads `latest.json` from
  `https://github.com/noahkiss/ukiyo/releases/latest/download/latest.json`, with
  `requireSignedVersion`. `tauri.conf.json` holds the placeholder
  `UKIYO_UPDATER_PUBKEY_PLACEHOLDER`; the release script merges the real public key in from
  `UKIYO_UPDATER_PUBKEY`. The updater is off, and makes no request, in a build with the
  placeholder, in a debug build (unless `UKIYO_UPDATER=real`), and with `UKIYO_UPDATER=off`.
  One check runs 30 s after launch. Settings > Connection > Updates shows the version and the
  answer, checks again, and installs (download, signature check, replace, restart). Window
  commands: `update_status`, `update_check`, `update_install`; an agent cannot update the
  app.
- **The method table**: section 2, `ukiyo_api_table!`. The wire did not change; the window's
  commands kept their names and params.
- **Schema version and surface guard** (`src-tauri/cli/src/schema.rs`, from QuadCam).
  `SCHEMA_VERSION` is 1 and counts removals and renames only. `ukiyo --schema-version`
  prints it; the MCP server reports it in `serverInfo.schemaVersion`. The guard tests compare
  the MCP tool listing and the clap command tree with `cli/tests/surface/mcp.txt` and
  `cli.txt`: a removal, a rename, a newly required parameter or a new value list on a free
  parameter fails unless `SCHEMA_VERSION` went up and `DEPRECATIONS` lists the item. An
  addition fails until the baseline is updated:
  `UKIYO_UPDATE_SURFACE=1 cargo test -p ukiyo-cli surface` (it also refreshes
  `mcp_tools.json`). A deprecated item gets a note in the tool listing and a warning on
  stderr from the CLI.
- **The release** (`tools/release/release.sh [--dry-run]`, Apple Silicon only). It checks that
  `tauri.conf.json` and `Cargo.toml` state one version, builds the bundle, checks it
  (`CFBundleShortVersionString`, an arm64 `ukiyo-cli` that runs, the code signature), and
  writes to `local/release/<version>/`:
  - `ukiyo-<version>-arm64.zip` (ditto), the cask's download; the script prints its sha256;
  - `ukiyo-<version>-arm64.app.tar.gz` and its `.sig` (`tauri signer sign --app-version`);
  - `latest.json` for the `darwin-aarch64` platform.

  `--dry-run` uploads nothing and allows a build without keys (no updater, an empty
  signature). Without it, the script needs both keys, a clean tree and HEAD at the tag
  `v<version>` on origin; it creates the GitHub release if needed and attaches the files, and
  never replaces an attached zip, archive or signature.
- **Keys.** The updater keypair lives in 1Password, read through op-connect into the
  environment for one run; it is never written to a file. The release reads
  `UKIYO_UPDATER_PUBKEY`, `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (empty for a key without a password).
- **Signing.** `tauri.conf.json` signs ad hoc (`signingIdentity "-"`), as QuadCam's does. A
  public release is signed with the Developer ID and notarized in CI, as QuadCam's
  `release.yml` does with noahkiss/workflows (`macos-sign-notarize`), before the zip and the
  updater archive are made. That workflow is not set up for Ukiyo yet.
- **The cask** is drafted, not in the tap: `app "Ukiyo.app"`, `binary
  "#{appdir}/Ukiyo.app/Contents/MacOS/ukiyo-cli", target: "ukiyo"`, `depends_on arch: :arm64`,
  `depends_on macos: :ventura`, `auto_updates true`, livecheck `:github_latest`, and `zap` of
  the Application Support, Caches, Saved Application State and WebKit folders of
  `com.nkmkdigital.ukiyo`.
