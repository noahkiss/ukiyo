//! Recipe families: one recipe made for several sensors. Fuji X Weekly often publishes an
//! X-Trans IV recipe again for X-Trans V with Color Chrome FX Blue or Clarity changed; a
//! family keeps those versions together so the app shows one card and picks the version
//! for the connected camera.

use super::catalogue::{normalised, CatalogRecipe};
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

/// A family's versions, newest sensor first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Variant {
    pub sensor: String,
    /// A `CatalogRecipe::id`.
    pub recipe: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Family {
    /// `fam:` and the newest variant's recipe id.
    pub id: String,
    pub title: String,
    pub variants: Vec<Variant>,
    /// The variants' signals, summed.
    pub popularity: super::catalogue::Popularity,
}

/// The generations in order; Bayer and GFX bodies are not ranked, so never join a family.
pub fn sensor_rank(sensor: &str) -> Option<u8> {
    Some(match sensor {
        "X-Trans I" => 1,
        "X-Trans II" => 2,
        "X-Trans III" => 3,
        "X-Trans IV" => 4,
        "X-Trans V" => 5,
        _ => return None,
    })
}

/// The first generation that has a film simulation. Approximate: within a generation the
/// first bodies sometimes lack what later firmware added.
fn sim_since(sim: &str) -> u8 {
    match sim {
        "Reala Ace" | "Nostalgic Neg." => 5,
        "Classic Negative" | "Eterna Bleach Bypass" => 4,
        "Eterna" | "Acros" | "Acros + R filter" | "Acros + Ye filter" | "Acros + G filter" => 3,
        "Classic Chrome" => 2,
        _ => 1,
    }
}

/// Settings that change between sensors for the same look. Highlight and Shadow may also
/// move by up to one step, when the names or a link already tie the two together.
const SENSOR_FIELDS: &[&str] = &["color chrome FX blue", "clarity", "color chrome", "smooth skin", "D-range priority", "grain"];

static SENSOR_PARENS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*\((?:x-?trans|bayer|gfx)[^)]*\)").unwrap());
static SENSOR_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bx[- ]?trans\s*(?:iv|v|iii|ii|i|\d)\b|\b(?:fifth|fourth|third|5th|4th|3rd)[- ]gen\w*|\bx-?[a-z]{1,3}\d+\w*|\bx100\w*|\bgfx\w*|\bfujifilm\b|\bfuji\b|\bfilm simulation\b|\brecipes?\b|\bfor\b|\bcameras?\b|\bversion\b").unwrap()
});

/// A name without its sensor words: "Kodak Portra 400 v2 for X-Trans IV" -> "kodak portra 400 v2".
pub fn stem(name: &str) -> String {
    let s = SENSOR_WORDS.replace_all(name, " ");
    super::norm(&s)
}

/// The recipe that has the setting a sensor lacks counts as the newer one.
fn sensor_only(a: &CatalogRecipe, b: &CatalogRecipe, ra: u8, rb: u8) -> Option<Vec<&'static str>> {
    let (na, nb) = (normalised(&a.settings), normalised(&b.settings));
    let diff: Vec<&'static str> = na.iter().zip(&nb).filter(|(x, y)| x.1 != y.1).map(|(x, _)| x.0).collect();
    if diff.is_empty() {
        return None;
    }
    let (old, new, old_rank) = if ra < rb { (a, b, ra) } else { (b, a, rb) };
    for f in &diff {
        match *f {
            "film simulation" => {
                let new_sim = new.settings.film_simulation.as_deref().unwrap_or("");
                let mono = |r: &CatalogRecipe| r.settings.film_simulation.as_deref().is_some_and(|x| x.starts_with("Acros") || x.starts_with("Monochrome") || x == "Sepia");
                if mono(old) != mono(new) {
                    return None;
                }
                // The older sensor must lack the newer one's simulation.
                if sim_since(new_sim) <= old_rank {
                    return None;
                }
            }
            "grain" => {
                let strength = |r: &CatalogRecipe| r.settings.grain_effect.clone().unwrap_or_else(|| "Off".into());
                if strength(old) != strength(new) && old_rank > 2 {
                    return None;
                }
            }
            // Newer sensors render tones a little differently; ports nudge them by a step.
            "highlight" | "shadow" => {
                let t = |r: &CatalogRecipe| if *f == "highlight" { r.settings.highlight_tone } else { r.settings.shadow_tone }.unwrap_or(0.0);
                if (t(old) - t(new)).abs() > 1.0 {
                    return None;
                }
            }
            f if SENSOR_FIELDS.contains(&f) => {}
            _ => return None,
        }
    }
    Some(diff)
}

