//! Film recipe text, as Fuji X Weekly prints it: one `Label: value` line per setting.
//! One parser serves the bundled catalogue (`examples/fxw_refresh.rs`) and the in-app paste
//! import (`recipe_parse`). Values stay in the units the camera menu shows (Highlight +1.5,
//! not the slot's ×10); docs/protocol.md "Recipe slots" has the slot encodings. A label the
//! parser does not know, or a value it cannot read, becomes a warning on that field.

pub mod catalogue;
pub mod html;
pub mod post;
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
        "d range priority" | "dynamic range priority" | "dr priority" | "dr p" => Field::DRangePriority,
        "grain effect" | "grain" | "grain effect roughness" | "grain roughness" => Field::Grain,
        "grain size" | "grain effect size" => Field::GrainSize,
        "color chrome effect" | "color chrome" | "colour chrome effect" | "colour chrome" | "color chrome effect strength" => Field::ColorChrome,
        "color chrome fx blue" | "color chrome effect blue" | "color chrome blue" | "colour chrome fx blue" | "color chrome fx blue effect" | "cc fx blue" => Field::ColorChromeFxBlue,
        "white balance" | "wb" | "white balance shift" => Field::WhiteBalance,
        "highlight" | "highlights" | "highlight tone" => Field::Highlight,
        "shadow" | "shadows" | "shadow tone" => Field::Shadow,
        "tone curve" => Field::ToneCurve,
        "color" | "colour" | "saturation" | "color saturation" => Field::Color,
        "sharpness" | "sharpening" => Field::Sharpness,
        "high iso nr" | "noise reduction" | "nr" | "high iso noise reduction" | "iso nr" => Field::HighIsoNr,
        "clarity" => Field::Clarity,
        "monochromatic color" | "monochromatic colour" | "monochrome color" | "toning" => Field::MonochromaticColor,
        "iso" | "exposure compensation" | "exposure comp" | "exposure" | "ev" | "exposure compensation typically" => Field::Note,
        // Movie settings in video recipes.
        "interframe nr" | "interframe noise reduction" => Field::Note,
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

/// Parses a recipe block: one setting per line. Blank lines are skipped. An unlabelled line
/// that names a film simulation sets it, which is how older posts open the block.
pub fn parse_text(text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut seen: Vec<Field> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim().trim_matches('\u{a0}').trim();
        if line.is_empty() {
            continue;
        }
        let Some((label, value)) = split_line(line) else {
            match values::film_sim(line) {
                Some(sim) if out.settings.film_simulation.is_none() => {
                    out.settings.film_simulation = Some(sim);
                    seen.push(Field::FilmSimulation);
                }
                _ => out.warnings.push(Warning { kind: WarningKind::Unlabelled, label: String::new(), value: line.to_string(), message: "Not a setting".into() }),
            }
            continue;
        };
        let Some(field) = field_for(label) else {
            out.warnings.push(Warning { kind: WarningKind::UnknownLabel, label: label.into(), value: value.into(), message: "Unknown setting".into() });
            continue;
        };
        if field == Field::Note {
            out.notes.push(Note { label: label.into(), value: value.into() });
            continue;
        }
        if seen.contains(&field) {
            out.warnings.push(Warning { kind: WarningKind::Duplicate, label: label.into(), value: value.into(), message: "Given twice; the first value is kept".into() });
            continue;
        }
        seen.push(field);
        if let Err(message) = values::apply(&mut out.settings, field, value) {
            out.warnings.push(Warning { kind: WarningKind::BadValue, label: label.into(), value: value.into(), message });
        }
    }
    out
}
