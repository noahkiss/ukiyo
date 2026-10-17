//! Recipes in the library (`library/recipes/<slug>.json`, id `recipe:<slug>`) and the bundled
//! catalogue (`cat:<id>`, read only). A recipe holds slot settings in menu units, as the
//! parser gives them; writing one to a slot is a plan (`lab`).

use super::{item_id, now_ms, slug, Error, ErrorCode, Hit, Kind, Saved, Store, SCHEMA};
use crate::recipes::catalogue::{self, CatalogRecipe};
use crate::recipes::{bundle, slot, Note, Settings};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Where a library recipe came from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct RecipeSource {
    /// `cat:<id>` when it was copied from the catalogue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// The post it was published in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Its creator, and the publisher for a catalogue recipe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<String>,
}

/// A recipe in the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Recipe {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub settings: Settings,
    /// Settings a slot cannot hold: ISO, exposure compensation.
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub sensor: Option<String>,
    #[serde(default)]
    pub source: Option<RecipeSource>,
    /// The person's own text about it.
    #[serde(default)]
    pub comment: Option<String>,
}

/// What `library_save {kind: recipe}` takes as `item`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeItem {
    #[serde(default)]
    pub settings: Option<Settings>,
    /// Label to value, as a post writes them ("Highlight": "+1.5"), applied over `settings`
    /// or over the recipe of that name already saved.
    #[serde(default)]
    pub set: indexmap::IndexMap<String, String>,
    #[serde(default)]
    pub notes: Option<Vec<Note>>,
    #[serde(default)]
    pub sensor: Option<String>,
    #[serde(default)]
    pub source: Option<RecipeSource>,
    #[serde(default)]
    pub comment: Option<String>,
}

/// The slot problems of a recipe's settings, as save warnings.
pub fn slot_warnings(s: &Settings) -> Vec<String> {
    match slot::encode(s) {
        Ok(_) => vec![],
        Err(p) => p.into_iter().map(|p| format!("not writable to a slot: {}", p.message)).collect(),
    }
}

/// One line about a recipe's settings: "Classic Chrome, DR400, grain Weak Small, WB 5200K".
pub fn summary(s: &Settings) -> String {
    let mut parts: Vec<String> = vec![s.film_simulation.clone().unwrap_or_else(|| "no film simulation".into())];
    if let Some(d) = &s.dynamic_range {
        parts.push(d.clone());
    }
    if let Some(g) = s.grain_effect.as_deref().filter(|g| *g != "Off") {
        parts.push(format!("grain {g}{}", s.grain_size.as_deref().map(|z| format!(" {z}")).unwrap_or_default()));
    }
    match (s.white_balance.as_deref(), s.color_temp) {
        (Some("Kelvin"), Some(k)) => parts.push(format!("WB {k}K")),
        (Some(w), _) => parts.push(format!("WB {w}")),
        _ => {}
    }
    parts.join(", ")
}

fn credit_line(r: &CatalogRecipe) -> String {
    let who = r.credit.author.clone().unwrap_or_else(|| r.credit.name.clone());
    let site = bundle::catalog().sources.iter().find(|s| s.id == r.credit.source).map(|s| s.name.clone()).unwrap_or_else(|| r.credit.source.clone());
    if r.credit.author.is_some() {
        format!("{who}, {site}")
    } else {
        site
    }
}

/// A catalogue recipe as a search hit.
pub fn catalogue_hit(r: &CatalogRecipe) -> Hit {
    let sensor = r.sensor.clone().map(|s| format!(" ({s})")).unwrap_or_default();
    Hit {
        id: format!("cat:{}", r.id),
        kind: "catalogue".into(),
        name: r.name.clone(),
        summary: format!("{}{sensor}; {}", summary(&r.settings), credit_line(r)),
        at: None,
        credit: Some(credit_line(r)),
        url: Some(r.credit.url.clone()),
        sensor: r.sensor.clone(),
    }
}

pub fn recipe_hit(r: &Recipe) -> Hit {
    Hit {
        id: r.id.clone(),
        kind: "recipe".into(),
        name: r.name.clone(),
        summary: summary(&r.settings),
        at: Some(r.updated_at),
        credit: r.source.as_ref().and_then(|s| s.credit.clone()),
        url: r.source.as_ref().and_then(|s| s.url.clone()),
        sensor: r.sensor.clone(),
    }
}

/// Whether a recipe passes the search filters: every query word in its name, film
/// simulation or credit; the sensor and film simulation, any case.
pub fn matches(name: &str, s: &Settings, sensor: Option<&str>, credit: &str, words: &[String], want_sensor: Option<&str>, want_sim: Option<&str>) -> bool {
    let hay = format!("{name} {} {credit}", s.film_simulation.as_deref().unwrap_or("")).to_lowercase();
    words.iter().all(|w| hay.contains(w))
        && want_sensor.is_none_or(|w| sensor.is_some_and(|x| x.eq_ignore_ascii_case(w.trim())))
        && want_sim.is_none_or(|w| s.film_simulation.as_deref().is_some_and(|x| x.to_lowercase().contains(&w.trim().to_lowercase())))
}

