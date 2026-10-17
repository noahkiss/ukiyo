//! Samples are recipe blocks as Fuji X Weekly published them (settings lines only).

use super::html::blocks;
use super::post::{recipe_name, sensor, slug};
use super::*;

/// 2026, X-Trans V: every line labelled, several lines sharing one `<strong>`.
const NEW_HTML: &str = r#"<p class="wp-block-paragraph">Intro text.</p>
<p class="wp-block-paragraph"><strong>Film Simulation: Reala Ace</strong><br><strong>Dynamic Range: DR200</strong><br><strong>Grain Effect: Strong, Small</strong><br><strong>Color Chrome Effect: Off</strong><br><strong>Color Chrome FX Blue: Off</strong><br><strong>White Balance: 4500K, -1 Red &amp; -3 Blue<br>Highlight: +2<br>Shadow: 0<br>Color: +4<br>Sharpness: -1</strong><br><strong>High ISO NR: -4<br>Clarity: -3<br>ISO: Auto, up to ISO 6400<br>Exposure Compensation: -1/3 to +1/3 (typically)</strong></p>
<p class="wp-block-paragraph">More text.</p>"#;

/// 2022, X-Trans I: the simulation as an unlabelled first line, tone names in parentheses,
/// "Noise Reduction", no Color Chrome or Clarity.
const OLD_HTML: &str = r#"<figure><img src="x.jpg"/></figure>
<p class="wp-block-paragraph"><strong>Pro Neg. Std</strong><br><strong>Dynamic Range: DR400</strong><br><strong>Highlight: +1 (Medium-Hard)</strong><br><strong>Shadow: +2 (Hard)</strong><br><strong>Color: -1 (Medium-Low)</strong><br><strong>Sharpness: +2 (Hard)</strong><br><strong>Noise Reduction: -2 (Low)</strong><br><strong>White Balance: Daylight/Fine, -1 Red &amp; +3 Blue</strong><br><strong>ISO: Auto, up to ISO 3200<br>Exposure Compensation: +1/3 to +2/3 (typically)</strong></p>"#;

/// 2019, X-T30: blank lines between settings, "Sharpening", Kelvin-free daylight.
const OLD_TEXT: &str = "Classic Chrome\n\nDynamic Range: DR400\n\nHighlight: +1\n\nShadow: +2\n\nColor: 0\n\nNoise Reduction: -4\n\nSharpening: +2\n\nGrain Effect: Weak\n\nColor Chrome Effect: Weak\n\nWhite Balance: Daylight, +2 Red & -5 Blue\n\nISO: Auto, up to ISO 6400\n\nExposure Compensation: +1/3 to +1 (typically)";

#[test]
fn new_block() {
    let b = blocks(NEW_HTML);
    assert_eq!(b.len(), 1);
    let p = parse_text(&b[0].text);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let s = &p.settings;
    assert_eq!(s.film_simulation.as_deref(), Some("Reala Ace"));
    assert_eq!(s.dynamic_range.as_deref(), Some("DR200"));
    assert_eq!((s.grain_effect.as_deref(), s.grain_size.as_deref()), (Some("Strong"), Some("Small")));
    assert_eq!((s.color_chrome.as_deref(), s.color_chrome_fx_blue.as_deref()), (Some("Off"), Some("Off")));
    assert_eq!((s.white_balance.as_deref(), s.color_temp), (Some("Kelvin"), Some(4500)));
    assert_eq!((s.wb_shift_r, s.wb_shift_b), (Some(-1), Some(-3)));
    assert_eq!((s.highlight_tone, s.shadow_tone, s.color, s.sharpness), (Some(2.0), Some(0.0), Some(4.0), Some(-1.0)));
    assert_eq!((s.high_iso_nr, s.clarity), (Some(-4.0), Some(-3.0)));
    assert_eq!(p.notes, vec![Note { label: "ISO".into(), value: "Auto, up to ISO 6400".into() }, Note { label: "Exposure Compensation".into(), value: "-1/3 to +1/3 (typically)".into() }]);
}

