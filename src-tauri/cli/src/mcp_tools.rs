//! The MCP tool listing: names, descriptions, input schemas and annotations
//! (docs/agent-interface.md, section 4). `cli/tests/surface/mcp_tools.json` holds a copy, so
//! a change to the listing is always deliberate.

use serde_json::{json, Value};

pub const INSTRUCTIONS: &str = "Ukiyo manages a Fujifilm X-series camera on USB. It shares the camera with the Ukiyo app and the ukiyo CLI, through one owner process; every change shows in the app live. \
Start with camera_status. Read settings with settings_read. Every camera write is two calls: camera_plan builds a plan and writes nothing; show its steps to the user, and only after they agree call camera_apply with the plan_id. \
Plans write only properties whose catalog confidence is confirmed, unless the plan sets allow_unconfirmed; do that only when the user asks for it. \
A needs_* error carries a hint and a walkthrough: tell the user how to switch the camera's USB mode or STILL/MOVIE switch; never retry in a loop. \
When camera_status lists notices (a camera menu open, playback, the switch at MOVIE), pass each text to the user; the camera answers again once they act. \
Mapping properties: camera_watch shows what changed while the user turns a dial, then catalog_label names it. Plans expire after 5 minutes. \
The library on this Mac (recipes, presets, snapshots, favourite settings, preferences, camera history) and the bundled recipe catalogue are library_search, library_read, library_save and library_delete; none of them writes to the camera. Apply a preset or restore a snapshot with camera_plan {preset} or {snapshot}, then camera_apply. \
Recipes: recipe_parse reads pasted text or a post's URL; library_save {kind: recipe} keeps one; the camera's custom slots C1-C7 are library_read {id: slot:C1} and camera_plan {recipe, slot}, both in USB RAW CONV. mode.";

fn code_list(what: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string", "pattern": "^0x[0-9a-fA-F]{4}$" }, "description": what })
}

