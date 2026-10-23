//! Value readers for each field. Each returns `Err(message)` when it cannot read the value,
//! or read only part of it; whatever it could read is still applied.

use super::{norm, Field, Settings};
use regex::Regex;
use std::sync::LazyLock;

static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([+-]?)\s*(\d+(?:\.\d+)?|\.\d+)(?:\s*(½))?").unwrap());
/// "2,5": a decimal comma, only between digits.
static DECIMAL_COMMA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d),(\d)").unwrap());
static PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\([^)]*\)").unwrap());
static KELVIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(\d{4,5})\s*k\b").unwrap());
/// "K6300", "Kelvin 5200", "K: 5000".
static KELVIN_FIRST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bk(?:elvin)?\s*:?\s*(\d{4,5})\b").unwrap());
/// "10k".
static KELVIN_SHORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(\d{1,2})\s*k$").unwrap());
/// A bare "5400" or "7700+" where a white balance should be.
static KELVIN_BARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d{4,5})\+?$").unwrap());
/// "10,000": a thousands comma.
static THOUSANDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d),(\d{3})").unwrap());
static SHIFT_R: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)([+-]?\s*\d+)\s*(?:red|r)\b").unwrap());
/// "R:4 B:-5", "R +2, B -4": the letter first.
static SHIFT_R_FIRST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:red|r)\s*[:=]?\s*([+-]?\s*\d+)").unwrap());
static SHIFT_B_FIRST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:blue|b)\s*[:=]?\s*([+-]?\s*\d+)").unwrap());
static LETTER_COLON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:red|blue|r|b)\s*[:=]").unwrap());
static SHIFT_B: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)([+-]?\s*\d+)\s*(?:blue|b)\b").unwrap());
/// "+1 Highlight & -1 Shadow" or "H+1, S-1": a sign-bearing number next to H or S.
static CURVE_AFTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(h|s)(?:ighlights?|hadows?)?\s*[:=]?\s*([+-]?\s*\d+(?:\.\d+)?)").unwrap());
static CURVE_BEFORE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)([+-]?\s*\d+(?:\.\d+)?)\s*(h|s)(?:ighlights?|hadows?)?\b").unwrap());
static MONO_AFTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(wc|mg|warm|cool|magenta|green)\s*[:=]?\s*([+-]?\s*\d+(?:\.\d+)?)").unwrap());
static MONO_BEFORE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)([+-]?\s*\d+(?:\.\d+)?)\s*(wc|mg|warm|cool|magenta|green)\b").unwrap());

/// Dashes and minus signs as ASCII hyphens, so `−1` reads as -1.
fn ascii_minus(s: &str) -> String {
    s.replace(['\u{2212}', '\u{2013}', '\u{2014}', '\u{2011}', '\u{2010}'], "-")
}

/// Removes "(Medium-Hard)", "(typically)" and the like.
pub(crate) fn without_parens(s: &str) -> String {
    PAREN.replace_all(s, "").trim().to_string()
}

/// A tone given by its menu name, as the first bodies showed it: "Medium-Hard" is +1,
/// "Low" is -2.
fn tone_name(v: &str) -> Option<f64> {
    Some(match norm(v).as_str() {
        "soft" | "low" => -2.0,
        "medium soft" | "medium low" => -1.0,
        "standard" | "std" | "medium" | "normal" => 0.0,
        "medium hard" | "medium high" => 1.0,
        "hard" | "high" => 2.0,
        _ => return None,
    })
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
    let v = DECIMAL_COMMA.replace_all(v.trim_start_matches([':', ' ']), "$1.$2").into_owned();
    let Some(m) = NUMBER.captures(&v) else {
        return match tone_name(&v) {
            Some(n) => Ok((n, None)),
            None => Err("Not a number".into()),
        };
    };
    let mut n = signed_num(&m[1], &m[2]);
    if m.get(3).is_some() {
        n += if n < 0.0 { -0.5 } else { 0.5 };
    }
    let rest = v[m.get(0).unwrap().end()..].trim();
    Ok((n, (!rest.is_empty()).then(|| format!("Read {n}; ignored \"{rest}\""))))
}

static ALT_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*(?:;|\s—\s|\s–\s|\s-\s|\sor\s)\s*").unwrap());
static SENSOR_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*(?:\([^)]*\)|\bfor\b.*)$").unwrap());

/// The first value, the sensor it is for, and (sensor, value) for each other sensor.
pub(crate) type SensorAlternatives = (String, Option<String>, Vec<(String, String)>);

