//! Fuji X Weekly (fujixweekly.com): recipe blocks as `<p>` paragraphs of `Label: value` lines.

use super::{post_recipes, wp_feed, FeedItem, Fetch, Source};
use crate::recipes::catalogue::{SourceInfo, SourceRecipe};
use crate::recipes::post;

pub const SOURCE: Source = Source { info, fetch: Fetch::Feed { page: feed_page, recipes } };

fn info() -> SourceInfo {
    SourceInfo {
        id: "fujixweekly".into(),
        name: "Fuji X Weekly".into(),
        homepage: "https://fujixweekly.com".into(),
        credit: "Recipes © Ritchie Roesch, Fuji X Weekly (fujixweekly.com)".into(),
        compilation: false,
    }
}

fn feed_page(n: u32) -> String {
    wp_feed("https://fujixweekly.com", n)
}

fn recipes(item: &FeedItem) -> Vec<SourceRecipe> {
    post_recipes(item, post::recipe_name(&item.title), post::sensor(&item.title, &item.tags), Some("Ritchie Roesch".into()))
}
