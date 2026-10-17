//! Facts about a recipe post taken from its title, URL and tags: the recipe name, the sensor
//! generation, the id.

use super::html::decode_entities;
use regex::Regex;
use std::sync::LazyLock;

static XTRANS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bx[- ]?trans(?:\s+sensor)?\s*(iv|v|iii|ii|i|5|4|3|2|1)\b").unwrap());
/// "Fifth-Generation Cameras", "5th-Gen", "4th Gen".
static GENERATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(fifth|fourth|third|second|first|5th|4th|3rd|2nd|1st)[- ]gen(?:eration)?\b").unwrap());
static RECIPE_COLON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\brecipes?\s*[:–—-]\s*").unwrap());
static DASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+[–—-]\s+").unwrap());
static CAMERA_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:my\s+)?(?:fujifilm\s+|fuji\s+)?(?:(?:x-?trans\s+\w+|bayer|gfx\s*\w*|x-?[a-z]{1,3}\d+\w*|x100\w*|x70|xf10)(?:\s*(?:\+|&|and|/|,)\s*)?)*\s*").unwrap()
});
static SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*(?:film\s+simulation\s+)?(?:recipes?|settings)\s*$").unwrap());

/// Camera models to their sensor, longest names first so X100VI wins over X100V.
const MODELS: &[(&str, &str)] = &[
    ("x-t30 ii", "X-Trans IV"),
    ("x100vi", "X-Trans V"),
    ("x-h2s", "X-Trans V"),
    ("x-h2", "X-Trans V"),
    ("x-t50", "X-Trans V"),
    ("x-t5", "X-Trans V"),
    ("x-e5", "X-Trans V"),
    ("x-s20", "X-Trans IV"),
    ("x-s10", "X-Trans IV"),
    ("x-t30", "X-Trans IV"),
    ("x-t4", "X-Trans IV"),
    ("x-t3", "X-Trans IV"),
    ("x-pro3", "X-Trans IV"),
    ("x-e4", "X-Trans IV"),
    ("x-m5", "X-Trans IV"),
    ("x100v", "X-Trans IV"),
    ("x-t20", "X-Trans III"),
    ("x-t2", "X-Trans III"),
    ("x-pro2", "X-Trans III"),
    ("x-e3", "X-Trans III"),
    ("x-h1", "X-Trans III"),
    ("x100f", "X-Trans III"),
    ("x-t10", "X-Trans II"),
    ("x-t1", "X-Trans II"),
    ("x-e2s", "X-Trans II"),
    ("x-e2", "X-Trans II"),
    ("x100t", "X-Trans II"),
    ("x100s", "X-Trans II"),
    ("x70", "X-Trans II"),
    ("x-pro1", "X-Trans I"),
    ("x-e1", "X-Trans I"),
    ("x-m1", "X-Trans I"),
    ("x-t100", "Bayer"),
    ("x-t200", "Bayer"),
    ("x-a", "Bayer"),
    ("xf10", "Bayer"),
    ("gfx", "GFX"),
];

fn roman(n: &str) -> &'static str {
    match n.to_ascii_lowercase().as_str() {
        "v" | "5" => "X-Trans V",
        "iv" | "4" => "X-Trans IV",
        "iii" | "3" => "X-Trans III",
        "ii" | "2" => "X-Trans II",
        _ => "X-Trans I",
    }
}

/// The sensor generation a phrase names: "X-Trans V", "Fifth-Generation", "4th-Gen".
fn generation_in(text: &str) -> Option<String> {
    if let Some(m) = XTRANS.captures(text) {
        return Some(roman(&m[1]).into());
    }
    let m = GENERATION.captures(text)?;
    let n = match m[1].to_ascii_lowercase().as_str() {
        "fifth" | "5th" => "5",
        "fourth" | "4th" => "4",
        "third" | "3rd" => "3",
        "second" | "2nd" => "2",
        _ => "1",
    };
    Some(roman(n).into())
}

/// The sensor of the first camera model a phrase names.
fn model_in(text: &str) -> Option<String> {
    let lower = text.to_lowercase().replace("x-trans", "");
    if lower.contains("bayer") {
        return Some("Bayer".into());
    }
    // The first model mentioned decides: "X-Pro1 (+ X-E1)", "X100V & X-T4".
    let mut best: Option<(usize, &str)> = None;
    for (model, sensor) in MODELS {
        let mut from = 0;
        while let Some(i) = lower[from..].find(model).map(|i| i + from) {
            let after = lower[i + model.len()..].chars().next();
            let before = lower[..i].chars().next_back();
            let bounded = !after.is_some_and(|c| c.is_ascii_alphanumeric()) || *model == "x-a" || *model == "gfx";
            if bounded && !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-') {
                if best.is_none_or(|(b, _)| i < b) {
                    best = Some((i, sensor));
                }
                break;
            }
            from = i + model.len();
        }
    }
    best.map(|(_, s)| s.to_string())
}

/// The post's sensor: a generation in the title, then in the tags, then the first camera model
/// in the title, then in the tags. "for X and GFX Cameras" with an X-E5 tag is X-Trans V.
pub fn sensor(title: &str, tags: &[String]) -> Option<String> {
    let title = decode_entities(title);
    if let Some(g) = generation_in(&title).or_else(|| tags.iter().find_map(|t| generation_in(t))) {
        return Some(g);
    }
    let tag_model = || tags.iter().find_map(|t| model_in(t).filter(|s| s != "GFX"));
    match model_in(&title) {
        Some(g) if g == "GFX" => tag_model().or(Some(g)),
        Some(g) => Some(g),
        None => tag_model().or_else(|| tags.iter().find_map(|t| model_in(t))),
    }
}

/// The first sensor generation, else camera model, a post's text mentions.
pub fn sensor_in_text(text: &str) -> Option<String> {
    generation_in(text).or_else(|| model_in(text).filter(|s| s != "GFX"))
}

/// The recipe's name in a post title. "Fujifilm X-Trans V Film Simulation Recipe: Kodak Gold"
/// -> "Kodak Gold"; "CineStill 800T v2 — A Fujifilm Recipe for X-Trans V Cameras" ->
/// "CineStill 800T v2"; "My Fujifilm X-T30 Kodachrome 64 Film Simulation Recipe" ->
/// "Kodachrome 64". Falls back to the whole title.
pub fn recipe_name(title: &str) -> String {
    let t = decode_entities(title);
    let t = t.trim();
    let tidy = |s: &str| s.trim().trim_matches(['"', '“', '”', '\'', '‘', '’']).trim().to_string();
    if let Some(m) = RECIPE_COLON.find_iter(t).last() {
        let after = tidy(&t[m.end()..]);
        if !after.is_empty() {
            return after;
        }
    }
    let parts: Vec<&str> = DASH.split(t).collect();
    if parts.len() > 1 && parts[1..].iter().any(|p| p.to_lowercase().contains("recipe")) {
        let first = tidy(parts[0]);
        if !first.is_empty() {
            return first;
        }
    }
    let stripped = SUFFIX.replace(t, "");
    let stripped = CAMERA_PREFIX.replace(&stripped, "");
    let name = tidy(&stripped);
    if name.is_empty() {
        t.to_string()
    } else {
        name
    }
}

/// The last path segment of a post URL: ".../2022/04/11/kodacolor-vr/" -> "kodacolor-vr".
pub fn slug(url: &str) -> String {
    url.trim_end_matches('/').rsplit('/').next().unwrap_or(url).to_string()
}
