//! Value readers for each field. Each returns `Err(message)` when it cannot read the value,
//! or read only part of it; whatever it could read is still applied.

use super::{norm, Field, Settings};
use regex::Regex;
use std::sync::LazyLock;

static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([+-]?)\s*(\d+(?:\.\d+)?|\.\d+)(?:\s*(½))?").unwrap());
static PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\([^)]*\)").unwrap());
static KELVIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(\d{4,5})\s*k\b").unwrap());
static SHIFT_R: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)([+-]?\s*\d+)\s*(?:red|r)\b").unwrap());
static SHIFT_B: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)([+-]?\s*\d+)\s*(?:blue|b)\b").unwrap());
/// "+1 Highlight & -1 Shadow" or "H+1, S-1": a sign-bearing number next to H or S.
static CURVE_AFTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(h|s)(?:ighlights?|hadows?)?\s*[:=]?\s*([+-]?\s*\d+(?:\.\d+)?)").unwrap());
static CURVE_BEFORE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)([+-]?\s*\d+(?:\.\d+)?)\s*(h|s)(?:ighlights?|hadows?)?\b").unwrap());
static MONO_AFTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(wc|mg|warm|cool|magenta|green)\s*[:=]?\s*([+-]?\s*\d+(?:\.\d+)?)").unwrap());
static MONO_BEFORE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)([+-]?\s*\d+(?:\.\d+)?)\s*(wc|mg|warm|cool|magenta|green)\b").unwrap());

/// Dashes and minus signs as ASCII hyphens, so `−1` reads as -1.
fn ascii_minus(s: &str) -> String {
    s.replace(['\u{2212}', '\u{2013}', '\u{2014}'], "-")
}

/// Removes "(Medium-Hard)", "(typically)" and the like.
fn without_parens(s: &str) -> String {
    PAREN.replace_all(s, "").trim().to_string()
}

fn signed_num(sign: &str, digits: &str) -> f64 {
    let v: f64 = digits.trim().parse().unwrap_or(0.0);
    if sign.trim() == "-" {
        -v
    } else {
        v
    }
}

fn num_of(captured: &str) -> f64 {
    let c: String = captured.chars().filter(|c| !c.is_whitespace()).collect();
    match c.strip_prefix('-') {
        Some(rest) => -rest.trim_start_matches('+').parse::<f64>().unwrap_or(0.0),
        None => c.trim_start_matches('+').parse().unwrap_or(0.0),
    }
}

/// A tone value: "+1", "-1.5", "+2 (Hard)", "0", "+1½". Text after the number is reported
/// alongside the number it follows.
fn tone(value: &str) -> Result<(f64, Option<String>), String> {
    let v = ascii_minus(&without_parens(value));
    let Some(m) = NUMBER.captures(&v) else {
        return Err("Not a number".into());
    };
    let mut n = signed_num(&m[1], &m[2]);
    if m.get(3).is_some() {
        n += if n < 0.0 { -0.5 } else { 0.5 };
    }
    let rest = v[m.get(0).unwrap().end()..].trim();
    Ok((n, (!rest.is_empty()).then(|| format!("Read {n}; ignored \"{rest}\""))))
}

/// Text after the first of several alternatives: "Off (X-Trans V); Weak (X-Trans IV)",
/// "Strong — Weak for X-Trans V", "Off or N/A". Returns the first alternative and the rest.
fn first_alternative(value: &str) -> (String, Option<String>) {
    let v = ascii_minus(&without_parens(value));
    for sep in [" - ", ";", " or ", " — "] {
        if let Some((a, b)) = v.split_once(sep) {
            return (a.trim().to_string(), Some(b.trim().to_string()));
        }
    }
    (v, None)
}

fn strength_word(w: &str) -> Option<&'static str> {
    Some(match w {
        "off" | "none" | "0" => "Off",
        "weak" | "low" => "Weak",
        "strong" | "high" => "Strong",
        _ => return None,
    })
}

