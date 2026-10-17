//! Film recipe text, as Fuji X Weekly prints it: one `Label: value` line per setting.
//! One parser serves the bundled catalogue (`examples/fxw_refresh.rs`) and the in-app paste
//! import (`recipe_parse`). Values stay in the units the camera menu shows (Highlight +1.5,
//! not the slot's ×10); docs/protocol.md "Recipe slots" has the slot encodings. A label the
//! parser does not know, or a value it cannot read, becomes a warning on that field.

pub mod bundle;
pub mod catalogue;
pub mod fetch;
pub mod family;
pub mod html;
pub mod post;
pub mod slot;
pub mod sources;
pub(crate) mod values;

#[cfg(test)]
mod tests;

use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::LazyLock;

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
    /// Values the text gives for other sensors: "Off (X-Trans V); Weak (X-Trans IV)". The
    /// settings hold the first; each of these is one setting of another sensor's variant.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<Alternative>,
    /// The sensor the first value is for, when the text names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensor: Option<String>,
    /// From a post (`recipe_parse {url}`): the recipe's name, as its post names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The post the recipe came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Further recipes in the same post (other sensors, or a post that holds several).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<OtherRecipe>,
}

/// Another recipe found in the same post.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct OtherRecipe {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensor: Option<String>,
    pub settings: Settings,
    pub notes: Vec<Note>,
    pub warnings: Vec<Warning>,
}

/// Sets one setting from a label and a value as a post writes them ("Highlight", "+1.5"):
/// the editor's way to change a saved recipe. The parser's label variants all work.
pub fn set_field(s: &mut Settings, label: &str, value: &str) -> Result<(), String> {
    let field = field_for(label).filter(|f| *f != Field::Note).ok_or_else(|| format!("{label} is not a recipe setting"))?;
    if values::not_applicable(value) || value.trim().is_empty() {
        match field {
            Field::WhiteBalance => {
                s.white_balance = None;
                s.color_temp = None;
            }
            Field::WbShift => {
                s.wb_shift_r = None;
                s.wb_shift_b = None;
            }
            Field::Highlight => s.highlight_tone = None,
            Field::Shadow => s.shadow_tone = None,
            Field::ToneCurve => {
                s.highlight_tone = None;
                s.shadow_tone = None;
            }
            Field::Color => s.color = None,
            Field::Sharpness => s.sharpness = None,
            Field::HighIsoNr => s.high_iso_nr = None,
            Field::MonochromaticColor => {
                s.monochromatic_color_wc = None;
                s.monochromatic_color_mg = None;
            }
            Field::MonoWc => s.monochromatic_color_wc = None,
            Field::MonoMg => s.monochromatic_color_mg = None,
            f => values::clear(s, f),
        }
        return Ok(());
    }
    values::apply(s, field, value)
}

/// One setting's value for another sensor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Alternative {
    pub sensor: String,
    pub label: String,
    pub value: String,
}