/// Per-sensor alternatives in one value: "Off (X-Trans V); Weak (X-Trans IV)", "Strong —
/// Weak for X-Trans V", "Strong (X-Trans IV), Weak (X-Trans V)". Returns the first value, the
/// sensor it is for (if named) and the others by sensor. `None` when no later part names a
/// sensor: then the value is read as it is.
pub(crate) fn sensor_alternatives(value: &str) -> Option<SensorAlternatives> {
    let v = ascii_minus(value);
    let mut parts: Vec<&str> = ALT_SPLIT.split(&v).filter(|p| !p.trim().is_empty()).collect();
    let sensor = |p: &str| crate::recipes::post::sensor_in_text(p);
    if parts.len() < 2 || parts[1..].iter().all(|p| sensor(p).is_none()) {
        let commas: Vec<&str> = v.split(", ").collect();
        if commas.len() >= 2 && commas.iter().all(|p| sensor(p).is_some()) {
            parts = commas;
        } else {
            return None;
        }
    }
    let clean = |p: &str| SENSOR_TAG.replace(p.trim(), "").trim().to_string();
    let others: Vec<(String, String)> = parts[1..].iter().filter_map(|p| sensor(p).map(|s| (s, clean(p)))).collect();
    if others.is_empty() {
        return None;
    }
    Some((clean(parts[0]), sensor(parts[0]), others))
}

/// Unsets a field: a variant's "N/A".
pub(crate) fn clear(s: &mut Settings, field: Field) {
    match field {
        Field::FilmSimulation => s.film_simulation = None,
        Field::DynamicRange => s.dynamic_range = None,
        Field::DRangePriority => s.d_range_priority = None,
        Field::Grain => s.grain_effect = None,
        Field::GrainSize => s.grain_size = None,
        Field::ColorChrome => s.color_chrome = None,
        Field::ColorChromeFxBlue => s.color_chrome_fx_blue = None,
        Field::SmoothSkin => s.smooth_skin = None,
        Field::Clarity => s.clarity = None,
        _ => {}
    }
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
    // "Weak - Small", "Strong and Large": one strength and one size are not alternatives.
    let whole = norm(&without_parens(value));
    let words: Vec<&str> = whole.split_whitespace().filter(|w| *w != "and").collect();
    if words.len() == 2 && strength_word(words[0]).is_some() && matches!(words[1], "small" | "large") {
        s.grain_effect = strength_word(words[0]).map(str::to_string);
        s.grain_size = Some(capitalize(words[1]));
        return Ok(());
    }
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
    let n = norm(&without_parens(value));
    match n.rsplit(' ').next().unwrap_or(&n) {
        "small" => Ok("Small".into()),
        "large" => Ok("Large".into()),
        _ => Err("Not Small or Large".into()),
    }
}

