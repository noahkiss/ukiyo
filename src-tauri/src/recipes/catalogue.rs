//! The bundled recipe catalogue (`catalog/recipes/fujixweekly.json`): settings and a link
//! per recipe, credited to the source. No photos and no post text: those stay the author's.

use super::{html, parse_text, post, Note, Settings, Warning};
use serde::{Deserialize, Serialize};
use specta::Type;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Source {
    pub name: String,
    pub url: String,
    pub credit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueRecipe {
    /// The post's URL slug, with `-2`, `-3` for further recipes in the same post.
    pub id: String,
    /// The recipe name, from the post title (or the heading above the block).
    pub name: String,
    pub title: String,
    pub url: String,
    /// The post date, YYYY-MM-DD.
    pub date: String,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Catalogue {
    pub schema: u32,
    /// The day the feed was read, YYYY-MM-DD.
    pub fetched_at: String,
    pub source: Source,
    pub recipes: Vec<CatalogueRecipe>,
}

pub fn fxw_source() -> Source {
    Source {
        name: "Fuji X Weekly".into(),
        url: "https://fujixweekly.com".into(),
        credit: "Recipes © Ritchie Roesch, Fuji X Weekly (fujixweekly.com)".into(),
    }
}

/// The recipes in one post. Empty for a post with no settings block, such as a roundup.
pub fn from_post(title: &str, url: &str, date: &str, tags: &[String], post_html: &str) -> Vec<CatalogueRecipe> {
    let blocks = html::blocks(post_html);
    let base_name = post::recipe_name(title);
    let base_id = post::slug(url);
    let sensor = post::sensor(title, tags);
    let title = html::decode_entities(title);
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
            CatalogueRecipe {
                id: if i == 0 { base_id.clone() } else { format!("{base_id}-{}", i + 1) },
                name,
                title: title.clone(),
                url: url.to_string(),
                date: date.to_string(),
                sensor: sensor.clone(),
                video,
                settings: p.settings,
                notes: p.notes,
                warnings: p.warnings,
            }
        })
        .collect()
}
