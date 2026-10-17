# Fixtures

Sample data for the pages whose backend does not exist yet. Every export here is a stand-in:
swap it for a real command or event when that part of the backend lands, then delete it.

| File | Feeds | Real source, later |
|---|---|---|
| `hub.ts` | Hub: camera facts, shot counts and history, lenses, owner details, connect log | camera read on connect, a local connect history |
| `recipes.ts` | Recipes: slots C1-C7, the recipe library, the editor fields | RAW CONV. slot read and write, a library file |
| `fxw.ts` | Recipes > Import > Paste a link: what a pasted link parses to | fetching the post, then the Rust parser (task-fsd5pob8) |
| `settings.ts` | Settings: the menu tree, rows, presets, favourites, snapshots | catalog-driven camera settings |
| `preview.ts` | Browser preview only: a camera status for `pnpm dev` outside Tauri | none; inside the app the Lab's status is used |

No real serials, owner names or copyright strings: use "Your Name"-style placeholders.
