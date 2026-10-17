# The recipe catalogue

ukiyo bundles film simulation recipes from several recipe sites as presets: Recipes >
Library > Catalogue, and Import > Search the catalogue. Nothing is fetched at runtime.

## Credit

Each recipe is © its author and credits its first publisher, with a link to the post.

| Source | Author | Credit |
|---|---|---|
| [Fuji X Weekly](https://fujixweekly.com) | Ritchie Roesch | Recipes © Ritchie Roesch, Fuji X Weekly |
| [film.recipes](https://film.recipes) | Justin Gould | Recipes © Justin Gould, film.recipes |

ukiyo bundles only each recipe's settings, name, post title, date, author, sensor
generation and link. It bundles no photos, no post text, no comments and no author bios.

Henri-Pierre Chavaz's recipe database (a Google Sheet) is not used: his site's legal page
reserves all rights and forbids downloading or reusing its documents without written
permission.

## Files

- `catalog/recipes/sources/<id>.json` — one per source, the recipes as that site published
  them: `schema`, `id`, `name`, `homepage`, `credit`, `fetchedAt`, `recipes`.
- `catalog/recipes/catalog.json` — the merge the app loads: `schema`, `builtAt`,
  `sources`, `recipes`.

Both put one recipe per line, so a refresh diffs by recipe.

A source recipe:

| Field | Holds |
|---|---|
| `id` | The post's URL slug; `-2`, `-3` for further recipes in one post |
| `name`, `title`, `url`, `date`, `author` | Recipe name, post title, link, post date, author |
| `sensor` | "X-Trans V" ... "X-Trans I", "Bayer", "GFX", or absent |
| `video` | True for a video recipe |
| `settings` | The slot settings (below); a setting the post does not give is absent |
| `notes` | Settings a slot cannot hold: ISO, exposure compensation, Interframe NR, filters |
| `warnings` | Lines the parser could not use: unknown label, unreadable value, unlabelled line, duplicate |

A catalogue recipe has `id` (`<source>:<source id>` of its credit), `name`, `sensor`,
`video`, `settings`, `notes`, `warnings`, `hash`, `credit` and `alsoPublishedBy`. A credit
or later publication is `source`, `name`, `url`, `date` and `author`.

Settings use FilmKit's field names (`filmSimulation`, `dynamicRange`, `dRangePriority`,
`grainEffect`, `grainSize`, `colorChrome`, `colorChromeFxBlue`, `smoothSkin`,
`whiteBalance` with `colorTemp` for Kelvin, `wbShiftR`, `wbShiftB`, `highlightTone`,
`shadowTone`, `color`, `sharpness`, `highIsoNr`, `clarity`, `monochromaticColorWc`,
`monochromaticColorMg`). Values are what the camera menu shows (Highlight +1.5), not the
slot encoding; `docs/protocol.md` "Recipe slots" has that.

## De-duplication

The merge (`recipes::catalogue::merge`) normalises each recipe's settings: every absent
setting takes the camera default (Clarity 0, Grain Off, WB Auto, shifts 0), grain size
drops out when grain is off, and Kelvin reads as its number. It hashes the result
(FNV-1a, `hash`). Equal hashes are one recipe. The earliest-dated publication is the
credit; the others go under `alsoPublishedBy`. Video recipes merge only with video
recipes. A recipe with no film simulation is left out of `catalog.json` (it stays in its
source file).

Recipes whose settings differ in one field stay separate. The build summary lists them as
near-duplicates, with the field.

## Refresh

```bash
cd src-tauri
cargo run --example recipes_refresh                       # every source, then the merge
cargo run --example recipes_refresh -- --source filmrecipes
cargo run --example recipes_refresh -- --max-page 10
cargo run --example recipes_refresh -- --offline          # the caches only
cargo run --example recipes_refresh -- --build-only       # merge the source files only
```

The tool reads each site's WordPress feed (`/feed/?paged=N`, 10 posts a page) until it
ends, at least 2 s between requests, with a User-Agent that names ukiyo. It caches each page
in `local/recipe-feeds/<id>/`; delete that folder to read a feed again. A 429, a 5xx or
any other failure stops that source and leaves its file as it was. It prints, per source,
the counts and the most common warnings, then the merge counts and the near-duplicates.

## Adding a source

Add `src-tauri/src/recipes/sources/<id>.rs` with a `SOURCE`: its `SourceInfo`, its feed URL
per page, and a function from a feed item to recipes. Most adapters call `post_recipes`,
which finds the blocks and runs the shared parser. List it in `sources::all`. Sources earlier
in that list win credit ties on the same date.

## The parser

`src-tauri/src/recipes/` parses a recipe block, one `Label: value` line per setting. The
catalogue and the app's paste-text import (`recipe_parse`) use the same code.

- A block is a paragraph, or run of paragraphs, with "Film Simulation:" plus Dynamic
  Range, Grain or White Balance, or with three slot settings. A settings table (two cells a
  row) counts the same way. Older Fuji X Weekly posts open with the bare simulation name
  ("Classic Chrome") instead of the label. A block holding several recipes splits at each
  film simulation. Roundups and Patron early-access posts have no block.
- Labels vary: "Noise Reduction" and "ISO N.R." are High ISO NR, "Sharpening" is
  Sharpness, "Grain" is Grain Effect, "Col. Chr. Blue" is Color Chrome FX Blue. A label
  joining two settings ("Highlight & Shadow: +2") sets both. FilmKit's
  `parse-text-preset.ts` (MIT) was a reference for label variants; no code is taken from it.
- Tone values may carry a name, "+1 (Medium-Hard)". White balance is a preset or Kelvin,
  then the shift: "5200K, +2 Red & -4 Blue".
- Alternatives for other cameras ("Off (X-Trans V); Weak (X-Trans IV)") read the first
  value and warn about the rest. "N/A" leaves a setting unset with no warning.
- The sensor comes from the title ("X-Trans V", "Fifth-Generation"), then the tags, then
  the first camera model named. For film.recipes, it falls back to the first camera the
  post mentions.
