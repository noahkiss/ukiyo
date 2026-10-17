//! Reading the session logs back (`sessions/*.jsonl` in the data folder, written by `lab`).
//! Each line is one JSON object with a `kind` and `t`, the wall-clock ms.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// The session files, oldest first. Their names are start stamps, so name order is time order.
pub fn files(data_dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(data_dir.join("sessions"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    v.sort();
    v
}

#[derive(Debug, Clone, Serialize)]
pub struct Log {
    /// The files read, oldest first.
    pub files: Vec<String>,
    /// Each line as written, plus `session`: its file's name without `.jsonl`.
    pub entries: Vec<serde_json::Value>,
}

/// The log lines: with `since` (wall-clock ms), every line at or after it from every session;
/// without, the newest session whole. Lines that are not JSON objects are skipped.
pub fn read(data_dir: &Path, since: Option<i64>) -> std::io::Result<Log> {
    let all = files(data_dir);
    let chosen: Vec<PathBuf> = match since {
        Some(_) => all,
        None => all.into_iter().last().into_iter().collect(),
    };
    let mut log = Log { files: vec![], entries: vec![] };
    for f in chosen {
        let text = std::fs::read_to_string(&f)?;
        let name = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut any = false;
        for line in text.lines() {
            let Ok(serde_json::Value::Object(mut o)) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            if let Some(since) = since {
                if o.get("t").and_then(|t| t.as_i64()).is_none_or(|t| t < since) {
                    continue;
                }
            }
            o.insert("session".into(), name.clone().into());
            log.entries.push(o.into());
            any = true;
        }
        if any || since.is_none() {
            log.files.push(f.display().to_string());
        }
    }
    Ok(log)
}

/// Parses `--since`: a span back from `now_ms` (`90s`, `10m`, `2h`, `1d`), or a local date
/// or time (`2026-10-17`, `2026-10-17 14:30`, `2026-10-17T14:30:05`), or RFC 3339.
pub fn parse_since(s: &str, now_ms: i64) -> Option<i64> {
    let s = s.trim();
    if let Some(unit) = s.chars().last().filter(|c| c.is_ascii_alphabetic()) {
        if let Ok(n) = s[..s.len() - 1].parse::<f64>() {
            let per = match unit {
                's' => 1_000.0,
                'm' => 60_000.0,
                'h' => 3_600_000.0,
                'd' => 86_400_000.0,
                _ => return None,
            };
            return (n >= 0.0).then(|| now_ms - (n * per) as i64);
        }
    }
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(t.timestamp_millis());
    }
    use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
    let local = |n: NaiveDateTime| Local.from_local_datetime(&n).earliest().map(|t| t.timestamp_millis());
    for f in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, f) {
            return local(n);
        }
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().and_then(|d| local(d.and_hms_opt(0, 0, 0)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_forms() {
        let now = 10_000_000;
        assert_eq!(parse_since("90s", now), Some(now - 90_000));
        assert_eq!(parse_since("2h", now), Some(now - 7_200_000));
        assert_eq!(parse_since("1.5m", now), Some(now - 90_000));
        assert_eq!(parse_since("2026-10-17T14:30:00Z", now), Some(1_792_247_400_000));
        assert!(parse_since("2026-10-17", now).is_some());
        assert!(parse_since("2026-10-17 14:30", now).is_some());
        assert_eq!(parse_since("soon", now), None);
        assert_eq!(parse_since("5y", now), None);
    }

    #[test]
    fn reads_newest_or_since() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("sessions");
        std::fs::create_dir_all(&s).unwrap();
        std::fs::write(s.join("2026-10-16_100000.jsonl"), "{\"kind\":\"start\",\"t\":100}\n{\"kind\":\"stop\",\"t\":200}\n").unwrap();
        std::fs::write(s.join("2026-10-17_100000.jsonl"), "{\"kind\":\"start\",\"t\":300}\nnot json\n").unwrap();
        let log = read(d.path(), None).unwrap();
        assert_eq!(log.entries.len(), 1);
        assert_eq!(log.entries[0]["session"], "2026-10-17_100000");
        let log = read(d.path(), Some(150)).unwrap();
        assert_eq!(log.entries.iter().map(|e| e["t"].as_i64().unwrap()).collect::<Vec<_>>(), [200, 300]);
        assert_eq!(log.files.len(), 2);
        assert!(read(&d.path().join("none"), None).unwrap().entries.is_empty());
    }
}
