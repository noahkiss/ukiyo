//! The two rules on a label from an agent (docs/agent-interface.md, section 4, `catalog_label`).
//!
//! - **Evidence:** `confidence: confirmed` needs this session to have seen the code change: a
//!   `camera` change group, or a write's read-back group. The catalog drives write-gating, so
//!   this keeps an agent from un-gating a property only by labelling it.
//! - **Private text:** a label must not hold a string read from this camera (serials, owner
//!   strings), so the catalog stays shareable.
//!
//! The window's Lab (actor `app`) is the person mapping by hand, and these rules do not apply
//! to it.

use crate::api::{Error, ErrorCode};
use crate::camera::ptp::Value;
use crate::catalog::{Confidence, LabelPatch};
use crate::lab::Snapshot;
use crate::read::code_text;

/// Whether a label from `actor` may be applied.
pub fn check(snap: &Snapshot, actor: &str, patch: &LabelPatch) -> Result<(), Error> {
    if actor == "app" {
        return Ok(());
    }
    if patch.confidence == Some(Confidence::Confirmed) && !seen_change(snap, patch.code) {
        return Err(Error::new(ErrorCode::NoEvidence, format!("{} has not changed in this session, so it cannot be marked confirmed", code_text(patch.code)))
            .hint("Change it on the camera while camera_watch runs, or write it through a plan, then label it again. Use likely until then."));
    }
    let private = private_strings(snap);
    let texts = [patch.name.as_deref(), patch.menu.as_deref(), patch.notes.as_deref()].into_iter().flatten().chain(patch.values.iter().map(|(_, l)| l.as_str()));
    for t in texts {
        let t = t.to_lowercase();
        if private.iter().any(|p| t.contains(p.as_str())) {
            return Err(Error::new(ErrorCode::PrivateText, "the label holds text this camera reports (a serial number or an owner string)").hint("Describe the property instead of copying its value; the catalog is shared."));
        }
    }
    Ok(())
}

/// A change group for `code` in this session: the session log, else the Lab's feed.
fn seen_change(snap: &Snapshot, code: u16) -> bool {
    if snap.groups.iter().any(|g| g.changes.iter().any(|c| c.code == code)) {
        return true;
    }
    // The feed keeps the last 1000 groups; the session log keeps them all.
    let Some(text) = snap.session_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) else { return false };
    text.lines().filter(|l| l.contains("\"changes\"")).filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).any(|v| {
        v["kind"] == "changes" && v["group"]["changes"].as_array().is_some_and(|cs| cs.iter().any(|c| c["code"].as_u64() == Some(code as u64)))
    })
}

/// The camera's string values, lowercased: each whole value of 3 or more characters, and each
/// word of 5 or more with a digit in it (a serial inside a lens name).
fn private_strings(snap: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    for r in &snap.rows {
        let Some(Value::Str(s)) = &r.value else { continue };
        let s = s.trim().to_lowercase();
        if s.chars().count() >= 3 {
            out.push(s.clone());
        }
        for w in s.split(|c: char| !c.is_alphanumeric()) {
            if w.len() >= 5 && w.chars().any(|c| c.is_ascii_digit()) && w != s {
                out.push(w.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::owner::{Change, ChangeGroup, ChangeSource, PropRow, Status};
    use crate::camera::ptp::Form;

    fn snap(rows: Vec<PropRow>, groups: Vec<ChangeGroup>) -> Snapshot {
        Snapshot { status: Status::default(), rows, groups, catalog: None, catalog_path: None, session_path: None, fake: true, noisy: vec![] }
    }

    fn row(code: u16, value: Value) -> PropRow {
        PropRow { code, datatype: 0xffff, writable: true, form: Form::None, value: Some(value), error: None, available: true, changed_at: None }
    }

    fn patch(code: u16) -> LabelPatch {
        LabelPatch { code, ..Default::default() }
    }

    #[test]
    fn confirmed_needs_a_change_this_session() {
        let g = ChangeGroup { id: 1, at: 0.0, source: ChangeSource::Camera, changes: vec![Change { code: 0xd023, old: None, new: Some(Value::Int(2)) }], actor: None, closed_menu: false };
        let s = snap(vec![], vec![g]);
        let confirmed = |code| LabelPatch { confidence: Some(Confidence::Confirmed), ..patch(code) };
        assert!(check(&s, "mcp:test", &confirmed(0xd023)).is_ok());
        assert_eq!(check(&s, "mcp:test", &confirmed(0xd381)).unwrap_err().code, ErrorCode::NoEvidence);
        assert!(check(&s, "app", &confirmed(0xd381)).is_ok(), "the person in the Lab");
        assert!(check(&s, "mcp:test", &LabelPatch { confidence: Some(Confidence::Likely), ..patch(0xd381) }).is_ok());
    }

    #[test]
    fn camera_strings_stay_out() {
        let s = snap(vec![row(0xd215, Value::Str("Jane Doe".into())), row(0xd36d, Value::Str("XF23mm F1.4 R 4AB12345".into()))], vec![]);
        let named = |t: &str| LabelPatch { name: Some(t.into()), ..patch(0xd215) };
        assert_eq!(check(&s, "cli", &named("Owner jane doe")).unwrap_err().code, ErrorCode::PrivateText);
        assert_eq!(check(&s, "cli", &LabelPatch { notes: Some("serial 4ab12345".into()), ..patch(0xd36d) }).unwrap_err().code, ErrorCode::PrivateText);
        assert_eq!(check(&s, "cli", &LabelPatch { values: vec![("1".into(), "Jane Doe".into())], ..patch(0xd215) }).unwrap_err().code, ErrorCode::PrivateText);
        assert!(check(&s, "cli", &named("Copyright")).is_ok());
        assert!(check(&s, "app", &named("Jane Doe")).is_ok());
    }
}
