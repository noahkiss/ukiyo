//! The schema version of the MCP and CLI surface, and the rules that keep it stable (on
//! QuadCam's pattern).
//!
//! The surface (MCP tools and their parameters; CLI commands, flags and positional arguments)
//! only grows. A removal or a rename needs a [`SCHEMA_VERSION`] bump and a [`Deprecation`]
//! listed here for at least one version before it. The `surface_*` tests below compare the
//! surface with the baselines `tests/surface/mcp.txt` and `cli.txt` through [`check`]. After a
//! deliberate addition: `UKIYO_UPDATE_SURFACE=1 cargo test -p ukiyo-cli surface`.
//!
//! The binary reads [`SCHEMA_VERSION`] and [`DEPRECATIONS`]; the guard functions run in tests.
#![cfg_attr(not(test), allow(dead_code))]

use serde_json::{json, Value};
use std::collections::BTreeSet;

/// The version of the MCP and CLI surface. It starts at 1 and counts removals and renames only:
/// an added tool, action, parameter, command or flag leaves it alone. The MCP server reports it
/// in `serverInfo.schemaVersion`; `ukiyo --schema-version` prints it.
pub const SCHEMA_VERSION: u32 = 1;

/// One item of the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// An MCP tool.
    McpTool(&'static str),
    /// One value of an MCP tool's `action` parameter.
    McpAction {
        tool: &'static str,
        action: &'static str,
    },
    /// An MCP tool's parameter.
    McpParam {
        tool: &'static str,
        param: &'static str,
    },
    /// A CLI command by its path, for example `gear fc read`.
    CliCommand(&'static str),
    /// A CLI flag of a command (`""` for the root), for example `("gear fc read", "--port")`.
    CliFlag {
        command: &'static str,
        flag: &'static str,
    },
}

impl Item {
    /// The item's line in the baseline files.
    pub fn key(&self) -> String {
        match self {
            Item::McpTool(t) => format!("mcp.tool {t}"),
            Item::McpAction { tool, action } => format!("mcp.action {tool}.{action}"),
            Item::McpParam { tool, param } => format!("mcp.param {tool}.{param}"),
            Item::CliCommand(c) => format!("cli.command {c}"),
            Item::CliFlag { command, flag } => format!("cli.flag {command} {flag}"),
        }
    }

    /// Whether this item stands behind a line of the surface: the item's own line, and the lines
    /// below it (a tool's actions and parameters, a command's flags and subcommands).
    pub fn covers(&self, line: &str) -> bool {
        match self {
            Item::McpTool(t) => {
                line == format!("mcp.tool {t}")
                    || ["action", "param", "value", "required"]
                        .iter()
                        .any(|k| line.starts_with(&format!("mcp.{k} {t}.")))
            }
            Item::McpAction { tool, action } => {
                line == format!("mcp.action {tool}.{action}")
                    || line.starts_with(&format!("mcp.value {tool}.{action}="))
            }
            Item::McpParam { tool, param } => {
                line == format!("mcp.required {tool}.{param}")
                    || line.starts_with(&format!("mcp.param {tool}.{param} "))
                    || line.starts_with(&format!("mcp.value {tool}.{param}="))
                    || ["param", "value", "required"]
                        .iter()
                        .any(|k| line.starts_with(&format!("mcp.{k} {tool}.{param}.")))
            }
            Item::CliCommand(c) => {
                line == format!("cli.command {c}")
                    || ["command", "flag", "arg", "value"]
                        .iter()
                        .any(|k| line.starts_with(&format!("cli.{k} {c} ")))
            }
            Item::CliFlag { command, flag } => {
                line == format!("cli.flag {command} {flag}")
                    || line.starts_with(&format!("cli.flag {command} {flag} "))
                    || line.starts_with(&format!("cli.value {command} {flag}="))
            }
        }
    }

    /// The item as a person reads it in a warning.
    pub fn describe(&self) -> String {
        match self {
            Item::McpTool(t) => format!("the MCP tool {t}"),
            Item::McpAction { tool, action } => format!("{tool} action={action}"),
            Item::McpParam { tool, param } => format!("the {param} parameter of {tool}"),
            Item::CliCommand(c) => format!("the command `ukiyo {c}`"),
            Item::CliFlag { command, flag } => {
                format!("the flag {flag} of `ukiyo {command}`")
            }
        }
    }
}

/// A surface item that still works, and the one that replaces it.
#[derive(Debug, Clone, Copy)]
pub struct Deprecation {
    pub item: Item,
    /// What to use instead, in words an agent can act on.
    pub replacement: &'static str,
    /// The schema version that deprecated it. The item may go in a later version, never in this one.
    pub since: u32,
}

impl Deprecation {
    /// The warning an MCP result or a CLI run carries.
    pub fn warning(&self) -> String {
        format!(
            "Deprecated since schema {}: {} keeps working but a later schema version removes it. Use {}.",
            self.since,
            self.item.describe(),
            self.replacement
        )
    }
}

/// Everything deprecated now. Adding one is the first step of a removal or a rename; the item
/// keeps working until a later [`SCHEMA_VERSION`] drops it.
pub const DEPRECATIONS: &[Deprecation] = &[];

// ----- runtime: warnings -----

/// The deprecations an MCP call touches: its tool, its `action`, or a parameter it passes.
pub fn mcp_deprecations<'a>(
    table: &'a [Deprecation],
    tool: &str,
    args: &Value,
) -> Vec<&'a Deprecation> {
    table
        .iter()
        .filter(|d| match d.item {
            Item::McpTool(t) => t == tool,
            Item::McpAction { tool: t, action } => {
                t == tool && args.get("action").and_then(Value::as_str) == Some(action)
            }
            Item::McpParam { tool: t, param } => t == tool && args.get(param).is_some(),
            _ => false,
        })
        .collect()
}