/// Off, Weak or Strong, and a warning when text was ignored. "N/A" means the camera lacks the
/// setting: unset, no warning. "Off or N/A" is Off. Of alternatives for other cameras
/// ("Off (X-Trans V); Weak (X-Trans IV)") the first is read and the rest reported.
fn strength(value: &str) -> Result<(Option<String>, Option<String>), String> {
    let (first, rest) = first_alternative(value);
    // "Off or N/A": the alternative only says some cameras lack it.
    let rest = rest.filter(|r| !matches!(norm(r).as_str(), "n a" | "na" | "not available"));
    let n = norm(&first).replace("n a", "na");
    let words: Vec<&str> = n.split_whitespace().filter(|w| !matches!(*w, "on" | "or")).collect();
    let na = |w: &&str| matches!(*w, "na" | "not" | "available");
    let found = words.iter().find_map(|w| strength_word(w));
    let others = words.iter().filter(|w| strength_word(w).is_none() && !na(w)).count();
    match found {
        Some(w) if others == 0 => Ok((Some(w.into()), rest.map(|r| format!("Read {w}; ignored \"{r}\"")))),
        None if others == 0 && !words.is_empty() => Ok((None, None)),
        _ => Err("Not Off, Weak or Strong".into()),
    }
}

/// "Weak, Small", "Strong Large", "Off", "Weak". Grain size, when given, sets the size too.
fn grain(s: &mut Settings, value: &str) -> Result<(), String> {
    let (first, rest) = first_alternative(value);
    if let Some(rest) = rest {
        grain(s, &first)?;
        return Err(format!("Read {first}; ignored \"{rest}\""));
    }
    let n = norm(&first);
    let mut words: Vec<&str> = n.split_whitespace().collect();
    let size = words.iter().position(|w| *w == "small" || *w == "large");
    if let Some(i) = size {
        let w = words.remove(i);
        s.grain_size = Some(if w == "small" { "Small" } else { "Large" }.into());
    }
    let (g, extra) = strength(&words.join(" "))?;
    if g.is_some() {
        s.grain_effect = g;
    }
    extra.map_or(Ok(()), Err)
}

fn grain_size(value: &str) -> Result<String, String> {
    match norm(&without_parens(value)).as_str() {
        "small" => Ok("Small".into()),
        "large" => Ok("Large".into()),
        _ => Err("Not Small or Large".into()),
    }
}

fn dynamic_range(s: &mut Settings, value: &str) -> Result<(), String> {
    let n = norm(&without_parens(value)).replace(' ', "");
    s.dynamic_range = Some(
        match n.as_str() {
            "dr100" | "100" | "100%" => "DR100",
            "dr200" | "200" | "200%" => "DR200",
            "dr400" | "400" | "400%" => "DR400",
            "auto" | "drauto" => "Auto",
            // "DR-P Strong", "D-Range Priority Auto": D-Range Priority set from the DR line.
            other => {
                let rest = ["drangepriority", "drp"].iter().find_map(|p| other.strip_prefix(p)).unwrap_or("");
                let rest = rest.strip_prefix("drp").unwrap_or(rest);
                if matches!(rest, "strong" | "weak" | "auto") {
                    s.d_range_priority = Some(capitalize(rest));
                    return Ok(());
                }
                return Err("Not DR100, DR200, DR400 or Auto".into());
            }
        }
        .to_string(),
    );
    Ok(())
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
        None => String::new(),
    }
}

fn d_range_priority(value: &str) -> Result<String, String> {
    match norm(&without_parens(value)).as_str() {
        "off" => Ok("Off".into()),
        "weak" | "drp weak" | "dr p weak" => Ok("Weak".into()),
        "strong" | "drp strong" | "dr p strong" => Ok("Strong".into()),
        "auto" | "drp auto" | "dr p auto" => Ok("Auto".into()),
        _ => Err("Not Off, Weak, Strong or Auto".into()),
    }
}

