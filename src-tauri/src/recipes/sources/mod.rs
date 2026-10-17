//! One small adapter per recipe site. Each turns a feed item into recipes through the shared
//! parser (`recipes::parse_text`); `examples/recipes_refresh.rs` walks the feeds.

pub mod filmrecipes;
pub mod fujixweekly;
pub mod hpchavaz;

use super::catalogue::{SourceInfo, SourceRecipe};
use super::{html, parse_text, post, split_recipes, variant, Parsed};

/// A WordPress RSS item, as the refresh tool reads it.
#[derive(Debug, Clone, Default)]
pub struct FeedItem {
    pub title: String,
    pub link: String,
    /// YYYY-MM-DD.
    pub date: String,
    /// `dc:creator`.
    pub creator: String,
    /// `slash:comments`, when the feed gives it.
    pub comments: Option<u32>,
    pub tags: Vec<String>,
    /// `content:encoded`: the post HTML. Only its settings leave the adapter.
    pub html: String,
}

/// How the refresh tool reads a source.
pub enum Fetch {
    /// A WordPress feed: the URL for page `n` (1-based), and the recipes in one item.
    Feed { page: fn(u32) -> String, recipes: fn(&FeedItem) -> Vec<SourceRecipe> },
    /// One CSV download, and the recipes in it.
    Csv { url: &'static str, recipes: fn(&str) -> Vec<SourceRecipe> },
}

/// A site the refresh tool can read.
pub struct Source {
    pub info: fn() -> SourceInfo,
    pub fetch: Fetch,
}

/// Every source, in credit order: on a tie in date, the earlier source keeps the credit, so
/// the original sites come before a compilation.
pub fn all() -> Vec<Source> {
    vec![fujixweekly::SOURCE, filmrecipes::SOURCE, hpchavaz::SOURCE]
}

/// `/feed/` for page 1, `/feed/?paged=N` after.
pub(crate) fn wp_feed(base: &str, n: u32) -> String {
    if n == 1 {
        format!("{base}/feed/")
    } else {
        format!("{base}/feed/?paged={n}")
    }
}

/// The recipes in a post: every block, split where a block holds several. A post with several
/// names each from the heading above its block, after the post's own name.
pub(crate) fn post_recipes(item: &FeedItem, base_name: String, sensor: Option<String>, author: Option<String>) -> Vec<SourceRecipe> {
    let blocks: Vec<html::Block> = html::blocks(&item.html)
        .into_iter()
        .flat_map(|b| {
            let parts = split_recipes(&b.text);
            let n = parts.len();
            parts.into_iter().enumerate().map(move |(i, text)| html::Block { text, heading: if i == 0 || n == 1 { b.heading.clone() } else { None } })
        })
        .collect();
    let base_id = post::slug(&item.link);
    let title = html::decode_entities(&item.title);
    let many = blocks.len() > 1;
    let video = title.to_lowercase().contains("video recipe");
    blocks
        .iter()
        .enumerate()
        .flat_map(|(i, b)| {
            let p = parse_text(&b.text);
            let name = match (&b.heading, many) {
                (Some(h), true) if h.to_lowercase().contains(&base_name.to_lowercase()) || base_name.chars().count() > 40 => h.clone(),
                (Some(h), true) => format!("{base_name}: {h}"),
                (None, true) if i > 0 => format!("{base_name} ({})", i + 1),
                _ => base_name.clone(),
            };
            let r = SourceRecipe {
                id: if i == 0 { base_id.clone() } else { format!("{base_id}-{}", i + 1) },
                name,
                title: title.clone(),
                url: item.link.clone(),
                date: item.date.clone(),
                author: author.clone(),
                sensor: sensor.clone(),
                video,
                comments: item.comments,
                ..SourceRecipe::default()
            };
            with_variants(r, p)
        })
        .collect()
}

/// The recipe, plus one recipe per other sensor its text gives values for. Each variant
/// names the first in `variant_of`; the first takes the sensor its values are for.
pub(crate) fn with_variants(mut r: SourceRecipe, p: Parsed) -> Vec<SourceRecipe> {
    let mut sensors: Vec<&str> = p.alternatives.iter().map(|a| a.sensor.as_str()).collect();
    sensors.dedup();
    sensors.sort();
    sensors.dedup();
    let variants: Vec<SourceRecipe> = sensors
        .iter()
        .filter(|s| p.sensor.as_deref() != Some(**s))
        .map(|s| {
            let alts: Vec<_> = p.alternatives.iter().filter(|a| a.sensor == *s).collect();
            SourceRecipe {
                id: format!("{}--{}", r.id, s.to_lowercase().replace(' ', "-")),
                sensor: Some(s.to_string()),
                settings: variant(&p.settings, &alts),
                notes: p.notes.clone(),
                variant_of: Some(r.id.clone()),
                ..r.clone()
            }
        })
        .collect();
    if !variants.is_empty() {
        r.sensor = p.sensor.clone().or(r.sensor);
    }
    r.settings = p.settings;
    r.notes = p.notes;
    r.warnings = p.warnings;
    let mut out = vec![r];
    out.extend(variants);
    out
}
