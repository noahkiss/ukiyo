//! Henri-Pierre Chavaz's Fujifilm Film Simulation Recipes Database: a public Google Sheet
//! compiling recipes from many creators. One CSV download of its Recipes tab. Each recipe
//! credits its Creator column; the source is the compilation, linked to the sheet. A row's
//! own URL, when given, is the recipe's link.

use super::{Fetch, Source};
use crate::recipes::catalogue::{SourceInfo, SourceRecipe};
use crate::recipes::{parse_text, post, values::film_sim};

pub const SOURCE: Source = Source { info, fetch: Fetch::Csv { url: CSV, recipes } };

pub const SHEET: &str = "https://docs.google.com/spreadsheets/d/11PvZ7rOnwmbz5VylVIvzOuWwEa83c0DweAlBwJ2pZJc";
const CSV: &str = "https://docs.google.com/spreadsheets/d/11PvZ7rOnwmbz5VylVIvzOuWwEa83c0DweAlBwJ2pZJc/export?format=csv&gid=1806109569";

fn info() -> SourceInfo {
    SourceInfo {
        id: "hpchavaz".into(),
        name: "H-P Chavaz recipe database".into(),
        homepage: format!("{SHEET}/edit#gid=1806109569"),
        credit: "Recipes © their creators; compiled by Henri-Pierre Chavaz in the Fujifilm Film Simulation Recipes Database".into(),
        compilation: true,
    }
}

/// RFC 4180 CSV: quoted fields may hold commas, doubled quotes and newlines.
pub fn csv_rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            ('"', true) => quoted = false,
            ('"', false) if field.is_empty() => quoted = true,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (c, _) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// "2024-02-05" or "8/20/2023" (month first) to YYYY-MM-DD; anything else is unknown.
fn date(s: &str) -> String {
    let s = s.trim();
    if s.len() == 10 && s.as_bytes()[4] == b'-' {
        return s.to_string();
    }
    let parts: Vec<&str> = s.split('/').collect();
    match parts.as_slice() {
        [m, d, y] if y.len() == 4 => match (m.parse::<u32>(), d.parse::<u32>()) {
            (Ok(m), Ok(d)) if (1..=12).contains(&m) && (1..=31).contains(&d) => format!("{y}-{m:02}-{d:02}"),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

fn recipes(csv: &str) -> Vec<SourceRecipe> {
    let rows = csv_rows(csv);
    let Some(head) = rows.first() else { return Vec::new() };
    let col = |name: &str| head.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
    let (Some(creator), Some(name), Some(sensor), Some(base), Some(settings), Some(published), Some(url)) = (col("Creator"), col("Name"), col("Sensor"), col("Base"), col("Settings"), col("Published"), col("URL")) else {
        return Vec::new();
    };
    let kind = col("Type");
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in &rows[1..] {
        let get = |i: usize| row.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
        let (title, text) = (get(name), get(settings));
        if title.is_empty() || text.is_empty() {
            continue;
        }
        let mut p = parse_text(&text);
        // The Base column names the simulation when the settings do not.
        if p.settings.film_simulation.is_none() {
            p.settings.film_simulation = film_sim(&get(base));
        }
        let d = date(&get(published));
        let mut id = slugify(&format!("{} {}", get(creator), title));
        if !seen.insert(id.clone()) {
            id = format!("{id}-{}", out.len() + 1);
            seen.insert(id.clone());
        }
        let link = get(url);
        out.push(SourceRecipe {
            id,
            name: title.clone(),
            title,
            url: if link.starts_with("http") { link } else { format!("{SHEET}/edit#gid=1806109569") },
            date: d,
            author: Some(get(creator)).filter(|c| !c.is_empty()),
            sensor: post::sensor(&get(sensor), &[]),
            video: kind.is_some_and(|k| get(k).to_lowercase().contains("video")),
            settings: p.settings,
            notes: p.notes,
            warnings: p.warnings,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_with_quoted_newlines() {
        let csv = "Creator,Name,Type,Color /BW,Camera,Sensor,Base,Settings,Published,URL\nJustin Gould,Bananarama,,Color,X-S10,X-Trans IV,Classic Chrome,\"Grain Effect: Off\nColour Chrome Effect: Weak\nColour Chrome Blue: Off\nWhite Balance: 9900K\nWB Shift: 1 Red, -9 Blue\nDynamic Range: DR200\nHighlights: 0\nShadows: -2\nColor: -4\nISO Noise Reduction: -4\nEV suggestion: +1/3\",8/20/2023,https://film.recipes/x/\nosanbilgi ,\"Cuban, \"\"Negative\"\"\",,Color,X-T5,X-Trans V HR,Classic Negative,\"Dynamic Range: 400% D-Range\nPriority: Off\nWhite-balance: Auto WB\nShift: R:4 B:-5\",Unknown,\n";
        let r = recipes(csv);
        assert_eq!(r.len(), 2);
        let s = &r[0].settings;
        assert!(r[0].warnings.is_empty(), "{:?}", r[0].warnings);
        assert_eq!((s.film_simulation.as_deref(), s.white_balance.as_deref(), s.color_temp, s.wb_shift_r, s.wb_shift_b), (Some("Classic Chrome"), Some("Kelvin"), Some(9900), Some(1), Some(-9)));
        assert_eq!((s.color_chrome_fx_blue.as_deref(), s.high_iso_nr), (Some("Off"), Some(-4.0)));
        assert_eq!((r[0].date.as_str(), r[0].author.as_deref(), r[0].sensor.as_deref()), ("2023-08-20", Some("Justin Gould"), Some("X-Trans IV")));
        assert_eq!(r[1].name, "Cuban, \"Negative\"");
        assert!(r[1].warnings.is_empty(), "{:?}", r[1].warnings);
        let s = &r[1].settings;
        assert_eq!((s.film_simulation.as_deref(), s.dynamic_range.as_deref(), s.d_range_priority.as_deref()), (Some("Classic Negative"), Some("DR400"), Some("Off")));
        assert_eq!((s.white_balance.as_deref(), s.wb_shift_r, s.wb_shift_b), (Some("Auto"), Some(4), Some(-5)));
        assert_eq!((r[1].date.as_str(), r[1].author.as_deref(), r[1].sensor.as_deref()), ("", Some("osanbilgi"), Some("X-Trans V")));
        assert!(r[1].url.starts_with(SHEET));
    }
}
