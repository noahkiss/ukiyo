//! The one surface every front end speaks (docs/agent-interface.md, section 2).
//!
//! - `Request`: one variant per method. On the wire it is `{"method": "lab_write",
//!   "params": {"code": 53283, "value": {"kind": "int", "v": 2}}}`; a method with no params
//!   leaves `params` out. Method names are the window's Tauri command names.
//! - `Response`: one variant per method, in the same order, carrying that method's result:
//!   `{"method": "lab_write", "result": {...}}`.
//! - `Error`: a code and a message. The typed codes of section 5 come with plans (stage 4).
//! - `Event`: what the Lab reports while it runs. `ukiyo serve` sends them to every
//!   subscribed client (`serve::wire`); the app turns them into Tauri events.
//! - `call`: one typed function per method. `dispatch` and the Tauri commands both call these,
//!   so a method behaves the same on every surface.
//!
//! A new method is a `Request` variant, a `Response` variant, a `call` function and a
//! `dispatch` arm. Stage 8 turns this into one table and a macro (QuadCam's `api!`).

use crate::camera::owner::{ChangeGroup, PropRow, Status, WriteOutcome};
use crate::camera::ptp::Value;
use crate::catalog::{Catalog, LabelPatch};
use crate::lab::{FakeAction, Lab, Snapshot};
use crate::recipes::{self, Parsed};
use serde::{Deserialize, Serialize};
use specta::Type;

/// One call into the core.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    /// Everything the Lab knows now: status, rows, change feed, catalog, file paths.
    LabSnapshot,
    /// Writes one property and reads it back.
    LabWrite { code: u16, value: Value },
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
}

/// Every method, in `Request` order.
pub const METHODS: &[&str] = &[
    "lab_snapshot",
    "lab_write",
    "lab_pin",
    "lab_keep_awake",
    "lab_connect",
    "lab_export",
    "lab_label",
    "lab_note",
    "lab_fake",
    "recipe_parse",
];

impl Request {
    /// The method name, as on the wire.
    pub fn method(&self) -> &'static str {
        METHODS[match self {
            Request::LabSnapshot => 0,
            Request::LabWrite { .. } => 1,
            Request::LabPin { .. } => 2,
            Request::LabKeepAwake { .. } => 3,
            Request::LabConnect { .. } => 4,
            Request::LabExport => 5,
            Request::LabLabel { .. } => 6,
            Request::LabNote { .. } => 7,
            Request::LabFake { .. } => 8,
            Request::RecipeParse { .. } => 9,
        }]
    }
}

/// The answer to a `Request`: the variant of the same name.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "method", content = "result", rename_all = "snake_case")]
pub enum Response {
    LabSnapshot(Box<Snapshot>),
    LabWrite(WriteOutcome),
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
}

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
}

/// Why a request failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
}

