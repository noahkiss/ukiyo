//! ukiyo's recipe catalogue. Each source has its own file (`catalog/recipes/sources/<id>.json`)
//! with the recipes as that site published them; `merge` folds the sources into
//! `catalog/recipes/catalog.json`, one entry per distinct set of settings. Settings, names
//! and links only: photos and post text stay their authors'.

use super::{Note, Settings, Warning};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

pub const SCHEMA: u32 = 1;

/// The most fields a compilation row may differ from the post it links to and still fold in.
pub const LINK_MATCH_MAX_DIFF: usize = 3;

/// A site that publishes recipes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SourceInfo {
    /// Short stable id: "fujixweekly", "filmrecipes".
    pub id: String,
    pub name: String,
    pub homepage: String,
    pub credit: String,
    /// A compilation of other people's recipes: each recipe credits its `author`, and the
    /// source is shown as where it was found.
    #[serde(default)]
    pub compilation: bool,
}

/// A recipe as one source published it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecipe {
    /// The post's URL slug, with `-2`, `-3` for further recipes in the same post.
    pub id: String,
    /// The recipe name, from the post title (or the heading above the block).
    pub name: String,
    pub title: String,
    pub url: String,
    /// The post date, YYYY-MM-DD; empty when unknown.
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensor: Option<String>,
    /// A video recipe: settings for movie mode.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub video: bool,
    pub settings: Settings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,
}

/// `catalog/recipes/sources/<id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub schema: u32,
    #[serde(flatten)]
    pub source: SourceInfo,
    /// The day the source was read, YYYY-MM-DD.
    pub fetched_at: String,
    pub recipes: Vec<SourceRecipe>,
}

/// One publication of a recipe: who, where, when.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Publication {
    /// The `SourceInfo::id`.
    pub source: String,
    pub name: String,
    pub url: String,
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// For a compilation row matched to the post it links to: the settings it gives
    /// differently from that post. The post's own settings are the recipe's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub differs: Vec<String>,
}

/// One distinct recipe: settings published once or more.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRecipe {
    /// `<source>:<source recipe id>` of the primary publication.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensor: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub video: bool,
    pub settings: Settings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,
    /// Hash of the normalised settings (`settings_key`), 16 hex digits.
    pub hash: String,
    /// The earliest publication: the credit.
    pub credit: Publication,
    /// Later publications of the same settings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_published_by: Vec<Publication>,
}

/// `catalog/recipes/catalog.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecipeCatalog {
    pub schema: u32,
    /// The day the merge ran, YYYY-MM-DD.
    pub built_at: String,
    pub sources: Vec<SourceInfo>,
    pub recipes: Vec<CatalogRecipe>,
}

/// The settings as named fields with every absent setting at the camera default, so a post
/// that omits Clarity equals one that gives "Clarity: 0".
pub fn normalised(s: &Settings) -> Vec<(&'static str, String)> {
    let t = |v: Option<f64>| format!("{:.1}", v.unwrap_or(0.0));
    let e = |v: &Option<String>, d: &str| v.clone().unwrap_or_else(|| d.to_string());
    let grain = e(&s.grain_effect, "Off");
    let wb = match s.white_balance.as_deref() {
        Some("Kelvin") => format!("{}K", s.color_temp.unwrap_or(0)),
        Some(p) => p.to_string(),
        None => "Auto".into(),
    };
    vec![
        ("film simulation", e(&s.film_simulation, "?")),
        ("dynamic range", e(&s.dynamic_range, "DR100")),
        ("D-range priority", e(&s.d_range_priority, "Off")),
        // Grain size means nothing with grain off.
        ("grain", if grain == "Off" { grain } else { format!("{grain} {}", e(&s.grain_size, "Small")) }),
        ("color chrome", e(&s.color_chrome, "Off")),
        ("color chrome FX blue", e(&s.color_chrome_fx_blue, "Off")),
        ("smooth skin", e(&s.smooth_skin, "Off")),
        ("white balance", wb),
        ("WB shift red", s.wb_shift_r.unwrap_or(0).to_string()),
        ("WB shift blue", s.wb_shift_b.unwrap_or(0).to_string()),
        ("highlight", t(s.highlight_tone)),
        ("shadow", t(s.shadow_tone)),
        ("color", t(s.color)),
        ("sharpness", t(s.sharpness)),
        ("high ISO NR", t(s.high_iso_nr)),
        ("clarity", t(s.clarity)),
        ("mono WC", t(s.monochromatic_color_wc)),
        ("mono MG", t(s.monochromatic_color_mg)),
    ]
}

/// The normalised settings as one line: what the hash covers.
pub fn settings_key(s: &Settings) -> String {
    normalised(s).into_iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(";")
}

/// FNV-1a, 64-bit: stable across Rust versions, unlike `DefaultHasher`.
pub fn settings_hash(s: &Settings) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in settings_key(s).bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Two recipes whose settings differ in exactly one field. They stay separate entries.
#[derive(Debug, Clone, PartialEq)]
pub struct NearDuplicate {
    pub a: String,
    pub b: String,
    pub field: &'static str,
}

#[derive(Debug, Default)]
pub struct MergeReport {
    pub publications: usize,
    pub recipes: usize,
    /// Entries published more than once, and by more than one source.
    pub merged: usize,
    pub merged_across_sources: usize,
    /// Compilation rows whose settings differ from the post they link to, folded into it.
    pub matched_by_link: usize,
    /// Publications with no film simulation are left out: a slot cannot be set from them.
    /// They stay in their source file, with their warnings.
    pub left_out_without_sim: usize,
    pub near_duplicates: Vec<NearDuplicate>,
}

