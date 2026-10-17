//! Finds the recipe blocks in a post's HTML. Only the settings lines leave this module, and
//! the nearest heading as a name for posts with more than one block; never the post's prose.

use super::{field_for, split_line, values::film_sim, Field};
use regex::Regex;
use std::sync::LazyLock;

static ELEMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<(p|li|h[1-6])\b[^>]*>(.*?)</(?:p|li|h[1-6])>").unwrap());
static BR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").unwrap());
static ENTITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&(#[0-9]+|#[xX][0-9a-fA-F]+|[a-zA-Z]+);").unwrap());

/// Decodes the entities WordPress emits: numeric ones and the common named ones.
pub fn decode_entities(s: &str) -> String {
    ENTITY
        .replace_all(s, |c: &regex::Captures| {
            let e = &c[1];
            let ch = if let Some(hex) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(dec) = e.strip_prefix('#') {
                dec.parse().ok().and_then(char::from_u32)
            } else {
                match e {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    "ndash" => Some('–'),
                    "mdash" => Some('—'),
                    "rsquo" | "lsquo" => Some('\''),
                    "rdquo" | "ldquo" => Some('"'),
                    "hellip" => Some('…'),
                    "frac12" => Some('½'),
                    "minus" => Some('−'),
                    _ => None,
                }
            };
            ch.map_or_else(|| c[0].to_string(), |c| c.to_string())
        })
        .into_owned()
}

/// Inline HTML to text, one line per `<br>`.
pub fn to_text(fragment: &str) -> String {
    let with_breaks = BR.replace_all(fragment, "\n");
    let plain = TAG.replace_all(&with_breaks, "");
    decode_entities(&plain).replace('\u{a0}', " ")
}

/// A recipe block from a post.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// The settings lines, one per line.
    pub text: String,
    /// The heading or bold-only line just before the block, if any.
    pub heading: Option<String>,
}

#[derive(Debug, Default)]
struct LineStats {
    lines: usize,
    /// Lines that name a slot setting, or are a film simulation by themselves.
    slot: usize,
    notes: usize,
    film_sim_label: bool,
    has_dr_grain_wb: bool,
}

fn stats(text: &str) -> LineStats {
    let mut s = LineStats::default();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        s.lines += 1;
        match split_line(line).and_then(|(l, _)| field_for(l)) {
            Some(Field::Note) => s.notes += 1,
            Some(f) => {
                s.slot += 1;
                if f == Field::FilmSimulation {
                    s.film_sim_label = true;
                }
                if matches!(f, Field::DynamicRange | Field::Grain | Field::WhiteBalance) {
                    s.has_dr_grain_wb = true;
                }
            }
            None if split_line(line).is_none() && film_sim(line).is_some() => s.slot += 1,
            None => {}
        }
    }
    s
}

/// Mostly settings lines: a paragraph that can join a block split across several `<p>`s.
fn settings_like(s: &LineStats) -> bool {
    s.lines > 0 && (s.slot + s.notes) * 2 >= s.lines
}

/// A recipe: "Film Simulation:" plus Dynamic Range, Grain or White Balance, or three slot
/// settings (older posts open with the bare simulation name instead of the label).
fn is_recipe(s: &LineStats) -> bool {
    (s.film_sim_label && s.has_dr_grain_wb) || s.slot >= 3
}

/// The recipe blocks in a post, in order. Consecutive settings-like paragraphs with nothing
/// between them form one block.
pub fn blocks(html: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut heading: Option<String> = None;
    let mut group: Vec<String> = Vec::new();
    let mut group_heading: Option<String> = None;
    let mut last_end = 0usize;

    let flush = |group: &mut Vec<String>, gh: &mut Option<String>, out: &mut Vec<Block>| {
        if group.is_empty() {
            return;
        }
        let text = group.join("\n");
        if is_recipe(&stats(&text)) {
            out.push(Block { text, heading: gh.take() });
        }
        group.clear();
        *gh = None;
    };

    for m in ELEMENT.captures_iter(html) {
        let whole = m.get(0).unwrap();
        let tag = m[1].to_ascii_lowercase();
        let inner = &m[2];
        let text = to_text(inner).trim().to_string();
        // Only whitespace, or an empty paragraph, may sit between two parts of one block.
        let between = &html[last_end..whole.start()];
        let adjacent = TAG.replace_all(between, "").trim().is_empty();
        last_end = whole.end();
        if !adjacent {
            flush(&mut group, &mut group_heading, &mut out);
        }
        if text.is_empty() {
            continue;
        }
        let st = stats(&text);
        if tag.starts_with('h') || (!settings_like(&st) && is_bold_only(inner) && text.len() <= 80) {
            flush(&mut group, &mut group_heading, &mut out);
            heading = Some(text);
            continue;
        }
        if settings_like(&st) {
            if group.is_empty() {
                group_heading = heading.clone();
            }
            group.push(text);
        } else {
            flush(&mut group, &mut group_heading, &mut out);
        }
    }
    flush(&mut group, &mut group_heading, &mut out);
    out
}

/// A paragraph that is one bold run: a sub-heading in the post.
fn is_bold_only(inner: &str) -> bool {
    let t = inner.trim();
    let lower = t.to_ascii_lowercase();
    (lower.starts_with("<strong>") && lower.ends_with("</strong>") || lower.starts_with("<b>") && lower.ends_with("</b>")) && !BR.is_match(t)
}
