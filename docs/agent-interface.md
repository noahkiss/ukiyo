# Agent interface: CLI and MCP

A design for the `ukiyo` command line tool and its MCP server. Stage 1 (the core crate and
`api`) and stage 2 (the shared owner, `ukiyo serve`) are built; the rest is not (section 8). The
bm task is task-cv9i7ei1. The tool shapes follow the `mcp-tool-design` guidance: a small tool
surface, search then read, and plan then apply for every camera write.

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
  Three more modules are planned or built:
  - `library` (stage 6): presets, favourites, snapshots, user recipes, camera history, and
    preferences.
  - `plan` (stage 4): builds, checks and applies multi-property changes.
  - `api` (built): every front end speaks only this.
    - `Request`, one variant per method: `{"method": "lab_write", "params": {...}}`.
    - `Response`, the variant of the same name: `{"method": "lab_write", "result": {...}}`.
    - `Error`, a `code` and a `message`. Today the codes are `failed`, `no_fake_camera`,
      `bad_request` (a socket line that is not a request) and `unavailable` (no `serve` to
      answer); section 5's codes come with stage 4.
    - `Event`: `status`, `props`, `changes`, `catalog`, as `{"event": ..., "data": ...}`.
      `serve` sends these to every client that called `subscribe`.
    - `call::<method>`, one typed function per method, and `dispatch(&Lab, Request)`. The
      socket calls `dispatch`; MCP (stage 5) will send requests over the socket too.
    - Method names are the window's command names (`lab_snapshot`, `lab_write`, `lab_pin`,
      `lab_keep_awake`, `lab_connect`, `lab_export`, `lab_label`, `lab_note`, `lab_fake`,
      `recipe_parse`). New methods for the CLI and MCP join the same enums.
- `src-tauri/src/` keeps the window glue: Tauri commands, the four Tauri events
  (`events.rs`) and specta bindings. Each command sends its `api::Request` to `serve` over the
  socket, and the client turns `serve`'s events into the Tauri events. `recipe_parse` is the
  one exception: pure parsing with no shared state, so it runs in the app and works without
  `serve`. A command that waits on `serve` runs off the main thread. When `serve` is
  unreachable, `lab_snapshot` returns an empty snapshot whose status `reason` says why, and
  `lab_write` an outcome with `ok: false`, so the window's command types did not change.
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
6. Each connection names its `actor` (`app`, `cli`, `mcp:<client name>`) in `hello`. Stage 4
   writes it into the session log and the events, so the window can show "changed by an
   agent".
7. Nothing else touches USB. `tools/ptp/` scripts check `owner.lock` and refuse to run while
   it is held.

In code, `Lab::start` takes the held `OwnerLock`, so no path opens USB without the lock.

### The socket protocol

One JSON-RPC 2.0 message per line (`core/src/serve/wire.rs`):

```text
-> {"jsonrpc":"2.0","id":1,"method":"hello","params":{"actor":"app","protocol":1}}
<- {"jsonrpc":"2.0","id":1,"result":{"protocol":1,"version":"0.1.0","pid":4242,"fake":true}}
-> {"jsonrpc":"2.0","id":2,"method":"subscribe"}
<- {"jsonrpc":"2.0","id":2,"result":null}
<- {"jsonrpc":"2.0","method":"event","params":{"event":"status","data":{...}}}
-> {"jsonrpc":"2.0","id":3,"method":"lab_write","params":{"code":53283,"value":{"kind":"int","v":2}}}
<- {"jsonrpc":"2.0","id":3,"result":{"code":53283,"ok":true,...}}
<- {"jsonrpc":"2.0","id":4,"error":{"code":"bad_request","message":"..."}}
```

- `method` and `params` are an `api::Request`. `result` is the matching `api::Response`'s
  `result` (`null` for a method with none). `error` is an `api::Error`.
- `hello` and `subscribe` are the only methods outside `api`. `hello` with another `protocol`
  number is refused. After `subscribe`, the connection gets every `api::Event`.
