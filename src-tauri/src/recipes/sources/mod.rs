//! One small adapter per recipe site. Each turns a feed item into recipes through the shared
//! parser (`recipes::parse_text`); `examples/recipes_refresh.rs` walks the feeds.

pub mod filmrecipes;
pub mod fujixweekly;

use super::catalogue::{SourceInfo, SourceRecipe};
use super::{html, parse_text, post, split_recipes};

/// A WordPress RSS item, as the refresh tool reads it.
#[derive(Debug, Clone, Default)]
pub struct FeedItem {
    pub title: String,
    pub link: String,
    /// YYYY-MM-DD.
    pub date: String,
    /// `dc:creator`.
    pub creator: String,
    pub tags: Vec<String>,
    /// `content:encoded`: the post HTML. Only its settings leave the adapter.
    pub html: String,
}

/// A site the refresh tool can read.
pub struct Source {
    pub info: fn() -> SourceInfo,
    /// The feed URL for page `n` (1-based).
    pub feed_page: fn(u32) -> String,
    pub recipes: fn(&FeedItem) -> Vec<SourceRecipe>,
}

pub fn all() -> Vec<Source> {
    vec![fujixweekly::SOURCE, filmrecipes::SOURCE]
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
        .map(|(i, b)| {
            let p = parse_text(&b.text);
            let name = match (&b.heading, many) {
                (Some(h), true) if h.to_lowercase().contains(&base_name.to_lowercase()) => h.clone(),
                (Some(h), true) => format!("{base_name}: {h}"),
                (None, true) if i > 0 => format!("{base_name} ({})", i + 1),
                _ => base_name.clone(),
            };
            SourceRecipe {
                id: if i == 0 { base_id.clone() } else { format!("{base_id}-{}", i + 1) },
                name,
                title: title.clone(),
                url: item.link.clone(),
                date: item.date.clone(),
                author: author.clone(),
                sensor: sensor.clone(),
                video,
                settings: p.settings,
                notes: p.notes,
                warnings: p.warnings,
            }
        })
        .collect()
}
