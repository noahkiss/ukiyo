# The recipe catalogue

ukiyo bundles film simulation recipes from several recipe sites as presets: Recipes >
Library > Catalogue, and Import > Search the catalogue. Nothing is fetched at runtime.

## Credit

Each recipe is © its creator and credits its first publisher, with a link to the post.

| Source | Creator | Credit |
|---|---|---|
| [Fuji X Weekly](https://fujixweekly.com) | Ritchie Roesch | Recipes © Ritchie Roesch, Fuji X Weekly |
| [film.recipes](https://film.recipes) | Justin Gould | Recipes © Justin Gould, film.recipes |
| [Fujifilm Film Simulation Recipes Database](https://docs.google.com/spreadsheets/d/11PvZ7rOnwmbz5VylVIvzOuWwEa83c0DweAlBwJ2pZJc) | Many; its Creator column | Recipes © their creators; compiled by Henri-Pierre Chavaz |

ukiyo bundles only each recipe's settings, name, post title, date, creator, sensor
generation and link. It bundles no photos, no post text, no comments and no author bios.

The database is a compilation (`compilation: true`): each of its recipes credits the
creator in its Creator column, shown as "From <creator>, via H-P Chavaz recipe database".
A row's link is the creator's original post when the sheet gives one, else the sheet.
The adapter upgrades an http link to https (every such site served https when checked,
2026-10-17) and fixes the sheet's `fujixweekly-com` host typo.

The app opens a recipe's Post and Source links in the default browser. The window may open
only https links on the catalogue's hosts: `src-tauri/capabilities/links.json` lists them, one
`https://<host>/*` pattern per host found in a `url` or `homepage` field. An http link, or a
link to any other host, does not open.

## Files

- `catalog/recipes/sources/<id>.json` — one per source, the recipes as that site published
  them: `schema`, `id`, `name`, `homepage`, `credit`, `fetchedAt`, `recipes`.
- `catalog/recipes/catalog.json` — the merge the app loads: `schema`, `builtAt`,
  `sources`, `families`, `recipes`. The app loads it in its own chunk on first use.
- `catalog/recipes/reddit.json` — Reddit thread counts for the top cards (Popularity).

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
| `variantOf` | For another sensor's version given in the same post: the first recipe's id |
| `comments`, `mentions`, `links` | The post's comment count, how many other read posts link to it, and the catalogued posts it links to |

A catalogue recipe has `id` (`<source>:<source id>` of its credit), `name`, `sensor`,
`video`, `settings`, `notes`, `warnings`, `hash`, `credit`, `alsoPublishedBy` and
`popularity`. A credit
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

A compilation's publication is the credit only when no other source has the recipe; the
creator's own post wins even when the sheet gives an earlier date. A compilation row that
links to a post another source read is folded into that post's closest recipe when it is at
most three fields apart (`LINK_MATCH_MAX_DIFF`): the sheet's transcription differs, or the
post was revised. The post's settings stand, and the row lists the fields it gives
differently (`differs`). An unknown date sorts last.

Recipes whose settings differ in one field stay separate. The build summary lists them as
near-duplicates, with the field.

## Families

A family is one recipe made for several sensors (`recipes::family`). The library shows one
card per family, with the version for the connected camera's sensor, and chips for the
others. A camera model maps to its sensor in `app/src/lib/catalog.ts`. With no camera, the
card shows the newest version; with no exact match, the newest version the camera can take.

Two recipes for different sensor generations join when their settings differ only in
sensor-dependent fields (Color Chrome FX Blue, Clarity, Color Chrome, Smooth Skin, D-Range
Priority, grain size; Highlight or Shadow by at most one step; a film simulation the older
sensor lacks, never colour against monochrome) and one of these holds:

- one post gave both: "Color Chrome FX Blue: Off (X-Trans V); Weak (X-Trans IV)" yields two
  recipes, the second with `variantOf`;
- the names match once sensor words are removed ("for X-Trans IV", camera models);
- one post links to the other.

Settings one field apart with different names and no link do not join: on the real data
that paired unrelated recipes. Bodies of one generation with fewer settings (X-T3 and X-T30
lack Color Chrome FX Blue and Clarity) are not told apart, so their versions stay separate
cards. `families` lists each family's `id`, `title`, `variants` (sensor and recipe id,
newest sensor first) and `popularity`; the recipes stay as they are.

## Popularity

No source publishes downloads or favourites, so ukiyo has no download count. The score
uses three signals:

| Signal | From | Coverage |
|---|---|---|
| `comments` | `slash:comments` in the cached feeds | Fuji X Weekly; film.recipes mostly 0; none for the database |
| `mentions` | Other posts in the cached Fuji X Weekly and film.recipes feeds that link to the recipe's post, roundups included | Posts on those two sites |
| `reddit` | One hn-app search per card ("<name> fujifilm recipe"), top 100 cards by the first two signals; threads that name the recipe, and their summed score | Top 100 only; a search returns at most 10 threads |

A post holding several recipes splits its comments and mentions evenly among them. A family
sums its versions. The score:

    score = ln(1 + comments) + 2 ln(1 + mentions) + 1.5 ln(1 + Reddit threads)
            + 0.5 ln(1 + Reddit score / 10)

Logs keep one heavily discussed post from drowning the rest; mentions weigh most because
each is someone choosing to point at the recipe. The library sorts by score by default
(Popular, then Newest and A–Z) and marks the top tenth of cards "Popular".

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
ends, or the database's Recipes tab as one CSV download, at least 2 s between requests, with a User-Agent that names ukiyo. It caches each page
in `local/recipe-feeds/<id>/`; delete that folder to read a feed again. A 429, a 5xx or
any other failure stops that source and leaves its file as it was. It prints, per source,
the counts and the most common warnings, then the merge counts and the near-duplicates.

A refresh that adds a link host fails `cargo test link_hosts`. Regenerate the capability with
`UKIYO_UPDATE_LINK_HOSTS=1 cargo test link_hosts` and review the new hosts.

## Adding a source

Add `src-tauri/core/src/recipes/sources/<id>.rs` with a `SOURCE`: its `SourceInfo` and how to
fetch it, `Fetch::Feed` (the feed URL per page, and a function from a feed item to recipes)
or `Fetch::Csv` (one URL, and a function from the file to recipes). Feed adapters call
`post_recipes`, which finds the blocks and runs the shared parser. List it in `sources::all`. Sources earlier
in that list win credit ties on the same date.

## The parser

`src-tauri/core/src/recipes/` parses a recipe block, one `Label: value` line per setting. The
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
- Bare values name their setting: "DR400", "5200K", "R+2 B-4". A shift may have its own
  line ("WB Shift: 1 Red, -9 Blue", "Shift: R:4 B:-5").
- The sensor comes from the title ("X-Trans V", "Fifth-Generation"), then the tags, then
  the first camera model named. For film.recipes, it falls back to the first camera the
  post mentions.
