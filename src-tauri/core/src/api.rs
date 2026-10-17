//! The one surface every front end speaks (docs/agent-interface.md, section 2).
//!
//! - `Request`: one variant per method. On the wire it is `{"method": "plan_create",
//!   "params": {"set": {"AutoPowerOff": "OFF"}}}`; a method with no params
//!   leaves `params` out. Method names are the window's Tauri command names.
//! - `Response`: one variant per method, in the same order, carrying that method's result:
//!   `{"method": "plan_create", "result": {...}}`.
//! - `Error`: a typed code (section 5, "Typed errors"), a message, and what fixes it.
//! - `Event`: what the Lab reports while it runs. `ukiyo serve` sends them to every
//!   subscribed client (`serve::wire`); the app turns them into Tauri events.
//! - `call`: one typed function per method. `dispatch` and the Tauri commands both call these,
//!   so a method behaves the same on every surface.
//!
//! - `dispatch` takes the caller's `actor` (`app`, `cli`, `mcp:<client>`), which goes into
//!   the session log with every write, label and note.
//!
//! Camera writes exist only as plans: `plan_create`, then `plan_apply` (section 5).
//!
//! A new method is a `Request` variant, a `Response` variant, a `call` function and a
//! `dispatch` arm. Stage 8 turns this into one table and a macro (QuadCam's `api!`).

use crate::camera::owner::{ChangeGroup, PropRow, Status};
use crate::catalog::{Catalog, LabelPatch};
use crate::lab::{FakeAction, Lab, Snapshot};
use crate::plan::{Applied, Plan, PlanInput};
use crate::recipes::{self, Parsed};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;

/// One call into the core.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    /// Everything the Lab knows now: status, rows, change feed, catalog, file paths.
    LabSnapshot,
    /// Codes read on every poll, besides the ones the owner always reads.
    LabPin { codes: Vec<u16> },
    /// Holds AUTO POWER OFF at OFF while connected; remembered in `settings.json`.
    LabKeepAwake { on: bool },
    /// The Lab's connect switch: off stops the owner from connecting.
    LabConnect { on: bool },
    /// Writes a full property dump to `dumps/` and returns its path.
    LabExport,
    /// Labels one property in the model's catalog and saves the catalog.
    LabLabel { patch: LabelPatch },
    /// A note in the session log, tied to a change group.
    LabNote { group: Option<u32>, text: String },
    /// Drives the fake camera (`UKIYO_FAKE_CAMERA=1` only).
    LabFake { action: FakeAction },
    /// Parses pasted recipe text into slot settings, notes and per-field warnings.
    RecipeParse { text: String },
    /// Builds a plan against the live camera: names or codes to labels or raw values.
    /// Writes nothing.
    PlanCreate {
        set: IndexMap<String, String>,
        #[serde(default)]
        allow_unconfirmed: bool,
    },
    /// A stored plan, with its time left.
    PlanShow { plan_id: String },
    /// Re-checks a plan, writes its steps in order, reads each back, and logs them.
    PlanApply { plan_id: String },
    PlanDiscard { plan_id: String },
}

/// Every method, in `Request` order.
pub const METHODS: &[&str] = &[
    "lab_snapshot",
    "lab_pin",
    "lab_keep_awake",
    "lab_connect",
    "lab_export",
    "lab_label",
    "lab_note",
    "lab_fake",
    "recipe_parse",
    "plan_create",
    "plan_show",
    "plan_apply",
    "plan_discard",
];

impl Request {
    /// The method name, as on the wire.
    pub fn method(&self) -> &'static str {
        METHODS[match self {
            Request::LabSnapshot => 0,
            Request::LabPin { .. } => 1,
            Request::LabKeepAwake { .. } => 2,
            Request::LabConnect { .. } => 3,
            Request::LabExport => 4,
            Request::LabLabel { .. } => 5,
            Request::LabNote { .. } => 6,
            Request::LabFake { .. } => 7,
            Request::RecipeParse { .. } => 8,
            Request::PlanCreate { .. } => 9,
            Request::PlanShow { .. } => 10,
            Request::PlanApply { .. } => 11,
            Request::PlanDiscard { .. } => 12,
        }]
    }
}

