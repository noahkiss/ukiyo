# Fixtures

Sample data for the pages whose backend does not exist yet. Every export here is a stand-in:
swap it for a real command or event when that part of the backend lands, then delete it.

| File | Feeds | Real source, later |
|---|---|---|
| `hub.ts` | Browser preview only: the Hub's camera facts, shot history, lenses, owner details, connect log | in the app: the camera's properties and `library_read history` |
| `recipes.ts` | Recipes: slots C1-C7, the recipe library, the editor fields | browser preview only; in the app: `library_read slots` and the library (`lib/recipes.ts`) |
| `fxw.ts` | Recipes > Import > Paste a link: what a pasted link parses to | browser preview only; in the app: `recipe_parse {url}` |
| `settings.ts` | Browser preview only: the Settings menu tree, rows, presets, favourites, snapshots | in the app: the catalog's menus over the camera's properties, and the library |
| `preview.ts` | Browser preview only: a camera status for `pnpm dev` outside Tauri; `?camera=` picks the state (`movie`, `menu`, `playback` show the camera notices), `?model=` the body | none; inside the app the Lab's status is used |

No real serials, owner names or copyright strings: use "Your Name"-style placeholders.