/// Every tool, in section 4 order. `fake` adds `fake_camera`.
pub fn tools(fake: bool) -> Vec<Value> {
    let mut v = vec![
        json!({
            "name": "camera_status",
            "description": "Reports the camera connection, its USB mode, and which features work in that mode.\n\nBest for: the first call in any camera task, and after any needs_* error.\nReturns: link, transport (usb or wifi), reason, model, firmware, mode, capture (stills/movie), screen, battery, battery_percent (D36B), battery_note (over Wi-Fi: it drains the battery faster), card, keep_awake (the setting) and keep_awake_active (resetting the camera's auto power off timer now), the owner process (serve, its pid, fake or real camera), and available per feature (settings, slots, movie_props), and notices: [{ id: menu|playback|movie, text }], what the user must do at the camera first.\nNot for property values: use settings_read.",
            "inputSchema": { "type": "object", "properties": {
                "history": { "type": "boolean", "default": false, "description": "Also this camera's history on this Mac: connects, shot counts per day and month, lenses seen." },
            }, "additionalProperties": false },
            "annotations": { "title": "Camera status", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
        }),
        json!({
            "name": "camera_watch",
            "description": "Returns the change groups the camera reported after a cursor, waiting up to wait_s for new ones.\n\nBest for: Lab mapping (ask the user to turn a dial, then call this), and checking a change the user made on the camera body.\nReturns: { cursor, link, groups: [{ id, at, source: camera|write, changes: [{ code, name, old, new, old_label, new_label }] }], has_more }. Without since, only groups that arrive during the call. Pass the returned cursor as since on the next call so nothing is missed.\nQuery tips: codes filters the groups to those codes; it does not change what the camera polls.\nFollow up with catalog_label to name a code that moved.",
            "inputSchema": { "type": "object", "properties": {
                "since": { "type": "integer", "minimum": 0, "description": "The cursor from the last camera_watch: return groups after it." },
                "codes": code_list("Only changes of these codes: 0xd381."),
                "wait_s": { "type": "number", "minimum": 0, "maximum": 60, "default": 10, "description": "How long to wait for a group when none is there yet." },
            }, "additionalProperties": false },
            "annotations": { "title": "Watch camera changes", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false },
        }),
        json!({
            "name": "settings_read",
            "description": "Reads live camera settings with their catalog names, menu paths and value labels.\n\nBest for: \"what is X set to\", finding a setting by name or menu, browsing the menu tree.\nReturns: { rows: [{ code, name, menu, value, value_label, options?, writable, confidence, available, error? }], total, has_more }. options (the allowed values) appear when 10 rows or fewer match. view=menus returns { menus: [{ path, depth, count }] } instead.\nQuery tips: query words must all appear in the code, the name (FaceEyeDetection matches \"eye detection\"), the menu path or the notes. menu is a path prefix: \"SET UP > POWER MANAGEMENT\". Needs USB TETHER mode, except with codes, which read in any connected mode.\nFollow up with camera_plan to change a value. Owner strings (copyright, author) are settings too: query \"copyright\".",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string", "description": "Words to match: \"auto power off\"." },
                "codes": code_list("Only these codes."),
                "menu": { "type": "string", "description": "A menu path prefix." },
                "filter": { "type": "string", "enum": ["all", "favourites", "unnamed", "unconfirmed"], "default": "all", "description": "favourites: the user's favourite settings. unnamed: codes with no catalog name. unconfirmed: codes whose catalog confidence is not confirmed." },
                "view": { "type": "string", "enum": ["rows", "menus"], "default": "rows" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 25 },
            }, "additionalProperties": false },
            "annotations": { "title": "Read settings", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
        }),
        json!({
            "name": "camera_plan",
            "description": "Builds a change plan against the live camera. Writes nothing.\n\nBest for: every camera write: one setting, several, owner strings, a preset, a snapshot restore, or a recipe into a custom slot.\nReturns: { plan_id, expires_in_s, requires, steps: [{ code, name, menu, before, after, confidence, gate }], skipped: [{ code, name, value, reason, message }], blocked: [...], allow_unconfirmed }. blocked steps are not written; skipped ones need nothing (unchanged) or another mode.\nQuery tips: give exactly one source: set, preset (preset:<name>), snapshot (snapshot:<name>, a restore; the plan is also the snapshot's diff against the camera), or recipe (recipe:<name> or cat:<id>) with slot (C1-C7; USB RAW CONV. mode; the whole recipe or nothing). Steps come in a safe order: a step with purpose prerequisite sets what the next step needs for the time of the write, and a restore step puts it back; resets lists what the camera changes by itself. set maps a name to a value. The name is a catalog name (AutoPowerOff), a menu label (\"AUTO POWER OFF\") or a code (0xd381); the value is a value label (OFF) or a raw number. An unknown or ambiguous name or value fails with the valid choices.\nA plan with nothing to write fails with the reason (unconfirmed_property, needs_tether ...). allow_unconfirmed lets the plan write codes whose catalog confidence is not confirmed: only when the user asks.\nFollow up with camera_apply and the plan_id, after the user agrees to the steps.",
            "inputSchema": { "type": "object", "properties": {
                "set": { "type": "object", "additionalProperties": { "type": "string" }, "minProperties": 1, "description": "{ \"<name or code>\": \"<label or raw value>\" }" },
                "preset": { "type": "string", "description": "A preset id from library_search: preset:street." },
                "snapshot": { "type": "string", "description": "A snapshot id from library_search: snapshot:before-trip." },
                "recipe": { "type": "string", "description": "A recipe id: recipe:<name> or cat:<id>. Needs slot." },
                "slot": { "type": "string", "enum": ["C1", "C2", "C3", "C4", "C5", "C6", "C7"], "description": "The custom slot a recipe goes to." },
                "allow_unconfirmed": { "type": "boolean", "default": false },
            }, "additionalProperties": false },
            "annotations": { "title": "Plan a camera change", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false },
        }),
        json!({
            "name": "camera_apply",
            "description": "Applies a plan made by camera_plan: writes each step to the camera, reads it back, and logs it.\n\nBest for: the second step of every camera write, after the user agreed to the plan's steps.\nReturns: { plan_id, applied, failed, not_attempted, undo_plan_id, steps: [{ code, name, wanted, readback, ok, result, error, attempts }], side_effects }. A plan applies once.\nRefuses with plan_stale when a planned code changed since the plan, and plan_expired after 5 minutes or a restart of the owner: make a new plan. Stops at the first failed step; apply undo_plan_id to revert what was written.",
            "inputSchema": { "type": "object", "properties": {
                "plan_id": { "type": "string", "description": "From camera_plan: p-7f3a." },
            }, "required": ["plan_id"], "additionalProperties": false },
            "annotations": { "title": "Apply a camera plan", "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false },
        }),
        json!({
            "name": "library_search",
            "description": "Searches what Ukiyo keeps on this Mac (recipes, presets, snapshots, favourite settings, the trash) and the bundled recipe catalogue.\n\nBest for: finding a recipe, preset or snapshot by name, film simulation or sensor before reading or applying it.\nReturns: { items: [{ id, kind, name, summary, at, credit?, url?, sensor? }], total, has_more }. at is wall-clock ms of the last save (of the trashing, for trash). Catalogue items carry their credit and post link, most popular first.\nQuery tips: query words must all appear in the name (a preset's setting names; a recipe's film simulation and credit); no query lists everything of the kind. sensor (\"X-Trans V\") and film_sim (\"classic chrome\") narrow recipes and the catalogue.\nFollow up with library_read for the full item, camera_plan {preset} / {snapshot} to apply one, or camera_plan {recipe, slot}.",
            "inputSchema": { "type": "object", "properties": {
                "kind": { "type": "string", "enum": ["recipe", "catalogue", "preset", "snapshot", "favourite", "trash"] },
                "query": { "type": "string" },
                "sensor": { "type": "string", "description": "Recipes and catalogue: a sensor generation, X-Trans V." },
                "film_sim": { "type": "string", "description": "Recipes and catalogue: part of a film simulation name, classic chrome." },
                "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 20 },
            }, "required": ["kind"], "additionalProperties": false },
            "annotations": { "title": "Search the library", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
        }),
        json!({
            "name": "library_read",
            "description": "Reads one library item in full, a custom slot on the camera, or the diff of two items.\n\nBest for: a recipe's settings, a catalogue recipe, what the camera's C1-C7 slots hold, a preset's settings, a snapshot's values, the favourites, the preferences, or camera history.\nReturns: { kind, item }. kind is recipe ({ name, settings, notes, sensor, source }), catalogue ({ name, settings, notes, sensor, credit }), slots ([{ slot, name, settings, warnings, matches }]; matches lists library recipes with the same settings), preset ({ set: {name: value} }), snapshot ({ model, firmware, mode, capture, values: [{ code, name, value, label }] }), favourites, prefs, history (per camera: shots, per_day, per_month, connects, lenses), diff ({ added, removed, changed: [{ code, name, a, b, a_label, b_label }], same }) or recipediff ({ changed: [{ field, a, b }], same }).\nQuery tips: id is recipe:<name>, cat:<id>, slot:C1 to slot:C7, slots (all seven), preset:<name>, snapshot:<name>, favourites, prefs, history or history:<camera id>. A slot read needs USB RAW CONV. mode; it selects the slot and puts the selection back. compare_to: another snapshot for a snapshot; a recipe id for a recipe or a slot.\nNot for a snapshot against the live camera: camera_plan {snapshot} is that diff.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "string" },
                "compare_to": { "type": "string", "description": "A second snapshot id, or a recipe id to compare a recipe or slot with." },
            }, "required": ["id"], "additionalProperties": false },
            "annotations": { "title": "Read a library item", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
        }),
        json!({
            "name": "library_save",
            "description": "Creates or replaces one item on this Mac, or restores one from the trash. Never writes to the camera.\n\nBest for: saving a preset, taking a snapshot, adding a favourite setting, changing preferences, a note in the session log, a full property dump.\nReturns: { id, kind, replaced, warnings }. A replaced preset or snapshot keeps its old version in the trash for 30 days.\nQuery tips, by kind:\n- recipe: name and item { settings } (recipe_parse's settings; notes, sensor, source { url, credit }, comment optional), or item { set: { \"Highlight\": \"+1\" } } to change a saved recipe of that name, or from: cat:<id> to copy a catalogue recipe. warnings say what would not fit a slot.\n- preset: name, and item { set: { \"<name or code>\": \"<label or raw>\" } } or item { settings: [\"<name or code>\"] } (their values on the camera now), or from: snapshot:<name>.\n- snapshot: name; reads the camera now (tether mode).\n- favourite: name is the setting (a catalog name, menu label or 0x code).\n- prefs: item { theme: system|light|dark, brand_mark, app_icon, keep_awake, in_use_minutes, close_menu_to_write, transport: usb|wifi }; app_icon applies at the app's next start; transport reaches a running serve at once (wifi: wireless tether).\n- note: item { text, group? } into the session log. dump: a full Lab dump to dumps/.\n- any kind, from: trash:<name> restores that item.",
            "inputSchema": { "type": "object", "properties": {
                "kind": { "type": "string", "enum": ["recipe", "preset", "snapshot", "favourite", "prefs", "note", "dump"] },
                "name": { "type": "string" },
                "item": { "type": "object" },
                "from": { "type": "string", "description": "cat:<id> (a recipe copied from the catalogue), snapshot:<name> (a preset of its values) or trash:<name> (restore)." },
            }, "required": ["kind"], "additionalProperties": false },
            "annotations": { "title": "Save to the library", "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
        }),
        json!({
            "name": "library_delete",
            "description": "Deletes one library item: a preset or snapshot goes to the trash for 30 days; a favourite is removed from the list. Never touches the camera.\n\nReturns: { id, trashed_at, trash_id }. Restore with library_save { from: trash_id }.\nQuery tips: id is recipe:<name>, preset:<name>, snapshot:<name> or favourite:<code>. prefs, history, cat: and slot: ids cannot be deleted.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "string" },
            }, "required": ["id"], "additionalProperties": false },
            "annotations": { "title": "Delete a library item", "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false },
        }),
        json!({
            "name": "recipe_parse",
            "description": "Parses recipe text, or the recipe post at a URL, into slot settings. Stores nothing.\n\nBest for: importing a recipe someone pasted or linked.\nReturns: { name?, source_url?, author?, sensor?, settings, notes, warnings, others }. settings use the camera menu's units (highlightTone 1.5); notes are settings a slot cannot hold (ISO, exposure compensation); warnings are lines it could not use; others are further recipes in the same post.\nQuery tips: give exactly one of text and url. A post on fujixweekly.com or film.recipes reads as the catalogue does; any other page is searched for recipe blocks.\nFollow up with library_save {kind: recipe} to keep it, after the user checked the settings.",
            "inputSchema": { "type": "object", "properties": {
                "text": { "type": "string", "description": "The recipe as text: one Label: value line per setting." },
                "url": { "type": "string", "description": "An http or https link to a recipe post." },
            }, "additionalProperties": false },
            "annotations": { "title": "Parse a recipe", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": true },
        }),
        json!({
            "name": "catalog_label",
            "description": "Edits one property's entry in the connected model's catalog: name, menu path, value labels, confidence, notes. Never writes to the camera.\n\nBest for: Lab mapping, after camera_watch showed a code move.\nReturns: { model, path, code, entry }.\nQuery tips: an empty string clears a field; an empty label removes that value. confidence confirmed needs this session to have seen the code change (on the camera, or a write's read-back), else no_evidence. Text that matches a string the camera reports (serials, owner strings) is refused with private_text: the catalog is shared.\nNot for camera values: use camera_plan.",
            "inputSchema": { "type": "object", "properties": {
                "code": { "type": "string", "pattern": "^0x[0-9a-fA-F]{4}$" },
                "name": { "type": "string", "description": "CamelCase: AutoPowerOff." },
                "menu": { "type": "string", "description": "The menu path: SET UP > POWER MANAGEMENT > AUTO POWER OFF." },
                "values": { "type": "object", "additionalProperties": { "type": "string" }, "description": "{ \"<raw value>\": \"<label>\" }: { \"3\": \"OFF\" }." },
                "confidence": { "type": "string", "enum": ["confirmed", "likely", "guess"] },
                "notes": { "type": "string" },
            }, "required": ["code"], "additionalProperties": false },
            "annotations": { "title": "Label a catalog property", "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
        }),
    ];
    if fake {
        v.push(json!({
            "name": "fake_camera",
            "description": "Drives the fake camera, as the person at the camera would. Only with UKIYO_FAKE_CAMERA=1.\n\nBest for: testing a flow without hardware: open a menu, flip STILL/MOVIE, power off, unplug, nudge a random setting, start or stop automatic changes.\nReturns: { action }.",
            "inputSchema": { "type": "object", "properties": {
                "action": { "type": "string", "enum": ["openMenu", "closeMenu", "movie", "stills", "powerOff", "powerOn", "unplug", "nudge", "autoOn", "autoOff"] },
            }, "required": ["action"], "additionalProperties": false },
            "annotations": { "title": "Fake camera", "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false },
        }));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The surface guard: the listing in a fake build matches the checked-in copy. After a
    /// deliberate change: `UKIYO_UPDATE_SURFACE=1 cargo test -p ukiyo-cli surface`.
    #[test]
    fn surface() {
        let names = |fake| tools(fake).iter().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names(false), ["camera_status", "camera_watch", "settings_read", "camera_plan", "camera_apply", "library_search", "library_read", "library_save", "library_delete", "recipe_parse", "catalog_label"]);
        assert_eq!(names(true).last().map(String::as_str), Some("fake_camera"));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/surface/mcp_tools.json");
        let now = serde_json::to_string_pretty(&json!({ "tools": tools(true) })).unwrap() + "\n";
        if std::env::var_os("UKIYO_UPDATE_SURFACE").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &now).unwrap();
        }
        let saved = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(saved == now, "the MCP tool listing changed; if that is deliberate, run UKIYO_UPDATE_SURFACE=1 cargo test -p ukiyo-cli surface");
    }

    #[test]
    fn every_tool_has_the_fields_clients_need() {
        for t in tools(true) {
            assert!(t["description"].as_str().unwrap().len() > 40, "{}", t["name"]);
            assert_eq!(t["inputSchema"]["type"], "object");
            assert!(t["annotations"]["readOnlyHint"].is_boolean());
        }
    }
}
