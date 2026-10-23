//! The one surface every front end speaks (docs/agent-interface.md, section 2).
//!
//! - `ukiyo_api_table!`: the method table, every method once (QuadCam's `api!`). It makes
//!   `Request`, `Response`, `METHODS`, `Request::method` and `dispatch` here, and the app's
//!   Tauri commands in `src-tauri/src/lib.rs`. A new method is one row.
//! - `Request`: one variant per method. On the wire it is `{"method": "plan_create",
//!   "params": {"set": {"AutoPowerOff": "OFF"}}}`; a method with no params
//!   leaves `params` out. Method names are the window's Tauri command names.
//! - `Response`: one variant per method, carrying that method's result:
//!   `{"method": "plan_create", "result": {...}}`.
//! - `Error`: a typed code (section 5, "Typed errors"), a message, and what fixes it.
//! - `Event`: what the Lab reports while it runs. `ukiyo serve` sends them to every
//!   subscribed client (`serve::wire`); the app turns them into Tauri events.
//! - `call`: one typed function per method. The table's rows call these, and the CLI calls the
//!   library ones directly, so a method behaves the same on every surface.
//!
//! - `dispatch` takes the caller's `actor` (`app`, `cli`, `mcp:<client>`), which goes into
//!   the session log with every write, label and note.
//!
//! Camera writes exist only as plans: `plan_create`, then `plan_apply` (section 5).

use crate::camera::owner::{ChangeGroup, PropRow, Status};
use crate::catalog::{Catalog, LabelPatch};
use crate::lab::{FakeAction, Lab, Snapshot};
use crate::plan::{Applied, Plan, PlanInput};
use crate::library::prefs::{Prefs, PrefsPatch};
use crate::library::{self, Hits, SaveInput, Saved, SearchInput, Store, Trashed};
use crate::recipes::{self, Parsed};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::Path;