/// The white balance preset a phrase names.
fn wb_preset(base: &str) -> Option<&'static str> {
    let n = norm(base);
    Some(match n.as_str() {
        "auto" | "awb" => "Auto",
        "auto white priority" | "white priority" | "auto white" => "Auto White Priority",
        "auto ambience priority" | "ambience priority" | "auto ambiance priority" | "ambiance priority" => "Auto Ambience Priority",
        "daylight" | "fine" | "daylight fine" | "fine daylight" | "sunny" => "Daylight",
        "shade" | "shade cloudy" | "cloudy" | "cloudy shade" => "Shade",
        "fluorescent 1" | "fluorescent light 1" | "daylight fluorescent" | "fluorescent 1 daylight" => "Fluorescent 1",
        "fluorescent 2" | "fluorescent light 2" | "warm white fluorescent" | "fluorescent 2 warm white" => "Fluorescent 2",
        "fluorescent 3" | "fluorescent light 3" | "cool white fluorescent" | "fluorescent 3 cool white" => "Fluorescent 3",
        "incandescent" | "tungsten" | "incandescent light" => "Incandescent",
        "underwater" => "Underwater",
        "custom" | "custom 1" | "custom 2" | "custom 3" | "measured" => "Custom",
        _ => return None,
    })
}

/// "Daylight, +2 Red & -4 Blue", "5200K, -1 Red & +3 Blue", "Color Temperature 6500K".
fn white_balance(s: &mut Settings, value: &str) -> Result<(), String> {
    let v = ascii_minus(value);
    let mut problems = Vec::new();
    if let Some(m) = SHIFT_R.captures(&v) {
        s.wb_shift_r = Some(num_of(&m[1]) as i32);
    }
    if let Some(m) = SHIFT_B.captures(&v) {
        s.wb_shift_b = Some(num_of(&m[1]) as i32);
    }
    if s.wb_shift_r.is_some() != s.wb_shift_b.is_some() {
        problems.push("only one of the red and blue shifts given".to_string());
    }
    // The base is what comes before the shift: up to the first comma, or the first shift number.
    let cut = [v.find(','), SHIFT_R.find(&v).map(|m| m.start()), SHIFT_B.find(&v).map(|m| m.start())].into_iter().flatten().min().unwrap_or(v.len());
    let base = without_parens(&v[..cut]);
    if let Some(k) = KELVIN.captures(&base) {
        s.white_balance = Some("Kelvin".into());
        s.color_temp = k[1].parse().ok();
    } else if let Some(p) = wb_preset(&base) {
        s.white_balance = Some(p.into());
    } else if let Some(p) = wb_preset(&base.replace('/', " ")) {
        s.white_balance = Some(p.into());
    } else {
        problems.push(format!("\"{}\" is not a white balance", base.trim()));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(capitalize(&problems.join("; ")))
    }
}

/// "+1 Highlight & -1 Shadow", "H+1 S-1", "Highlight +1, Shadow -0.5". A Highlight or Shadow
/// line given earlier wins, and a different curve value is reported.
fn tone_curve(s: &mut Settings, value: &str) -> Result<(), String> {
    let v = ascii_minus(value);
    let mut pairs: Vec<(String, f64)> = CURVE_AFTER.captures_iter(&v).map(|m| (m[1].to_lowercase(), num_of(&m[2]))).collect();
    if pairs.is_empty() {
        pairs = CURVE_BEFORE.captures_iter(&v).map(|m| (m[2].to_lowercase(), num_of(&m[1]))).collect();
    }
    if pairs.is_empty() {
        return Err("No highlight or shadow value".into());
    }
    let mut conflicts = Vec::new();
    for (which, n) in pairs {
        let (slot, name) = if which == "h" { (&mut s.highlight_tone, "Highlight") } else { (&mut s.shadow_tone, "Shadow") };
        match *slot {
            Some(old) if old != n => conflicts.push(format!("{name} already given as {old}")),
            Some(_) => {}
            None => *slot = Some(n),
        }
    }
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(conflicts.join("; "))
    }
}

/// "WC +2 & MG -1", "+3 Warm", "0". A bare number is WC.
fn mono_color(s: &mut Settings, value: &str) -> Result<(), String> {
    let v = ascii_minus(value);
    let mut pairs: Vec<(String, f64)> = MONO_AFTER.captures_iter(&v).map(|m| (m[1].to_lowercase(), num_of(&m[2]))).collect();
    if pairs.is_empty() {
        pairs = MONO_BEFORE.captures_iter(&v).map(|m| (m[2].to_lowercase(), num_of(&m[1]))).collect();
    }
    if pairs.is_empty() && norm(&without_parens(value)) == "off" {
        s.monochromatic_color_wc = Some(0.0);
        s.monochromatic_color_mg = Some(0.0);
        return Ok(());
    }
    if pairs.is_empty() {
        let (n, extra) = tone(value)?;
        s.monochromatic_color_wc = Some(n);
        return extra.map_or(Ok(()), Err);
    }
    for (axis, n) in pairs {
        match axis.as_str() {
            "wc" | "warm" => s.monochromatic_color_wc = Some(n),
            "cool" => s.monochromatic_color_wc = Some(-n.abs()),
            "mg" | "magenta" => s.monochromatic_color_mg = Some(n),
            _ => s.monochromatic_color_mg = Some(-n.abs()),
        }
    }
    Ok(())
}