/// The deprecations a command line touches. `argv` is the arguments after the program name.
/// A command matches when its words open the arguments that are not flags; a flag matches
/// when it appears as `--flag` or `--flag=value`.
pub fn cli_deprecations<'a>(table: &'a [Deprecation], argv: &[String]) -> Vec<&'a Deprecation> {
    let words: Vec<&str> = argv.iter().map(String::as_str).filter(|a| !a.starts_with('-')).collect();
    let line = words.join(" ");
    let under = |c: &str| line == c || line.starts_with(&format!("{c} "));
    table
        .iter()
        .filter(|d| match d.item {
            Item::CliCommand(c) => under(c),
            Item::CliFlag { command, flag } => {
                (command.is_empty() || under(command))
                    && argv
                        .iter()
                        .any(|a| a == flag || a.starts_with(&format!("{flag}=")))
            }
            _ => false,
        })
        .collect()
}

/// Marks deprecated items in a tool list: a note on the tool's description, and
/// `"deprecated": true` on a parameter.
pub fn annotate_tools(tools: &mut Value, table: &[Deprecation]) {
    let Some(list) = tools.as_array_mut() else {
        return;
    };
    for tool in list {
        let name = tool["name"].as_str().unwrap_or("").to_string();
        let mut notes = Vec::new();
        for d in table {
            match d.item {
                Item::McpTool(t) if t == name => notes.push(d.warning()),
                Item::McpAction { tool: t, .. } if t == name => notes.push(d.warning()),
                Item::McpParam { tool: t, param } if t == name => {
                    notes.push(d.warning());
                    if let Some(p) = tool["inputSchema"]["properties"].get_mut(param) {
                        p["deprecated"] = json!(true);
                    }
                }
                _ => {}
            }
        }
        if !notes.is_empty() {
            let text = tool["description"].as_str().unwrap_or("");
            tool["description"] = json!(format!("{text}\n\n{}", notes.join("\n")));
        }
    }
}

// ----- the surface as lines -----