/// The catalogue recipes that match, most popular first.
pub fn search_catalogue(words: &[String], sensor: Option<&str>, sim: Option<&str>) -> Vec<Hit> {
    let mut v: Vec<&CatalogRecipe> = bundle::catalog().recipes.iter().filter(|r| matches(&r.name, &r.settings, r.sensor.as_deref(), &credit_line(r), words, sensor, sim)).collect();
    v.sort_by(|a, b| b.popularity.score.total_cmp(&a.popularity.score));
    v.into_iter().map(catalogue_hit).collect()
}

/// A field-by-field comparison of two recipes' settings, with absent settings at the camera
/// default (the catalogue's de-duplication rule).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct RecipeDiff {
    pub a: String,
    pub b: String,
    pub changed: Vec<FieldDiff>,
    pub same: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct FieldDiff {
    pub field: String,
    pub a: String,
    pub b: String,
}

pub fn diff(a_id: &str, a: &Settings, b_id: &str, b: &Settings) -> RecipeDiff {
    let (na, nb) = (catalogue::normalised(a), catalogue::normalised(b));
    let mut changed = vec![];
    let mut same = 0;
    for ((field, va), (_, vb)) in na.into_iter().zip(nb) {
        if va == vb {
            same += 1;
        } else {
            changed.push(FieldDiff { field: field.into(), a: va, b: vb });
        }
    }
    RecipeDiff { a: a_id.into(), b: b_id.into(), changed, same }
}

impl Store {
    pub fn recipe(&self, slug: &str) -> Result<Recipe, Error> {
        Self::load(&self.path(Kind::Recipe, slug))?.ok_or_else(|| Self::missing(&item_id(Kind::Recipe, slug)))
    }

    pub fn recipes(&self) -> Vec<Recipe> {
        let mut v: Vec<Recipe> = self.all(Kind::Recipe);
        v.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
        v
    }

    /// The settings of a recipe id: `recipe:<slug>` or `cat:<id>`, and its name.
    pub fn recipe_settings(&self, id: &str) -> Result<(String, Settings), Error> {
        if id.starts_with("cat:") {
            let r = bundle::recipe(id).ok_or_else(|| Error::new(ErrorCode::NotFound, format!("no {id} in the catalogue")).hint("library_search {kind: catalogue}, or ukiyo recipes search, finds one."))?;
            return Ok((r.name.clone(), r.settings.clone()));
        }
        let s = id.strip_prefix("recipe:").ok_or_else(|| Error::new(ErrorCode::NotFound, format!("{id} is not a recipe id (recipe:<name> or cat:<id>)")))?;
        let r = self.recipe(&slug(s))?;
        Ok((r.name, r.settings))
    }

    pub fn save_recipe(&self, actor: &str, mut r: Recipe) -> Result<Saved, Error> {
        let slug = slug(&r.name);
        if slug.is_empty() || slug == "unknown" {
            return Err(Error::new(ErrorCode::BadRequest, "recipe: give it a name"));
        }
        let _g = self.lock()?;
        let old: Option<Recipe> = Self::load(&self.path(Kind::Recipe, &slug))?;
        r.schema = SCHEMA;
        r.id = item_id(Kind::Recipe, &slug);
        r.updated_at = now_ms() as f64;
        r.created_at = old.map_or(r.updated_at, |o| o.created_at);
        let replaced = self.put(Kind::Recipe, &slug, &r)?;
        self.log(actor, "save", &r.id, serde_json::json!({ "replaced": replaced }));
        Ok(Saved { id: r.id.clone(), kind: "recipe".into(), replaced, warnings: slot_warnings(&r.settings) })
    }