#[test]
fn old_block_with_bare_simulation() {
    let b = blocks(OLD_HTML);
    assert_eq!(b.len(), 1);
    let p = parse_text(&b[0].text);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let s = &p.settings;
    assert_eq!(s.film_simulation.as_deref(), Some("Pro Neg. Std"));
    assert_eq!((s.highlight_tone, s.shadow_tone, s.color, s.sharpness, s.high_iso_nr), (Some(1.0), Some(2.0), Some(-1.0), Some(2.0), Some(-2.0)));
    assert_eq!((s.white_balance.as_deref(), s.wb_shift_r, s.wb_shift_b), (Some("Daylight"), Some(-1), Some(3)));
    assert_eq!((s.color_chrome.as_ref(), s.clarity, s.grain_effect.as_ref()), (None, None, None));
}

#[test]
fn old_text_with_blank_lines() {
    let p = parse_text(OLD_TEXT);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    assert_eq!(p.settings.film_simulation.as_deref(), Some("Classic Chrome"));
    assert_eq!(p.settings.sharpness, Some(2.0));
    assert_eq!(p.settings.grain_effect.as_deref(), Some("Weak"));
    assert_eq!(p.settings.grain_size, None);
    assert_eq!(p.notes.len(), 2);
}

#[test]
fn odd_lines_warn_per_field() {
    let p = parse_text("Film Simulation: Acros+R\nGrain Effect: Medium\nWhite Balance: Kinda warm, +2 Red & -3 Blue\nHighlight: +1 to +2\nMood: Rainy\nShadow: -1\nShadow: -2\nsome stray words\nMonochromatic Color: WC +3 & MG -1\nTone Curve: H +1.5 & S −0.5");
    let s = &p.settings;
    assert_eq!(s.film_simulation.as_deref(), Some("Acros + R filter"));
    assert_eq!(s.grain_effect, None);
    assert_eq!((s.white_balance.as_ref(), s.wb_shift_r, s.wb_shift_b), (None, Some(2), Some(-3)));
    assert_eq!(s.highlight_tone, Some(1.0), "the number is kept, with a warning");
    assert_eq!(s.shadow_tone, Some(-1.0), "the first value wins");
    assert_eq!((s.monochromatic_color_wc, s.monochromatic_color_mg), (Some(3.0), Some(-1.0)));
    let kinds: Vec<(WarningKind, &str)> = p.warnings.iter().map(|w| (w.kind, w.label.as_str())).collect();
    assert_eq!(
        kinds,
        vec![
            (WarningKind::BadValue, "Grain Effect"),
            (WarningKind::BadValue, "White Balance"),
            (WarningKind::BadValue, "Highlight"),
            (WarningKind::UnknownLabel, "Mood"),
            (WarningKind::Duplicate, "Shadow"),
            (WarningKind::Unlabelled, ""),
            (WarningKind::BadValue, "Tone Curve"),
        ]
    );
}

#[test]
fn tone_curve_sets_highlight_and_shadow() {
    let p = parse_text("Film Simulation: Classic Negative\nTone Curve: +1.5 Highlight & -0.5 Shadow");
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    assert_eq!((p.settings.highlight_tone, p.settings.shadow_tone), (Some(1.5), Some(-0.5)));
}

#[test]
fn film_simulation_spellings() {
    for (text, want) in [
        ("PRO Neg. Std", "Pro Neg. Std"),
        ("Pro Neg Hi", "Pro Neg. Hi"),
        ("Pro Neg. High", "Pro Neg. Hi"),
        ("Classic Neg.", "Classic Negative"),
        ("Nostalgic Negative", "Nostalgic Neg."),
        ("Acros + G", "Acros + G filter"),
        ("Monochrome+Ye", "Monochrome + Ye filter"),
        ("Eterna Bleach Bypass", "Eterna Bleach Bypass"),
        ("Velvia (Vivid)", "Velvia"),
    ] {
        assert_eq!(values::film_sim(text).as_deref(), Some(want), "{text}");
    }
}

