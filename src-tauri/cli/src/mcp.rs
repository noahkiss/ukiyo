//! `ukiyo mcp`: the MCP server on stdio (docs/agent-interface.md, section 4).
//!
//! - JSON-RPC 2.0, one message per line. stdout carries only MCP messages; logs go to stderr.
//! - A client of `ukiyo serve` as actor `mcp:<client name>`, like the CLI: it never opens USB,
//!   and spawns `serve` when none runs. The connection stays open between calls, so plans
//!   live their 5 minutes, and drops after `UKIYO_MCP_IDLE` seconds (default 300) with no
//!   call, so `serve` can idle out and release the camera.
//! - Tools return text plus `structuredContent`. A failure is an `isError` result whose
//!   `structuredContent.error` is the typed error: `code`, `message`, `hint`, `walkthrough`,
//!   `mode`.

use crate::cmds;
use crate::conn;
use crate::mcp_tools::{tools, INSTRUCTIONS};
use crate::out::{Fail, Out};
use crate::writes;
use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead as _, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ukiyo_core::api::{Request, Response};
use ukiyo_core::camera::owner::ChangeGroup;
use ukiyo_core::catalog::{self, Confidence, LabelPatch};
use ukiyo_core::lab::FakeAction;
use ukiyo_core::plan::Plan;
use ukiyo_core::read::{self, Filter, Need};
use ukiyo_core::serve::client::Client;

const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Logs go to stderr, and `--quiet` silences the CLI's own progress lines.
const OUT: Out = Out { json: true, quiet: true };

/// A change group page.
const WATCH_MAX: usize = 50;

fn fake_build() -> bool {
    std::env::var("UKIYO_FAKE_CAMERA").is_ok_and(|v| v == "1")
}

/// The one connection to `serve`, made on the first tool call.
struct Link {
    actor: String,
    client: Option<Arc<Client>>,
    last: Arc<Mutex<Instant>>,
    busy: Arc<AtomicBool>,
}

impl Link {
    fn client(&mut self) -> Arc<Client> {
        if self.client.is_none() {
            let c = conn::client_as(OUT, &self.actor, None);
            idle_drop(c.clone(), self.last.clone(), self.busy.clone());
            self.client = Some(c);
        }
        self.client.clone().unwrap()
    }
}

/// Drops the connection after the idle time with no call. The next call connects again.
fn idle_drop(c: Arc<Client>, last: Arc<Mutex<Instant>>, busy: Arc<AtomicBool>) {
    let idle = std::env::var("UKIYO_MCP_IDLE").ok().and_then(|s| s.parse::<f64>().ok()).filter(|s| s.is_finite() && *s >= 0.0).unwrap_or(300.0);
    let idle = Duration::from_secs_f64(idle);
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500).min(idle.max(Duration::from_millis(50))));
        if !busy.load(Ordering::SeqCst) && last.lock().unwrap().elapsed() >= idle && c.welcome().is_some() {
            c.disconnect();
        }
    });
}

pub struct Server {
    link: Link,
    fake: bool,
}

impl Server {
    pub fn new() -> Server {
        Server { link: Link { actor: "mcp".into(), client: None, last: Arc::new(Mutex::new(Instant::now())), busy: Arc::new(AtomicBool::new(false)) }, fake: fake_build() }
    }

