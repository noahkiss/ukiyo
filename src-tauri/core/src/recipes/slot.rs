//! The C1-C7 custom slots: their PTP fields (D18D-D1A5, read and written in USB RAW CONV.
//! mode after selecting the slot with D18C), and the recipe settings in slot encoding.
//!
//! Encodings follow fujifilm-ptp-recipes (ILFforever, docs CC BY 4.0) and FilmKit (MIT); see
//! docs/protocol.md, "Recipe slots". Settings stay in menu units (Highlight +1.5); a slot
//! holds most of them ×10.

use super::Settings;
use crate::camera::ptp::Value;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Selects the slot the fields below read and write: 1 to 7.
pub const SELECT: u16 = 0xD18C;
/// The slot's name, a PTP string.
pub const NAME: u16 = 0xD18D;
pub const UINT16: u16 = 0x0004;
pub const INT16: u16 = 0x0003;
pub const STRING: u16 = 0xFFFF;

pub const FILM_SIM: u16 = 0xD192;
pub const DRANGE: u16 = 0xD190;
pub const DRP: u16 = 0xD191;
pub const MONO_WC: u16 = 0xD193;
pub const MONO_MG: u16 = 0xD194;
pub const GRAIN: u16 = 0xD195;
pub const COLOR_CHROME: u16 = 0xD196;
pub const FX_BLUE: u16 = 0xD197;
pub const SMOOTH_SKIN: u16 = 0xD198;
pub const WB: u16 = 0xD199;
pub const WB_SHIFT_R: u16 = 0xD19A;
pub const WB_SHIFT_B: u16 = 0xD19B;
pub const KELVIN: u16 = 0xD19C;
pub const HIGHLIGHT: u16 = 0xD19D;
pub const SHADOW: u16 = 0xD19E;
pub const COLOR: u16 = 0xD19F;
pub const SHARPNESS: u16 = 0xD1A0;
pub const HIGH_ISO_NR: u16 = 0xD1A1;
pub const CLARITY: u16 = 0xD1A2;

/// Every field a slot read returns, with its datatype, in the order plans write them:
/// film simulation first, then D-range priority before dynamic range, the effects, white
/// balance, then the tones (ptp-recipes "Suggested Write Order"). D18E, D18F, D1A3 and D1A4
/// are not recipe settings: read, never written. D1A5 belongs to the live block and is never
/// touched.
pub const FIELDS: &[(u16, u16)] = &[
    (FILM_SIM, UINT16),
    (DRP, UINT16),
    (DRANGE, UINT16),
    (GRAIN, UINT16),
    (COLOR_CHROME, UINT16),
    (FX_BLUE, UINT16),
    (SMOOTH_SKIN, UINT16),
    (WB, UINT16),
    (KELVIN, UINT16),
    (WB_SHIFT_R, INT16),
    (WB_SHIFT_B, INT16),
    (HIGHLIGHT, INT16),
    (SHADOW, INT16),
    (COLOR, INT16),
    (SHARPNESS, INT16),
    (HIGH_ISO_NR, UINT16),
    (CLARITY, INT16),
    (MONO_WC, INT16),
    (MONO_MG, INT16),
    (0xD18E, UINT16),
    (0xD18F, UINT16),
    (0xD1A3, UINT16),
    (0xD1A4, UINT16),
];

/// Fields a slot plan may write: the recipe settings, then the name.
pub fn writable(code: u16) -> bool {
    code == NAME || FIELDS.iter().take(19).any(|(c, _)| *c == code)
}

/// The datatype of a slot code, for reads and writes without a descriptor (RAW CONV. mode
/// sends none).
pub fn datatype(code: u16) -> Option<u16> {
    if code == NAME {
        return Some(STRING);
    }
    if code == SELECT {
        return Some(UINT16);
    }
    FIELDS.iter().find(|(c, _)| *c == code).map(|(_, t)| *t)
}

