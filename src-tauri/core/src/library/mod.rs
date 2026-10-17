//! The library: what ukiyo keeps on this Mac (docs/agent-interface.md, section 8, stage 6).
//!
//! In the data folder, which is local and never committed:
//!
//! | Path | Holds |
//! |---|---|
//! | `settings.json` | preferences (`prefs`) |
//! | `library/presets/<slug>.json` | a preset: named settings and their values |
//! | `library/snapshots/<slug>.json` | a snapshot: the camera's settings at one moment |
//! | `library/favourites.json` | the favourite settings, by code |
//! | `library/.trash/<stamp>-<kind>-<slug>.json` | deleted and replaced items, 30 days |
//! | `library/log.jsonl` | every save, delete and restore, with its actor |
//! | `library/.lock` | the `flock` every write holds |
//! | `history/<camera id>.json`, `history/.salt` | per-camera history (`history`) |
//!
//! Every file carries `schema`. Every write holds `library/.lock` and goes to a temp file
//! that is renamed into place, so a reader never sees half a file and two processes never
//! lose each other's change. Any process may use the library: the CLI and the MCP server
//! call it in their own process, and go through `serve` only to capture a snapshot or apply
//! an item. The window sends every library call to `serve`, except `prefs_get`.
//!
//! Ids: `preset:<slug>`, `snapshot:<slug>`, `favourite:<code>`, `favourites`, `prefs`,
//! `history`, `history:<camera id>`, `trash:<name>`.

pub mod history;
pub mod prefs;
pub mod snapshot;

use crate::api::{Error, ErrorCode};
use crate::catalog::{self, Catalog};
use crate::read::{self, RawValue};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub use snapshot::{Diff, SavedValue, SettingsSnapshot};

pub const SCHEMA: u32 = 1;
/// How long a trashed item can be restored.
pub const TRASH_DAYS: i64 = 30;

fn failed(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::Failed, e.to_string())
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        failed(e)
    }
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Writes `bytes` to a temp file beside `path`, then renames it into place. Mode 0600.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.{}.tmp", path.file_name().and_then(|n| n.to_str()).unwrap_or("file"), std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// The held library lock; dropping it releases the `flock`.
pub struct Guard(#[allow(dead_code)] std::fs::File);

/// A name as a file name: `Before trip` is `before-trip`.
pub fn slug(name: &str) -> String {
    catalog::slug(name)
}

/// The kinds of item the library stores in files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Preset,
    Snapshot,
}

impl Kind {
    fn dir(self) -> &'static str {
        match self {
            Kind::Preset => "presets",
            Kind::Snapshot => "snapshots",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Preset => "preset",
            Kind::Snapshot => "snapshot",
        }
    }

    fn parse(s: &str) -> Option<Kind> {
        match s {
            "preset" => Some(Kind::Preset),
            "snapshot" => Some(Kind::Snapshot),
            _ => None,
        }
    }
}

/// A parsed id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Id {
    Item(Kind, String),
    Favourite(u16),
    Favourites,
    Prefs,
    History(Option<String>),
    Trash(String),
    /// `cat:`, `recipe:` and `slot:` ids: the recipe library (stage 7).
    Recipe(String),
}

impl Id {
    pub fn parse(id: &str) -> Result<Id, Error> {
        let id = id.trim();
        let bad = || Error::new(ErrorCode::NotFound, format!("no such id {id:?}")).hint("Ids: preset:<name>, snapshot:<name>, favourite:<code>, favourites, prefs, history, trash:<name>.");
        Ok(match id.split_once(':') {
            None => match id {
                "favourites" => Id::Favourites,
                "prefs" => Id::Prefs,
                "history" => Id::History(None),
                _ => return Err(bad()),
            },
            Some((k, rest)) if !rest.trim().is_empty() => match k {
                "preset" | "snapshot" => Id::Item(Kind::parse(k).unwrap(), slug(rest)),
                "favourite" => Id::Favourite(read::parse_code(rest).ok_or_else(bad)?),
                "history" => Id::History(Some(rest.to_string())),
                "trash" => Id::Trash(rest.to_string()),
                "cat" | "recipe" | "slot" => Id::Recipe(id.to_string()),
                _ => return Err(bad()),
            },
            Some(_) => return Err(bad()),
        })
    }