impl Error {
    fn failed(message: impl Into<String>) -> Self {
        Error { code: ErrorCode::Failed, message: message.into() }
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

    /// Blocks until the owner answers (at most 15 s).
    pub fn lab_write(lab: &Lab, code: u16, value: Value) -> WriteOutcome {
        lab.write(code, value)
    }

    pub fn lab_pin(lab: &Lab, codes: Vec<u16>) {
        lab.pin(codes)
    }

    pub fn lab_keep_awake(lab: &Lab, on: bool) {
        lab.keep_awake(on)
    }

    pub fn lab_connect(lab: &Lab, on: bool) {
        lab.connect(on)
    }

    /// Blocks until the dump is written (at most 60 s).
    pub fn lab_export(lab: &Lab) -> Result<String, Error> {
        lab.export().map_err(Error::failed)
    }

    pub fn lab_label(lab: &Lab, patch: LabelPatch) -> Result<Catalog, Error> {
        lab.label(patch).map_err(Error::failed)
    }

    pub fn lab_note(lab: &Lab, group: Option<u32>, text: String) {
        lab.note(group, text)
    }

    pub fn lab_fake(lab: &Lab, action: FakeAction) -> Result<(), Error> {
        lab.fake(action).map_err(|message| Error { code: ErrorCode::NoFakeCamera, message })
    }

    pub fn recipe_parse(text: &str) -> Parsed {
        recipes::parse_text(text)
    }
}

/// Answers one request. Methods that wait on the camera block the calling thread.
pub fn dispatch(lab: &Lab, req: Request) -> Result<Response, Error> {
    Ok(match req {
        Request::LabSnapshot => Response::LabSnapshot(Box::new(call::lab_snapshot(lab))),
        Request::LabWrite { code, value } => Response::LabWrite(call::lab_write(lab, code, value)),
        Request::LabPin { codes } => {
            call::lab_pin(lab, codes);
            Response::LabPin
        }
        Request::LabKeepAwake { on } => {
            call::lab_keep_awake(lab, on);
            Response::LabKeepAwake
        }
        Request::LabConnect { on } => {
            call::lab_connect(lab, on);
            Response::LabConnect
        }
        Request::LabExport => Response::LabExport(call::lab_export(lab)?),
        Request::LabLabel { patch } => Response::LabLabel(Box::new(call::lab_label(lab, patch)?)),
        Request::LabNote { group, text } => {
            call::lab_note(lab, group, text);
            Response::LabNote
        }
        Request::LabFake { action } => {
            call::lab_fake(lab, action)?;
            Response::LabFake
        }
        Request::RecipeParse { text } => Response::RecipeParse(Box::new(call::recipe_parse(&text))),
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
        let r = req(json!({ "method": "lab_write", "params": { "code": 53283, "value": { "kind": "int", "v": 2 } } }));
        assert!(matches!(r, Request::LabWrite { code: 0xD023, value: Value::Int(2) }));
        assert_eq!(r.method(), "lab_write");
        assert_eq!(serde_json::to_value(&r).unwrap()["method"], "lab_write");
        let r = req(json!({ "method": "lab_snapshot" }));
        assert!(matches!(r, Request::LabSnapshot));
        assert_eq!(serde_json::to_value(Response::LabPin).unwrap(), json!({ "method": "lab_pin" }));
        let e = Error { code: ErrorCode::NoFakeCamera, message: "no fake camera".into() };
        assert_eq!(serde_json::to_value(e).unwrap(), json!({ "code": "no_fake_camera", "message": "no fake camera" }));
        assert!(serde_json::from_value::<Request>(json!({ "method": "lab_reset_usb" })).is_err());
    }

    /// Every method name round-trips: a `Request` serialized with `method()` as its tag.
    #[test]
    fn method_names_match_the_wire() {
        let all = [
            Request::LabSnapshot,
            Request::LabWrite { code: 1, value: Value::Int(0) },
            Request::LabPin { codes: vec![] },
            Request::LabKeepAwake { on: false },
            Request::LabConnect { on: true },
            Request::LabExport,
            Request::LabLabel { patch: LabelPatch::default() },
            Request::LabNote { group: None, text: String::new() },
            Request::LabFake { action: FakeAction::Nudge },
            Request::RecipeParse { text: String::new() },
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
            let Response::LabSnapshot(s) = dispatch(&lab, Request::LabSnapshot).unwrap() else { panic!() };
            if s.status.link == Link::Connected && !s.rows.is_empty() {
                break;
            }
            assert!(Instant::now() < deadline, "fake camera never connected");
            std::thread::sleep(Duration::from_millis(20));
        }
        let w = dispatch(&lab, req(json!({ "method": "lab_write", "params": { "code": 0xD023, "value": { "kind": "int", "v": 2 } } }))).unwrap();
        let Response::LabWrite(out) = w else { panic!("{w:?}") };
        assert!(out.ok, "{out:?}");
        let n = dispatch(&lab, req(json!({ "method": "lab_note", "params": { "group": null, "text": "via api" } }))).unwrap();
        assert!(matches!(n, Response::LabNote));
        assert!(matches!(dispatch(&lab, req(json!({ "method": "lab_fake", "params": { "action": "nudge" } }))).unwrap(), Response::LabFake));
        let p = dispatch(&lab, req(json!({ "method": "recipe_parse", "params": { "text": "Film Simulation: Classic Chrome" } }))).unwrap();
        let Response::RecipeParse(parsed) = p else { panic!("{p:?}") };
        assert!(serde_json::to_value(&parsed).unwrap().is_object());
        lab.shutdown();
    }
}