/// The method table: every method once. Each row names the method, its `Request` variant and
/// params, its result (none for a method with no result), and the body `dispatch` runs:
///
/// ```text
/// /// Doc comment.
/// method_name: Variant { field: Type, ... } -> ResultType = |lab, actor| body;
/// method_name: Variant(input: Type) -> ResultType = |lab, actor| body;
/// method_name: Variant = |lab, actor| body;
/// ```
///
/// A body returns `Result<ResultType, Error>` (`Result<(), Error>` without a result). Types and
/// calls are `$crate::` paths, since the app crate expands the table too.
///
/// The rows sit in three groups, by what the window does with them:
///
/// - `result`: the app forwards the method to `serve` as a Tauri command of the same name and
///   params, returning its result.
/// - `quiet`: the same, returning nothing; a failure goes to the app's log.
/// - `own`: the app writes its own command, or has none (`plan_show`).
///
/// `ukiyo_api_table!(gen)` calls the macro `gen` with the rows; `ukiyo_api_table!(gen [x])`
/// passes `[x]` first. Here it makes `Request`, `Response`, `METHODS`, `Request::method` and
/// `dispatch`; in `src-tauri/src/lib.rs` it makes the Tauri commands. A new method is one row.
#[macro_export]
macro_rules! ukiyo_api_table {
    ($gen:ident $([$($extra:tt)*])?) => {
        $gen! {
            $([$($extra)*])?
            result {
                /// Writes a full property dump to `dumps/` and returns its path.
                lab_export: LabExport -> String = |lab, actor| $crate::api::call::lab_export(lab, actor);
                /// Labels one property in the model's catalog and saves the catalog.
                lab_label: LabLabel { patch: $crate::catalog::LabelPatch } -> Box<$crate::catalog::Catalog> =
                    |lab, actor| $crate::api::call::lab_label(lab, actor, patch).map(Box::new);
                /// Builds a plan against the live camera: names or codes to labels or raw values.
                /// Writes nothing.
                plan_create: PlanCreate {
                    #[serde(default)]
                    set: indexmap::IndexMap<String, String>,
                    #[serde(default)]
                    allow_unconfirmed: bool,
                    /// A library item to plan instead of `set`: `preset:<slug>`, `snapshot:<slug>`, or a
                    /// recipe (`recipe:<slug>`, `cat:<id>`) with `slot`.
                    #[serde(default)]
                    from: Option<String>,
                    /// The custom slot a recipe goes to: `C1` to `C7`.
                    #[serde(default)]
                    slot: Option<String>,
                } -> Box<$crate::plan::Plan> = |lab, actor| {
                    $crate::api::call::plan_create(lab, $crate::plan::PlanInput { set, allow_unconfirmed, from, slot }).map(Box::new)
                };
                /// Re-checks a plan, writes its steps in order, reads each back, and logs them.
                plan_apply: PlanApply { plan_id: String } -> Box<$crate::plan::Applied> =
                    |lab, actor| $crate::api::call::plan_apply(lab, actor, &plan_id).map(Box::new);
                /// Searches the library on this Mac: presets, snapshots, favourites, the trash.
                library_search: LibrarySearch(input: $crate::library::SearchInput) -> $crate::library::Hits =
                    |lab, actor| Ok($crate::api::call::library_search(lab.data_dir(), &input));
                /// One library item in full, or the diff of two items. `slot:C1` .. `slot:C7` and
                /// `slots` read the camera (RAW CONV. mode).
                library_read: LibraryRead {
                    id: String,
                    #[serde(default)]
                    compare_to: Option<String>,
                } -> Box<$crate::library::Read> = |lab, actor| {
                    if $crate::api::call::is_slot_id(&id) {
                        $crate::api::call::slot_read(lab, actor, &id, compare_to.as_deref())
                    } else {
                        $crate::api::call::library_read(lab.data_dir(), &id, compare_to.as_deref())
                    }
                    .map(Box::new)
                };
                /// Moves a preset or snapshot to the trash, or removes a favourite.
                library_delete: LibraryDelete { id: String } -> $crate::library::Trashed =
                    |lab, actor| $crate::api::call::library_delete(lab.data_dir(), actor, &id);
                /// Changes some preferences. `keep_awake` reaches the owner at once.
                prefs_set: PrefsSet { patch: $crate::library::prefs::PrefsPatch } -> Box<$crate::library::prefs::Prefs> = |lab, actor| {
                    patch.check()?;
                    if let Some(on) = patch.keep_awake {
                        $crate::api::call::lab_keep_awake(lab, actor, on);
                    }
                    if let Some(kind) = patch.transport {
                        $crate::api::call::lab_transport(lab, actor, kind);
                    }
                    $crate::api::call::prefs_set(lab.data_dir(), &patch).map(Box::new)
                };
            }
            quiet {
                /// Codes read on every poll, besides the ones the owner always reads.
                lab_pin: LabPin { codes: Vec<u16> } = |lab, actor| {
                    $crate::api::call::lab_pin(lab, codes);
                    Ok(())
                };
                /// The keep-awake setting: while Ukiyo is in use, the owner resets the camera's auto
                /// power off timer. Remembered in `settings.json`.
                lab_keep_awake: LabKeepAwake { on: bool } = |lab, actor| {
                    $crate::api::call::lab_keep_awake(lab, actor, on);
                    Ok(())
                };
                /// How `serve` reaches the camera: `usb` or `wifi` (wireless tether). The owner
                /// closes the session cleanly and looks the other way. Remembered in `settings.json`.
                lab_transport: LabTransport { kind: $crate::camera::transport::TransportKind } = |lab, actor| {
                    $crate::api::call::lab_transport(lab, actor, kind);
                    Ok(())
                };
                /// The Lab's connect switch: off stops the owner from connecting.
                lab_connect: LabConnect { on: bool } = |lab, actor| {
                    $crate::api::call::lab_connect(lab, actor, on);
                    Ok(())
                };
                /// A note in the session log, tied to a change group.
                lab_note: LabNote { group: Option<u32>, text: String } = |lab, actor| {
                    $crate::api::call::lab_note(lab, actor, group, text);
                    Ok(())
                };
                /// Drops a plan without writing it.
                plan_discard: PlanDiscard { plan_id: String } = |lab, actor| $crate::api::call::plan_discard(lab, &plan_id);
                /// Input in the window: Ukiyo is in use (keep-awake). Every other request counts too.
                activity: Activity = |lab, actor| Ok(());
            }
            own {
                /// Everything the Lab knows now: status, rows, change feed, catalog, file paths.
                lab_snapshot: LabSnapshot -> Box<$crate::lab::Snapshot> = |lab, actor| Ok(Box::new($crate::api::call::lab_snapshot(lab)));
                /// Drives the fake camera (`UKIYO_FAKE_CAMERA=1` only).
                lab_fake: LabFake { action: $crate::lab::FakeAction } = |lab, actor| $crate::api::call::lab_fake(lab, action);
                /// Parses recipe text, or the recipe post at a URL, into slot settings, notes and
                /// per-field warnings. Exactly one of `text` and `url`. Stores nothing.
                recipe_parse: RecipeParse {
                    #[serde(default)]
                    text: Option<String>,
                    #[serde(default)]
                    url: Option<String>,
                } -> Box<$crate::recipes::Parsed> = |lab, actor| $crate::api::call::recipe_parse(text.as_deref(), url.as_deref()).map(Box::new);
                /// A stored plan, with its time left.
                plan_show: PlanShow { plan_id: String } -> Box<$crate::plan::Plan> = |lab, actor| $crate::api::call::plan_show(lab, &plan_id).map(Box::new);
                /// Creates or replaces one library item, or restores one from the trash. Never writes
                /// to the camera.
                library_save: LibrarySave(input: $crate::library::SaveInput) -> $crate::library::Saved = |lab, actor| {
                    let snap = $crate::api::save_needs_camera(&input).then(|| lab.snapshot());
                    let ctx = $crate::library::Context { camera: snap.as_ref(), catalog: None };
                    $crate::api::call::library_save(lab.data_dir(), actor, &input, ctx)
                };
                /// The preferences in `settings.json`.
                prefs_get: PrefsGet -> Box<$crate::library::prefs::Prefs> = |lab, actor| Ok(Box::new($crate::api::call::prefs_get(lab.data_dir())));
            }
        }
    };
}

