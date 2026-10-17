//! Film recipe text, as Fuji X Weekly prints it: one `Label: value` line per setting.
//! One parser serves the bundled catalogue (`examples/fxw_refresh.rs`) and the in-app paste
//! import (`recipe_parse`). Values stay in the units the camera menu shows (Highlight +1.5,
//! not the slot's ×10); docs/protocol.md "Recipe slots" has the slot encodings. A label the
//! parser does not know, or a value it cannot read, becomes a warning on that field.

pub mod catalogue;
pub mod html;
pub mod post;
pub mod sources;
mod values;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use specta::Type;

/// The settings a custom slot holds. Field names follow FilmKit. `None`: the text does not
/// give the setting (older posts lack Color Chrome, FX Blue and Clarity, for example).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Canonical name: "Classic Chrome", "Pro Neg. Std", "Acros + R filter", ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub film_simulation: Option<String>,
    /// "DR100", "DR200", "DR400" or "Auto".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_range: Option<String>,
    /// "Off", "Weak", "Strong" or "Auto".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d_range_priority: Option<String>,
    /// "Off", "Weak" or "Strong".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain_effect: Option<String>,
    /// "Small" or "Large".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_chrome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_chrome_fx_blue: Option<String>,
    /// A preset ("Auto", "Daylight", "Fluorescent 3", ...) or "Kelvin" with `color_temp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub white_balance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_temp: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wb_shift_r: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wb_shift_b: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight_tone: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_tone: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharpness: Option<f64>,
    /// "High ISO NR"; older posts call it "Noise Reduction".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_iso_nr: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clarity: Option<f64>,
    /// "Off", "Weak" or "Strong".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smooth_skin: Option<String>,
    /// Monochromatic Color, warm (+) to cool (-).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monochromatic_color_wc: Option<f64>,
    /// Monochromatic Color, magenta (+) to green (-).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monochromatic_color_mg: Option<f64>,
}

/// A setting the text gives that a slot cannot hold: ISO, exposure compensation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Note {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WarningKind {
    /// A `Label: value` line whose label is not a known setting.
    UnknownLabel,
    /// A known label whose value could not be read, or was read only in part.
    BadValue,
    /// A line with no label that is not a film simulation name.
    Unlabelled,
    /// A setting given twice; the first value is kept.
    Duplicate,
}

/// One line the parser could not use as it stands. Nothing is dropped without one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Warning {
    pub kind: WarningKind,
    /// The label as written; empty for an unlabelled line.
    pub label: String,
    pub value: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct Parsed {
    pub settings: Settings,
    pub notes: Vec<Note>,
    pub warnings: Vec<Warning>,
}

/// What a label names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    FilmSimulation,
    DynamicRange,
    DRangePriority,
    Grain,
    GrainSize,
    ColorChrome,
    ColorChromeFxBlue,
    WhiteBalance,
    Highlight,
    Shadow,
    ToneCurve,
    Color,
    Sharpness,
    HighIsoNr,
    Clarity,
    MonochromaticColor,
    MonoWc,
    MonoMg,
    SmoothSkin,
    /// A shooting note: kept, but not a slot setting.
    Note,
}

