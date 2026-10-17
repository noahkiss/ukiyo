//! The MCP tool listing: names, descriptions, input schemas and annotations
//! (docs/agent-interface.md, section 4). `cli/tests/surface/mcp_tools.json` holds a copy, so
//! a change to the listing is always deliberate.

use serde_json::{json, Value};

pub const INSTRUCTIONS: &str = "Ukiyo manages a Fujifilm X-series camera on USB. It shares the camera with the Ukiyo app and the ukiyo CLI, through one owner process; every change shows in the app live. \
Start with camera_status. Read settings with settings_read. Every camera write is two calls: camera_plan builds a plan and writes nothing; show its steps to the user, and only after they agree call camera_apply with the plan_id. \
Plans write only properties whose catalog confidence is confirmed, unless the plan sets allow_unconfirmed; do that only when the user asks for it. \
A needs_* error carries a hint and a walkthrough: tell the user how to switch the camera's USB mode or STILL/MOVIE switch; never retry in a loop. \
Mapping properties: camera_watch shows what changed while the user turns a dial, then catalog_label names it. Plans expire after 5 minutes.";

fn code_list(what: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string", "pattern": "^0x[0-9a-fA-F]{4}$" }, "description": what })
}

/// Every tool, in section 4 order. `fake` adds `fake_camera`.
pub fn tools(fake: bool) -> Vec<Value> {
    let mut v = vec![
        json!({
            "name": "camera_status",
            "description": "Reports the camera connection, its USB mode, and which features work in that mode.\n\nBest for: the first call in any camera task, and after any needs_* error.\nReturns: link, reason, model, firmware, mode, capture (stills/movie), screen, battery, card, keep_awake, the owner process (serve, its pid, fake or real camera), and available per feature (settings, slots, movie_props).\nNot for property values: use settings_read.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
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
                "filter": { "type": "string", "enum": ["all", "unnamed", "unconfirmed"], "default": "all", "description": "unnamed: codes with no catalog name. unconfirmed: codes whose catalog confidence is not confirmed." },
                "view": { "type": "string", "enum": ["rows", "menus"], "default": "rows" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 25 },
            }, "additionalProperties": false },
            "annotations": { "title": "Read settings", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
        }),
        json!({
            "name": "camera_plan",
            "description": "Builds a change plan against the live camera. Writes nothing.\n\nBest for: every camera write: one setting, several, or owner strings.\nReturns: { plan_id, expires_in_s, requires, steps: [{ code, name, menu, before, after, confidence, gate }], skipped: [{ code, name, value, reason, message }], blocked: [...], allow_unconfirmed }. blocked steps are not written; skipped ones need nothing (unchanged) or another mode.\nQuery tips: set maps a name to a value. The name is a catalog name (AutoPowerOff), a menu label (\"AUTO POWER OFF\") or a code (0xd381); the value is a value label (OFF) or a raw number. An unknown or ambiguous name or value fails with the valid choices.\nA plan with nothing to write fails with the reason (unconfirmed_property, needs_tether ...). allow_unconfirmed lets the plan write codes whose catalog confidence is not confirmed: only when the user asks.\nFollow up with camera_apply and the plan_id, after the user agrees to the steps.",
            "inputSchema": { "type": "object", "properties": {
                "set": { "type": "object", "additionalProperties": { "type": "string" }, "minProperties": 1, "description": "{ \"<name or code>\": \"<label or raw value>\" }" },
                "allow_unconfirmed": { "type": "boolean", "default": false },
            }, "required": ["set"], "additionalProperties": false },
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
        assert_eq!(names(false), ["camera_status", "camera_watch", "settings_read", "camera_plan", "camera_apply", "catalog_label"]);
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