/// The MCP surface as sorted lines: one per tool, `action` value and parameter (with its type
/// and enum values), and one per required parameter. An object parameter with `properties`
/// (or an array of such objects) adds its keys one level down, as `tool.param.key`.
pub fn mcp_surface(tools: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for tool in tools.as_array().into_iter().flatten() {
        let name = tool["name"].as_str().unwrap_or("?");
        out.insert(format!("mcp.tool {name}"));
        let schema = &tool["inputSchema"];
        object_lines(&mut out, name, schema);
        let Some(props) = schema["properties"].as_object() else {
            continue;
        };
        for (p, v) in props {
            let inner = if v.get("properties").is_some() {
                v
            } else {
                &v["items"]
            };
            if inner.get("properties").is_some() {
                object_lines(&mut out, &format!("{name}.{p}"), inner);
            }
        }
    }
    out
}

/// The lines of one object schema's properties and required keys, under `prefix`.
fn object_lines(out: &mut BTreeSet<String>, prefix: &str, schema: &Value) {
    for r in schema["required"].as_array().into_iter().flatten() {
        out.insert(format!(
            "mcp.required {prefix}.{}",
            r.as_str().unwrap_or("?")
        ));
    }
    let Some(props) = schema["properties"].as_object() else {
        return;
    };
    for (p, v) in props {
        out.insert(format!("mcp.param {prefix}.{p} {}", type_of(v)));
        let values = v
            .get("enum")
            .or_else(|| v["items"].get("enum"))
            .and_then(Value::as_array);
        for e in values.into_iter().flatten() {
            let e = e.as_str().map_or_else(|| e.to_string(), str::to_string);
            if p == "action" && !prefix.contains('.') {
                out.insert(format!("mcp.action {prefix}.{e}"));
            } else {
                out.insert(format!("mcp.value {prefix}.{p}={e}"));
            }
        }
    }
}

/// A property's type as one word: `string`, `array<string>`, `integer|null`, `any`.
fn type_of(v: &Value) -> String {
    let one = |t: &Value| t.as_str().unwrap_or("any").to_string();
    let base = match &v["type"] {
        Value::Array(a) => a.iter().map(one).collect::<Vec<_>>().join("|"),
        Value::Null => "any".into(),
        t => one(t),
    };
    if base == "array" {
        if let Some(items) = v.get("items").filter(|i| i.is_object()) {
            return format!("array<{}>", type_of(items));
        }
    }
    base
}

/// Which surface a baseline or a listing covers.
pub fn render(version: u32, lines: &BTreeSet<String>) -> String {
    let mut s = format!("# schema_version {version}\n");
    for l in lines {
        s.push_str(l);
        s.push('\n');
    }
    s
}

/// A baseline file: its schema version and its lines.
pub fn parse_baseline(text: &str) -> (u32, BTreeSet<String>) {
    let mut version = 0;
    let mut lines = BTreeSet::new();
    for l in text.lines().filter(|l| !l.is_empty()) {
        match l.strip_prefix("# schema_version ") {
            Some(v) => version = v.trim().parse().unwrap_or(0),
            None => {
                lines.insert(l.to_string());
            }
        }
    }
    (version, lines)
}

/// The CLI surface as sorted lines, read from the clap command tree: one per command, flag
/// (with ` VALUE` when it takes one, and its listed values) and positional argument. The
/// global flags (`--json`, `--quiet`) are listed once, on the root, whose path is `""`.
pub fn cli_surface(root: &clap::Command) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    walk(root, "", &mut out);
    out
}