- Replies can come out of order: `lab_write` and `lab_export` wait on the camera on their own
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
  `Ukiyo.app/Contents/MacOS/ukiyo-cli` (stage 8). It is signed and notarized with the app.
  Until then, `cargo build -p ukiyo-cli` builds it, and the app needs it for nothing: the app
  spawns its own executable as `serve` (rule 3 above).
- Settings > Command line has one button, "Install command line tool". It symlinks
  `~/.local/bin/ukiyo` to the bundled binary, and shows the path if that directory is not on
  `PATH`. `ukiyo install --prefix DIR` does the same from a shell.
- The symlink follows app updates, since it points into the bundle.
- MCP client config:

  ```json
  { "mcpServers": { "ukiyo": { "command": "ukiyo", "args": ["mcp"] } } }
  ```

## 3. CLI

### Command tree

```
ukiyo status                                   link, mode, model, battery, card, what works now
ukiyo camera  watch [--code C]... [--for 60s]  the change feed, live
              connect | disconnect             the Lab's connect switch
              export                           write a full dump to dumps/
ukiyo settings [QUERY] [--menu PATH] [--code C]... [--favourites] [--unnamed] [--menus]
              set NAME=VALUE...                one or more settings, through a plan
              fav add|rm NAME...
ukiyo presets ls | show ID | save NAME [--from-snapshot ID | NAME...] | apply ID | rm ID
ukiyo snapshots ls | save [NAME] | diff A [B] | restore ID | rm ID
ukiyo recipes ls [QUERY] [--catalogue] [--sensor S] [--sim F]
              show ID | edit ID FIELD=VALUE... | rm ID
              import (--text FILE|- | --url URL | cat:ID) [--save]
              slots                            read C1-C7 (RAW CONV. mode)
              write SLOT ID                    write a recipe to a slot, through a plan
ukiyo owner   get | set FIELD=VALUE...         copyright, author, prefix, device-name
ukiyo catalog show [CODE] | label CODE [--name N] [--menu M] [--value K=LABEL]...
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

### Flags (every command)

| Flag | Effect |
|---|---|
| `--json` | One JSON document on stdout, nothing else. Errors too. |
| `--dry-run` | Build and print the plan; apply nothing. Only on commands that write. |
| `--yes` | Apply a plan without the y/N prompt. Without a TTY and without `--yes`, a write prints the plan and exits 6. |
| `--allow-unconfirmed` | Lets a plan include properties whose catalog confidence is not `confirmed`. |
| `--quiet` | No progress lines on stderr. |

Values: `NAME` is a catalog name (`AutoPowerOff`), a menu label (`"AUTO POWER OFF"`), or a code
(`0xd381`). `VALUE` is a value label (`OFF`) or a raw number (`3`). The core resolves both and
refuses an ambiguous match with the candidates.

### `--json` shapes

Every command returns one envelope:

```json
{ "ok": true, "data": { } }
{ "ok": false, "error": { "code": "needs_rawconv", "message": "Recipe slots read only in USB RAW CONV./BACKUP RESTORE mode.", "hint": "Unplug the cable, set SET UP > CONNECTION SETTING > CONNECTION MODE to USB RAW CONV./BACKUP RESTORE, plug back in.", "walkthrough": "ukiyo help modes rawconv", "mode": "tether" } }
```

`data` uses the same shapes as the MCP `structuredContent` in section 4. Key shapes:

```jsonc
// status
{ "link": "connected", "reason": null, "model": "X-T5", "firmware": "x.yy", "mode": "tether",
  "capture": "stills", "screen": "shooting", "battery": "...", "card": "...", "lens": "...",
  "owner": "app", "available": { "settings": true, "slots": false, "movie_props": false } }

// a plan (settings set, presets apply, snapshots restore, recipes write, owner set)
{ "plan_id": "p-7f3a", "expires_in_s": 300, "requires": { "mode": "tether", "capture": "stills" },
  "steps": [ { "code": "0xd381", "name": "AutoPowerOff", "before": "5 MIN", "after": "OFF",
               "confidence": "confirmed", "gate": "ok" } ],
  "skipped": [ { "code": "0x....", "name": "...", "reason": "needs_movie" } ],
  "blocked": [ { "code": "0x....", "name": "...", "reason": "unconfirmed_property" } ] }