#[test]
fn white_balance_forms() {
    let wb = |v: &str| {
        let p = parse_text(&format!("White Balance: {v}"));
        (p.settings.white_balance, p.settings.color_temp, p.settings.wb_shift_r, p.settings.wb_shift_b, p.warnings.len())
    };
    assert_eq!(wb("Auto, 0 Red & 0 Blue"), (Some("Auto".into()), None, Some(0), Some(0), 0));
    assert_eq!(wb("Color Temperature 6300K, +3 Red & −5 Blue"), (Some("Kelvin".into()), Some(6300), Some(3), Some(-5), 0));
    assert_eq!(wb("Fluorescent 3 (Cool White), +1 Red & -3 Blue"), (Some("Fluorescent 3".into()), None, Some(1), Some(-3), 0));
    assert_eq!(wb("Auto White Priority, +2R & -4B"), (Some("Auto White Priority".into()), None, Some(2), Some(-4), 0));
    assert_eq!(wb("10000K, -7 Red & +8 Blue"), (Some("Kelvin".into()), Some(10000), Some(-7), Some(8), 0));
}

#[test]
fn dynamic_range_and_priority() {
    let p = parse_text("Dynamic Range: DR-Auto\nD-Range Priority: Strong");
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    assert_eq!((p.settings.dynamic_range.as_deref(), p.settings.d_range_priority.as_deref()), (Some("Auto"), Some("Strong")));
    let p = parse_text("Color Chrome FX Blue: N/A");
    assert!(p.warnings.is_empty() && p.settings.color_chrome_fx_blue.is_none());
}

#[test]
fn blocks_split_across_paragraphs_and_headings() {
    let html = "<h3>Version 1</h3>\n<p><strong>Film Simulation: Classic Chrome</strong></p>\n<p><strong>Dynamic Range: DR200</strong></p>\n<p><strong>Highlight: -1</strong></p>\n<p>Some prose about the recipe that runs on for a while.</p>\n<p><strong>Version 2</strong></p>\n<p>Film Simulation: Eterna<br>Grain Effect: Weak, Large<br>White Balance: Shade, 0 Red &amp; 0 Blue</p>";
    let b = blocks(html);
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].heading.as_deref(), Some("Version 1"));
    assert_eq!(b[0].text.lines().count(), 3);
    assert_eq!(b[1].heading.as_deref(), Some("Version 2"));
}

#[test]
fn roundup_without_settings_has_no_block() {
    let html = "<p>Here are ten recipes for autumn. <a href=\"x\">Kodak Gold</a> uses Classic Chrome.</p><p>Film Simulation: Classic Chrome is used by several.</p>";
    assert!(blocks(html).is_empty());
}

#[test]
fn names_from_titles() {
    for (title, want) in [
        ("Fujifilm X-Trans III + X-T3 &#038; X-T30 Film Simulation Recipe: Kodacolor VR", "Kodacolor VR"),
        ("CineStill 800T v2 — A Fujifilm Recipes for X-Trans V Cameras", "CineStill 800T v2"),
        ("My Fujifilm X-T30 Kodachrome 64 Film Simulation Recipe", "Kodachrome 64"),
        ("Fujifilm X-Trans I (X-E1 + X-Pro1) Film Simulation Recipe: Ektachrome", "Ektachrome"),
        ("My Fujifilm X100F Classic Chrome Film Simulation Recipe", "Classic Chrome"),
        ("Fujifilm X-Trans V Film Simulation Recipe: &#8220;Kodak Gold&#8221;", "Kodak Gold"),
    ] {
        assert_eq!(recipe_name(title), want, "{title}");
    }
}