/// The order a plan writes slot codes in.
pub fn order(code: u16) -> usize {
    FIELDS.iter().position(|(c, _)| *c == code).unwrap_or(if code == NAME { 100 } else { 99 })
}

/// "C3" or "3" as 3.
pub fn parse_slot(s: &str) -> Option<u8> {
    let t = s.trim();
    let t = t.strip_prefix("slot:").unwrap_or(t);
    let t = t.strip_prefix('C').or_else(|| t.strip_prefix('c')).unwrap_or(t);
    t.parse::<u8>().ok().filter(|n| (1..=7).contains(n))
}

pub fn slot_name(n: u8) -> String {
    format!("C{n}")
}

/// Longest slot name a plan writes. The X-T5's limit is not mapped; names past it are cut.
pub const NAME_MAX: usize = 25;

const SIMS: &[(i64, &str)] = &[
    (1, "Provia"),
    (2, "Velvia"),
    (3, "Astia"),
    (4, "Pro Neg. Hi"),
    (5, "Pro Neg. Std"),
    (6, "Monochrome"),
    (7, "Monochrome + Ye filter"),
    (8, "Monochrome + R filter"),
    (9, "Monochrome + G filter"),
    (10, "Sepia"),
    (11, "Classic Chrome"),
    (12, "Acros"),
    (13, "Acros + Ye filter"),
    (14, "Acros + R filter"),
    (15, "Acros + G filter"),
    (16, "Eterna"),
    (17, "Classic Negative"),
    (18, "Eterna Bleach Bypass"),
    (19, "Nostalgic Neg."),
    (20, "Reala Ace"),
];

/// Film simulations that take Monochromatic Color (and leave Color out).
pub fn is_mono(sim: i64) -> bool {
    matches!(sim, 6..=9 | 12..=15)
}

/// A film simulation's canonical name (the parser's) as its code.
pub fn sim_code(name: &str) -> Option<i64> {
    SIMS.iter().find(|(_, n)| n.eq_ignore_ascii_case(name.trim())).map(|(c, _)| *c)
}

pub fn sim_name(code: i64) -> Option<&'static str> {
    SIMS.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

const WBS: &[(i64, &str)] = &[
    (2, "Auto"),
    (0x8020, "Auto White Priority"),
    (0x8021, "Auto Ambience Priority"),
    (4, "Daylight"),
    (0x8006, "Shade"),
    (0x8001, "Fluorescent 1"),
    (0x8002, "Fluorescent 2"),
    (0x8003, "Fluorescent 3"),
    (6, "Incandescent"),
    (8, "Underwater"),
    (0x8007, "Kelvin"),
    (0x8008, "Custom"),
    (0x8009, "Custom 2"),
    (0x800A, "Custom 3"),
];

/// High ISO NR: wire value to menu step (not linear).
const NR: &[(i64, i64)] = &[(32768, -4), (28672, -3), (16384, -2), (12288, -1), (8192, 0), (4096, 1), (0, 2), (24576, 3), (20480, 4)];

fn strength(v: i64) -> Option<&'static str> {
    match v {
        1 => Some("Off"),
        2 => Some("Weak"),
        3 => Some("Strong"),
        _ => None,
    }
}

fn strength_code(s: &str) -> Option<i64> {
    match s {
        "Off" => Some(1),
        "Weak" => Some(2),
        "Strong" => Some(3),
        _ => None,
    }
}

/// A slot as the camera holds it, decoded.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct SlotView {
    /// `slot:C3`.
    pub id: String,
    pub slot: String,
    pub name: String,
    pub settings: Settings,
    /// Field values this build cannot read as a setting.
    pub warnings: Vec<String>,
    /// Every field as read: code, catalog name, raw value.
    pub fields: Vec<SlotField>,
    /// Library recipes (`recipe:<slug>`) whose settings equal the slot's.
    #[serde(default)]
    pub matches: Vec<String>,
    /// Why this slot could not be read, when it could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SlotField {
    pub code: String,
    pub name: Option<String>,
    pub value: Option<i64>,
}