/// A `dispatch` arm's response: the body's result in the variant, or the bare variant.
macro_rules! api_respond {
    ($v:ident, $body:expr) => {{
        let out: Result<(), Error> = $body;
        out?;
        Response::$v
    }};
    ($v:ident, $body:expr, $ret:ty) => {{
        let out: Result<$ret, Error> = $body;
        Response::$v(out?)
    }};
}

/// The core's side of the table: `Request`, `Response`, `METHODS`, `Request::method` and
/// `dispatch`.
macro_rules! api_core {
    ($(
        $group:ident {
            $(
                $(#[$m:meta])*
                $name:ident : $v:ident
                $( { $( $(#[$fm:meta])* $f:ident : $fty:ty ),* $(,)? } )?
                $( ( $nt:ident : $ntty:ty ) )?
                $( -> $ret:ty )?
                = |$lab:ident, $actor:ident| $body:expr;
            )*
        }
    )*) => {
        /// One call into the core.
        #[derive(Debug, Clone, Serialize, Deserialize, Type)]
        #[serde(tag = "method", content = "params", rename_all = "snake_case")]
        pub enum Request {
            $($(
                $(#[$m])*
                $v $( { $( $(#[$fm])* $f: $fty ),* } )? $( ($ntty) )?,
            )*)*
        }

        /// The answer to a `Request`: the variant of the same name, with the method's result.
        #[derive(Debug, Clone, Serialize, Deserialize, Type)]
        #[serde(tag = "method", content = "result", rename_all = "snake_case")]
        pub enum Response {
            $($( $v $( ($ret) )?, )*)*
        }

        /// Every method, in table order.
        pub const METHODS: &[&str] = &[$($(stringify!($name)),*),*];

        impl Request {
            /// The method name, as on the wire.
            #[allow(unused_variables)]
            pub fn method(&self) -> &'static str {
                match self {
                    $($( Request::$v $( { $( $f: _ ),* } )? $( ($nt) )? => stringify!($name), )*)*
                }
            }
        }

        /// Answers one request from `actor`. Methods that wait on the camera block the calling
        /// thread. Every request means Ukiyo is in use, for keep-awake.
        #[allow(unused_variables)]
        pub fn dispatch(lab: &Lab, actor: &str, req: Request) -> Result<Response, Error> {
            call::activity(lab);
            Ok(match req {
                $($(
                    Request::$v $( { $($f),* } )? $( ($nt) )? => {
                        let $lab = lab;
                        let $actor = actor;
                        api_respond!($v, $body $(, $ret)?)
                    }
                )*)*
            })
        }
    };
}

ukiyo_api_table!(api_core);

/// Why a request failed (docs/agent-interface.md, section 5, "Typed errors").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The request ran and did not succeed; the message says why.
    Failed,
    /// A fake-camera method in a session with the real camera.
    NoFakeCamera,
    /// A socket line that is not a known request (`serve::wire`).
    BadRequest,
    /// No `ukiyo serve` to answer: it did not start, or the connection dropped mid-request.
    Unavailable,
    /// No camera on the bus, or the Lab's connect switch is off.
    NotConnected,
    /// Auto power off took the camera off USB.
    CameraAsleep,
    /// The camera answered 0x2019 for longer than the owner retries.
    Busy,
    /// A menu is open on the camera.
    MenuOpen,
    /// The camera shows playback, which answers no reads or writes.
    InPlayback,
    NeedsTether,
    NeedsRawconv,
    NeedsStills,
    NeedsMovie,
    /// A process outside ukiyo holds the USB device.
    OwnerConflict,
    /// The catalog does not mark the code `confirmed`, and the plan does not allow it.
    UnconfirmedProperty,
    ReadOnly,
    /// The catalog marks the code `never_write`: a command or status code. No flag lifts it.
    NeverWrite,
    ValueOutOfRange,
    NotFound,
    Ambiguous,
    /// A planned code changed since the plan was made.
    PlanStale,
    /// No such plan: expired (5 minutes), applied, discarded, or `serve` restarted.
    PlanExpired,
    /// The write was accepted but the camera holds another value.
    ReadbackMismatch,
    /// A label marks a code `confirmed` that has not changed in this session (`evidence`).
    NoEvidence,
    /// A label holds a string this camera reports: a serial or an owner string (`evidence`).
    PrivateText,
}

impl ErrorCode {
    /// The wire name: `needs_rawconv`.
    pub fn name(self) -> &'static str {
        use ErrorCode::*;
        match self {
            Failed => "failed",
            NoFakeCamera => "no_fake_camera",
            BadRequest => "bad_request",
            Unavailable => "unavailable",
            NotConnected => "not_connected",
            CameraAsleep => "camera_asleep",
            Busy => "busy",
            MenuOpen => "menu_open",
            InPlayback => "in_playback",
            NeedsTether => "needs_tether",
            NeedsRawconv => "needs_rawconv",
            NeedsStills => "needs_stills",
            NeedsMovie => "needs_movie",
            OwnerConflict => "owner_conflict",
            UnconfirmedProperty => "unconfirmed_property",
            ReadOnly => "read_only",
            NeverWrite => "never_write",
            ValueOutOfRange => "value_out_of_range",
            NotFound => "not_found",
            Ambiguous => "ambiguous",
            PlanStale => "plan_stale",
            PlanExpired => "plan_expired",
            ReadbackMismatch => "readback_mismatch",
            NoEvidence => "no_evidence",
            PrivateText => "private_text",
        }
    }

    /// The code of a wire name; `failed` for a name this build does not know.
    pub fn from_name(name: &str) -> ErrorCode {
        serde_json::from_value(serde_json::Value::String(name.into())).unwrap_or(ErrorCode::Failed)
    }
}

/// Why a request failed, and what fixes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// The command that prints the mode walkthrough, when a mode switch fixes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walkthrough: Option<String>,
    /// The camera's USB mode, for a `needs_*` error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Error { code, message: message.into(), hint: None, walkthrough: None, mode: None }
    }

    fn failed(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::Failed, message)
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl From<crate::read::Problem> for Error {
    fn from(p: crate::read::Problem) -> Error {
        Error { code: ErrorCode::from_name(p.code), message: p.message, hint: Some(p.hint), walkthrough: p.walkthrough, mode: p.mode.map(Into::into) }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// What the Lab reports while it runs.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The link, mode and camera details changed.
    Status(Status),
    /// A full set of property rows (after a connect or a mode change).
    Props(Vec<PropRow>),
    /// One change group from the poll or a write.
    Changes(ChangeGroup),
    /// The catalog for a newly seen model (`None`: no catalog file for it yet).
    Catalog(Option<Catalog>),
}

/// One typed function per method. Each keeps its method's own result type, so a front end
/// that knows the method needs no `Response` match.
pub mod call {
    use super::*;

    pub fn lab_snapshot(lab: &Lab) -> Snapshot {
        lab.snapshot()
    }

    pub fn lab_pin(lab: &Lab, codes: Vec<u16>) {
        lab.pin(codes)
    }

    pub fn lab_keep_awake(lab: &Lab, actor: &str, on: bool) {
        lab.keep_awake(actor, on)
    }

    pub fn lab_connect(lab: &Lab, actor: &str, on: bool) {
        lab.connect(actor, on)
    }

    pub fn lab_transport(lab: &Lab, actor: &str, kind: crate::camera::transport::TransportKind) {
        lab.transport(actor, kind)
    }

    /// Blocks until the dump is written (at most 60 s).
    pub fn lab_export(lab: &Lab, actor: &str) -> Result<String, Error> {
        lab.export(actor)
    }

    /// A label from anyone but the window must pass `evidence::check`.
    pub fn lab_label(lab: &Lab, actor: &str, patch: LabelPatch) -> Result<Catalog, Error> {
        crate::evidence::check(&lab.snapshot(), actor, &patch)?;
        lab.label(actor, patch).map_err(Error::failed)
    }

    pub fn lab_note(lab: &Lab, actor: &str, group: Option<u32>, text: String) {
        lab.note(actor, group, text)
    }

    pub fn lab_fake(lab: &Lab, action: FakeAction) -> Result<(), Error> {
        lab.fake(action).map_err(|message| Error::new(ErrorCode::NoFakeCamera, message))
    }

    /// Text is parsed at once; a URL is fetched first (at most 20 s), so call it off the
    /// main thread.
    pub fn recipe_parse(text: Option<&str>, url: Option<&str>) -> Result<Parsed, Error> {
        match (text.filter(|t| !t.trim().is_empty()), url.filter(|u| !u.trim().is_empty())) {
            (Some(t), None) => Ok(recipes::parse_text(t)),
            (None, Some(u)) => recipes::fetch::parse_url(u).map_err(|e| match e {
                recipes::fetch::FetchError::BadUrl(m) => Error::new(ErrorCode::BadRequest, m),
                recipes::fetch::FetchError::Failed(m) => Error::new(ErrorCode::Failed, format!("the post could not be read: {m}")).hint("Check the link, or paste the recipe's text instead."),
            }),
            _ => Err(Error::new(ErrorCode::BadRequest, "recipe_parse takes exactly one of text and url")),
        }
    }

    /// `slot:C1` .. `slot:C7` or `slots`, read from the camera (RAW CONV. mode). With
    /// `compare_to` (a recipe id), the slot's settings against that recipe.
    pub fn slot_read(lab: &Lab, actor: &str, id: &str, compare_to: Option<&str>) -> Result<library::Read, Error> {
        let slots: Vec<u8> = match library::Id::parse(id)? {
            library::Id::Slot(n) => vec![n],
            library::Id::Slots => (1..=7).collect(),
            _ => return Err(Error::new(ErrorCode::NotFound, format!("{id} is not a slot id"))),
        };
        let views = lab.read_slots(actor, slots)?;
        if let Some(b) = compare_to {
            let v = views.first().ok_or_else(|| Error::new(ErrorCode::Failed, "the slot read returned nothing"))?;
            let (_, s) = Store::new(lab.data_dir()).recipe_settings(b)?;
            return Ok(library::Read::RecipeDiff(library::recipes::diff(&v.id, &v.settings, b, &s)));
        }
        Ok(library::Read::Slots(views))
    }

    /// Whether `library_read` must go to the camera.
    pub fn is_slot_id(id: &str) -> bool {
        let id = id.trim();
        id == "slots" || id.starts_with("slot:")
    }

    pub fn plan_create(lab: &Lab, input: PlanInput) -> Result<Plan, Error> {
        lab.plan(input)
    }

    pub fn plan_show(lab: &Lab, plan_id: &str) -> Result<Plan, Error> {
        lab.plan_show(plan_id)
    }

    /// Blocks while the steps are written: each write waits for the owner (at most 15 s).
    pub fn plan_apply(lab: &Lab, actor: &str, plan_id: &str) -> Result<Applied, Error> {
        lab.apply(actor, plan_id)
    }

    pub fn plan_discard(lab: &Lab, plan_id: &str) -> Result<(), Error> {
        lab.discard(plan_id)
    }

    pub fn activity(lab: &Lab) {
        lab.activity()
    }

    // The library methods take the data folder, not the Lab: every front end runs them in
    // its own process, with or without `serve`. `serve` answers them too.

    pub fn library_search(data_dir: &Path, input: &SearchInput) -> Hits {
        Store::new(data_dir).search(input)
    }

    pub fn library_read(data_dir: &Path, id: &str, compare_to: Option<&str>) -> Result<library::Read, Error> {
        Store::new(data_dir).read(id, compare_to)
    }

    /// `ctx.camera` is the Lab's snapshot, for a snapshot or a preset of current values.
    pub fn library_save(data_dir: &Path, actor: &str, input: &SaveInput, ctx: library::Context) -> Result<Saved, Error> {
        Store::new(data_dir).save(actor, input, ctx)
    }

    pub fn library_delete(data_dir: &Path, actor: &str, id: &str) -> Result<Trashed, Error> {
        Store::new(data_dir).delete(actor, id)
    }

    pub fn prefs_get(data_dir: &Path) -> Prefs {
        library::prefs::load(data_dir)
    }

    /// Saves the patch. A `keep_awake` change must also reach a running owner: the caller
    /// sends `lab_keep_awake` (as `dispatch` does).
    pub fn prefs_set(data_dir: &Path, patch: &PrefsPatch) -> Result<Prefs, Error> {
        library::prefs::update(data_dir, patch)
    }
}

/// Whether a save needs the camera's state: a snapshot, or a preset of current values.
pub fn save_needs_camera(input: &SaveInput) -> bool {
    match input.kind {
        library::SaveKind::Snapshot => input.from.is_none(),
        library::SaveKind::Preset => input.item.as_ref().is_some_and(|i| i.get("settings").is_some()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::owner::Link;
    use indexmap::IndexMap;
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn req(v: serde_json::Value) -> Request {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn wire_shapes() {
        let r = req(json!({ "method": "plan_create", "params": { "set": { "AutoPowerOff": "OFF" } } }));
        let Request::PlanCreate { set, allow_unconfirmed, .. } = &r else { panic!("{r:?}") };
        assert_eq!((set["AutoPowerOff"].as_str(), *allow_unconfirmed), ("OFF", false));
        assert_eq!(r.method(), "plan_create");
        assert_eq!(serde_json::to_value(&r).unwrap()["method"], "plan_create");
        assert!(serde_json::from_value::<Request>(json!({ "method": "lab_write", "params": { "code": 1, "value": { "kind": "int", "v": 2 } } })).is_err(), "writes are plans only");
        let r = req(json!({ "method": "lab_snapshot" }));
        assert!(matches!(r, Request::LabSnapshot));
        assert_eq!(serde_json::to_value(Response::LabPin).unwrap(), json!({ "method": "lab_pin" }));
        let e = Error::new(ErrorCode::NoFakeCamera, "no fake camera");
        assert_eq!(serde_json::to_value(e).unwrap(), json!({ "code": "no_fake_camera", "message": "no fake camera" }));
        let e = Error::new(ErrorCode::NeedsRawconv, "slots").hint("switch");
        assert_eq!(serde_json::to_value(e).unwrap(), json!({ "code": "needs_rawconv", "message": "slots", "hint": "switch" }));
        assert!(serde_json::from_value::<Request>(json!({ "method": "lab_reset_usb" })).is_err());
    }

    /// Every code's wire name is its serde name.
    #[test]
    fn error_code_names() {
        use ErrorCode::*;
        for c in [Failed, NoFakeCamera, BadRequest, Unavailable, NotConnected, CameraAsleep, Busy, MenuOpen, InPlayback, NeedsTether, NeedsRawconv, NeedsStills, NeedsMovie, OwnerConflict, UnconfirmedProperty, ReadOnly, NeverWrite, ValueOutOfRange, NotFound, Ambiguous, PlanStale, PlanExpired, ReadbackMismatch, NoEvidence, PrivateText] {
            assert_eq!(serde_json::to_value(c).unwrap(), json!(c.name()));
            assert_eq!(ErrorCode::from_name(c.name()), c);
        }
        assert_eq!(ErrorCode::from_name("whatever"), Failed);
    }

    /// Every method name round-trips: a `Request` serialized with `method()` as its tag.
    #[test]
    fn method_names_match_the_wire() {
        let all = [
            Request::LabSnapshot,
            Request::LabPin { codes: vec![] },
            Request::LabKeepAwake { on: false },
            Request::LabConnect { on: true },
            Request::LabTransport { kind: crate::camera::transport::TransportKind::Wifi },
            Request::LabExport,
            Request::LabLabel { patch: LabelPatch::default() },
            Request::LabNote { group: None, text: String::new() },
            Request::LabFake { action: FakeAction::Nudge },
            Request::RecipeParse { text: None, url: None },
            Request::PlanCreate { set: IndexMap::new(), allow_unconfirmed: false, from: None, slot: None },
            Request::PlanShow { plan_id: String::new() },
            Request::PlanApply { plan_id: String::new() },
            Request::PlanDiscard { plan_id: String::new() },
            Request::Activity,
            Request::LibrarySearch(SearchInput::default()),
            Request::LibraryRead { id: "prefs".into(), compare_to: None },
            Request::LibrarySave(SaveInput { kind: library::SaveKind::Prefs, name: None, item: None, from: None }),
            Request::LibraryDelete { id: String::new() },
            Request::PrefsGet,
            Request::PrefsSet { patch: PrefsPatch::default() },
        ];
        assert_eq!(all.len(), METHODS.len());
        let mut seen = std::collections::BTreeSet::new();
        for r in &all {
            assert!(METHODS.contains(&r.method()), "{} is not in METHODS", r.method());
            assert_eq!(serde_json::to_value(r).unwrap()["method"], r.method());
            assert!(seen.insert(r.method()), "{} twice", r.method());
        }
    }

    /// Requests as JSON through `dispatch`, on the fake camera.
    #[test]
    fn dispatch_on_the_fake_camera() {
        std::env::set_var("UKIYO_FAKE_CAMERA", "1");
        let data = tempfile::tempdir().unwrap();
        let cats = tempfile::tempdir().unwrap();
        let lock = crate::serve::OwnerLock::try_acquire(&data.path().join("owner.lock")).unwrap();
        let lab = Lab::start(lock, data.path().into(), cats.path().into(), |_| {});
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let Response::LabSnapshot(s) = dispatch(&lab, "test", Request::LabSnapshot).unwrap() else { panic!() };
            if s.status.link == Link::Connected && !s.rows.is_empty() {
                break;
            }
            assert!(Instant::now() < deadline, "fake camera never connected");
            std::thread::sleep(Duration::from_millis(20));
        }
        let p = dispatch(&lab, "test", req(json!({ "method": "plan_create", "params": { "set": { "0xd023": "2" }, "allow_unconfirmed": true } }))).unwrap();
        let Response::PlanCreate(plan) = p else { panic!("{p:?}") };
        let a = dispatch(&lab, "test", req(json!({ "method": "plan_apply", "params": { "plan_id": plan.plan_id } }))).unwrap();
        let Response::PlanApply(out) = a else { panic!("{a:?}") };
        assert_eq!(out.applied, 1, "{out:?}");
        let e = dispatch(&lab, "test", Request::PlanApply { plan_id: plan.plan_id.clone() }).unwrap_err();
        assert_eq!(e.code, ErrorCode::PlanExpired, "a plan applies once");
        let n = dispatch(&lab, "test", req(json!({ "method": "lab_note", "params": { "group": null, "text": "via api" } }))).unwrap();
        assert!(matches!(n, Response::LabNote));
        assert!(matches!(dispatch(&lab, "test", req(json!({ "method": "lab_fake", "params": { "action": "nudge" } }))).unwrap(), Response::LabFake));
        let p = dispatch(&lab, "test", req(json!({ "method": "recipe_parse", "params": { "text": "Film Simulation: Classic Chrome" } }))).unwrap();
        let Response::RecipeParse(parsed) = p else { panic!("{p:?}") };
        assert!(serde_json::to_value(&parsed).unwrap().is_object());
        lab.shutdown();
    }

    /// The Lab's write box sends `plan_create` with one raw code and `allow_unconfirmed`
    /// (app/src/pages/Lab.tsx): a never_write code is refused there too.
    #[test]
    fn lab_write_box_refuses_never_write_codes() {
        std::env::set_var("UKIYO_FAKE_CAMERA", "1");
        let data = tempfile::tempdir().unwrap();
        let cats = tempfile::tempdir().unwrap();
        std::fs::copy(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/x-t5.json"), cats.path().join("x-t5.json")).unwrap();
        let lock = crate::serve::OwnerLock::try_acquire(&data.path().join("owner.lock")).unwrap();
        let lab = Lab::start(lock, data.path().into(), cats.path().into(), |_| {});
        let deadline = Instant::now() + Duration::from_secs(10);
        while !(lab.snapshot().status.link == Link::Connected && !lab.snapshot().rows.is_empty() && lab.snapshot().catalog.is_some()) {
            assert!(Instant::now() < deadline, "fake camera never connected");
            std::thread::sleep(Duration::from_millis(20));
        }
        for (code, raw) in [("0xd17f", "2"), ("0xd17b", "1"), ("0xd208", "512"), ("0xd21c", "0")] {
            let e = dispatch(&lab, "app", req(json!({ "method": "plan_create", "params": { "set": { code: raw }, "allow_unconfirmed": true } }))).unwrap_err();
            assert_eq!(e.code, ErrorCode::NeverWrite, "{code}: {e:?}");
            assert!(e.message.starts_with(code), "{}", e.message);
        }
        lab.shutdown();
    }
}