#[test]
fn sensors_from_titles_and_tags() {
    assert_eq!(sensor("Fujifilm X-Trans III + X-T3 &#038; X-T30 Film Simulation Recipe: Kodacolor VR", &[]).as_deref(), Some("X-Trans III"));
    assert_eq!(sensor("CineStill 800T v2 — A Fujifilm Recipes for X-Trans V Cameras", &[]).as_deref(), Some("X-Trans V"));
    assert_eq!(sensor("Fujifilm X-Pro1 (+ X-E1) FXW App Patron Early-Access Film Simulation Recipe: Color Analog", &[]).as_deref(), Some("X-Trans I"));
    assert_eq!(sensor("My Fujifilm X-T30 Kodachrome 64 Film Simulation Recipe", &[]).as_deref(), Some("X-Trans IV"));
    assert_eq!(sensor("My Fujifilm X100F Classic Chrome Film Simulation Recipe", &[]).as_deref(), Some("X-Trans III"));
    assert_eq!(sensor("Fujifilm X100VI Film Simulation Recipe: Something", &[]).as_deref(), Some("X-Trans V"));
    assert_eq!(sensor("Kodak Gold", &["X-Trans IV".into()]).as_deref(), Some("X-Trans IV"));
    assert_eq!(sensor("Classic Retro — A Fujifilm Recipe for Fifth-Generation Cameras", &[]).as_deref(), Some("X-Trans V"));
    assert_eq!(sensor("7 Must-Try Recipes for 4th-Gen Fujifilm Cameras", &[]).as_deref(), Some("X-Trans IV"));
    assert_eq!(sensor("Vivid Chrome — A Fujifilm Recipe for X and GFX Cameras", &["Fujifilm GFX100RF".into(), "Fujifilm X-E5".into()]).as_deref(), Some("X-Trans V"));
    assert_eq!(sensor("Provia Positive", &["Fujifilm X-T5".into()]).as_deref(), Some("X-Trans V"));
    assert_eq!(sensor("Something unrelated", &[]), None);
}

#[test]
fn slugs() {
    assert_eq!(slug("https://fujixweekly.com/2022/04/11/fujifilm-x-trans-iii-kodacolor-vr/"), "fujifilm-x-trans-iii-kodacolor-vr");
}

#[test]
fn variants_seen_in_the_full_feed() {
    let p = parse_text("Color Chrome Effect: Off or N/A\nColor Chrome FX Blue: Off (X-Trans V); Weak (X-Trans IV)\nGrain Effect: Weak, Small — Weak for X-T3/X-T30\nDynamic Range: D-Range Priority (DR-P) Auto\nWhite Balance: Cloudy/Shade, -3 Red & +7 Blue\nSharpeness: -2\nHighlight & Shadow: +2\nColor +4\nMonochromatic Color (Toning): WC 0 & MG 0 (Off)\nAspect Ratio: 1:1");
    let s = &p.settings;
    assert_eq!(s.color_chrome.as_deref(), Some("Off"));
    assert_eq!(s.color_chrome_fx_blue.as_deref(), Some("Off"));
    assert_eq!((s.grain_effect.as_deref(), s.grain_size.as_deref()), (Some("Weak"), Some("Small")));
    assert_eq!((s.dynamic_range.as_ref(), s.d_range_priority.as_deref()), (None, Some("Auto")));
    assert_eq!(s.white_balance.as_deref(), Some("Shade"));
    assert_eq!((s.sharpness, s.highlight_tone, s.shadow_tone, s.color), (Some(-2.0), Some(2.0), Some(2.0), Some(4.0)));
    assert_eq!((s.monochromatic_color_wc, s.monochromatic_color_mg), (Some(0.0), Some(0.0)));
    assert_eq!(p.notes.len(), 1);
    // Values for other cameras are variants, not warnings.
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let alts: Vec<(&str, &str)> = p.alternatives.iter().map(|a| (a.label.as_str(), a.sensor.as_str())).collect();
    assert_eq!(alts, vec![("Color Chrome FX Blue", "X-Trans IV"), ("Grain Effect", "X-Trans IV")]);
}

