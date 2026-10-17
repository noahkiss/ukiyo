//! The bundled recipe catalogue (`catalog/recipes/catalog.json`), compiled into the core so
//! the CLI, the MCP server and `serve` search it with no file to find.

use super::catalogue::{CatalogRecipe, RecipeCatalog};
use std::sync::LazyLock;

const TEXT: &str = include_str!("../../../../catalog/recipes/catalog.json");

static CATALOG: LazyLock<RecipeCatalog> = LazyLock::new(|| serde_json::from_str(TEXT).expect("catalog/recipes/catalog.json parses"));

pub fn catalog() -> &'static RecipeCatalog {
    &CATALOG
}

/// A catalogue recipe by its id, with or without `cat:`.
pub fn recipe(id: &str) -> Option<&'static CatalogRecipe> {
    let id = id.strip_prefix("cat:").unwrap_or(id);
    CATALOG.recipes.iter().find(|r| r.id == id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_bundled_catalogue_loads() {
        let c = super::catalog();
        assert!(c.recipes.len() > 100);
        let first = &c.recipes[0];
        assert_eq!(super::recipe(&format!("cat:{}", first.id)).map(|r| &r.id), Some(&first.id));
    }
}