/// Applies alternative lines to a copy of the settings: another sensor's variant. A
/// not-applicable value ("N/A") clears the setting.
pub fn variant(settings: &Settings, alts: &[&Alternative]) -> Settings {
    let mut s = settings.clone();
    for a in alts {
        if let Some(field) = field_for(&a.label) {
            if values::not_applicable(&a.value) {
                values::clear(&mut s, field);
            } else {
                let _ = values::apply(&mut s, field, &a.value);
            }
        }
    }
    s
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
    /// The red and blue shift on a line of its own.
    WbShift,
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
        "d range priority" | "dynamic range priority" | "dr priority" | "dr p" | "priority" | "wide dynamic range" | "wide d range" => Field::DRangePriority,
        "grain effect" | "grain" | "grain effect roughness" | "grain roughness" => Field::Grain,
        "grain size" | "grain effect size" => Field::GrainSize,
        "color chrome effect" | "color chrome" | "colour chrome effect" | "colour chrome" | "color chrome effect strength" | "col chr effect" | "col chr" | "cc effect" | "chrome fx" | "chrome color fx" | "color chrome fx" | "ccr" | "cc" => Field::ColorChrome,
        "color chrome fx blue" | "color chrome effect blue" | "color chrome blue" | "colour chrome fx blue" | "colour chrome effect blue" | "color chrome fx blue effect" | "cc fx blue" | "colour chrome blue" | "colour chrome blue effect" | "color chrome blue effect" | "col chr blue" | "col chr fx blue" | "chrome fx blue" | "ccb" => Field::ColorChromeFxBlue,
        "wb shift" | "white balance shift" | "wb color shift" | "wb colour shift" | "shift" | "color shift" | "wb shift r b" | "wb shift r" | "wb offset" | "auto wb shift" | "awb shift" | "wb shift rb" => Field::WbShift,
        "white balance" | "wb" | "color temperature" | "colour temperature" | "color temp" | "colour temp" | "kelvin" => Field::WhiteBalance,
        "highlight" | "highlights" | "highlight tone" | "hightlight" | "hightlight tone" | "highlights tone" | "h tone" | "highlight tones" => Field::Highlight,
        "shadow" | "shadows" | "shadow tone" | "shadows tone" | "s tone" | "shading tone" | "shadow tones" => Field::Shadow,
        "tone curve" => Field::ToneCurve,
        "color" | "colour" | "saturation" | "color saturation" => Field::Color,
        "sharpness" | "sharpening" | "sharpeness" | "sharp" => Field::Sharpness,
        "high iso nr" | "noise reduction" | "nr" | "high iso noise reduction" | "iso nr" | "iso n r" | "high iso n r" | "iso noise reduction" | "noise" | "noise red" | "noise redution" => Field::HighIsoNr,
        "clarity" => Field::Clarity,
        "monochromatic color" | "monochromatic colour" | "monochrome color" | "mono colour" | "mono color" | "mono shift" | "monochrome shift" | "toning" | "monochromatic color toning" | "b w toning" | "tone" => Field::MonochromaticColor,
        "wc" | "mono wc" | "monochrome wc" | "warm cool" | "monochromatic color wc" | "mono warm cool" => Field::MonoWc,
        "mg" | "mono mg" | "monochrome mg" | "magenta green" | "monochromatic color mg" | "mono magenta green" => Field::MonoMg,
        "smooth skin effect" | "smooth skin" => Field::SmoothSkin,
        "iso" | "exposure compensation" | "exposure comp" | "exposure" | "ev" | "ev comp" | "exposure compensation typically" | "ev suggestion" | "ev compensation" | "exp comp" | "push pull process" | "push pull" | "push pull exposure" | "aperture" => Field::Note,
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

static BARE_DR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^dr[- ]?(100|200|400|auto)$").unwrap());
static BARE_KELVIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\d{4,5}\s*k$").unwrap());
static BARE_SHIFT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^r\s*[+-]?\d+\s*[,&]?\s*b\s*[+-]?\d+$").unwrap());

/// A line with no colon: a label then its value ("Dynamic Range DR400", "Color +4"), or a
/// bare value that names its setting ("DR400", "5200K", "R+2 B-4").
fn colonless(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    if BARE_DR.is_match(t) {
        return Some(("Dynamic Range".into(), t.into()));
    }
    if BARE_KELVIN.is_match(t) {
        return Some(("White Balance".into(), t.into()));
    }
    if BARE_SHIFT.is_match(t) {
        return Some(("WB Shift".into(), t.into()));
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    (1..words.len().min(5)).rev().find_map(|k| {
        let label = words[..k].join(" ");
        field_for(&label).map(|_| (label, words[k..].join(" ")))
    })
}

/// A line holding several settings with no colons, "0EV, Dynamic Range DR100, Grain Effect
/// Weak", as one line each; any other line as it is.
fn one_line_settings(line: &str) -> Vec<&str> {
    let parts: Vec<&str> = line.split(", ").collect();
    if !line.contains(':') && parts.len() >= 3 && parts.iter().filter(|p| colonless(p).is_some()).count() * 2 > parts.len() {
        parts
    } else {
        vec![line]
    }
}

/// The longest note or warning value kept. Longer text is a comment, not a setting, and is
/// cut so no prose is stored.
const VALUE_MAX: usize = 100;

fn clip(s: &str) -> String {
    match s.char_indices().nth(VALUE_MAX) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// Parses a recipe block: one setting per line. Blank lines are skipped. An unlabelled line
/// that names a film simulation sets it, which is how older posts open the block. A label
/// joining two settings ("Highlight & Shadow: +2") sets both.
pub fn parse_text(text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut seen: Vec<Field> = Vec::new();
    for raw in text.lines().flat_map(one_line_settings) {
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
            let value = match values::sensor_alternatives(&value) {
                Some((first, first_sensor, others)) => {
                    if out.sensor.is_none() {
                        out.sensor = first_sensor;
                    }
                    out.alternatives.extend(others.into_iter().map(|(sensor, v)| Alternative { sensor, label: label.clone(), value: v }));
                    first
                }
                None => value.clone(),
            };
            if let Err(message) = values::apply(&mut out.settings, field, &value) {
                out.warnings.push(Warning { kind: WarningKind::BadValue, label: label.clone(), value: value.clone(), message });
            }
        }
    }
    for n in &mut out.notes {
        n.value = clip(&n.value);
    }
    for w in &mut out.warnings {
        w.value = clip(&w.value);
    }
    out
}