#[test]
fn a_block_with_two_recipes_splits() {
    let parts = split_recipes("Exposure 1\nAcros\nDynamic Range: DR200\nHighlight: +1\nExposure 2\nVelvia\nDynamic Range: DR100\nHighlight: -1");
    assert_eq!(parts.len(), 2);
    assert_eq!(parse_text(&parts[1]).settings.film_simulation.as_deref(), Some("Velvia"));
}

#[test]
fn film_recipes_settings_table() {
    let html = r#"<h2 class="wp-block-heading">Kodak Gold II Film Recipe Settings</h2>
<div class="recipe-settings-wrap"><table class="recipe-settings"><tbody>
<tr><td>Film Simulation</td><td>Classic Chrome</td></tr>
<tr><td>Grain Effect</td><td>Weak, Small</td></tr>
<tr><td>Col. Chr. Effect</td><td>Weak</td></tr>
<tr><td>Col. Chr. Blue</td><td>Off</td></tr>
<tr><td>White Balance</td><td>Daylight, +4 Red, &#8209;5 Blue</td></tr>
<tr><td>Dynamic Range</td><td>DR200</td></tr>
<tr><td>Highlights</td><td>&#8209;1</td></tr>
<tr><td>Shadows</td><td>1</td></tr>
<tr><td>Colour</td><td>1</td></tr>
<tr><td>Sharpness</td><td>&#8209;2</td></tr>
<tr><td>ISO N.R.</td><td>&#8209;4</td></tr>
<tr><td>Clarity</td><td>0</td></tr>
<tr><td>Mono Colour</td><td>WC +1, MG -2</td></tr>
<tr><td>EV Comp.</td><td>+1/3</td></tr>
</tbody></table></div>"#;
    let b = blocks(html);
    assert_eq!(b.len(), 1);
    assert_eq!(super::sources::filmrecipes::name("Kodak Gold II &#8211; Classic Kodak Film Recipe", html), "Kodak Gold II");
    let p = parse_text(&b[0].text);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let s = &p.settings;
    assert_eq!((s.color_chrome.as_deref(), s.color_chrome_fx_blue.as_deref()), (Some("Weak"), Some("Off")));
    assert_eq!((s.white_balance.as_deref(), s.wb_shift_r, s.wb_shift_b), (Some("Daylight"), Some(4), Some(-5)));
    assert_eq!((s.highlight_tone, s.sharpness, s.high_iso_nr), (Some(-1.0), Some(-2.0), Some(-4.0)));
    assert_eq!((s.monochromatic_color_wc, s.monochromatic_color_mg), (Some(1.0), Some(-2.0)));
    assert_eq!(values::film_sim("Mono Red Filter").as_deref(), Some("Monochrome + R filter"));
    assert_eq!(p.notes.len(), 1);
}

fn src(id: &str, recipes: Vec<catalogue::SourceRecipe>) -> catalogue::SourceFile {
    catalogue::SourceFile { schema: 1, source: catalogue::SourceInfo { id: id.into(), name: id.into(), homepage: String::new(), credit: String::new(), compilation: id == "compiled" }, fetched_at: "2026-10-17".into(), recipes }
}

fn rec(id: &str, date: &str, text: &str) -> catalogue::SourceRecipe {
    let p = parse_text(text);
    catalogue::SourceRecipe { id: id.into(), name: id.into(), title: id.into(), url: format!("https://example.com/{id}"), date: date.into(), author: None, sensor: None, video: false, settings: p.settings, notes: p.notes, warnings: p.warnings, ..Default::default() }
}