/// Folds the sources into the catalogue. Equal settings hashes are one recipe; the earliest
/// publication is the credit (ties go to the earlier source in `sources`), the rest are
/// "also published by". A compilation's publication is the credit only when no other
/// source has the recipe. Video recipes only merge with video recipes. Publications with no
/// film simulation are left out.
pub fn merge(sources: &[SourceFile], built_at: &str) -> (RecipeCatalog, MergeReport) {
    let mut report = MergeReport::default();
    let mut groups: BTreeMap<String, Vec<(usize, &SourceRecipe)>> = BTreeMap::new();
    for (si, f) in sources.iter().enumerate() {
        for r in &f.recipes {
            report.publications += 1;
            if r.settings.film_simulation.is_none() {
                report.left_out_without_sim += 1;
                continue;
            }
            let key = format!("{}{}", if r.video { "video:" } else { "" }, settings_hash(&r.settings));
            groups.entry(key).or_default().push((si, r));
        }
    }

    // A compilation row that links to a post another source read is that post's recipe, even
    // when its settings were transcribed differently: fold it into the closest recipe there,
    // when that is at most LINK_MATCH_MAX_DIFF fields apart.
    let link = |u: &str| u.trim_end_matches('/').replace("://www.", "://").replace("http://", "https://");
    let compiled = |si: usize| sources[si].source.compilation;
    let mut by_url: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (key, pubs) in &groups {
        for (si, r) in pubs {
            if !compiled(*si) {
                by_url.entry(link(&r.url)).or_default().push(key.clone());
            }
        }
    }
    let mut differs: BTreeMap<(usize, String), Vec<String>> = BTreeMap::new();
    let orphan_keys: Vec<String> = groups.iter().filter(|(_, p)| p.iter().all(|(si, _)| compiled(*si))).map(|(k, _)| k.clone()).collect();
    for key in orphan_keys {
        let pubs = groups.remove(&key).unwrap_or_default();
        for (si, r) in pubs {
            let mine = normalised(&r.settings);
            let target = by_url.get(&link(&r.url)).and_then(|keys| {
                keys.iter()
                    .map(|k| {
                        let theirs = normalised(&groups[k][0].1.settings);
                        let d: Vec<String> = mine.iter().zip(&theirs).filter(|(a, b)| a.1 != b.1).map(|(a, _)| a.0.to_string()).collect();
                        (d.len(), k.clone(), d)
                    })
                    .min()
            });
            match target {
                // More than three fields apart is likely another recipe in the same post.
                Some((n, k, d)) if n <= LINK_MATCH_MAX_DIFF => {
                    report.matched_by_link += 1;
                    differs.insert((si, r.id.clone()), d);
                    groups.get_mut(&k).unwrap().push((si, r));
                }
                _ => groups.entry(key.clone()).or_default().push((si, r)),
            }
        }
    }

    let mut recipes: Vec<CatalogRecipe> = groups
        .into_values()
        .map(|mut pubs| {
            // An unknown date sorts last: it cannot claim to be first.
            let when = |r: &SourceRecipe| if r.date.is_empty() { "9999".to_string() } else { r.date.clone() };
            // A compilation's row credits the same creator; the creator's own post comes first.
            pubs.sort_by(|(sa, a), (sb, b)| compiled(*sa).cmp(&compiled(*sb)).then(when(a).cmp(&when(b))).then(sa.cmp(sb)).then(a.id.cmp(&b.id)));
            let (psi, primary) = pubs[0];
            let publication = |si: usize, r: &SourceRecipe| Publication {
                source: sources[si].source.id.clone(),
                name: r.name.clone(),
                url: r.url.clone(),
                date: r.date.clone(),
                author: r.author.clone(),
                differs: differs.get(&(si, r.id.clone())).cloned().unwrap_or_default(),
            };
            if pubs.len() > 1 {
                report.merged += 1;
                if pubs.iter().any(|(si, _)| *si != psi) {
                    report.merged_across_sources += 1;
                }
            }
            CatalogRecipe {
                id: format!("{}:{}", sources[psi].source.id, primary.id),
                name: primary.name.clone(),
                sensor: primary.sensor.clone(),
                video: primary.video,
                settings: primary.settings.clone(),
                notes: primary.notes.clone(),
                warnings: primary.warnings.clone(),
                hash: settings_hash(&primary.settings),
                credit: publication(psi, primary),
                also_published_by: pubs[1..].iter().map(|(si, r)| publication(*si, r)).collect(),
            }
        })
        .collect();
    recipes.sort_by(|a, b| b.credit.date.cmp(&a.credit.date).then_with(|| a.id.cmp(&b.id)));
    report.recipes = recipes.len();

    let norm: Vec<Vec<(&'static str, String)>> = recipes.iter().map(|r| normalised(&r.settings)).collect();
    for i in 0..recipes.len() {
        for j in i + 1..recipes.len() {
            if recipes[i].video != recipes[j].video {
                continue;
            }
            let mut diff = norm[i].iter().zip(&norm[j]).filter(|(a, b)| a.1 != b.1);
            if let (Some((a, _)), None) = (diff.next(), diff.next()) {
                report.near_duplicates.push(NearDuplicate { a: recipes[i].id.clone(), b: recipes[j].id.clone(), field: a.0 });
            }
        }
    }
    let catalog = RecipeCatalog { schema: SCHEMA, built_at: built_at.to_string(), sources: sources.iter().map(|f| f.source.clone()).collect(), recipes };
    (catalog, report)
}