/// Lowercase letters and digits, single spaces: "High ISO NR" -> "high iso nr".
pub(crate) fn norm(s: &str) -> String {
    let mapped: String = s.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' }).collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The field a label names, if the parser knows it.
pub(crate) fn field_for(label: &str) -> Option<Field> {
    let l = norm(label);
    let l = l.strip_suffix(" setting").unwrap_or(&l);
    Some(match l {
        "film simulation" | "film sim" | "simulation" | "film simulation mode" => Field::FilmSimulation,
        "dynamic range" | "dr" | "d range" => Field::DynamicRange,
        "d range priority" | "dynamic range priority" | "dr priority" | "dr p" | "wide dynamic range" | "wide d range" => Field::DRangePriority,
        "grain effect" | "grain" | "grain effect roughness" | "grain roughness" => Field::Grain,
        "grain size" | "grain effect size" => Field::GrainSize,
        "color chrome effect" | "color chrome" | "colour chrome effect" | "colour chrome" | "color chrome effect strength" | "col chr effect" | "col chr" | "cc effect" => Field::ColorChrome,
        "color chrome fx blue" | "color chrome effect blue" | "color chrome blue" | "colour chrome fx blue" | "colour chrome effect blue" | "color chrome fx blue effect" | "cc fx blue" | "col chr blue" | "col chr fx blue" => Field::ColorChromeFxBlue,
        "white balance" | "wb" | "white balance shift" | "color temperature" | "colour temperature" | "color temp" | "colour temp" | "kelvin" => Field::WhiteBalance,
        "highlight" | "highlights" | "highlight tone" => Field::Highlight,
        "shadow" | "shadows" | "shadow tone" => Field::Shadow,
        "tone curve" => Field::ToneCurve,
        "color" | "colour" | "saturation" | "color saturation" => Field::Color,
        "sharpness" | "sharpening" | "sharpeness" => Field::Sharpness,
        "high iso nr" | "noise reduction" | "nr" | "high iso noise reduction" | "iso nr" | "iso n r" | "high iso n r" => Field::HighIsoNr,
        "clarity" => Field::Clarity,
        "monochromatic color" | "monochromatic colour" | "monochrome color" | "mono colour" | "mono color" | "toning" | "monochromatic color toning" | "b w toning" | "tone" => Field::MonochromaticColor,
        "wc" | "mono wc" | "monochrome wc" | "warm cool" | "monochromatic color wc" | "mono warm cool" => Field::MonoWc,
        "mg" | "mono mg" | "monochrome mg" | "magenta green" | "monochromatic color mg" | "mono magenta green" => Field::MonoMg,
        "smooth skin effect" | "smooth skin" => Field::SmoothSkin,
        "iso" | "exposure compensation" | "exposure comp" | "exposure" | "ev" | "ev comp" | "exposure compensation typically" => Field::Note,
        // Movie settings in video recipes.
        "interframe nr" | "interframe noise reduction" => Field::Note,
        // Shooting notes that are not slot settings.
        "image quality" | "aspect ratio" | "image size" | "flash" | "filters" | "filter" | "lens" | "lens filter" => Field::Note,
        _ => return None,
    })
}

/// Splits `Label: value`. A colon inside a value ("ISO: Auto, up to ISO 6400") stays.
pub(crate) fn split_line(line: &str) -> Option<(&str, &str)> {
    let (label, value) = line.split_once(':')?;
    let label = label.trim();
    // A label is a few words, not a sentence that happens to hold a colon.
    if label.is_empty() || label.len() > 40 || label.split_whitespace().count() > 6 {
        return None;
    }
    Some((label, value.trim()))
}

/// Splits a block that holds several recipes where a second film simulation starts: a
/// "Film Simulation:" line or a bare simulation name, once the current part already has one.
pub fn split_recipes(text: &str) -> Vec<String> {
    let mut parts: Vec<Vec<&str>> = vec![Vec::new()];
    let mut has_sim = false;
    for line in text.lines() {
        let t = line.trim();
        let starts = match split_line(t) {
            Some((l, _)) => field_for(l) == Some(Field::FilmSimulation),
            None => values::film_sim(t).is_some(),
        };
        if starts && has_sim {
            parts.push(Vec::new());
            has_sim = false;
        }
        has_sim |= starts;
        parts.last_mut().unwrap().push(line);
    }
    parts.into_iter().map(|p| p.join("\n")).filter(|p| !p.trim().is_empty()).collect()
}

/// A line with no colon that starts with a slot label: "Dynamic Range DR400", "Color +4".
fn colonless(line: &str) -> Option<(String, String)> {
    let words: Vec<&str> = line.split_whitespace().collect();
    (1..words.len().min(5)).rev().find_map(|k| {
        let label = words[..k].join(" ");
        field_for(&label).filter(|f| *f != Field::Note).map(|_| (label, words[k..].join(" ")))
    })
}

/// Parses a recipe block: one setting per line. Blank lines are skipped. An unlabelled line
/// that names a film simulation sets it, which is how older posts open the block. A label
/// joining two settings ("Highlight & Shadow: +2") sets both.
pub fn parse_text(text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut seen: Vec<Field> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim().trim_matches('\u{a0}').trim();
        if line.is_empty() {
            continue;
        }
        let (label, value) = match split_line(line) {
            Some((l, v)) => (l.to_string(), v.to_string()),
            None => match colonless(line) {
                Some(lv) => lv,
                None => {
                    match values::film_sim(line) {
                        Some(sim) if out.settings.film_simulation.is_none() => {
                            out.settings.film_simulation = Some(sim);
                            seen.push(Field::FilmSimulation);
                        }
                        _ => out.warnings.push(Warning { kind: WarningKind::Unlabelled, label: String::new(), value: line.to_string(), message: "Not a setting".into() }),
                    }
                    continue;
                }
            },
        };
        let fields: Vec<Field> = match field_for(&label) {
            Some(f) => vec![f],
            None => {
                let parts: Vec<Option<Field>> = label.split(" & ").map(field_for).collect();
                if parts.len() > 1 && parts.iter().all(Option::is_some) {
                    parts.into_iter().flatten().collect()
                } else {
                    out.warnings.push(Warning { kind: WarningKind::UnknownLabel, label, value, message: "Unknown setting".into() });
                    continue;
                }
            }
        };
        for field in fields {
            if field == Field::Note {
                out.notes.push(Note { label: label.clone(), value: value.clone() });
                continue;
            }
            if seen.contains(&field) {
                out.warnings.push(Warning { kind: WarningKind::Duplicate, label: label.clone(), value: value.clone(), message: "Given twice; the first value is kept".into() });
                continue;
            }
            seen.push(field);
            if let Err(message) = values::apply(&mut out.settings, field, &value) {
                out.warnings.push(Warning { kind: WarningKind::BadValue, label: label.clone(), value: value.clone(), message });
            }
        }
    }
    out
}
