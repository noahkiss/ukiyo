# Recipes from Fuji X Weekly

ukiyo bundles the film simulation recipes published on [Fuji X Weekly](https://fujixweekly.com)
as presets in the Recipes library and in Import > Search Fuji X Weekly.

## Credit

The recipes are © Ritchie Roesch, Fuji X Weekly (fujixweekly.com). ukiyo bundles only each
recipe's settings, its name, post date, sensor generation and a link to its post. It bundles
no photos, no post text, no comments and no author bios. Every card and the import search
credit Fuji X Weekly and link to the post.

## The catalogue

`catalog/recipes/fujixweekly.json`, one recipe per line:

| Field | Holds |
|---|---|
| `schema` | Format version, now 1 |
| `fetchedAt` | The day the feed was read |
| `source` | Name, URL and credit line |
| `recipes[].id` | The post's URL slug; `-2`, `-3` for further recipes in one post |
| `recipes[].name`, `title`, `url`, `date` | Recipe name (from the title), post title, link, post date |
| `recipes[].sensor` | "X-Trans V" ... "X-Trans I", "Bayer", "GFX", or absent |
| `recipes[].video` | True for a video recipe |
| `recipes[].settings` | The slot settings (below); a setting the post does not give is absent |
| `recipes[].notes` | Settings a slot cannot hold: ISO, exposure compensation, Interframe NR |
| `recipes[].warnings` | Lines the parser could not use: unknown label, unreadable value, unlabelled line, duplicate |

Settings use FilmKit's field names (`filmSimulation`, `dynamicRange`, `grainEffect`,
`grainSize`, `colorChrome`, `colorChromeFxBlue`, `whiteBalance` with `colorTemp` for Kelvin,
`wbShiftR`, `wbShiftB`, `highlightTone`, `shadowTone`, `color`, `sharpness`, `highIsoNr`,
`clarity`, `monochromaticColorWc`, `monochromaticColorMg`). Values are what the camera menu
shows (Highlight +1.5), not the slot encoding; `docs/protocol.md` "Recipe slots" has that.

## Refresh

```bash
cd src-tauri
cargo run --example fxw_refresh             # the whole feed
cargo run --example fxw_refresh -- --max-page 10
cargo run --example fxw_refresh -- --offline  # the cache only
```

The tool reads the RSS feed (`/feed/?paged=N`, 10 posts a page; the REST API is closed)
until it ends, at least 2 s between requests, with a User-Agent that names ukiyo. It caches
each page in `local/fxw-feed/`; delete that folder to read the feed again. It prints the
counts and the most common warnings.

## The parser

`src-tauri/src/recipes/` parses a recipe block, one `Label: value` line per setting. The
bundled catalogue and the app's paste-text import (`recipe_parse`) use the same code.

- A block is a paragraph with "Film Simulation:" plus Dynamic Range, Grain or White Balance,
  or with three slot settings. Older posts open with the bare simulation name ("Classic
  Chrome") instead of the label. Roundups and Patron early-access posts have no block.
- Labels vary: "Noise Reduction" is High ISO NR, "Sharpening" is Sharpness, "Grain" is
  Grain Effect. Tone values may carry a name, "+1 (Medium-Hard)".
- White balance is a preset or Kelvin, then the shift: "5200K, +2 Red & -4 Blue".
- "N/A" leaves a setting unset with no warning: the camera lacks it.
- The sensor comes from the title ("X-Trans V", "Fifth-Generation"), then the tags, then the
  first camera model named.