fn int(values: &[(u16, Option<Value>)], code: u16) -> Option<i64> {
    values.iter().find(|(c, _)| *c == code).and_then(|(_, v)| match v {
        Some(Value::Int(n)) => Some(*n),
        _ => None,
    })
}

fn tenth(n: i64) -> f64 {
    n as f64 / 10.0
}

/// Settings from a slot's field values. A value no setting means is a warning, and its
/// setting stays unset.
pub fn decode(values: &[(u16, Option<Value>)]) -> (Settings, Vec<String>) {
    let mut s = Settings::default();
    let mut w = vec![];
    let get = |c| int(values, c);
    let mut warn = |what: &str, v: i64| w.push(format!("{what}: no setting has the value {v}"));
    if let Some(v) = get(FILM_SIM) {
        match sim_name(v) {
            Some(n) => s.film_simulation = Some(n.into()),
            None => warn("film simulation", v),
        }
    }
    let drp = get(DRP);
    if let Some(v) = drp {
        match v {
            0 => s.d_range_priority = Some("Off".into()),
            1 => s.d_range_priority = Some("Weak".into()),
            2 => s.d_range_priority = Some("Strong".into()),
            32768 => s.d_range_priority = Some("Auto".into()),
            v => warn("D-range priority", v),
        }
    }
    match get(DRANGE) {
        // 65535 while D-range priority is on: the camera owns the dynamic range then.
        Some(65535) if drp.is_some_and(|d| d != 0) => {}
        Some(0) => s.dynamic_range = Some("Auto".into()),
        Some(n @ (100 | 200 | 400)) => s.dynamic_range = Some(format!("DR{n}")),
        Some(v) => warn("dynamic range", v),
        None => {}
    }
    match get(GRAIN) {
        Some(1 | 6) => s.grain_effect = Some("Off".into()),
        Some(v @ 2..=5) => {
            s.grain_effect = Some(if v % 2 == 0 { "Weak" } else { "Strong" }.into());
            s.grain_size = Some(if v <= 3 { "Small" } else { "Large" }.into());
        }
        Some(v) => warn("grain", v),
        None => {}
    }
    for (code, what, field) in [(COLOR_CHROME, "color chrome", &mut s.color_chrome), (FX_BLUE, "color chrome FX blue", &mut s.color_chrome_fx_blue), (SMOOTH_SKIN, "smooth skin", &mut s.smooth_skin)] {
        if let Some(v) = get(code) {
            match strength(v) {
                Some(x) => *field = Some(x.into()),
                None => w.push(format!("{what}: no setting has the value {v}")),
            }
        }
    }
    if let Some(v) = get(WB) {
        match WBS.iter().find(|(c, _)| *c == v) {
            Some((_, n)) => {
                s.white_balance = Some((*n).into());
                if *n == "Kelvin" {
                    s.color_temp = get(KELVIN).map(|k| k as u32);
                }
            }
            None => w.push(format!("white balance: no setting has the value {v}")),
        }
    }
    s.wb_shift_r = get(WB_SHIFT_R).map(|v| v as i32);
    s.wb_shift_b = get(WB_SHIFT_B).map(|v| v as i32);
    // -32768: the default or unset sentinel (ptp-recipes).
    let scaled = |c| get(c).filter(|v| *v != -32768).map(tenth);
    if drp.is_none_or(|d| d == 0) {
        s.highlight_tone = scaled(HIGHLIGHT);
        s.shadow_tone = scaled(SHADOW);
    }
    let mono = s.film_simulation.as_deref().and_then(sim_code).is_some_and(is_mono);
    if !mono {
        s.color = scaled(COLOR);
    }
    s.sharpness = scaled(SHARPNESS);
    s.clarity = scaled(CLARITY);
    if let Some(v) = get(HIGH_ISO_NR) {
        match NR.iter().find(|(c, _)| *c == v) {
            Some((_, n)) => s.high_iso_nr = Some(*n as f64),
            None => w.push(format!("high ISO NR: no setting has the value {v}")),
        }
    }
    if mono {
        s.monochromatic_color_wc = scaled(MONO_WC);
        s.monochromatic_color_mg = scaled(MONO_MG);
    }
    (s, w)
}