// an applied plan
{ "plan_id": "p-7f3a", "applied": 1, "failed": 0, "not_attempted": 0, "undo_plan_id": "p-91c0",
  "steps": [ { "code": "0xd381", "wanted": "OFF", "readback": "OFF", "ok": true, "result": "OK" } ],
  "side_effects": [ { "code": "0x....", "old": "...", "new": "..." } ] }
```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Done. |
| 1 | Internal or I/O error. |
| 2 | Usage error: bad flag, unknown name, ambiguous value. |
| 3 | No camera: not connected, asleep, unplugged. |
| 4 | Wrong camera state: `needs_*`, `menu_open`, `busy`. |
| 5 | Refused by a gate: unconfirmed, read-only, out of range. |
| 6 | Not applied: declined at the prompt, `--dry-run`, no TTY without `--yes`, plan stale or expired. |
| 7 | Applied in part: at least one step failed. The output lists each step. |

### Example sessions

```console
$ ukiyo status
X-T5 · tether, stills, shooting · battery ... · owner: app
Works now: settings, presets, snapshots, owner strings, Lab
Needs RAW CONV.: recipe slots (ukiyo help modes rawconv)

$ ukiyo settings "eye focus"
AF/MF > FACE/EYE DETECTION SETTING   FaceEyeDetection   Face on, eye auto   confirmed

$ ukiyo settings set "AUTO POWER OFF=OFF" --dry-run
Plan p-7f3a (expires in 5 min), needs tether + stills
  AutoPowerOff   5 MIN -> OFF   confirmed
Not applied (--dry-run).

$ ukiyo plan apply p-7f3a --yes
AutoPowerOff   OFF  read back OFF  OK
Applied 1 of 1. Undo: ukiyo plan apply p-91c0

$ ukiyo recipes write C3 cat:<recipe-id> --json
{"ok":false,"error":{"code":"needs_rawconv", ...}}
$ echo $?
4