    /// An id from the CLI, where `presets show street` means `preset:street`.
    pub fn of_kind(kind: Kind, s: &str) -> Result<Id, Error> {
        if s.contains(':') {
            let id = Id::parse(s)?;
            match &id {
                Id::Item(k, _) if *k == kind => Ok(id),
                _ => Err(Error::new(ErrorCode::NotFound, format!("{s} is not a {} id", kind.name()))),
            }
        } else {
            Ok(Id::Item(kind, slug(s)))
        }
    }
}

pub fn item_id(kind: Kind, slug: &str) -> String {
    format!("{}:{slug}", kind.name())
}

fn not_built(what: &str) -> Error {
    Error::new(ErrorCode::NotFound, format!("{what} come with the recipe library, which is not built yet")).hint("docs/agent-interface.md, section 8, stage 7")
}

/// A preset: settings by name or code, to value labels or raw values. Applying it is a plan
/// with exactly this `set` (`plan_create`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Preset {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub created_at: f64,
    pub updated_at: f64,
    /// The model it was saved from, when known.
    #[serde(default)]
    pub model: Option<String>,
    pub set: IndexMap<String, String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// One favourite setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Favourite {
    pub code: String,
    /// The catalog name when it was added.
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct Favourites {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub items: Vec<Favourite>,
}

impl Favourites {
    pub fn codes(&self) -> Vec<u16> {
        self.items.iter().filter_map(|f| read::parse_code(&f.code)).collect()
    }
}

/// A search hit.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Hit {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub summary: String,
    /// Wall-clock ms of the last save, or of the trashing for a trashed item.
    pub at: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Hits {
    pub items: Vec<Hit>,
    pub total: u32,
    pub has_more: bool,
}

