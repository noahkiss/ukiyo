mod events;
mod updater;

use events::{CatalogChanged, LabChanges, LabProps, LabStatus};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tauri_specta::Event as _;
use ukiyo_core::api::{Event, Request, Response};
use ukiyo_core::camera::owner::Status;
use ukiyo_core::lab::{FakeAction, Snapshot};
use ukiyo_core::library::prefs::Prefs;
use ukiyo_core::library::{self, SaveInput, SaveKind, Saved};
use ukiyo_core::serve::client::{self, Client, SpawnFn};
use ukiyo_core::{paths, recipes, serve};

// The app is a client of `ukiyo serve`, the one camera owner (docs/agent-interface.md, "One
// USB owner"). Each command sends its `api::Request` over the socket. The window's names and
// types are the api's. Errors reach the window as their message. The window writes only
// through plans, like the CLI: `plan_create`, then `plan_apply`; `serve` logs each write with
// the actor `app`. Every forwarded command runs off the
// main thread, since a call may wait for `serve` to start; they take the `AppHandle` rather
// than a borrowed `State` so they keep their plain return types.

/// Sends one request to `serve` and waits for the reply, on a blocking thread.
async fn forward(app: &AppHandle, req: Request) -> Result<Response, String> {
    let c = app.state::<Arc<Client>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || c.call(req)).await.map_err(|e| e.to_string())?.map_err(|e| e.message)
}

fn unexpected(r: Response) -> String {
    format!("unexpected reply from ukiyo serve: {r:?}")
}

/// Logs the failure of a command that returns nothing to the window.
fn log_failure(method: &str, r: Result<Response, String>) {
    if let Err(e) = r {
        eprintln!("ukiyo: {method}: {e}");
    }
}