#[test]
fn merge_keeps_the_earliest_credit_and_flags_near_duplicates() {
    // Same settings written two ways: absent Clarity equals "Clarity: 0", "Daylight" equals "Fine".
    let a = rec("gold", "2022-01-10", "Film Simulation: Classic Chrome\nDynamic Range: DR200\nWhite Balance: Daylight, +2 Red & -4 Blue\nHighlight: -1");
    let b = rec("gold-copy", "2020-05-01", "Classic Chrome\nDynamic Range: DR200\nWhite Balance: Fine, +2 Red, -4 Blue\nHighlight: -1\nClarity: 0\nGrain Effect: Off, Large");
    let c = rec("gold-warm", "2023-01-01", "Film Simulation: Classic Chrome\nDynamic Range: DR200\nWhite Balance: Daylight, +3 Red & -4 Blue\nHighlight: -1");
    let (cat, report) = catalogue::merge(&[src("one", vec![a, c]), src("two", vec![b])], "2026-10-17", &Default::default());
    assert_eq!(cat.recipes.len(), 2);
    let gold = cat.recipes.iter().find(|r| r.also_published_by.len() == 1).unwrap();
    assert_eq!((gold.credit.source.as_str(), gold.credit.name.as_str()), ("two", "gold-copy"));
    assert_eq!(gold.also_published_by[0].source, "one");
    assert_eq!((report.merged, report.merged_across_sources), (1, 1));
    assert_eq!(report.near_duplicates.len(), 1);
    assert_eq!(report.near_duplicates[0].field, "WB shift red");
}

#[test]
fn compilation_variants() {
    let p = parse_text("Classic Negative\nDR400\n5200K\nR+2 B-4\nSharp -2\nH-Tone +1\nS-Tone -1\nGrain: Weak - Small\nColor: N/A\nchrome fx Strong\nNoise: -4\nWB Shift R: 2 B:-3\nExposure Compensation +1/3");
    let s = &p.settings;
    let labels: Vec<&str> = p.warnings.iter().map(|w| w.label.as_str()).collect();
    assert_eq!(labels, vec!["WB Shift R"], "the second shift line is a duplicate: {:?}", p.warnings);
    assert_eq!((s.dynamic_range.as_deref(), s.white_balance.as_deref(), s.color_temp), (Some("DR400"), Some("Kelvin"), Some(5200)));
    assert_eq!((s.wb_shift_r, s.wb_shift_b), (Some(2), Some(-4)));
    assert_eq!((s.sharpness, s.highlight_tone, s.shadow_tone, s.high_iso_nr, s.color), (Some(-2.0), Some(1.0), Some(-1.0), Some(-4.0), None));
    assert_eq!((s.grain_effect.as_deref(), s.grain_size.as_deref(), s.color_chrome.as_deref()), (Some("Weak"), Some("Small"), Some("Strong")));
    assert_eq!(p.notes.len(), 1);
    let p = parse_text("WB Shift R: 2 B:-3");
    assert_eq!((p.settings.wb_shift_r, p.settings.wb_shift_b, p.warnings.len()), (Some(2), Some(-3), 0));
}

#[test]
fn compilation_rows_yield_to_the_post_they_link_to() {
    let post = rec("gold", "2022-01-10", "Film Simulation: Classic Chrome\nDynamic Range: DR200\nHighlight: -1");
    // Same settings, dated a day earlier: the creator's own post still keeps the credit.
    let mut early = rec("row-early", "2022-01-09", "Classic Chrome\nDR200\nHighlight: -1");
    early.url = "https://example.org/x".into();
    // Transcribed differently, but it links to the post.
    let mut typo = rec("row-typo", "2022-01-10", "Classic Chrome\nDR400\nHighlight: -1");
    typo.url = "http://www.example.com/gold/".into();
    let (cat, report) = catalogue::merge(&[src("site", vec![post]), src("compiled", vec![early, typo])], "2026-10-17", &Default::default());
    assert_eq!(cat.recipes.len(), 1);
    let r = &cat.recipes[0];
    assert_eq!(r.credit.source, "site");
    assert_eq!(r.settings.dynamic_range.as_deref(), Some("DR200"));
    assert_eq!(r.also_published_by.len(), 2);
    assert_eq!(r.also_published_by.iter().find(|p| p.name == "row-typo").unwrap().differs, vec!["dynamic range".to_string()]);
    assert_eq!(report.matched_by_link, 1);
}