fn walk(cmd: &clap::Command, path: &str, out: &mut BTreeSet<String>) {
    if !path.is_empty() {
        out.insert(format!("cli.command {path}"));
    }
    for a in cmd.get_arguments() {
        let id = a.get_id().as_str();
        if id == "help" || id == "version" {
            continue;
        }
        // A bool switch lists true and false; only a flag that takes a value has a value list.
        let values: Vec<String> = if a.get_action().takes_values() { a.get_possible_values().iter().map(|v| v.get_name().to_string()).collect() } else { vec![] };
        if a.is_positional() {
            let name = a.get_value_names().and_then(|n| n.first()).map_or_else(|| id.to_uppercase(), |n| n.to_string());
            let many = a.get_num_args().is_some_and(|n| n.max_values() > 1);
            let name = if a.is_required_set() { format!("<{name}>") } else { format!("[{name}]") };
            out.insert(format!("cli.arg {path} {name}{}", if many { "..." } else { "" }));
            continue;
        }
        let long = a.get_long().map(|l| format!("--{l}"));
        let short = a.get_short().map(|s| format!("-{s}"));
        let mut flag = match (&short, &long) {
            (Some(s), Some(l)) => format!("{s},{l}"),
            (None, Some(l)) => l.clone(),
            (Some(s), None) => s.clone(),
            (None, None) => continue,
        };
        if a.get_action().takes_values() {
            flag.push_str(" VALUE");
        }
        out.insert(format!("cli.flag {path} {flag}"));
        let key = long.or(short).unwrap_or_default();
        for v in values {
            out.insert(format!("cli.value {path} {key}={v}"));
        }
    }
    for sub in cmd.get_subcommands() {
        let next = if path.is_empty() { sub.get_name().to_string() } else { format!("{path} {}", sub.get_name()) };
        walk(sub, &next, out);
    }
}

// ----- the guard -----

/// What the surface did since the baseline.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Gone (or changed) with no schema bump and deprecation behind it: fails the guard.
    pub removed: Vec<String>,
    /// Gone, and allowed: the version moved on and the item was deprecated.
    pub removed_allowed: Vec<String>,
    /// A parameter that became required: breaks callers, so it fails the guard.
    pub newly_required: Vec<String>,
    /// A value list on a parameter or flag that took any value before: a narrowing that
    /// breaks callers, so it fails the guard.
    pub narrowed: Vec<String>,
    /// New items: allowed, but the baseline wants updating.
    pub added: Vec<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.removed.is_empty() && self.newly_required.is_empty() && self.narrowed.is_empty()
    }
}

/// Compares the surface now with the baseline. An item the baseline has and the surface lacks
/// (a removal, a rename, a type change) passes only when `version` is above the baseline's
/// version and `table` deprecates the item at a version below `version`.
pub fn check(
    baseline: (u32, &BTreeSet<String>),
    current: &BTreeSet<String>,
    version: u32,
    table: &[Deprecation],
) -> Report {
    let (base_version, base) = baseline;
    let mut r = Report::default();
    for gone in base.difference(current) {
        // A parameter that stopped being required loosens the contract.
        if gone.starts_with("mcp.required ") {
            continue;
        }
        let allowed = version > base_version
            && table
                .iter()
                .any(|d| d.since < version && d.item.covers(gone));
        if allowed {
            r.removed_allowed.push(gone.clone());
        } else {
            r.removed.push(gone.clone());
        }
    }
    for new in current.difference(base) {
        if let Some(key) = new.strip_prefix("mcp.required ") {
            if required_breaks(base, key) {
                r.newly_required.push(new.clone());
            } else {
                r.added.push(new.clone());
            }
        } else if narrows(base, new) {
            r.narrowed.push(new.clone());
        } else {
            r.added.push(new.clone());
        }
    }
    r
}

/// Whether a new required key (`tool.param` or `tool.param.key`) breaks a caller: only when
/// the baseline knew what holds it. A new tool's required parameter, or a required key of a
/// new object parameter, breaks no one. A baseline with no keys of a parameter predates the
/// one-level guard, so its keys count as added.
fn required_breaks(base: &BTreeSet<String>, key: &str) -> bool {
    match key.split('.').collect::<Vec<_>>()[..] {
        [tool, _] => base.contains(&format!("mcp.tool {tool}")),
        [tool, param, _] => base
            .iter()
            .any(|l| l.starts_with(&format!("mcp.param {tool}.{param}."))),
        _ => true,
    }
}