/// The answer to a `Request`: the variant of the same name.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "method", content = "result", rename_all = "snake_case")]
pub enum Response {
    LabSnapshot(Box<Snapshot>),
    LabPin,
    LabKeepAwake,
    LabConnect,
    /// The dump's path.
    LabExport(String),
    /// The catalog after the label.
    LabLabel(Box<Catalog>),
    LabNote,
    LabFake,
    RecipeParse(Box<Parsed>),
    PlanCreate(Box<Plan>),
    PlanShow(Box<Plan>),
    PlanApply(Box<Applied>),
    PlanDiscard,
}

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
    NeedsTether,
    NeedsRawconv,
    NeedsStills,
    NeedsMovie,
    /// A process outside ukiyo holds the USB device.
    OwnerConflict,
    /// The catalog does not mark the code `confirmed`, and the plan does not allow it.
    UnconfirmedProperty,
    ReadOnly,
    ValueOutOfRange,
    NotFound,
    Ambiguous,
    /// A planned code changed since the plan was made.
    PlanStale,
    /// No such plan: expired (5 minutes), applied, discarded, or `serve` restarted.
    PlanExpired,
    /// The write was accepted but the camera holds another value.
    ReadbackMismatch,
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
            NeedsTether => "needs_tether",
            NeedsRawconv => "needs_rawconv",
            NeedsStills => "needs_stills",
            NeedsMovie => "needs_movie",
            OwnerConflict => "owner_conflict",
            UnconfirmedProperty => "unconfirmed_property",
            ReadOnly => "read_only",
            ValueOutOfRange => "value_out_of_range",
            NotFound => "not_found",
            Ambiguous => "ambiguous",
            PlanStale => "plan_stale",
            PlanExpired => "plan_expired",
            ReadbackMismatch => "readback_mismatch",
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

    /// Blocks until the dump is written (at most 60 s).
    pub fn lab_export(lab: &Lab, actor: &str) -> Result<String, Error> {
        lab.export(actor)
    }

    pub fn lab_label(lab: &Lab, actor: &str, patch: LabelPatch) -> Result<Catalog, Error> {
        lab.label(actor, patch).map_err(Error::failed)
    }

    pub fn lab_note(lab: &Lab, actor: &str, group: Option<u32>, text: String) {
        lab.note(actor, group, text)
    }

    pub fn lab_fake(lab: &Lab, action: FakeAction) -> Result<(), Error> {
        lab.fake(action).map_err(|message| Error::new(ErrorCode::NoFakeCamera, message))
    }

    pub fn recipe_parse(text: &str) -> Parsed {
        recipes::parse_text(text)
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
}

/// Answers one request from `actor`. Methods that wait on the camera block the calling thread.
pub fn dispatch(lab: &Lab, actor: &str, req: Request) -> Result<Response, Error> {
    Ok(match req {
        Request::LabSnapshot => Response::LabSnapshot(Box::new(call::lab_snapshot(lab))),
        Request::LabPin { codes } => {
            call::lab_pin(lab, codes);
            Response::LabPin
        }
        Request::LabKeepAwake { on } => {
            call::lab_keep_awake(lab, actor, on);
            Response::LabKeepAwake
        }
        Request::LabConnect { on } => {
            call::lab_connect(lab, actor, on);
            Response::LabConnect
        }
        Request::LabExport => Response::LabExport(call::lab_export(lab, actor)?),
        Request::LabLabel { patch } => Response::LabLabel(Box::new(call::lab_label(lab, actor, patch)?)),
        Request::LabNote { group, text } => {
            call::lab_note(lab, actor, group, text);
            Response::LabNote
        }
        Request::LabFake { action } => {
            call::lab_fake(lab, action)?;
            Response::LabFake
        }
        Request::RecipeParse { text } => Response::RecipeParse(Box::new(call::recipe_parse(&text))),
        Request::PlanCreate { set, allow_unconfirmed } => Response::PlanCreate(Box::new(call::plan_create(lab, PlanInput { set, allow_unconfirmed })?)),
        Request::PlanShow { plan_id } => Response::PlanShow(Box::new(call::plan_show(lab, &plan_id)?)),
        Request::PlanApply { plan_id } => Response::PlanApply(Box::new(call::plan_apply(lab, actor, &plan_id)?)),
        Request::PlanDiscard { plan_id } => {
            call::plan_discard(lab, &plan_id)?;
            Response::PlanDiscard
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::owner::Link;
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn req(v: serde_json::Value) -> Request {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn wire_shapes() {
        let r = req(json!({ "method": "plan_create", "params": { "set": { "AutoPowerOff": "OFF" } } }));
        let Request::PlanCreate { set, allow_unconfirmed } = &r else { panic!("{r:?}") };
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
        for c in [Failed, NoFakeCamera, BadRequest, Unavailable, NotConnected, CameraAsleep, Busy, MenuOpen, NeedsTether, NeedsRawconv, NeedsStills, NeedsMovie, OwnerConflict, UnconfirmedProperty, ReadOnly, ValueOutOfRange, NotFound, Ambiguous, PlanStale, PlanExpired, ReadbackMismatch] {
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
            Request::LabExport,
            Request::LabLabel { patch: LabelPatch::default() },
            Request::LabNote { group: None, text: String::new() },
            Request::LabFake { action: FakeAction::Nudge },
            Request::RecipeParse { text: String::new() },
            Request::PlanCreate { set: IndexMap::new(), allow_unconfirmed: false },
            Request::PlanShow { plan_id: String::new() },
            Request::PlanApply { plan_id: String::new() },
            Request::PlanDiscard { plan_id: String::new() },
        ];
        assert_eq!(all.len(), METHODS.len());
        for (r, name) in all.iter().zip(METHODS) {
            assert_eq!(r.method(), *name);
            assert_eq!(serde_json::to_value(r).unwrap()["method"], *name);
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
}
