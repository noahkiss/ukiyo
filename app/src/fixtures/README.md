# Fixtures

Sample data for the pages whose backend does not exist yet. Every export here is a stand-in:
swap it for a real command or event when that part of the backend lands, then delete it.

| File | Feeds | Real source, later |
|---|---|---|
| `hub.ts` | Hub: camera facts, shot counts and history, lenses, owner details, connect log | camera read on connect, a local connect history |
| `recipes.ts` | Recipes: slots C1-C7, the recipe library, the editor fields | RAW CONV. slot read and write, a library file |
| `fxw.ts` | Recipes > Import: Fuji X Weekly search results; a pasted link or text parses to the first one | the RSS fetch and the recipe parser |
| `settings.ts` | Settings: the menu tree, rows, presets, favourites, snapshots | catalog-driven camera settings |
| `preview.ts` | Browser preview only: a camera status for `pnpm dev` outside Tauri; `?model=` picks the body | none; inside the app the Lab's status is used |

No real serials, owner names or copyright strings: use "Your Name"-style placeholders.