fn dynamic_range(s: &mut Settings, value: &str) -> Result<(), String> {
    let n = norm(&without_parens(value));
    // "400% D-Range": the value, then the setting's name again.
    let n = n.strip_suffix(" d range").unwrap_or(&n).replace(' ', "");
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

/// The white balance preset a phrase names. Words that only say "white balance shift"
/// around it ("WB Auto", "Daylight. Shift:") are dropped first.
fn wb_preset(base: &str) -> Option<&'static str> {
    let n = norm(base).replace("flourescent", "fluorescent").replace("fkuorescent", "fluorescent");
    let n = n.split(' ').filter(|w| !matches!(*w, "wb" | "shift" | "with" | "a" | "color" | "colour")).collect::<Vec<_>>().join(" ");
    Some(match n.as_str() {
        "auto" | "awb" | "auto wb" => "Auto",
        "auto white priority" | "white priority" | "auto white" => "Auto White Priority",
        "auto ambience priority" | "ambience priority" | "auto ambiance priority" | "ambiance priority" | "auto ambience" | "auto ambiance" => "Auto Ambience Priority",
        "daylight" | "fine" | "daylight fine" | "fine daylight" | "sunny" | "direct sunlight" => "Daylight",
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

/// A shift read from text: its value and the span of its text.
type Shift = Option<(i32, std::ops::Range<usize>)>;

/// The red and blue shifts in `v`. Two regimes: the number before its letter ("+2 Red & -4
/// Blue", "+2R -4B") or after it ("R+2 B-4", "Red -3 Blue +3", "R:4 B:-5"). The regime that
/// reads more shifts wins; on a tie, the letter-first one when the text opens with a letter or
/// holds "R:" or "B:". A shift the other regime reads is taken for an axis the winner lacks,
/// unless its text overlaps the winner's. A number past ±9 is not a shift.
fn shifts(v: &str) -> (Shift, Shift) {
    let read = |re: &Regex| -> Shift {
        let m = re.captures(v)?;
        let n = num_of(&m[1]);
        (n.abs() <= 9.0).then(|| (n as i32, m.get(0).unwrap().range()))
    };
    let number_first = (read(&SHIFT_R), read(&SHIFT_B));
    let letter_first = (read(&SHIFT_R_FIRST), read(&SHIFT_B_FIRST));
    let count = |p: &(Shift, Shift)| u8::from(p.0.is_some()) + u8::from(p.1.is_some());
    let opens_with_letter = v.trim_start().chars().next().is_some_and(|c| c.is_ascii_alphabetic()) || LETTER_COLON.is_match(v);
    let letter_wins = match count(&letter_first).cmp(&count(&number_first)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => opens_with_letter,
    };
    let (won, other) = if letter_wins { (letter_first, number_first) } else { (number_first, letter_first) };
    let apart = |a: &Shift, b: &Shift| !matches!((a, b), (Some((_, x)), Some((_, y))) if x.start < y.end && y.start < x.end);
    let r = won.0.clone().or_else(|| other.0.clone().filter(|_| apart(&other.0, &won.1)));
    let b = won.1.clone().or_else(|| other.1.clone().filter(|_| apart(&other.1, &won.0)));
    (r, b)
}

/// "Daylight, +2 Red & -4 Blue", "5200K, -1 Red & +3 Blue", "Color Temperature 6500K", "Auto,
/// R+2 B-4", "K6300 R-2 B5", "10,000K".
fn white_balance(s: &mut Settings, value: &str) -> Result<(), String> {
    let v = ascii_minus(value);
    let v = THOUSANDS.replace_all(&v, "$1$2").into_owned();
    let mut problems = Vec::new();
    let (r, b) = shifts(&v);
    if let Some((n, _)) = &r {
        s.wb_shift_r = Some(*n);
    }
    if let Some((n, _)) = &b {
        s.wb_shift_b = Some(*n);
    }
    if r.is_some() != b.is_some() {
        problems.push("only one of the red and blue shifts given".to_string());
    }
    // The base is what comes before the shift: up to the first comma, or the first shift.
    let cut = [v.find(','), r.map(|x| x.1.start), b.map(|x| x.1.start)].into_iter().flatten().min().unwrap_or(v.len());
    let base = without_parens(&v[..cut]);
    let kelvin = KELVIN.captures(&base).or_else(|| KELVIN_FIRST.captures(&base)).or_else(|| KELVIN_BARE.captures(&base)).and_then(|k| k[1].parse::<u32>().ok()).or_else(|| KELVIN_SHORT.captures(&base).and_then(|k| k[1].parse::<u32>().ok()).map(|n| n * 1000));
    if let Some(k) = kelvin {
        s.white_balance = Some("Kelvin".into());
        s.color_temp = Some(k);
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

/// A shift line on its own: "1 Red, -9 Blue", "R:4 B:-5", "+2R & -4B".
fn wb_shift(s: &mut Settings, value: &str) -> Result<(), String> {
    let v = ascii_minus(value);
    let (r, b) = shifts(&v);
    let (r, b) = (r.map(|x| x.0), b.map(|x| x.0));
    // "WB Shift R: 2 B:-3": the label holds the R, the value starts with its number.
    let r = r.or_else(|| NUMBER.captures(v.trim()).filter(|_| b.is_some()).map(|m| signed_num(&m[1], &m[2]) as i32).filter(|n| n.abs() <= 9));
    match (r, b) {
        (None, None) => Err("No red or blue shift".into()),
        (r, b) => {
            s.wb_shift_r = r.or(s.wb_shift_r);
            s.wb_shift_b = b.or(s.wb_shift_b);
            if r.is_some() && b.is_some() {
                Ok(())
            } else {
                Err("Only one of the red and blue shifts given".into())
            }
        }
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
    if let Some(rest) = n.strip_prefix("mono ").or_else(|| (n == "mono").then_some("")) {
        return mono("Monochrome", rest);
    }
    Some(
        match n.as_str() {
            "provia" | "provia std" | "std" => "Provia",
            "velvia" | "velvia vivid" | "vivid" => "Velvia",
            "astia" | "astia soft" | "soft" => "Astia",
            "classic chrome" => "Classic Chrome",
            "pro neg hi" | "proneg hi" | "pro neg high" | "proneg high" => "Pro Neg. Hi",
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

/// "N/A", "-", "/": the setting does not apply to the camera the recipe was written for.
pub(crate) fn not_applicable(value: &str) -> bool {
    let n = norm(value);
    !value.trim().is_empty() && (n.is_empty() || matches!(n.as_str(), "n a" | "na" | "not available" | "none given"))
}

/// Reads `value` into `field`. The caller has already handled notes. A not-applicable value
/// leaves the field unset, with no warning.
pub(crate) fn apply(s: &mut Settings, field: Field, value: &str) -> Result<(), String> {
    if not_applicable(value) {
        return Ok(());
    }
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
        Field::WbShift => wb_shift(s, value),
        Field::Highlight => set_tone(&mut s.highlight_tone, value),
        Field::Shadow => set_tone(&mut s.shadow_tone, value),
        Field::ToneCurve => tone_curve(s, value),
        Field::Color => set_tone(&mut s.color, value),
        Field::Sharpness => set_tone(&mut s.sharpness, value),
        Field::HighIsoNr => set_tone(&mut s.high_iso_nr, value),
        Field::Clarity => set_tone(&mut s.clarity, value),
        Field::MonochromaticColor => mono_color(s, value),
        Field::MonoWc => set_tone(&mut s.monochromatic_color_wc, value),
        Field::MonoMg => set_tone(&mut s.monochromatic_color_mg, value),
        Field::SmoothSkin => {
            let (v, extra) = strength(value)?;
            s.smooth_skin = v;
            extra.map_or(Ok(()), Err)
        }
        Field::Note => Ok(()),
    }
}