/// The app's side of the method table (`ukiyo_core::ukiyo_api_table!`): one Tauri command per
/// `result` and `quiet` row, with the row's name, params and docs, forwarded to `serve`.
/// `own` rows are written below by hand. `api_commands` collects the generated commands and
/// the hand-written ones named in `[...]`.
macro_rules! tauri_commands {
    (
        [$($($own:ident)::+),* $(,)?]
        result {
            $(
                $(#[$m:meta])*
                $name:ident : $v:ident
                $( { $( $(#[$fm:meta])* $f:ident : $fty:ty ),* $(,)? } )?
                $( ( $nt:ident : $ntty:ty ) )?
                -> $ret:ty
                = |$lab:ident, $actor:ident| $body:expr;
            )*
        }
        quiet {
            $(
                $(#[$qm:meta])*
                $qname:ident : $qv:ident
                $( { $( $(#[$qfm:meta])* $qf:ident : $qfty:ty ),* $(,)? } )?
                = |$qlab:ident, $qactor:ident| $qbody:expr;
            )*
        }
        own { $($rest:tt)* }
    ) => {
        $(
            $(#[$m])*
            #[tauri::command]
            #[specta::specta]
            async fn $name(app: AppHandle $( $(, $f: $fty)* )? $(, $nt: $ntty)?) -> Result<$ret, String> {
                match forward(&app, Request::$v $( { $($f),* } )? $( ($nt) )?).await? {
                    Response::$v(out) => Ok(out),
                    r => Err(unexpected(r)),
                }
            }
        )*
        $(
            $(#[$qm])*
            #[tauri::command]
            #[specta::specta]
            async fn $qname(app: AppHandle $( $(, $qf: $qfty)* )?) {
                log_failure(stringify!($qname), forward(&app, Request::$qv $( { $($qf),* } )?).await)
            }
        )*
        fn api_commands() -> tauri_specta::Commands<tauri::Wry> {
            tauri_specta::collect_commands![$($($own)::+,)* $($name,)* $($qname,)*]
        }
    };
}

ukiyo_core::ukiyo_api_table!(tauri_commands [lab_snapshot, lab_fake, recipe_parse, library_save, recipe_save, prefs_get, updater::update_status, updater::update_check, updater::update_install]);

// Without `serve`, the window gets an empty snapshot whose status says why.
#[tauri::command]
#[specta::specta]
async fn lab_snapshot(app: AppHandle) -> Snapshot {
    match forward(&app, Request::LabSnapshot).await {
        Ok(Response::LabSnapshot(s)) => *s,
        other => Snapshot {
            status: Status { reason: Some(other.map_or_else(|e| e, unexpected)), ..Status::default() },
            rows: vec![],
            groups: vec![],
            catalog: None,
            catalog_path: None,
            session_path: None,
            fake: false,
            noisy: vec![],
            shots: vec![],
        },
    }
}

#[tauri::command]
#[specta::specta]
async fn lab_fake(app: AppHandle, action: FakeAction) -> Result<(), String> {
    forward(&app, Request::LabFake { action }).await.map(|_| ())
}

// The library: presets, snapshots, favourites, camera history, the trash. `serve` answers
// these from the data folder, and logs saves and deletes with the actor `app`.

/// A preset's settings, as `library_save` takes them from the window: `set` outright, or the
/// camera's current values of `settings`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SaveItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<indexmap::IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Creates or replaces a preset, snapshot or favourite, or restores one from the trash.
/// Never writes to the camera.
#[tauri::command]
#[specta::specta]
async fn library_save(app: AppHandle, kind: SaveKind, name: Option<String>, item: Option<SaveItem>, from: Option<String>) -> Result<Saved, String> {
    let item = item.map(|i| serde_json::to_value(i).map_err(|e| e.to_string())).transpose()?;
    match forward(&app, Request::LibrarySave(SaveInput { kind, name, item, from })).await? {
        Response::LibrarySave(s) => Ok(s),
        r => Err(unexpected(r)),
    }
}

/// A recipe as the window saves it (`library_save {kind: recipe}`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RecipeItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<recipes::Settings>,
    /// Label to value, as a post writes them, over the saved recipe of that name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<indexmap::IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<recipes::Note>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<library::RecipeSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Creates or replaces a library recipe; `from: cat:<id>` copies a catalogue recipe.
#[tauri::command]
#[specta::specta]
async fn recipe_save(app: AppHandle, name: Option<String>, item: Option<RecipeItem>, from: Option<String>) -> Result<Saved, String> {
    let item = item.map(|i| serde_json::to_value(i).map_err(|e| e.to_string())).transpose()?;
    match forward(&app, Request::LibrarySave(SaveInput { kind: SaveKind::Recipe, name, item, from })).await? {
        Response::LibrarySave(s) => Ok(s),
        r => Err(unexpected(r)),
    }
}

// Read in this process, not through `serve`: the window needs its theme before `serve` may
// have started.
/// The preferences in `settings.json`.
#[tauri::command]
#[specta::specta]
fn prefs_get() -> Prefs {
    ukiyo_core::api::call::prefs_get(&paths::data_dir())
}

// No shared state, so it runs in the app and works without `serve`; a URL is fetched on a
// blocking thread.
/// Parses pasted recipe text, or the recipe post at a URL, into slot settings, notes and
/// per-field warnings.
#[tauri::command]
#[specta::specta]
async fn recipe_parse(text: Option<String>, url: Option<String>) -> Result<recipes::Parsed, String> {
    tauri::async_runtime::spawn_blocking(move || ukiyo_core::api::call::recipe_parse(text.as_deref(), url.as_deref()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.message)
}

pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(api_commands())
        // The bundled recipe catalogue's shape; the window imports the JSON itself.
        .typ::<recipes::catalogue::RecipeCatalog>()
        .events(tauri_specta::collect_events![LabStatus, LabProps, LabChanges, CatalogChanged])
        // Wall-clock ms and codes fit a JS number.
        .dangerously_cast_bigints_to_number()
}

pub fn bindings_language() -> specta_typescript::Typescript {
    specta_typescript::Typescript::default()
        .header("// Generated by tauri-specta from src-tauri/src/lib.rs (ukiyo_lib::specta_builder).\n// Do not edit. Regenerate: UKIYO_UPDATE_BINDINGS=1 cargo test bindings\n")
}

/// `serve` is this same executable with the `serve` argument (`main.rs`), so the owner is
/// always the app's own build, in development and in the bundle.
fn spawn_fn(data_dir: std::path::PathBuf) -> SpawnFn {
    Arc::new(move || serve::spawn_detached(&std::env::current_exe()?, &data_dir))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let specta = specta_builder();
    #[cfg(debug_assertions)]
    {
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/src/bindings.ts");
        if let Err(e) = specta.export(bindings_language(), &out) {
            eprintln!("ukiyo: bindings.ts not written: {e}");
        }
    }
    let typed = specta.invoke_handler();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(typed)
        .setup(move |app| {
            specta.mount_events(app);
            updater::install(app.handle());
            // The same folder Tauri names `app_data_dir`; `serve` resolves it the same way.
            let data_dir = paths::data_dir();
            paths::prepare_data_dir(&data_dir)?;
            let handle = app.handle().clone();
            let on_event: client::EventFn = Arc::new(move |e| {
                let _ = match e {
                    Event::Status(x) => LabStatus(x).emit(&handle),
                    Event::Props(x) => LabProps(x).emit(&handle),
                    Event::Changes(x) => LabChanges(x).emit(&handle),
                    Event::Catalog(x) => CatalogChanged(x).emit(&handle),
                };
            });
            let opts = client::Options::new(data_dir.clone(), "app", Some(spawn_fn(data_dir)));
            let client = Client::new(opts, Some(on_event));
            // The app is a client of `serve` for as long as it is open.
            client.keep_connected();
            app.manage(client);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("ukiyo starts");
    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            // `serve` keeps the camera for its idle time, then disconnects cleanly.
            if let Some(c) = handle.try_state::<Arc<Client>>() {
                c.close();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn bindings() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("bindings.ts");
        super::specta_builder().export(super::bindings_language(), &out).unwrap();
        let fresh = std::fs::read_to_string(out).unwrap();
        let committed = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/src/bindings.ts");
        if std::env::var_os("UKIYO_UPDATE_BINDINGS").is_some() {
            std::fs::write(&committed, &fresh).unwrap();
            return;
        }
        let on_disk = std::fs::read_to_string(&committed).unwrap_or_default();
        assert!(on_disk == fresh, "app/src/bindings.ts is out of date: UKIYO_UPDATE_BINDINGS=1 cargo test bindings");
    }

    /// The hosts of every https `url` and `homepage` in the bundled recipe catalogue.
    fn catalogue_link_hosts(root: &std::path::Path) -> std::collections::BTreeSet<String> {
        fn walk(v: &serde_json::Value, out: &mut std::collections::BTreeSet<String>) {
            match v {
                serde_json::Value::Object(m) => {
                    for (k, v) in m {
                        if let (true, Some(u)) = (k == "url" || k == "homepage", v.as_str()) {
                            if let Some(host) = https_host(u) {
                                out.insert(host);
                            }
                        }
                        walk(v, out);
                    }
                }
                serde_json::Value::Array(a) => a.iter().for_each(|v| walk(v, out)),
                _ => {}
            }
        }
        let dir = root.join("catalog/recipes");
        let mut files = vec![dir.join("catalog.json")];
        files.extend(std::fs::read_dir(dir.join("sources")).unwrap().map(|e| e.unwrap().path()));
        let mut out = std::collections::BTreeSet::new();
        for f in files.iter().filter(|f| f.extension().is_some_and(|x| x == "json")) {
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
            walk(&v, &mut out);
        }
        out
    }

    /// The host of an https URL, or None when the URL is not https or the host is malformed.
    fn https_host(u: &str) -> Option<String> {
        let rest = u.strip_prefix("https://")?;
        let host = rest.split(['/', '?', '#']).next()?.to_ascii_lowercase();
        let ok = host.contains('.') && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        ok.then_some(host)
    }

    /// `capabilities/links.json` lets the window open https links on the catalogue's hosts only.
    #[test]
    fn link_hosts() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let allow: Vec<_> = catalogue_link_hosts(&manifest.join(".."))
            .into_iter()
            .map(|h| serde_json::json!({ "url": format!("https://{h}/*") }))
            .collect();
        let cap = serde_json::json!({
            "$schema": "../gen/schemas/desktop-schema.json",
            "identifier": "links",
            "description": "Open recipe links in the default browser: https on the recipe catalogue's hosts only. Generated: UKIYO_UPDATE_LINK_HOSTS=1 cargo test link_hosts",
            "windows": ["main"],
            "permissions": [{ "identifier": "opener:allow-open-url", "allow": allow }]
        });
        let fresh = serde_json::to_string_pretty(&cap).unwrap() + "\n";
        let committed = manifest.join("capabilities/links.json");
        if std::env::var_os("UKIYO_UPDATE_LINK_HOSTS").is_some() {
            std::fs::write(&committed, &fresh).unwrap();
            return;
        }
        let on_disk = std::fs::read_to_string(&committed).unwrap_or_default();
        assert!(on_disk == fresh, "capabilities/links.json is out of date: UKIYO_UPDATE_LINK_HOSTS=1 cargo test link_hosts");
    }

    #[test]
    fn https_host_rejects_http_and_malformed() {
        assert_eq!(https_host("https://Film.Recipes/2024/x/?a#b").as_deref(), Some("film.recipes"));
        assert_eq!(https_host("http://fujixweekly.com/"), None);
        assert_eq!(https_host("https://fujixweekly-com/2019/"), None);
        assert_eq!(https_host("https://a.com@evil.com/"), None);
    }
}
