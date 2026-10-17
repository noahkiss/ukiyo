//! Output: the `--json` envelope, errors and their exit codes (docs/agent-interface.md,
//! section 3, "`--json` shapes" and "Exit codes").

use serde::Serialize;
use std::io::Write as _;
use ukiyo_core::api;
use ukiyo_core::read::Problem;

/// A failed command: what `--json` prints under `error`, plus the exit code.
#[derive(Debug, Clone, Serialize)]
pub struct Fail {
    pub code: String,
    pub message: String,
    #[serde(flatten)]
    pub more: Box<More>,
    #[serde(skip)]
    pub exit: i32,
}

/// What fixes it (boxed, to keep `Result<_, Fail>` small).
#[derive(Debug, Clone, Default, Serialize)]
pub struct More {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub walkthrough: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// The exit code for an error code.
pub fn exit_for(code: &str) -> i32 {
    match code {
        "usage" | "not_found" | "ambiguous" => 2,
        "not_connected" | "camera_asleep" => 3,
        "busy" | "menu_open" | "owner_conflict" => 4,
        c if c.starts_with("needs_") => 4,
        "unconfirmed_property" | "read_only" | "value_out_of_range" | "no_evidence" | "private_text" => 5,
        "plan_stale" | "plan_expired" => 6,
        "readback_mismatch" => 7,
        _ => 1,
    }
}

impl Fail {
    pub fn new(code: &str, message: impl Into<String>) -> Fail {
        Fail { code: code.into(), message: message.into(), more: Box::default(), exit: exit_for(code) }
    }

    pub fn usage(message: impl Into<String>) -> Fail {
        Fail::new("usage", message)
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Fail {
        self.more.hint = Some(hint.into());
        self
    }
}

impl From<Problem> for Fail {
    fn from(p: Problem) -> Fail {
        let more = More { hint: Some(p.hint), walkthrough: p.walkthrough, mode: p.mode.map(Into::into) };
        Fail { exit: exit_for(p.code), code: p.code.into(), message: p.message, more: Box::new(more) }
    }
}

impl From<api::Error> for Fail {
    fn from(e: api::Error) -> Fail {
        let code = e.code.name();
        let mut f = Fail::new(code, e.message);
        f.more = Box::new(More { hint: e.hint, walkthrough: e.walkthrough, mode: e.mode });
        if code == "unavailable" && f.more.hint.is_none() {
            f.hint("ukiyo serve owns the camera; its log is serve.log in the data folder.")
        } else {
            f
        }
    }
}

/// Where a command's output goes.
#[derive(Debug, Clone, Copy)]
pub struct Out {
    pub json: bool,
    pub quiet: bool,
}

impl Out {
    /// A progress line on stderr, unless `--quiet`.
    pub fn progress(&self, line: &str) {
        if !self.quiet {
            eprintln!("{line}");
        }
    }

    /// The `--json` success envelope, one line.
    pub fn data(&self, data: impl Serialize) {
        line(&serde_json::json!({ "ok": true, "data": data }).to_string());
    }

    /// Prints a failure: the envelope with `--json`, else text on stderr. Returns its exit code.
    pub fn fail(&self, f: &Fail) -> i32 {
        if self.json {
            line(&serde_json::json!({ "ok": false, "error": f }).to_string());
        } else {
            eprintln!("ukiyo: {}", f.message);
            if let Some(h) = &f.more.hint {
                eprintln!("{h}");
            }
            if let Some(w) = &f.more.walkthrough {
                eprintln!("How to switch: {w}");
            }
        }
        f.exit
    }
}

/// One line on stdout. A closed stdout (`| head`) ends the process quietly.
pub fn line(s: &str) {
    let mut o = std::io::stdout().lock();
    if writeln!(o, "{s}").and_then(|_| o.flush()).is_err() {
        std::process::exit(0);
    }
}

/// Lays out rows in columns, two spaces apart; the last column is not padded.
pub fn table(rows: &[Vec<String>]) -> Vec<String> {
    let n = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..n).map(|i| rows.iter().filter_map(|r| r.get(i)).map(|c| c.chars().count()).max().unwrap_or(0)).collect();
    rows.iter()
        .map(|r| {
            let mut s = String::new();
            for (i, c) in r.iter().enumerate() {
                if i + 1 < r.len() {
                    s.push_str(&format!("{c:<w$}  ", w = widths[i]));
                } else {
                    s.push_str(c);
                }
            }
            s.trim_end().to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn exit_codes() {
        for (code, exit) in [("usage", 2), ("not_connected", 3), ("camera_asleep", 3), ("needs_tether", 4), ("busy", 4), ("unavailable", 1), ("failed", 1), ("not_found", 2), ("unconfirmed_property", 5), ("plan_stale", 6), ("readback_mismatch", 7)] {
            assert_eq!(super::exit_for(code), exit, "{code}");
        }
    }

    #[test]
    fn columns() {
        let t = super::table(&[vec!["a".into(), "bb".into(), "c".into()], vec!["ddd".into(), "e".into(), "f".into()]]);
        assert_eq!(t, ["a    bb  c", "ddd  e   f"]);
    }
}
