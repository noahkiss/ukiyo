//! film.recipes (Justin Gould): one settings table per post, under a heading such as
//! "Kodak Gold II Film Recipe Settings".

use super::{post_recipes, wp_feed, FeedItem, Source};
use crate::recipes::catalogue::{SourceInfo, SourceRecipe};
use crate::recipes::{html, post};
use regex::Regex;
use std::sync::LazyLock;

pub const SOURCE: Source = Source { info, feed_page, recipes };

static SETTINGS_HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*(?:film\s+)?recipe\s+settings\s*$").unwrap());

fn info() -> SourceInfo {
    SourceInfo {
        id: "filmrecipes".into(),
        name: "film.recipes".into(),
        homepage: "https://film.recipes".into(),
        credit: "Recipes © Justin Gould, film.recipes".into(),
    }
}

fn feed_page(n: u32) -> String {
    wp_feed("https://film.recipes", n)
}

/// "Kodak Gold II – Classic Kodak Film Recipe" -> "Kodak Gold II"; "Rizzle Clicks, Grainy
/// with Knockout Reds" -> "Rizzle Clicks". The settings heading, when present, wins.
pub fn name(title: &str, html_body: &str) -> String {
    let headed = html::blocks(html_body).into_iter().find_map(|b| b.heading).map(|h| SETTINGS_HEADING.replace(&h, "").trim().to_string()).filter(|h| !h.is_empty());
    if let Some(h) = headed {
        return h;
    }
    let t = html::decode_entities(title);
    let first = t.split([',', '–', '—']).next().unwrap_or(&t).trim();
    if first.is_empty() {
        post::recipe_name(title)
    } else {
        first.to_string()
    }
}

fn recipes(item: &FeedItem) -> Vec<SourceRecipe> {
    let author = if item.creator.is_empty() || item.creator == "justingould" { "Justin Gould".to_string() } else { item.creator.clone() };
    // Titles rarely name a sensor: the tags, else the first camera the post mentions.
    let sensor = post::sensor(&item.title, &item.tags).or_else(|| post::sensor_in_text(&html::to_text(&item.html)));
    post_recipes(item, name(&item.title, &item.html), sensor, Some(author))
}