/// The canonical film simulation a phrase names: "PRO Neg. Std", "Acros+R", "Classic Neg.".
pub(crate) fn film_sim(phrase: &str) -> Option<String> {
    let mut n = norm(&without_parens(phrase));
    for drop in [" filter", " film simulation", " simulation"] {
        if let Some(stripped) = n.strip_suffix(drop) {
            n = stripped.to_string();
        }
    }
    let n = n.replace("standard", "std").replace("negative", "neg");
    let mono = |base: &str, rest: &str| -> Option<String> {
        let f = match rest.trim() {
            "" => return Some(base.to_string()),
            "r" | "red" => "R",
            "ye" | "y" | "yellow" => "Ye",
            "g" | "green" => "G",
            _ => return None,
        };
        Some(format!("{base} + {f} filter"))
    };
    if let Some(rest) = n.strip_prefix("acros") {
        return mono("Acros", rest);
    }
    if let Some(rest) = n.strip_prefix("monochrome") {
        return mono("Monochrome", rest);
    }
    Some(
        match n.as_str() {
            "provia" | "provia std" | "std" => "Provia",
            "velvia" | "velvia vivid" | "vivid" => "Velvia",
            "astia" | "astia soft" | "soft" => "Astia",
            "classic chrome" => "Classic Chrome",
            "pro neg hi" | "proneg hi" => "Pro Neg. Hi",
            "pro neg std" | "proneg std" => "Pro Neg. Std",
            "classic neg" => "Classic Negative",
            "nostalgic neg" => "Nostalgic Neg.",
            "eterna" | "eterna cinema" => "Eterna",
            "eterna bleach bypass" | "bleach bypass" => "Eterna Bleach Bypass",
            "reala ace" => "Reala Ace",
            "sepia" => "Sepia",
            _ => return None,
        }
        .to_string(),
    )
}

/// Reads `value` into `field`. The caller has already handled notes.
pub(crate) fn apply(s: &mut Settings, field: Field, value: &str) -> Result<(), String> {
    let set_tone = |slot: &mut Option<f64>, value: &str| -> Result<(), String> {
        let (n, extra) = tone(value)?;
        *slot = Some(n);
        extra.map_or(Ok(()), Err)
    };
    match field {
        Field::FilmSimulation => match film_sim(value) {
            Some(sim) => {
                s.film_simulation = Some(sim);
                Ok(())
            }
            None => Err("Not a film simulation".into()),
        },
        Field::DynamicRange => dynamic_range(s, value),
        Field::DRangePriority => d_range_priority(value).map(|v| s.d_range_priority = Some(v)),
        Field::Grain => grain(s, value),
        Field::GrainSize => grain_size(value).map(|v| s.grain_size = Some(v)),
        Field::ColorChrome => {
            let (v, extra) = strength(value)?;
            s.color_chrome = v;
            extra.map_or(Ok(()), Err)
        }
        Field::ColorChromeFxBlue => {
            let (v, extra) = strength(value)?;
            s.color_chrome_fx_blue = v;
            extra.map_or(Ok(()), Err)
        }
        Field::WhiteBalance => white_balance(s, value),
        Field::Highlight => set_tone(&mut s.highlight_tone, value),
        Field::Shadow => set_tone(&mut s.shadow_tone, value),
        Field::ToneCurve => tone_curve(s, value),
        Field::Color => set_tone(&mut s.color, value),
        Field::Sharpness => set_tone(&mut s.sharpness, value),
        Field::HighIsoNr => set_tone(&mut s.high_iso_nr, value),
        Field::Clarity => set_tone(&mut s.clarity, value),
        Field::MonochromaticColor => mono_color(s, value),
        Field::Note => Ok(()),
    }
}