$ ukiyo snapshots save before-trip && ukiyo snapshots diff before-trip
12 settings differ from the camera ...
```

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
  owner process (`app` or `serve`), and `available` per feature with the missing requirement.
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

### Deviations from the bm proposal

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
- The Lab's write box in the window is the one place that writes unconfirmed codes without a
  flag. That is the mapping tool; agents use the flag instead.
- Recipe slot fields (D18E-D1A5) are not yet confirmed by a write. Until they are, slot writes
  need `allow_unconfirmed`.

### Plan, then apply

- Every camera write is a plan first. That includes one property, presets, snapshot restores,
  owner strings and recipe slot writes.
- A plan holds each step's `before`. Apply re-reads them and refuses with `plan_stale` if any
  changed.
- Plans live in the owner for 5 minutes and die with it.
- `--dry-run` stops after the plan. A TTY prompt or `--yes` is needed to apply.
- Steps run in plan order. A slot write is: select the slot (D18C), write the fields and name
  (D18D), restore D18C. Apply stops at the first failure and returns an undo plan.

### Read-back and the session log

- The owner reads back every write at once (owner.rs). Apply reports `wanted` and `readback`
  per step, and flags a step whose read-back differs (the camera enforces its own rules: ISO
  125 forces D-Range 100).
- After the last step, apply reports `side_effects`: other codes that changed during the apply.
- Every request that writes goes to `sessions/*.jsonl`, with `actor`: `app`, `cli` or
  `mcp:<client name>`. That includes plans applied, library saves, labels and notes.

### Typed errors

Each error has `code`, `message`, `hint`, and `walkthrough` where a mode switch fixes it. The
walkthrough is the USB mode guidance (task-klgaidjz): `ukiyo help modes <mode>` in the CLI, and
the Connect page's guide in the window.

| Code | When | Hint |
|---|---|---|
| `not_connected` | No camera on the bus | Plug in the cable and turn the camera on. |
| `camera_asleep` | Auto power off took the camera off USB | Turn the camera on. Keep-awake holds AUTO POWER OFF at OFF. |
| `busy` | The camera answers 0x2019 | Wait; try again in a few seconds. |
| `menu_open` | D22E = 3, or 0xA001 | Close the menu on the camera. |
| `needs_tether` | Live settings in another mode | Switch to USB TETHER SHOOTING FIXED. |
| `needs_rawconv` | Slot contents in another mode | Switch to USB RAW CONV./BACKUP RESTORE. |
| `needs_stills`, `needs_movie` | A property of the other capture mode | Set the STILL/MOVIE switch. |
| `owner_conflict` | A process outside ukiyo holds the USB device | Quit that process (PTPCamera is handled by the owner). |
| `unconfirmed_property` | Gate | Confirm the property in the Lab, or pass `allow_unconfirmed`. |
| `read_only`, `value_out_of_range`, `not_found`, `ambiguous` | Gate or lookup | Lists the valid values or candidates. |
| `plan_stale`, `plan_expired` | Apply | Make a new plan. |

### No power cycling, no reconnect loops

- No tool and no command turns the camera off, resets USB on demand, or retries a connect.
- `camera connect` only lifts the Lab's stopped state; the owner then follows its 3 s and 30 s
  rules.
- A client never retries a failed request in a loop. It reconnects to the socket once if
  `serve` restarted, and spawns `serve` at most once per command.
- Keep-awake writes D381 only through the owner, and the owner restores the old value on a
  clean disconnect.

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
| Recipes | Read C1-C7 | `recipes slots` | `library_read {id: slot:Cn}` |
| Recipes | Slot state (matches, edited, new) | `recipes slots` | `library_read {id: slot:Cn, compare_to}` |
| Recipes | Write a recipe to a slot | `recipes write` | `camera_plan {recipe, slot}` + `camera_apply` |
| Recipes | Import: paste text | `recipes import --text` | `recipe_parse {text}` + `library_save` |
| Recipes | Import: paste a link | `recipes import --url` | `recipe_parse {url}` + `library_save` |
| Recipes | Import: search the catalogue | `recipes ls --catalogue` | `library_search {kind: catalogue}` |
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
| Lab | Write box | `settings set --allow-unconfirmed` | `camera_plan {set, allow_unconfirmed}` |
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
work (task-fsd5pob8) lands, since both touch `src-tauri/src/lib.rs`.

1. **Core crate.** Move the camera, catalog, recipes and Lab logic into `ukiyo-core`. Add the
   `api` request and response enums. The app calls `api`; no behaviour changes. Done
   2026-10-17: `src-tauri/core/`, section 2.
2. **Shared owner.** `ukiyo serve` with `owner.lock`, `ukiyo.sock`, events, spawn on demand
   and idle exit. The app becomes a client: its Tauri commands forward to the socket, and its
   events come from `subscribe`. No behaviour change in the window. Done 2026-10-17:
   `core/src/serve/`, `src-tauri/cli/`, section 2. The `tools/ptp/` lock check stays in stage 3.
3. **Read-only CLI.** `status`, `settings` (read), `camera watch`, `catalog show`, `log show`,
   `help modes`, `--json` and exit codes, working with the app open or closed. `tools/ptp/`
   scripts check the lock.
4. **Plans and gated writes.** The `plan` module, typed errors, the `actor` field in the
   session log. CLI: `settings set`, `owner`, `plan`, `camera connect|disconnect|export`,
   `log note`. The window's Lab write box moves onto plans too.
5. **MCP server.** `ukiyo mcp` with `camera_status`, `camera_watch`, `settings_read`,
   `camera_plan`, `camera_apply`, `catalog_label`, and `fake_camera` in fake builds.
6. **Library.** Presets, favourites, snapshots, history and preferences in `ukiyo-core`, with
   the trash. Preferences move out of the window's local storage into `settings.json`. CLI:
   `presets`, `snapshots`, `settings fav`, `prefs`. MCP: `library_search`, `library_read`,
   `library_save`, `library_delete`. The Settings and Hub pages switch from fixtures.
7. **Recipes.** The user recipe library, catalogue search, `recipe_parse` with URLs, slot reads,
   and slot write plans (gated until the slot fields are confirmed). CLI: `recipes`.
8. **Packaging,** on QuadCam's pattern (`~/develop/quadcam`, its cask in the owner's tap):
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