/// Whether a new value line puts a value list on a parameter or flag the baseline has with
/// no values: it took any value before, so the list narrows it.
fn narrows(base: &BTreeSet<String>, line: &str) -> bool {
    if let Some(rest) = line.strip_prefix("mcp.value ") {
        let Some((key, _)) = rest.split_once('=') else {
            return false;
        };
        let known = base
            .iter()
            .any(|l| l.starts_with(&format!("mcp.param {key} ")));
        let had_values = base
            .iter()
            .any(|l| l.starts_with(&format!("mcp.value {key}=")));
        return known && !had_values;
    }
    if let Some(rest) = line.strip_prefix("cli.value ") {
        let Some((head, _)) = rest.split_once('=') else {
            return false;
        };
        let Some((command, long)) = head.rsplit_once(' ') else {
            return false;
        };
        let flag = format!("cli.flag {command} ");
        let known = base.iter().any(|l| {
            l.strip_prefix(&flag).is_some_and(|f| {
                f.strip_suffix(" VALUE")
                    .is_some_and(|f| f == long || f.ends_with(&format!(",{long}")))
            })
        });
        let had_values = base
            .iter()
            .any(|l| l.starts_with(&format!("cli.value {command} {long}=")));
        return known && !had_values;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(lines: &[&str]) -> BTreeSet<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    const OLD: Deprecation = Deprecation {
        item: Item::McpAction {
            tool: "tool_x",
            action: "old",
        },
        replacement: "tool_x action=new",
        since: 1,
    };

    #[test]
    fn an_added_item_passes_and_is_reported() {
        let base = set(&["mcp.tool tool_x"]);
        let now = set(&["mcp.tool tool_x", "mcp.tool tool_y"]);
        let r = check((1, &base), &now, 1, &[]);
        assert!(r.ok());
        assert_eq!(r.added, ["mcp.tool tool_y"]);
    }

    #[test]
    fn a_removal_fails_without_a_bump_and_a_deprecation() {
        let base = set(&["mcp.action tool_x.old"]);
        let now = set(&[]);
        for (version, table) in [(1, &[OLD][..]), (2, &[][..])] {
            let r = check((1, &base), &now, version, table);
            assert!(!r.ok(), "version {version}");
            assert_eq!(r.removed, ["mcp.action tool_x.old"]);
        }
    }

    #[test]
    fn a_deprecated_removal_passes_after_a_bump() {
        let base = set(&["mcp.action tool_x.old"]);
        let r = check((1, &base), &set(&[]), 2, &[OLD]);
        assert!(r.ok());
        assert_eq!(r.removed_allowed, ["mcp.action tool_x.old"]);
    }

    #[test]
    fn a_rename_is_a_removal_and_an_addition() {
        let base = set(&["mcp.param tool_x.log_dir string"]);
        let now = set(&["mcp.param tool_x.logs string"]);
        let r = check((1, &base), &now, 1, &[]);
        assert!(!r.ok());
        assert_eq!(r.added.len(), 1);
    }

    #[test]
    fn a_newly_required_parameter_fails() {
        let base = set(&["mcp.tool tool_x"]);
        let now = set(&["mcp.tool tool_x", "mcp.required tool_x.ids"]);
        let r = check((1, &base), &now, 1, &[]);
        assert!(!r.ok());
        assert_eq!(r.newly_required, ["mcp.required tool_x.ids"]);
    }

    #[test]
    fn a_new_tools_required_parameter_is_an_addition() {
        let base = set(&["mcp.tool tool_x"]);
        let now = set(&[
            "mcp.tool tool_x",
            "mcp.tool tool_y",
            "mcp.required tool_y.action",
        ]);
        let r = check((1, &base), &now, 1, &[]);
        assert!(r.ok(), "{r:?}");
        assert_eq!(r.added.len(), 2);
    }

    #[test]
    fn a_nested_key_that_becomes_required_fails_unless_its_object_is_new() {
        let base = set(&[
            "mcp.tool tool_x",
            "mcp.param tool_x.values object",
            "mcp.param tool_x.values.a string",
        ]);
        let mut now = base.clone();
        now.insert("mcp.param tool_x.values.b string".into());
        now.insert("mcp.required tool_x.values.b".into());
        let r = check((1, &base), &now, 1, &[]);
        assert_eq!(r.newly_required, ["mcp.required tool_x.values.b"]);

        // A new object parameter with a required key breaks no caller.
        let mut now = base.clone();
        now.insert("mcp.param tool_x.where object".into());
        now.insert("mcp.param tool_x.where.lat number".into());
        now.insert("mcp.required tool_x.where.lat".into());
        let r = check((1, &base), &now, 1, &[]);
        assert!(r.ok(), "{r:?}");
    }

    #[test]
    fn a_removed_nested_key_fails() {
        let base = set(&[
            "mcp.param tool_x.values object",
            "mcp.param tool_x.values.elrs_wifi_interval integer|null",
        ]);
        let now = set(&[
            "mcp.param tool_x.values object",
            "mcp.param tool_x.values.wifi_interval integer|null",
        ]);
        let r = check((1, &base), &now, 1, &[]);
        assert_eq!(
            r.removed,
            ["mcp.param tool_x.values.elrs_wifi_interval integer|null"]
        );
        // Deprecating the parameter covers its keys.
        let dep = Deprecation {
            item: Item::McpParam {
                tool: "tool_x",
                param: "values",
            },
            replacement: "x",
            since: 1,
        };
        assert!(check((1, &base), &now, 2, &[dep]).ok());
    }

    #[test]
    fn a_value_list_on_a_free_parameter_narrows_it() {
        let base = set(&[
            "mcp.param tool_x.label string",
            "mcp.param tool_x.kind string",
            "mcp.value tool_x.kind=a",
        ]);
        let mut now = base.clone();
        now.insert("mcp.value tool_x.label=one".into());
        now.insert("mcp.value tool_x.kind=b".into());
        let r = check((1, &base), &now, 1, &[]);
        assert_eq!(r.narrowed, ["mcp.value tool_x.label=one"]);
        assert_eq!(r.added, ["mcp.value tool_x.kind=b"]);
        assert!(!r.ok());

        // A new parameter with values is an addition.
        let now = set(&[
            "mcp.param tool_x.label string",
            "mcp.param tool_x.kind string",
            "mcp.value tool_x.kind=a",
            "mcp.param tool_x.mode string",
            "mcp.value tool_x.mode=fast",
        ]);
        assert!(check((1, &base), &now, 1, &[]).ok());
    }

    #[test]
    fn a_value_list_on_a_free_cli_flag_narrows_it() {
        let base = set(&[
            "cli.flag gear sims --to VALUE",
            "cli.flag gear sims -p,--rate-profile VALUE",
            "cli.flag cut --removed VALUE",
            "cli.value cut --removed=keep",
        ]);
        let mut now = base.clone();
        now.insert("cli.value gear sims --to=liftoff".into());
        now.insert("cli.value gear sims --rate-profile=1".into());
        now.insert("cli.value cut --removed=trash".into());
        let r = check((1, &base), &now, 1, &[]);
        assert_eq!(
            r.narrowed,
            [
                "cli.value gear sims --rate-profile=1",
                "cli.value gear sims --to=liftoff"
            ]
        );
        assert_eq!(r.added, ["cli.value cut --removed=trash"]);
    }

    #[test]
    fn the_mcp_surface_reads_object_parameters_one_level_down() {
        let tools = json!([{
            "name": "tool_x",
            "inputSchema": {
                "required": ["action"],
                "properties": {
                    "action": {"type": "string", "enum": ["read"]},
                    "values": {"type": "object", "properties": {
                        "mode": {"type": ["string", "null"], "enum": ["a", "b", null]},
                        "deep": {"type": "object", "properties": {"z": {"type": "string"}}}
                    }},
                    "cuts": {"type": "array", "items": {"type": "object",
                        "required": ["start"],
                        "properties": {"start": {"type": "number"}}}}
                }
            }
        }]);
        let lines = mcp_surface(&tools);
        for l in [
            "mcp.action tool_x.read",
            "mcp.required tool_x.action",
            "mcp.param tool_x.values object",
            "mcp.param tool_x.values.mode string|null",
            "mcp.value tool_x.values.mode=a",
            "mcp.param tool_x.values.deep object",
            "mcp.param tool_x.cuts array<object>",
            "mcp.param tool_x.cuts.start number",
            "mcp.required tool_x.cuts.start",
        ] {
            assert!(lines.contains(l), "{l} in {lines:#?}");
        }
        assert!(!lines.iter().any(|l| l.contains("deep.z")));
    }

    #[test]
    fn a_loosened_requirement_passes() {
        let base = set(&["mcp.required tool_x.ids"]);
        let r = check((1, &base), &set(&[]), 1, &[]);
        assert!(r.ok());
    }

    #[test]
    fn the_baseline_round_trips() {
        let lines = set(&["cli.command gear", "mcp.tool tool_x"]);
        let (v, back) = parse_baseline(&render(3, &lines));
        assert_eq!((v, back), (3, lines));
    }

    #[test]
    fn mcp_warnings_match_tool_action_and_param() {
        let table = [
            OLD,
            Deprecation {
                item: Item::McpParam {
                    tool: "tool_x",
                    param: "gone",
                },
                replacement: "the stay parameter",
                since: 1,
            },
        ];
        let hit = |args: Value| mcp_deprecations(&table, "tool_x", &args).len();
        assert_eq!(hit(json!({"action": "old"})), 1);
        assert_eq!(hit(json!({"action": "new"})), 0);
        assert_eq!(hit(json!({"action": "new", "gone": 1})), 1);
        assert!(mcp_deprecations(&table, "tool_y", &json!({"action": "old"})).is_empty());
        assert!(table[0].warning().contains("tool_x action=old"));
    }

    #[test]
    fn cli_warnings_match_command_and_flag() {
        let table = [
            Deprecation {
                item: Item::CliCommand("gear map"),
                replacement: "gear switch-map",
                since: 1,
            },
            Deprecation {
                item: Item::CliFlag {
                    command: "import",
                    flag: "--output",
                },
                replacement: "--output-dir",
                since: 1,
            },
        ];
        let v = |a: &[&str]| -> Vec<String> { a.iter().map(|s| s.to_string()).collect() };
        assert_eq!(
            cli_deprecations(&table, &v(&["gear", "map", "--live"])).len(),
            1
        );
        assert_eq!(
            cli_deprecations(
                &table,
                &v(&["--json", "status", "gear", "map"])
            )
            .len(),
            0
        );
        assert_eq!(cli_deprecations(&table, &v(&["gear", "mapx"])).len(), 0);
        assert_eq!(
            cli_deprecations(&table, &v(&["import", "--output=/x"])).len(),
            1
        );
        assert_eq!(
            cli_deprecations(&table, &v(&["verify", "--output"])).len(),
            0
        );
    }

    #[test]
    fn annotate_marks_the_description_and_the_parameter() {
        let mut tools = json!([{
            "name": "tool_x",
            "description": "Does x.",
            "inputSchema": {"properties": {"gone": {"type": "string"}}},
        }]);
        let table = [Deprecation {
            item: Item::McpParam {
                tool: "tool_x",
                param: "gone",
            },
            replacement: "stay",
            since: 1,
        }];
        annotate_tools(&mut tools, &table);
        assert_eq!(
            tools[0]["inputSchema"]["properties"]["gone"]["deprecated"],
            true
        );
        assert!(tools[0]["description"]
            .as_str()
            .unwrap()
            .contains("Use stay"));
    }

    #[test]
    fn the_surface_lists_tools_actions_params_and_required() {
        let tools = json!([{
            "name": "tool_x",
            "inputSchema": {
                "required": ["action"],
                "properties": {
                    "action": {"type": "string", "enum": ["a", "b"]},
                    "ids": {"type": "array", "items": {"type": "string"}},
                    "mode": {"type": "string", "enum": ["fast"]},
                },
            },
        }]);
        let s = mcp_surface(&tools);
        for line in [
            "mcp.tool tool_x",
            "mcp.required tool_x.action",
            "mcp.param tool_x.action string",
            "mcp.action tool_x.a",
            "mcp.param tool_x.ids array<string>",
            "mcp.value tool_x.mode=fast",
        ] {
            assert!(s.contains(line), "{line}\n{s:#?}");
        }
    }

    #[test]
    fn an_items_key_is_a_line_it_covers() {
        let items = [Item::McpTool("tool_x"), Item::McpAction { tool: "tool_x", action: "read" }, Item::CliCommand("status"), Item::CliFlag { command: "status", flag: "--history" }];
        for i in items {
            assert!(i.covers(&i.key()), "{}", i.key());
        }
    }

    // ----- the guard on the real surface -----

    fn baseline_path(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/surface").join(name)
    }

    /// Compares one surface with its baseline file, or writes the baseline when asked.
    fn guard(file: &str, current: BTreeSet<String>, table: &[Deprecation]) {
        let path = baseline_path(file);
        let update = std::env::var_os("UKIYO_UPDATE_SURFACE").is_some();
        let (base_version, base) = match std::fs::read_to_string(&path) {
            Ok(t) => parse_baseline(&t),
            Err(_) if update => {
                std::fs::write(&path, render(SCHEMA_VERSION, &current)).unwrap();
                return;
            }
            Err(e) => panic!("{}: {e}; run with UKIYO_UPDATE_SURFACE=1 to create it", path.display()),
        };
        let report = check((base_version, &base), &current, SCHEMA_VERSION, table);
        for item in &report.removed_allowed {
            eprintln!("surface: removed after deprecation: {item}");
        }
        assert!(
            report.ok(),
            "the {file} surface broke its contract.\n\
             Removed or renamed with no schema bump and deprecation:\n  {}\n\
             Newly required:\n  {}\n\
             Narrowed to a value list:\n  {}\n\
             Keep the old item working and add the new one, or deprecate the old one in \
             schema::DEPRECATIONS, bump SCHEMA_VERSION in a later release, then update the baseline.",
            report.removed.join("\n  "),
            report.newly_required.join("\n  "),
            report.narrowed.join("\n  "),
        );
        if update {
            std::fs::write(&path, render(SCHEMA_VERSION, &current)).unwrap();
            return;
        }
        assert!(
            report.added.is_empty(),
            "{} new item(s) in {file}. Update the baseline: UKIYO_UPDATE_SURFACE=1 cargo test -p ukiyo-cli surface\n  {}",
            report.added.len(),
            report.added.join("\n  ")
        );
        assert_eq!(base_version, SCHEMA_VERSION, "{file} names another schema version");
    }

    fn mcp_now() -> BTreeSet<String> {
        mcp_surface(&Value::Array(crate::mcp_tools::tools(true)))
    }

    fn cli_now() -> BTreeSet<String> {
        use clap::CommandFactory;
        cli_surface(&crate::Cli::command())
    }

    #[test]
    fn surface_mcp_keeps_the_baseline() {
        guard("mcp.txt", mcp_now(), DEPRECATIONS);
    }

    #[test]
    fn surface_cli_keeps_the_baseline() {
        guard("cli.txt", cli_now(), DEPRECATIONS);
    }

    #[test]
    fn surface_deprecations_name_live_items() {
        let live: BTreeSet<String> = mcp_now().into_iter().chain(cli_now()).collect();
        for d in DEPRECATIONS {
            assert!(d.since <= SCHEMA_VERSION, "{:?} is deprecated in the future", d.item);
            assert!(live.iter().any(|l| d.item.covers(l)) || d.since < SCHEMA_VERSION, "{} is deprecated but not on the surface", d.item.describe());
        }
    }

    #[test]
    fn the_cli_surface_reads_flags_values_and_arguments() {
        let lines = cli_now();
        for l in ["cli.command status", "cli.flag  --json", "cli.flag status --history", "cli.command settings set", "cli.arg settings set <NAME=VALUE>...", "cli.command mcp"] {
            assert!(lines.contains(l), "{l} in {lines:#?}");
        }
        assert!(!lines.iter().any(|l| l.ends_with(" --help")), "help flags stay out");
    }
}