/// Why a setting cannot go into a slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Problem {
    pub field: String,
    pub message: String,
}

fn steps(v: f64, lo: f64, hi: f64, step: f64, what: &str, p: &mut Vec<Problem>) -> Option<i64> {
    let ok = v >= lo && v <= hi && ((v - lo) / step).fract().abs() < 1e-6;
    if !ok {
        p.push(Problem { field: what.into(), message: format!("{what} {v:+} is not a slot value: {lo:+} to {hi:+} in steps of {step}") });
        return None;
    }
    Some((v * 10.0).round() as i64)
}

/// The field values a slot write sets for these settings, in write order, or every problem.
/// An absent setting takes the camera default, as the catalogue's de-duplication reads it
/// (`catalogue::normalised`), so a slot never keeps half of the recipe it held before. Fields
/// that do not apply are left out: dynamic range and the tones while D-range priority is on,
/// Color under a monochrome simulation, Monochromatic Color under a colour one.
pub fn encode(s: &Settings) -> Result<Vec<(u16, i64)>, Vec<Problem>> {
    let mut p = vec![];
    let mut out: Vec<(u16, i64)> = vec![];
    let sim = match s.film_simulation.as_deref() {
        Some(n) => match sim_code(n) {
            Some(c) => c,
            None => {
                p.push(Problem { field: "filmSimulation".into(), message: format!("{n} is not a film simulation this build can write") });
                0
            }
        },
        None => {
            p.push(Problem { field: "filmSimulation".into(), message: "the recipe has no film simulation".into() });
            0
        }
    };
    out.push((FILM_SIM, sim));
    let drp = match s.d_range_priority.as_deref().unwrap_or("Off") {
        "Off" => 0,
        "Weak" => 1,
        "Strong" => 2,
        "Auto" => 32768,
        other => {
            p.push(Problem { field: "dRangePriority".into(), message: format!("D-range priority {other} is not Off, Weak, Strong or Auto") });
            0
        }
    };
    out.push((DRP, drp));
    if drp == 0 {
        let dr = match s.dynamic_range.as_deref().unwrap_or("DR100") {
            "Auto" => Some(0),
            "DR100" => Some(100),
            "DR200" => Some(200),
            "DR400" => Some(400),
            other => {
                p.push(Problem { field: "dynamicRange".into(), message: format!("dynamic range {other} is not DR100, DR200, DR400 or Auto") });
                None
            }
        };
        if let Some(dr) = dr {
            out.push((DRANGE, dr));
        }
    }
    let grain = match (s.grain_effect.as_deref().unwrap_or("Off"), s.grain_size.as_deref().unwrap_or("Small")) {
        ("Off", _) => Some(1),
        ("Weak", "Small") => Some(2),
        ("Strong", "Small") => Some(3),
        ("Weak", "Large") => Some(4),
        ("Strong", "Large") => Some(5),
        (g, z) => {
            p.push(Problem { field: "grainEffect".into(), message: format!("grain {g} {z} is not a slot value") });
            None
        }
    };
    out.extend(grain.map(|g| (GRAIN, g)));
    for (code, field, v) in [(COLOR_CHROME, "colorChrome", &s.color_chrome), (FX_BLUE, "colorChromeFxBlue", &s.color_chrome_fx_blue), (SMOOTH_SKIN, "smoothSkin", &s.smooth_skin)] {
        let v = v.as_deref().unwrap_or("Off");
        match strength_code(v) {
            Some(c) => out.push((code, c)),
            None => p.push(Problem { field: field.into(), message: format!("{v} is not Off, Weak or Strong") }),
        }
    }
    let wb = s.white_balance.as_deref().unwrap_or("Auto");
    match WBS.iter().find(|(_, n)| n.eq_ignore_ascii_case(wb)) {
        Some((c, n)) => {
            out.push((WB, *c));
            if *n == "Kelvin" {
                match s.color_temp {
                    Some(k) if (2500..=10000).contains(&k) => out.push((KELVIN, i64::from(k))),
                    Some(k) => p.push(Problem { field: "colorTemp".into(), message: format!("{k}K is not a slot value: 2500K to 10000K") }),
                    None => p.push(Problem { field: "colorTemp".into(), message: "Kelvin white balance without a temperature".into() }),
                }
            }
        }
        None => p.push(Problem { field: "whiteBalance".into(), message: format!("{wb} is not a white balance this build can write") }),
    }
    for (code, field, v) in [(WB_SHIFT_R, "wbShiftR", s.wb_shift_r), (WB_SHIFT_B, "wbShiftB", s.wb_shift_b)] {
        let v = v.unwrap_or(0);
        if (-9..=9).contains(&v) {
            out.push((code, i64::from(v)));
        } else {
            p.push(Problem { field: field.into(), message: format!("a white balance shift of {v} is not -9 to 9") });
        }
    }
    if drp == 0 {
        out.extend(steps(s.highlight_tone.unwrap_or(0.0), -2.0, 4.0, 0.5, "highlight", &mut p).map(|v| (HIGHLIGHT, v)));
        out.extend(steps(s.shadow_tone.unwrap_or(0.0), -2.0, 4.0, 0.5, "shadow", &mut p).map(|v| (SHADOW, v)));
    }
    let mono = is_mono(sim);
    if !mono {
        out.extend(steps(s.color.unwrap_or(0.0), -4.0, 4.0, 1.0, "color", &mut p).map(|v| (COLOR, v)));
    }
    out.extend(steps(s.sharpness.unwrap_or(0.0), -4.0, 4.0, 1.0, "sharpness", &mut p).map(|v| (SHARPNESS, v)));
    let nr = s.high_iso_nr.unwrap_or(0.0);
    match NR.iter().find(|(_, n)| (*n as f64 - nr).abs() < 1e-6) {
        Some((c, _)) => out.push((HIGH_ISO_NR, *c)),
        None => p.push(Problem { field: "highIsoNr".into(), message: format!("high ISO NR {nr:+} is not -4 to +4 in whole steps") }),
    }
    out.extend(steps(s.clarity.unwrap_or(0.0), -5.0, 5.0, 1.0, "clarity", &mut p).map(|v| (CLARITY, v)));
    if mono {
        out.extend(steps(s.monochromatic_color_wc.unwrap_or(0.0), -18.0, 18.0, 1.0, "monochromatic color WC", &mut p).map(|v| (MONO_WC, v)));
        out.extend(steps(s.monochromatic_color_mg.unwrap_or(0.0), -18.0, 18.0, 1.0, "monochromatic color MG", &mut p).map(|v| (MONO_MG, v)));
    }
    if p.is_empty() {
        Ok(out)
    } else {
        Err(p)
    }
}

