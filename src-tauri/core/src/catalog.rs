//! The property catalog: one JSON file per camera model (`catalog/<model>.json`,
//! docs/catalog.md). It names codes, labels their values and records how sure each entry
//! is. It never holds a current value, a serial number or an owner string.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Confirmed,
    Likely,
    Guess,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Where the setting lives on the camera, `SECTION > ITEM`, or the dial or switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu: Option<String>,
    /// The PTP datatype: uint16, int32, string, ...
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub datatype: Option<String>,
    /// Value (decimal, or the text of a string value) to the label the camera shows.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub values: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// The raw value divided by this is the value in `unit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<Confidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Connection and capture modes that read it: stills, movie, rawconv.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// The camera takes a write of this code only while another code holds one of some
    /// values (Monochromatic Color needs a monochrome film simulation). Plans set such a
    /// prerequisite first, for the time of the write when they must.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<Requirement>,
    /// Writing this code sets another code on the camera (choosing ACROS sets Color to 0).
    /// Plans write that other code after this one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resets: Vec<Reset>,
}

/// One write condition: `code` must hold one of `values` (raw keys, as in `values`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct Requirement {
    pub code: String,
    pub values: Vec<String>,
    /// The value a plan sets for the time of the write when none of `values` holds; the first
    /// of `values` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp: Option<String>,
}

/// One camera rule: writing one of `when` (any value when empty) sets `code` to `to`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct Reset {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<String>,
    pub code: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Usb {
    pub vendor: String,
    pub product: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Catalog {
    pub schema: u32,
    pub model: String,
    pub manufacturer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usb: Option<Usb>,
    /// Firmware versions the entries were mapped on.
    #[serde(default)]
    pub firmware: Vec<String>,
    /// Keyed by the code as `0x` and four lowercase hex digits.
    pub properties: IndexMap<String, Entry>,
}

/// A labelling edit from the Lab. `None` leaves a field alone; an empty string clears it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LabelPatch {
    pub code: u16,
    pub name: Option<String>,
    pub menu: Option<String>,
    /// Value key to label; an empty label removes it.
    pub values: Vec<(String, String)>,
    pub confidence: Option<Confidence>,
    pub notes: Option<String>,
}

pub fn key(code: u16) -> String {
    format!("{code:#06x}")
}

/// The catalog file name for a model: `X-T5` is `x-t5`.
pub fn slug(model: &str) -> String {
    let s: String = model
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "unknown".into()
    } else {
        s
    }
}

impl Catalog {
    pub fn empty(model: &str, manufacturer: &str) -> Self {
        Self {
            schema: SCHEMA,
            model: model.into(),
            manufacturer: manufacturer.into(),
            usb: None,
            firmware: Vec::new(),
            properties: IndexMap::new(),
        }
    }

    pub fn path(dir: &Path, model: &str) -> PathBuf {
        dir.join(format!("{}.json", slug(model)))
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let c: Catalog = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        anyhow::ensure!(c.schema == SCHEMA, "{} has schema {}, this build reads {SCHEMA}", path.display(), c.schema);
        Ok(c)
    }

    /// Writes the file whole (temp file, then rename), codes and value labels in order.
    pub fn save(&mut self, path: &Path) -> anyhow::Result<()> {
        self.properties.sort_by(|a, _, b, _| a.cmp(b));
        for e in self.properties.values_mut() {
            sort_values(&mut e.values);
        }
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn apply(&mut self, p: &LabelPatch, datatype: Option<&str>) {
        let e = self.properties.entry(key(p.code)).or_default();
        let set = |field: &mut Option<String>, v: &Option<String>| {
            if let Some(v) = v {
                let v = v.trim();
                *field = (!v.is_empty()).then(|| v.to_string());
            }
        };
        let before = e.clone();
        set(&mut e.name, &p.name);
        set(&mut e.menu, &p.menu);
        set(&mut e.notes, &p.notes);
        for (k, label) in &p.values {
            let label = label.trim();
            if label.is_empty() {
                e.values.shift_remove(k);
            } else {
                e.values.insert(k.clone(), label.to_string());
            }
        }
        if let Some(c) = p.confidence {
            e.confidence = Some(c);
        }
        if e.datatype.is_none() {
            e.datatype = datatype.map(str::to_string);
        }
        if *e != before {
            e.source = Some(match e.source.take() {
                Some(s) if s.contains("lab session") => s,
                Some(s) => format!("{s} + lab session"),
                None => "lab session".into(),
            });
        }
    }
}

fn sort_values(v: &mut IndexMap<String, String>) {
    if v.keys().all(|k| k.parse::<i64>().is_ok()) {
        v.sort_by(|a, _, b, _| a.parse::<i64>().unwrap().cmp(&b.parse::<i64>().unwrap()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_catalog() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/x-t5.json")
    }

    #[test]
    fn the_committed_catalog_loads_and_saves_byte_for_byte() {
        let text = std::fs::read_to_string(repo_catalog()).unwrap();
        let mut c = Catalog::load(&repo_catalog()).unwrap();
        assert_eq!(c.model, "X-T5");
        assert_eq!(c.properties["0xd381"].values["3"], "OFF");
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("x-t5.json");
        c.save(&out).unwrap();
        assert_eq!(std::fs::read_to_string(out).unwrap(), text, "the seed script and the app write the same format");
    }

    #[test]
    fn labels_merge_and_mark_the_source() {
        let mut c = Catalog::empty("X-T5", "FUJIFILM");
        c.properties.insert(key(0xD1B2), Entry { source: Some("menu map".into()), ..Default::default() });
        c.apply(
            &LabelPatch {
                code: 0xD1B2,
                name: Some("JpegHeif".into()),
                menu: Some("IMAGE QUALITY SETTING > SELECT JPEG/HEIF".into()),
                values: vec![("18".into(), "HEIF".into()), ("7".into(), "JPEG".into())],
                confidence: Some(Confidence::Confirmed),
                notes: None,
            },
            Some("uint16"),
        );
        let e = &c.properties["0xd1b2"];
        assert_eq!(e.source.as_deref(), Some("menu map + lab session"));
        assert_eq!(e.datatype.as_deref(), Some("uint16"));
        c.apply(&LabelPatch { code: 0xD1B2, values: vec![("18".into(), "".into())], name: Some("".into()), ..Default::default() }, None);
        let e = &c.properties["0xd1b2"];
        assert!(e.name.is_none());
        assert_eq!(e.values.len(), 1);
        assert_eq!(e.source.as_deref(), Some("menu map + lab session"));
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("X-T5"), "x-t5");
        assert_eq!(slug("X100VI"), "x100vi");
        assert_eq!(slug(" GFX 100S II "), "gfx-100s-ii");
    }

    #[test]
    fn the_catalog_holds_no_current_values() {
        let text = std::fs::read_to_string(repo_catalog()).unwrap();
        for word in ["\"current\"", "\"default\"", "serial\":", "\"value\""] {
            assert!(!text.contains(word), "catalog has {word}");
        }
    }
}