    /// `library_save {kind: recipe}`: `from: cat:<id>` copies a catalogue recipe (under
    /// `name`, else its own); `item` gives the settings, or `set` changes the saved recipe
    /// of that name.
    pub fn save_recipe_input(&self, actor: &str, name: Option<String>, item: Option<serde_json::Value>, from: Option<&str>) -> Result<Saved, Error> {
        let item: RecipeItem = match item {
            None | Some(serde_json::Value::Null) => RecipeItem::default(),
            Some(v) => serde_json::from_value(v).map_err(|e| Error::new(ErrorCode::BadRequest, format!("recipe: {e}")))?,
        };
        let mut r = if let Some(from) = from {
            let c = bundle::recipe(from).filter(|_| from.starts_with("cat:")).ok_or_else(|| Error::new(ErrorCode::NotFound, format!("no {from} in the catalogue")).hint("from is cat:<id>, from library_search {kind: catalogue}."))?;
            Recipe {
                schema: SCHEMA,
                id: String::new(),
                name: name.clone().unwrap_or_else(|| c.name.clone()),
                created_at: 0.0,
                updated_at: 0.0,
                settings: c.settings.clone(),
                notes: c.notes.clone(),
                sensor: c.sensor.clone(),
                source: Some(RecipeSource { from: Some(format!("cat:{}", c.id)), url: Some(c.credit.url.clone()), credit: Some(credit_line(c)) }),
                comment: None,
            }
        } else {
            let name = name.clone().filter(|n| !n.trim().is_empty()).ok_or_else(|| Error::new(ErrorCode::BadRequest, "recipe: give it a name"))?;
            let old = self.recipe(&slug(&name)).ok();
            match (&item.settings, old) {
                (Some(s), old) => Recipe {
                    schema: SCHEMA,
                    id: String::new(),
                    name,
                    created_at: 0.0,
                    updated_at: 0.0,
                    settings: s.clone(),
                    notes: vec![],
                    sensor: None,
                    source: None,
                    comment: old.and_then(|o| o.comment),
                },
                (None, Some(old)) if !item.set.is_empty() || item.comment.is_some() || item.notes.is_some() || item.sensor.is_some() || item.source.is_some() => old,
                (None, _) => return Err(Error::new(ErrorCode::BadRequest, "recipe: give item.settings, item.set on a saved recipe, or from: cat:<id>")),
            }
        };
        for (label, value) in &item.set {
            crate::recipes::set_field(&mut r.settings, label, value).map_err(|m| Error::new(ErrorCode::BadRequest, format!("recipe: {label}: {m}")))?;
        }
        if let Some(n) = item.notes {
            r.notes = n;
        }
        if item.sensor.is_some() {
            r.sensor = item.sensor;
        }
        if item.source.is_some() {
            r.source = item.source;
        }
        if item.comment.is_some() {
            r.comment = item.comment.filter(|c| !c.trim().is_empty());
        }
        self.save_recipe(actor, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipes_save_edit_search_and_trash() {
        let d = tempfile::tempdir().unwrap();
        let st = Store::new(d.path());
        let settings = Settings { film_simulation: Some("Classic Chrome".into()), highlight_tone: Some(-1.0), ..Default::default() };
        let s = st.save_recipe_input("test", Some("Street CC".into()), Some(serde_json::json!({ "settings": settings })), None).unwrap();
        assert_eq!((s.id.as_str(), s.replaced), ("recipe:street-cc", false));
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let e = st.save_recipe_input("test", Some("Street CC".into()), Some(serde_json::json!({ "set": { "Highlight": "+5", "Grain": "Strong, Large" } })), None).unwrap();
        assert!(e.replaced);
        assert_eq!(e.warnings.len(), 1, "highlight +5 does not fit a slot: {:?}", e.warnings);
        let r = st.recipe("street-cc").unwrap();
        assert_eq!((r.settings.highlight_tone, r.settings.grain_size.as_deref()), (Some(5.0), Some("Large")));
        assert_eq!(r.settings.film_simulation.as_deref(), Some("Classic Chrome"), "an edit keeps the rest");
        let cat = &bundle::catalog().recipes[0];
        let c = st.save_recipe_input("test", None, None, Some(&format!("cat:{}", cat.id))).unwrap();
        let copy = st.recipe(c.id.strip_prefix("recipe:").unwrap()).unwrap();
        assert_eq!(copy.settings, cat.settings);
        assert!(copy.source.unwrap().credit.is_some());
        let t = st.delete("test", "recipe:street-cc").unwrap();
        assert!(t.trash_id.unwrap().starts_with("trash:"));
        assert!(st.recipe("street-cc").is_err());
        assert!(st.trash().iter().any(|h| h.kind == "recipe"));
    }

    #[test]
    fn the_catalogue_searches_by_words_sensor_and_simulation() {
        let all = search_catalogue(&[], None, None);
        assert_eq!(all.len(), bundle::catalog().recipes.len());
        let cc = search_catalogue(&[], Some("x-trans v"), Some("classic chrome"));
        assert!(!cc.is_empty());
        assert!(cc.iter().all(|h| h.sensor.as_deref() == Some("X-Trans V") && h.summary.starts_with("Classic Chrome")));
        let first = &all[0];
        let words: Vec<String> = first.name.to_lowercase().split_whitespace().map(str::to_string).collect();
        assert!(search_catalogue(&words, None, None).iter().any(|h| h.id == first.id));
    }

    #[test]
    fn diffs_read_absent_settings_as_defaults() {
        let a = Settings { film_simulation: Some("Provia".into()), ..Default::default() };
        let b = Settings { film_simulation: Some("Provia".into()), clarity: Some(0.0), color: Some(2.0), ..Default::default() };
        let d = diff("a", &a, "b", &b);
        assert_eq!(d.changed.len(), 1);
        assert_eq!(d.changed[0].field, "color");
    }
}