/// What `library_search` looks through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum SearchKind {
    Preset,
    Snapshot,
    Favourite,
    Trash,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SearchInput {
    pub kind: SearchKind,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// One item as `library_read` returns it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "item", rename_all = "lowercase")]
pub enum Read {
    Preset(Preset),
    Snapshot(Box<SettingsSnapshot>),
    Favourites(Favourites),
    Prefs(Box<prefs::Prefs>),
    History(Vec<history::HistoryView>),
    Diff(Diff),
}

/// What `library_save` stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum SaveKind {
    Preset,
    Snapshot,
    Favourite,
    Prefs,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SaveInput {
    pub kind: SaveKind,
    /// The item's name: a preset or snapshot name, or a setting for a favourite.
    #[serde(default)]
    pub name: Option<String>,
    /// - preset: `{ "set": { "<name or code>": "<label or raw>" }, "notes"?: "..." }`, or
    ///   `{ "settings": ["<name or code>", ...] }` to take those settings' values from the camera now
    /// - prefs: a `PrefsPatch`
    #[serde(default)]
    #[specta(type = Option<serde_json::Value>)]
    pub item: Option<serde_json::Value>,
    /// preset: `snapshot:<slug>` copies a snapshot's values. Any kind: `trash:<name>`
    /// restores a trashed item.
    #[serde(default)]
    pub from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Saved {
    pub id: String,
    pub kind: String,
    /// An item of that id existed; its previous version is in the trash.
    pub replaced: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Trashed {
    pub id: String,
    /// Wall-clock ms; `None` for a favourite, which is removed from the list, not trashed.
    pub trashed_at: Option<f64>,
    /// `trash:<name>`, for `library_save {from}` to restore it.
    pub trash_id: Option<String>,
}

/// What a save may use besides its input: the camera now (through `serve`), or a catalog.
#[derive(Default, Clone, Copy)]
pub struct Context<'a> {
    pub camera: Option<&'a crate::lab::Snapshot>,
    pub catalog: Option<&'a Catalog>,
}

impl<'a> Context<'a> {
    fn catalog(&self) -> Option<&'a Catalog> {
        self.camera.and_then(|c| c.catalog.as_ref()).or(self.catalog)
    }
}

/// The library in one data folder.
#[derive(Debug, Clone)]
pub struct Store {
    data_dir: PathBuf,
}

impl Store {
    pub fn new(data_dir: &Path) -> Store {
        Store { data_dir: data_dir.to_path_buf() }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn root(&self) -> PathBuf {
        self.data_dir.join("library")
    }

    fn trash_dir(&self) -> PathBuf {
        self.root().join(".trash")
    }

    fn path(&self, kind: Kind, slug: &str) -> PathBuf {
        self.root().join(kind.dir()).join(format!("{slug}.json"))
    }

    fn favourites_path(&self) -> PathBuf {
        self.root().join("favourites.json")
    }

    /// Takes `library/.lock` (blocking). Every write holds it.
    pub fn lock(&self) -> std::io::Result<Guard> {
        use std::os::fd::AsRawFd as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        std::fs::create_dir_all(self.root())?;
        let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).mode(0o600).open(self.root().join(".lock"))?;
        // SAFETY: flock on a descriptor this function owns.
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Guard(f))
    }

    /// One line in `library/log.jsonl`. Best effort.
    fn log(&self, actor: &str, op: &str, id: &str, extra: serde_json::Value) {
        let mut v = serde_json::json!({ "t": now_ms(), "actor": actor, "op": op, "id": id });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.root().join("log.jsonl")) {
            let _ = writeln!(f, "{v}");
        }
    }

    fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, Error> {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).map(Some).map_err(|e| failed(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn missing(id: &str) -> Error {
        Error::new(ErrorCode::NotFound, format!("no {id} in the library")).hint("library_search, or ukiyo presets ls / ukiyo snapshots ls, lists what is there.")
    }

    pub fn preset(&self, slug: &str) -> Result<Preset, Error> {
        Self::load(&self.path(Kind::Preset, slug))?.ok_or_else(|| Self::missing(&item_id(Kind::Preset, slug)))
    }

    pub fn snapshot(&self, slug: &str) -> Result<SettingsSnapshot, Error> {
        Self::load(&self.path(Kind::Snapshot, slug))?.ok_or_else(|| Self::missing(&item_id(Kind::Snapshot, slug)))
    }

    fn all<T: serde::de::DeserializeOwned>(&self, kind: Kind) -> Vec<T> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(self.root().join(kind.dir()))
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
            .unwrap_or_default();
        paths.sort();
        paths.iter().filter_map(|p| Self::load(p).ok().flatten()).collect()
    }

    pub fn presets(&self) -> Vec<Preset> {
        let mut v: Vec<Preset> = self.all(Kind::Preset);
        v.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
        v
    }

    pub fn snapshots(&self) -> Vec<SettingsSnapshot> {
        let mut v: Vec<SettingsSnapshot> = self.all(Kind::Snapshot);
        v.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
        v
    }

    pub fn favourites(&self) -> Favourites {
        Self::load(&self.favourites_path()).ok().flatten().unwrap_or_default()
    }

    /// Moves a file into the trash; returns the trash name. Caller holds the lock.
    fn to_trash(&self, kind: Kind, slug: &str) -> Result<Option<(String, f64)>, Error> {
        let from = self.path(kind, slug);
        if !from.exists() {
            return Ok(None);
        }
        self.purge_trash();
        std::fs::create_dir_all(self.trash_dir())?;
        let at = now_ms();
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%3f");
        let name = format!("{stamp}-{}-{slug}", kind.name());
        std::fs::rename(&from, self.trash_dir().join(format!("{name}.json")))?;
        Ok(Some((name, at as f64)))
    }

    /// Drops trashed items older than [`TRASH_DAYS`].
    fn purge_trash(&self) {
        let limit = std::time::Duration::from_secs(TRASH_DAYS as u64 * 86_400);
        for e in std::fs::read_dir(self.trash_dir()).into_iter().flatten().flatten() {
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > limit);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// Writes an item, trashing the one it replaces. Caller holds the lock.
    fn put<T: Serialize>(&self, kind: Kind, slug: &str, item: &T) -> Result<bool, Error> {
        let replaced = self.to_trash(kind, slug)?.is_some();
        let text = serde_json::to_string_pretty(item).map_err(failed)?;
        write_atomic(&self.path(kind, slug), text.as_bytes())?;
        Ok(replaced)
    }

    pub fn save_preset(&self, actor: &str, mut p: Preset) -> Result<Saved, Error> {
        let slug = slug(&p.name);
        let _g = self.lock()?;
        let old: Option<Preset> = Self::load(&self.path(Kind::Preset, &slug))?;
        p.schema = SCHEMA;
        p.id = item_id(Kind::Preset, &slug);
        p.updated_at = now_ms() as f64;
        p.created_at = old.map_or(p.updated_at, |o| o.created_at);
        let replaced = self.put(Kind::Preset, &slug, &p)?;
        self.log(actor, "save", &p.id, serde_json::json!({ "replaced": replaced }));
        Ok(Saved { id: p.id, kind: "preset".into(), replaced, warnings: vec![] })
    }

    pub fn save_snapshot(&self, actor: &str, mut s: SettingsSnapshot) -> Result<Saved, Error> {
        let slug = slug(&s.name);
        let _g = self.lock()?;
        s.schema = SCHEMA;
        s.id = item_id(Kind::Snapshot, &slug);
        let replaced = self.put(Kind::Snapshot, &slug, &s)?;
        self.log(actor, "save", &s.id, serde_json::json!({ "replaced": replaced, "values": s.values.len() }));
        Ok(Saved { id: s.id, kind: "snapshot".into(), replaced, warnings: vec![] })
    }

    /// Adds favourites by code; `name` is each one's catalog name. Already there is fine.
    pub fn add_favourites(&self, actor: &str, items: Vec<Favourite>) -> Result<Saved, Error> {
        let _g = self.lock()?;
        let mut f = self.favourites();
        let mut replaced = false;
        let ids: Vec<String> = items.iter().map(|i| format!("favourite:{}", i.code)).collect();
        for it in items {
            match f.items.iter_mut().find(|x| x.code == it.code) {
                Some(x) => {
                    replaced = true;
                    x.name = it.name.or(x.name.take());
                }
                None => f.items.push(it),
            }
        }
        f.schema = SCHEMA;
        write_atomic(&self.favourites_path(), serde_json::to_string_pretty(&f).map_err(failed)?.as_bytes())?;
        let id = ids.join(", ");
        self.log(actor, "save", &id, serde_json::json!({}));
        Ok(Saved { id, kind: "favourite".into(), replaced, warnings: vec![] })
    }

    pub fn remove_favourite(&self, actor: &str, code: u16) -> Result<Trashed, Error> {
        let key = catalog::key(code);
        let id = format!("favourite:{key}");
        let _g = self.lock()?;
        let mut f = self.favourites();
        let n = f.items.len();
        f.items.retain(|x| x.code != key);
        if f.items.len() == n {
            return Err(Self::missing(&id));
        }
        f.schema = SCHEMA;
        write_atomic(&self.favourites_path(), serde_json::to_string_pretty(&f).map_err(failed)?.as_bytes())?;
        self.log(actor, "delete", &id, serde_json::json!({}));
        Ok(Trashed { id, trashed_at: None, trash_id: None })
    }

    /// Deletes a preset or snapshot (to the trash), or removes a favourite.
    pub fn delete(&self, actor: &str, id: &str) -> Result<Trashed, Error> {
        match Id::parse(id)? {
            Id::Item(kind, slug) => {
                let _g = self.lock()?;
                let id = item_id(kind, &slug);
                let (name, at) = self.to_trash(kind, &slug)?.ok_or_else(|| Self::missing(&id))?;
                self.log(actor, "delete", &id, serde_json::json!({ "trash": name }));
                Ok(Trashed { id, trashed_at: Some(at), trash_id: Some(format!("trash:{name}")) })
            }
            Id::Favourite(code) => self.remove_favourite(actor, code),
            Id::Recipe(_) => Err(not_built("recipes")),
            other => Err(Error::new(ErrorCode::ReadOnly, format!("{id} cannot be deleted")).hint(match other {
                Id::Trash(_) => "A trashed item is dropped after 30 days.",
                _ => "Only presets, snapshots and favourites are deleted.",
            })),
        }
    }

    /// Trashed items, newest first.
    pub fn trash(&self) -> Vec<Hit> {
        self.purge_trash();
        let mut v: Vec<Hit> = std::fs::read_dir(self.trash_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.path().file_stem()?.to_str()?.to_string();
                let (kind, slug) = trash_parts(&name)?;
                let at = e.metadata().ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as f64;
                let item: serde_json::Value = Self::load(&e.path()).ok().flatten()?;
                Some(Hit {
                    id: format!("trash:{name}"),
                    kind: kind.name().into(),
                    name: item["name"].as_str().unwrap_or(&slug).to_string(),
                    summary: format!("{} {}, restores as {}", kind.name(), item["name"].as_str().unwrap_or(&slug), item_id(kind, &slug)),
                    at: Some(at),
                })
            })
            .collect();
        v.sort_by(|a, b| b.id.cmp(&a.id));
        v
    }

    /// Puts a trashed item back. An item now at its id goes to the trash in its place.
    pub fn restore(&self, actor: &str, trash: &str) -> Result<Saved, Error> {
        let name = trash.strip_prefix("trash:").unwrap_or(trash);
        let (kind, slug) = trash_parts(name).ok_or_else(|| Self::missing(trash))?;
        let _g = self.lock()?;
        let from = self.trash_dir().join(format!("{name}.json"));
        if !from.exists() {
            return Err(Self::missing(&format!("trash:{name}")));
        }
        let replaced = self.to_trash(kind, &slug)?.is_some();
        std::fs::create_dir_all(self.root().join(kind.dir()))?;
        std::fs::rename(&from, self.path(kind, &slug))?;
        let id = item_id(kind, &slug);
        self.log(actor, "restore", &id, serde_json::json!({ "from": name, "replaced": replaced }));
        Ok(Saved { id, kind: kind.name().into(), replaced, warnings: vec![] })
    }

    pub fn search(&self, input: &SearchInput) -> Hits {
        let words: Vec<String> = input.query.as_deref().unwrap_or("").split_whitespace().map(str::to_lowercase).collect();
        let all: Vec<(Hit, String)> = match input.kind {
            SearchKind::Preset => self
                .presets()
                .into_iter()
                .map(|p| {
                    let keys: Vec<&str> = p.set.keys().map(String::as_str).collect();
                    let hay = format!("{} {} {}", p.name, keys.join(" "), p.notes.clone().unwrap_or_default());
                    (Hit { id: p.id.clone(), kind: "preset".into(), name: p.name.clone(), summary: preset_summary(&p), at: Some(p.updated_at) }, hay)
                })
                .collect(),
            SearchKind::Snapshot => self
                .snapshots()
                .into_iter()
                .map(|s| {
                    let hay = format!("{} {}", s.name, s.model.clone().unwrap_or_default());
                    (Hit { id: s.id.clone(), kind: "snapshot".into(), name: s.name.clone(), summary: s.summary(), at: Some(s.created_at) }, hay)
                })
                .collect(),
            SearchKind::Favourite => self
                .favourites()
                .items
                .into_iter()
                .map(|f| {
                    let name = f.name.clone().unwrap_or_else(|| f.code.clone());
                    let hay = format!("{} {}", f.code, name);
                    (Hit { id: format!("favourite:{}", f.code), kind: "favourite".into(), summary: format!("{} {name}", f.code), name, at: None }, hay)
                })
                .collect(),
            SearchKind::Trash => self.trash().into_iter().map(|h| (h.clone(), format!("{} {}", h.name, h.id))).collect(),
        };
        let hits: Vec<Hit> = all.into_iter().filter(|(_, hay)| words.iter().all(|w| hay.to_lowercase().contains(w))).map(|(h, _)| h).collect();
        let total = hits.len();
        let limit = input.limit.unwrap_or(20).max(1) as usize;
        Hits { has_more: total > limit, total: total as u32, items: hits.into_iter().take(limit).collect() }
    }

    pub fn read(&self, id: &str, compare_to: Option<&str>) -> Result<Read, Error> {
        if let Some(b) = compare_to {
            let (Id::Item(Kind::Snapshot, a), Id::Item(Kind::Snapshot, b)) = (Id::parse(id)?, Id::parse(b)?) else {
                return Err(Error::new(ErrorCode::NotFound, "compare_to compares two snapshots").hint("For a snapshot against the camera, plan it: camera_plan {snapshot}."));
            };
            return Ok(Read::Diff(snapshot::diff(&self.snapshot(&a)?, &self.snapshot(&b)?)));
        }
        Ok(match Id::parse(id)? {
            Id::Item(Kind::Preset, s) => Read::Preset(self.preset(&s)?),
            Id::Item(Kind::Snapshot, s) => Read::Snapshot(Box::new(self.snapshot(&s)?)),
            Id::Favourites | Id::Favourite(_) => Read::Favourites(self.favourites()),
            Id::Prefs => Read::Prefs(Box::new(prefs::load(&self.data_dir))),
            Id::History(key) => {
                let all = history::views(&self.data_dir);
                Read::History(match key {
                    None => all,
                    Some(k) => {
                        let v: Vec<_> = all.into_iter().filter(|h| h.camera == k).collect();
                        if v.is_empty() {
                            return Err(Self::missing(&format!("history:{k}")));
                        }
                        v
                    }
                })
            }
            Id::Trash(_) => return Err(Error::new(ErrorCode::NotFound, "a trashed item is read after it is restored").hint("library_save {from: \"trash:...\"}")),
            Id::Recipe(_) => return Err(not_built("recipes and slots")),
        })
    }

    /// `library_save`. `ctx` gives the camera (for a snapshot, or a preset of current values)
    /// and the catalog (to resolve names).
    pub fn save(&self, actor: &str, input: &SaveInput, ctx: Context) -> Result<Saved, Error> {
        if let Some(from) = input.from.as_deref().filter(|f| f.starts_with("trash:")) {
            return self.restore(actor, from);
        }
        let item = input.item.clone().unwrap_or(serde_json::Value::Null);
        match input.kind {
            SaveKind::Prefs => {
                let patch: prefs::PrefsPatch = serde_json::from_value(item).map_err(|e| Error::new(ErrorCode::BadRequest, format!("prefs: {e}")))?;
                if patch.is_empty() {
                    return Err(Error::new(ErrorCode::BadRequest, "prefs: name at least one of theme, brand_mark, app_icon, keep_awake, in_use_minutes, close_menu_to_write"));
                }
                prefs::update(&self.data_dir, &patch)?;
                self.log(actor, "save", "prefs", serde_json::json!({ "patch": patch }));
                Ok(Saved { id: "prefs".into(), kind: "prefs".into(), replaced: true, warnings: vec![] })
            }
            SaveKind::Favourite => {
                let names: Vec<String> = match (&input.name, item.as_array()) {
                    (Some(n), _) => vec![n.clone()],
                    (None, Some(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
                    _ => vec![],
                };
                if names.is_empty() {
                    return Err(Error::new(ErrorCode::BadRequest, "favourite: give the setting as name (a catalog name, menu label or 0x code)"));
                }
                let favs = names.iter().map(|n| resolve_favourite(ctx, n)).collect::<Result<Vec<_>, _>>()?;
                self.add_favourites(actor, favs)
            }
            SaveKind::Snapshot => {
                let cam = ctx.camera.ok_or_else(|| Error::new(ErrorCode::NotConnected, "a snapshot reads the camera, and none answered"))?;
                let name = input.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d %H%M").to_string());
                let s = snapshot::capture(cam, &name)?;
                self.save_snapshot(actor, s)
            }
            SaveKind::Preset => {
                let name = input.name.clone().filter(|n| !slug(n).is_empty() && slug(n) != "unknown").ok_or_else(|| Error::new(ErrorCode::BadRequest, "preset: give it a name"))?;
                let notes = item.get("notes").and_then(|v| v.as_str()).map(str::to_string);
                let mut warnings = vec![];
                let (set, model) = if let Some(from) = input.from.as_deref() {
                    let Id::Item(Kind::Snapshot, s) = Id::parse(from)? else {
                        return Err(Error::new(ErrorCode::BadRequest, "preset: from is a snapshot:<name> or a trash:<name>"));
                    };
                    let snap = self.snapshot(&s)?;
                    (snapshot::as_set(&snap), snap.model.clone())
                } else if let Some(names) = item.get("settings").and_then(|v| v.as_array()) {
                    let cam = ctx.camera.ok_or_else(|| Error::new(ErrorCode::NotConnected, "preset settings are read from the camera, and none answered"))?;
                    let names: Vec<String> = names.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
                    (snapshot::current_set(cam, &names)?, cam.status.model.clone())
                } else if let Some(set) = item.get("set").and_then(|v| v.as_object()) {
                    let mut m = IndexMap::new();
                    for (k, v) in set {
                        let v = match v {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Number(n) => n.to_string(),
                            other => return Err(Error::new(ErrorCode::BadRequest, format!("preset: the value of {k} is {other}, not a label or a number"))),
                        };
                        m.insert(k.clone(), v);
                    }
                    if let Some(cam) = ctx.camera {
                        warnings = snapshot::check_set(cam, &m);
                    }
                    (m, ctx.camera.and_then(|c| c.status.model.clone()))
                } else {
                    return Err(Error::new(ErrorCode::BadRequest, "preset: give item.set ({name: value}), item.settings ([names] read from the camera), or from: snapshot:<name>"));
                };
                if set.is_empty() {
                    return Err(Error::new(ErrorCode::BadRequest, "preset: it names no setting"));
                }
                let p = Preset { schema: SCHEMA, id: String::new(), name, created_at: 0.0, updated_at: 0.0, model, set, notes };
                let mut saved = self.save_preset(actor, p)?;
                saved.warnings = warnings;
                Ok(saved)
            }
        }
    }
}

fn trash_parts(name: &str) -> Option<(Kind, String)> {
    // <yyyymmdd>-<hhmmssmmm>-<kind>-<slug>
    let mut it = name.splitn(4, '-');
    let (_, _, kind, slug) = (it.next()?, it.next()?, it.next()?, it.next()?);
    Some((Kind::parse(kind)?, slug.to_string()))
}

fn preset_summary(p: &Preset) -> String {
    let n = p.set.len();
    let first: Vec<String> = p.set.iter().take(3).map(|(k, v)| format!("{k}={v}")).collect();
    let more = if n > 3 { format!(" and {} more", n - 3) } else { String::new() };
    format!("{n} setting{}: {}{more}", if n == 1 { "" } else { "s" }, first.join(", "))
}

/// A favourite from a name: a `0x` code, or a catalog name or menu label.
fn resolve_favourite(ctx: Context, name: &str) -> Result<Favourite, Error> {
    let cat = ctx.catalog();
    let code = match ctx.camera {
        Some(cam) => crate::plan::resolve_code(cam, name)?,
        None => {
            let snap = crate::lab::Snapshot::of_catalog(cat.cloned());
            crate::plan::resolve_code(&snap, name)?
        }
    };
    let name = cat.and_then(|c| c.properties.get(&catalog::key(code))).and_then(|e| e.name.clone());
    Ok(Favourite { code: catalog::key(code), name })
}

/// A raw value from a saved one.
pub fn value_of(v: &RawValue) -> crate::camera::ptp::Value {
    use crate::camera::ptp::Value;
    match v {
        RawValue::Int(n) => Value::Int(*n),
        RawValue::Str(s) => Value::Str(s.clone()),
        RawValue::Array(a) => Value::Array(a.clone()),
    }
}

#[cfg(test)]
mod tests;