    /// Answers one message. Notifications get no reply.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned()?;
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or("");
                let version = if SUPPORTED_VERSIONS.contains(&asked) { asked } else { SUPPORTED_VERSIONS[1] };
                if let Some(name) = params["clientInfo"]["name"].as_str().map(str::trim).filter(|n| !n.is_empty()) {
                    // Set before the first call connects; the actor goes into the session log.
                    if self.link.client.is_none() {
                        self.link.actor = format!("mcp:{name}");
                    }
                }
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "ukiyo", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools(self.fake) })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                if !tools(self.fake).iter().any(|t| t["name"] == name) {
                    Err(json!({ "code": -32602, "message": format!("unknown tool: {name}") }))
                } else {
                    let args = params.get("arguments").cloned().unwrap_or(json!({}));
                    Ok(self.call_tool(name, &args))
                }
            }
            _ => Err(json!({ "code": -32601, "message": format!("method not found: {method}") })),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
        })
    }

    /// Runs a tool. A failure is an `isError` result the agent can read and act on.
    fn call_tool(&mut self, name: &str, args: &Value) -> Value {
        self.link.busy.store(true, Ordering::SeqCst);
        let r = self.run(name, args);
        *self.link.last.lock().unwrap() = Instant::now();
        self.link.busy.store(false, Ordering::SeqCst);
        match r {
            Ok((text, data)) => json!({ "content": [{ "type": "text", "text": text }], "structuredContent": data, "isError": false }),
            Err(f) => {
                let mut t = format!("{}: {}", f.code, f.message);
                if let Some(h) = &f.more.hint {
                    t.push_str(&format!("\n{h}"));
                }
                if let Some(w) = &f.more.walkthrough {
                    t.push_str(&format!("\nHow to switch modes: {w}"));
                }
                json!({ "content": [{ "type": "text", "text": t }], "structuredContent": { "error": f }, "isError": true })
            }
        }
    }

    fn run(&mut self, name: &str, a: &Value) -> Result<(String, Value), Fail> {
        match name {
            "camera_status" => {
                let _: NoArgs = args(a)?;
                let c = self.link.client();
                let snap = conn::settled(&c, None)?;
                let v = read::status_view(&snap, c.welcome().as_ref());
                Ok((cmds::status_lines(&v).join("\n"), to_value(&v)))
            }
            "camera_watch" => self.watch(args(a)?),
            "settings_read" => self.settings(args(a)?),
            "camera_plan" => self.plan(args(a)?),
            "camera_apply" => {
                let x: ApplyArgs = args(a)?;
                let c = self.link.client();
                let plan = match c.call(Request::PlanShow { plan_id: x.plan_id.clone() })? {
                    Response::PlanShow(p) => *p,
                    r => return Err(unexpected(r)),
                };
                let applied = match c.call(Request::PlanApply { plan_id: x.plan_id })? {
                    Response::PlanApply(a) => *a,
                    r => return Err(unexpected(r)),
                };
                let mut text = writes::applied_lines(&applied, plan.steps.len());
                if let Some(u) = &applied.undo_plan_id {
                    if let Some(last) = text.last_mut() {
                        *last = format!("Applied {} of {}. Undo: camera_apply {{\"plan_id\": \"{u}\"}}", applied.applied, plan.steps.len());
                    }
                }
                Ok((text.join("\n"), to_value(&applied)))
            }
            "catalog_label" => self.label(args(a)?),
            "fake_camera" => {
                let x: FakeArgs = args(a)?;
                let action: FakeAction = serde_json::from_value(json!(x.action)).map_err(|_| bad(format!("no fake action {}", x.action)))?;
                let c = self.link.client();
                c.call(Request::LabFake { action })?;
                Ok((format!("Fake camera: {}.", x.action), json!({ "action": x.action })))
            }
            _ => Err(bad(format!("unknown tool: {name}"))),
        }
    }

    fn watch(&mut self, x: WatchArgs) -> Result<(String, Value), Fail> {
        let codes = parse_codes(&x.codes)?;
        let wait = x.wait_s.unwrap_or(10.0);
        if !(0.0..=60.0).contains(&wait) {
            return Err(bad("wait_s is 0 to 60"));
        }
        let c = self.link.client();
        let snap = conn::settled(&c, None)?;
        let latest = newest(&snap.groups);
        // A cursor past the newest group: `serve` restarted and its ids began again.
        let reset = x.since.is_some_and(|s| s > latest);
        let since = match x.since {
            Some(s) if !reset => s,
            _ => latest,
        };
        let deadline = Instant::now() + Duration::from_secs_f64(wait);
        let mut snap = snap;
        loop {
            let mut fresh: Vec<&ChangeGroup> = snap.groups.iter().filter(|g| g.id > since).collect();
            fresh.sort_by_key(|g| g.id);
            let views: Vec<(u32, read::GroupView)> = fresh.iter().filter_map(|g| read::group_view(snap.catalog.as_ref(), g, &codes).map(|v| (g.id, v))).collect();
            if !views.is_empty() || Instant::now() >= deadline {
                let has_more = views.len() > WATCH_MAX;
                let page: Vec<read::GroupView> = views.into_iter().take(WATCH_MAX).map(|(_, v)| v).collect();
                // Past every group looked at, unless the page was cut short.
                let cursor = if has_more { page.last().map_or(since, |g| g.id) } else { newest(&snap.groups).max(since) };
                let mut lines: Vec<String> = page.iter().flat_map(cmds::group_lines).collect();
                if lines.is_empty() {
                    lines.push(format!("No changes in {wait} s."));
                }
                lines.push(format!("Cursor {cursor}{}", if has_more { "; more groups wait: call again with since" } else { "" }));
                let mut data = json!({ "cursor": cursor, "link": snap.status.link, "groups": page, "has_more": has_more });
                if reset {
                    data["reset"] = json!(true);
                    lines.insert(0, "The owner restarted; the change feed began again.".into());
                }
                return Ok((lines.join("\n"), data));
            }
            std::thread::sleep(Duration::from_millis(200));
            snap = conn::snapshot(&c)?;
        }
    }

    fn settings(&mut self, x: SettingsArgs) -> Result<(String, Value), Fail> {
        let codes = parse_codes(&x.codes)?;
        let need = if codes.is_empty() { Need::Tether } else { Need::Connected };
        let c = self.link.client();
        let snap = conn::settled(&c, Some(need))?;
        if x.view.as_deref() == Some("menus") {
            let m = read::menus(&snap);
            let text: Vec<String> = m.iter().map(|m| format!("{}{}  {}", "  ".repeat(m.depth), m.path.rsplit(" > ").next().unwrap_or(&m.path), m.count)).collect();
            return Ok((text.join("\n"), json!({ "menus": m })));
        }
        if let Some(v) = x.view.as_deref().filter(|v| *v != "rows") {
            return Err(bad(format!("no view {v}: pick rows or menus")));
        }
        let (unnamed, unconfirmed) = match x.filter.as_deref().unwrap_or("all") {
            "all" => (false, false),
            "unnamed" => (true, false),
            "unconfirmed" => (false, true),
            other => return Err(bad(format!("no filter {other}: pick all, unnamed or unconfirmed"))),
        };
        let query = x.query.filter(|q| !q.trim().is_empty());
        let f = Filter { query, codes, menu: x.menu, unnamed, unconfirmed, limit: Some(x.limit.unwrap_or(25).max(1)) };
        let rows = read::settings(&snap, &f);
        let mut text: Vec<String> = Vec::new();
        for r in &rows.rows {
            let conf = r.confidence.map(|c| to_value(&c).as_str().unwrap_or_default().to_string()).unwrap_or_else(|| "-".into());
            text.push(format!("{} {} = {} ({conf}) {}", r.code, r.name.as_deref().unwrap_or("-"), cmds::value_text(r), r.menu.as_deref().unwrap_or("")).trim_end().to_string());
            if let Some(o) = cmds::options_text(r) {
                text.push(format!("    {o}"));
            }
        }
        text.push(format!("{} of {} rows{}", rows.rows.len(), rows.total, if rows.has_more { "; narrow the query or raise limit for more" } else { "" }));
        Ok((text.join("\n"), to_value(&rows)))
    }

    fn plan(&mut self, x: PlanArgs) -> Result<(String, Value), Fail> {
        if x.set.is_empty() {
            return Err(bad("set needs at least one name and value"));
        }
        let c = self.link.client();
        conn::settled(&c, None)?;
        let plan: Plan = match c.call(Request::PlanCreate { set: x.set, allow_unconfirmed: x.allow_unconfirmed })? {
            Response::PlanCreate(p) => *p,
            r => return Err(unexpected(r)),
        };
        if plan.steps.is_empty() {
            let _ = c.call(Request::PlanDiscard { plan_id: plan.plan_id.clone() });
            if let Some(e) = plan.empty_reason() {
                return Err(e.into());
            }
            let mut text: Vec<String> = writes::plan_lines(&plan).into_iter().skip(1).collect();
            text.push("Nothing to change: every value is already set. No plan to apply.".into());
            let mut data = to_value(&plan);
            data["plan_id"] = Value::Null;
            return Ok((text.join("\n"), data));
        }
        let mut text = writes::plan_lines(&plan);
        text.push(format!("Nothing is written yet. After the user agrees: camera_apply {{\"plan_id\": \"{}\"}}", plan.plan_id));
        Ok((text.join("\n"), to_value(&plan)))
    }

    fn label(&mut self, x: LabelArgs) -> Result<(String, Value), Fail> {
        let code = read::parse_code(&x.code).ok_or_else(|| bad(format!("not a property code: {} (write it as 0xd381)", x.code)))?;
        let confidence = match x.confidence.as_deref() {
            None => None,
            Some(s) => Some(serde_json::from_value::<Confidence>(json!(s)).map_err(|_| bad(format!("no confidence {s}: pick confirmed, likely or guess")))?),
        };
        let patch = LabelPatch { code, name: x.name, menu: x.menu, values: x.values.into_iter().collect(), confidence, notes: x.notes };
        let c = self.link.client();
        conn::settled(&c, Some(Need::Connected))?;
        let cat = match c.call(Request::LabLabel { patch })? {
            Response::LabLabel(cat) => *cat,
            r => return Err(unexpected(r)),
        };
        let path = conn::snapshot(&c)?.catalog_path;
        let key = catalog::key(code);
        let entry = cat.properties.get(&key).cloned().unwrap_or_default();
        let text = format!(
            "{key} in the {} catalog: {}, {}{}",
            cat.model,
            entry.name.as_deref().unwrap_or("no name"),
            entry.confidence.map(|c| to_value(&c).as_str().unwrap_or_default().to_string()).unwrap_or_else(|| "no confidence".into()),
            path.as_deref().map(|p| format!(". Saved to {p}")).unwrap_or_default()
        );
        Ok((text, json!({ "model": cat.model, "path": path, "code": key, "entry": entry })))
    }
}