#[test]
fn settings_on_one_line_and_long_comments() {
    let p = parse_text("0EV, Dynamic Range DR100, Film Simulation Acros+R, Grain Effect Strong, WB Shift R0 B0, Highlight Tone +3, Sharpness +4");
    let s = &p.settings;
    assert_eq!((s.film_simulation.as_deref(), s.dynamic_range.as_deref(), s.grain_effect.as_deref()), (Some("Acros + R filter"), Some("DR100"), Some("Strong")));
    assert_eq!((s.wb_shift_r, s.wb_shift_b, s.highlight_tone, s.sharpness), (Some(0), Some(0), Some(3.0), Some(4.0)));
    let long = "a comment that goes on ".repeat(10);
    let p = parse_text(&format!("White Balance: Auto\nISO: {long}"));
    assert!(p.notes[0].value.chars().count() <= 101);
}

#[test]
fn per_sensor_alternatives_become_variants() {
    let p = parse_text("Film Simulation: Classic Chrome\nColor Chrome FX Blue: Off (X-Trans V); Weak (X-Trans IV)\nClarity: 0 — N/A for X-T3/X-T30\nDynamic Range: DR400");
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    assert_eq!(p.sensor.as_deref(), Some("X-Trans V"));
    assert_eq!(p.settings.color_chrome_fx_blue.as_deref(), Some("Off"));
    let iv: Vec<&Alternative> = p.alternatives.iter().filter(|a| a.sensor == "X-Trans IV").collect();
    assert_eq!(iv.len(), 2, "{:?}", p.alternatives);
    let v = variant(&p.settings, &iv);
    assert_eq!((v.color_chrome_fx_blue.as_deref(), v.clarity), (Some("Weak"), None));
}

#[test]
fn sensor_versions_form_a_family() {
    let mut v = rec("gold-v", "2023-01-01", "Film Simulation: Classic Chrome\nColor Chrome FX Blue: Off\nClarity: -2\nShadow: 0.5\nDynamic Range: DR400");
    v.name = "Kodak Gold for X-Trans V".into();
    v.sensor = Some("X-Trans V".into());
    let mut iv = rec("gold-iv", "2021-01-01", "Film Simulation: Classic Chrome\nColor Chrome FX Blue: Weak\nClarity: -2\nShadow: 0\nDynamic Range: DR400");
    iv.name = "Kodak Gold".into();
    iv.sensor = Some("X-Trans IV".into());
    // A different look that happens to be one field apart, for the same sensor: not a family.
    let mut other = rec("other", "2022-01-01", "Film Simulation: Classic Chrome\nColor Chrome FX Blue: Strong\nClarity: -2\nShadow: 0\nDynamic Range: DR400");
    other.sensor = Some("X-Trans IV".into());
    let (cat, _) = catalogue::merge(&[src("site", vec![v, iv, other])], "2026-10-17", &Default::default());
    assert_eq!(cat.families.len(), 1);
    let f = &cat.families[0];
    assert_eq!(f.title, "Kodak Gold for X-Trans V");
    assert_eq!(f.variants.iter().map(|x| x.sensor.as_str()).collect::<Vec<_>>(), vec!["X-Trans V", "X-Trans IV"]);
    assert_eq!(super::family::stem("Kodak Gold for X-Trans V"), "kodak gold");
}

#[test]
fn popularity_score_is_a_log_blend() {
    use catalogue::{Popularity, RedditCount};
    assert_eq!(Popularity::score_of(0, 0, None), 0.0);
    let s = Popularity::score_of(10, 5, Some(&RedditCount { threads: 3, score: 90 }));
    let want = (11f64).ln() + 2.0 * (6f64).ln() + 1.5 * (4f64).ln() + 0.5 * (10f64).ln();
    assert!((s - (want * 100.0).round() / 100.0).abs() < 1e-9);
}