/// A slot name for a recipe name: printable, at most [`NAME_MAX`] characters.
pub fn name_for(name: &str) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control()).collect();
    clean.trim().chars().take(NAME_MAX).collect::<String>().trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_back(fields: &[(u16, i64)]) -> Vec<(u16, Option<Value>)> {
        fields.iter().map(|(c, v)| (*c, Some(Value::Int(*v)))).collect()
    }

    #[test]
    fn a_recipe_round_trips_through_the_slot_encoding() {
        let s = Settings {
            film_simulation: Some("Classic Negative".into()),
            dynamic_range: Some("DR400".into()),
            grain_effect: Some("Strong".into()),
            grain_size: Some("Large".into()),
            color_chrome: Some("Strong".into()),
            color_chrome_fx_blue: Some("Weak".into()),
            white_balance: Some("Kelvin".into()),
            color_temp: Some(5800),
            wb_shift_r: Some(1),
            wb_shift_b: Some(-3),
            highlight_tone: Some(-2.0),
            shadow_tone: Some(1.5),
            color: Some(4.0),
            sharpness: Some(-2.0),
            high_iso_nr: Some(-4.0),
            clarity: Some(-3.0),
            ..Default::default()
        };
        let enc = encode(&s).unwrap();
        let get = |c: u16| enc.iter().find(|(x, _)| *x == c).map(|(_, v)| *v);
        assert_eq!(get(FILM_SIM), Some(17));
        assert_eq!(get(GRAIN), Some(5));
        assert_eq!(get(WB), Some(0x8007));
        assert_eq!(get(KELVIN), Some(5800));
        assert_eq!(get(SHADOW), Some(15));
        assert_eq!(get(HIGH_ISO_NR), Some(32768));
        assert_eq!(get(MONO_WC), None, "no Monochromatic Color under a colour simulation");
        assert_eq!(enc[0].0, FILM_SIM, "film simulation first");
        let (back, w) = decode(&read_back(&enc));
        assert!(w.is_empty(), "{w:?}");
        let want = Settings { smooth_skin: Some("Off".into()), d_range_priority: Some("Off".into()), ..s };
        assert_eq!(back, want);
    }

    #[test]
    fn mono_recipes_leave_color_out_and_drp_leaves_the_tones() {
        let s = Settings { film_simulation: Some("Acros + R filter".into()), d_range_priority: Some("Strong".into()), monochromatic_color_wc: Some(3.0), ..Default::default() };
        let enc = encode(&s).unwrap();
        let codes: Vec<u16> = enc.iter().map(|(c, _)| *c).collect();
        assert!(codes.contains(&MONO_WC) && codes.contains(&MONO_MG));
        assert!(!codes.contains(&COLOR) && !codes.contains(&DRANGE) && !codes.contains(&HIGHLIGHT));
        assert_eq!(enc.iter().find(|(c, _)| *c == MONO_WC).unwrap().1, 30);
    }

    #[test]
    fn values_outside_the_slot_ranges_are_problems() {
        let s = Settings { film_simulation: Some("Provia".into()), highlight_tone: Some(5.0), wb_shift_r: Some(12), high_iso_nr: Some(0.5), ..Default::default() };
        let p = encode(&s).unwrap_err();
        let fields: Vec<&str> = p.iter().map(|x| x.field.as_str()).collect();
        assert_eq!(fields, ["wbShiftR", "highlight", "highIsoNr"]);
        assert!(encode(&Settings::default()).is_err(), "a recipe needs a film simulation");
    }

    #[test]
    fn default_slots_read_grain_six_as_off_and_drp_hides_the_range() {
        let raw = read_back(&[(FILM_SIM, 1), (DRP, 32768), (DRANGE, 65535), (GRAIN, 6), (WB, 2), (HIGHLIGHT, 0), (COLOR, -32768), (HIGH_ISO_NR, 8192)]);
        let (s, w) = decode(&raw);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.grain_effect.as_deref(), Some("Off"));
        assert_eq!(s.dynamic_range, None);
        assert_eq!(s.highlight_tone, None);
        assert_eq!(s.color, None, "the unset sentinel");
        assert_eq!(s.high_iso_nr, Some(0.0));
        let (_, w) = decode(&read_back(&[(FILM_SIM, 77)]));
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn slots_and_names() {
        assert_eq!(parse_slot("C3"), Some(3));
        assert_eq!(parse_slot("slot:c7"), Some(7));
        assert_eq!(parse_slot("8"), None);
        assert_eq!(name_for("  A very long recipe name that goes on and on "), "A very long recipe name t");
        assert!(writable(NAME) && writable(MONO_MG) && !writable(0xD18E) && !writable(0xD1A5));
        assert!(order(FILM_SIM) < order(MONO_WC) && order(MONO_MG) < order(NAME));
    }
}