fn newest(groups: &[ChangeGroup]) -> u32 {
    groups.iter().map(|g| g.id).max().unwrap_or(0)
}

fn to_value<T: serde::Serialize>(t: &T) -> Value {
    serde_json::to_value(t).unwrap_or(Value::Null)
}

fn bad(message: impl Into<String>) -> Fail {
    Fail::new("bad_request", message)
}

fn unexpected(r: Response) -> Fail {
    Fail::new("failed", format!("unexpected reply: {}", to_value(&r)))
}

fn args<T: DeserializeOwned>(a: &Value) -> Result<T, Fail> {
    let a = if a.is_null() { json!({}) } else { a.clone() };
    serde_json::from_value(a).map_err(|e| bad(format!("bad arguments: {e}")))
}

fn parse_codes(codes: &[String]) -> Result<Vec<u16>, Fail> {
    codes.iter().map(|c| read::parse_code(c).ok_or_else(|| bad(format!("not a property code: {c} (write it as 0xd381)")))).collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WatchArgs {
    since: Option<u32>,
    #[serde(default)]
    codes: Vec<String>,
    wait_s: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsArgs {
    query: Option<String>,
    #[serde(default)]
    codes: Vec<String>,
    menu: Option<String>,
    filter: Option<String>,
    view: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanArgs {
    set: IndexMap<String, String>,
    #[serde(default)]
    allow_unconfirmed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyArgs {
    plan_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelArgs {
    code: String,
    name: Option<String>,
    menu: Option<String>,
    #[serde(default)]
    values: IndexMap<String, String>,
    confidence: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FakeArgs {
    action: String,
}

/// Reads JSON-RPC lines from stdin and answers on stdout until stdin closes.
pub fn main() -> i32 {
    if let Err(f) = conn::check_data_dir() {
        eprintln!("ukiyo mcp: {}", f.message);
        return 2;
    }
    let mut server = Server::new();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => server.handle(&msg),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {e}") } })),
        };
        if let Some(r) = reply {
            if writeln!(out, "{r}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
    if let Some(c) = server.link.client.take() {
        c.close();
    }
    0
}