struct Find(Vec<usize>);

impl Find {
    fn root(&mut self, i: usize) -> usize {
        let mut i = i;
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }
    fn join(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.root(a), self.root(b));
        if ra != rb {
            self.0[rb] = ra;
        }
    }
}

/// Why two recipes joined, for the build summary.
#[derive(Debug, Clone, PartialEq)]
pub enum Reason {
    /// One post gave values for two sensors.
    SamePost,
    SameName,
    Linked,
    /// One sensor-dependent field apart, with different names and no link.
    NearDuplicate,
}

/// Two recipes joined: their indexes, why, and the fields they differ in.
pub type Join = (usize, usize, Reason, Vec<&'static str>);

/// Groups recipes into families. `urls[i]` are recipe i's publication links, `links[i]` the
/// posts its publications link to, `variant_of` pairs of recipes one post gave for two
/// sensors. Two recipes for different sensors join when their settings differ only in
/// sensor-dependent fields and they share a name, link to each other, come from one post,
/// or are one field apart. Returns the families and every join, with its reason.
pub fn group(recipes: &[CatalogRecipe], urls: &[Vec<String>], links: &[Vec<String>], variant_of: &[(usize, usize)]) -> (Vec<Family>, Vec<Join>) {
    let n = recipes.len();
    let mut find = Find((0..n).collect());
    let mut joins = Vec::new();
    for &(a, b) in variant_of {
        find.join(a, b);
        joins.push((a, b, Reason::SamePost, Vec::new()));
    }
    let ranks: Vec<Option<u8>> = recipes.iter().map(|r| r.sensor.as_deref().and_then(sensor_rank)).collect();
    let stems: Vec<String> = recipes.iter().map(|r| stem(&r.name)).collect();
    let url_sets: Vec<HashSet<&str>> = urls.iter().map(|u| u.iter().map(String::as_str).collect()).collect();
    for i in 0..n {
        let Some(ri) = ranks[i] else { continue };
        for j in i + 1..n {
            let Some(rj) = ranks[j] else { continue };
            if ri == rj || recipes[i].video != recipes[j].video {
                continue;
            }
            let Some(diff) = sensor_only(&recipes[i], &recipes[j], ri, rj) else { continue };
            let linked = links[i].iter().any(|l| url_sets[j].contains(l.as_str())) || links[j].iter().any(|l| url_sets[i].contains(l.as_str()));
            let reason = if !stems[i].is_empty() && stems[i] == stems[j] {
                Reason::SameName
            } else if linked {
                Reason::Linked
            } else if diff.len() == 1 && !diff.iter().any(|f| matches!(*f, "highlight" | "shadow" | "film simulation")) {
                Reason::NearDuplicate
            } else {
                continue;
            };
            find.join(i, j);
            joins.push((i, j, reason, diff));
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = find.root(i);
        groups.entry(r).or_default().push(i);
    }
    let mut families: Vec<Family> = groups
        .into_values()
        .filter(|g| g.len() > 1)
        .map(|mut g| {
            // Newest sensor first, then newest post.
            g.sort_by(|&a, &b| ranks[b].cmp(&ranks[a]).then_with(|| recipes[b].credit.date.cmp(&recipes[a].credit.date)));
            let head = &recipes[g[0]];
            Family {
                id: format!("fam:{}", head.id),
                title: SENSOR_PARENS.replace_all(&head.name, "").split_whitespace().collect::<Vec<_>>().join(" "),
                variants: g.iter().map(|&i| Variant { sensor: recipes[i].sensor.clone().unwrap_or_default(), recipe: recipes[i].id.clone() }).collect(),
                popularity: Default::default(),
            }
        })
        .collect();
    families.sort_by(|a, b| a.id.cmp(&b.id));
    (families, joins)
}
