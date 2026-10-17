// FIXTURE: what a pasted Fuji X Weekly link parses to, until the app can fetch a post.
// TODO(task-fsd5pob8): fetch the post and run its blocks through the Rust parser
// (recipes::sources::fujixweekly), as the catalogue refresh does. The title is made up; the
// link goes to the site's own search for it.
import type { CatalogEntry } from "../lib/catalog";

const base = { wbShift: { r: 0, b: 0 }, grain: "Off", grainSize: "Small", colorChrome: "Off", colorChromeBlue: "Off", dr: "DR100", highlights: 0, shadows: 0, colour: 0, sharpness: 0, noiseReduction: 0, clarity: 0 } as const;

export const fxwPasteResult: CatalogEntry = {
  id: "f1",
  name: "Autumn Tram",
  credit: { source: "fujixweekly", name: "Autumn Tram", date: "2026-10-09", url: `https://fujixweekly.com/?s=${encodeURIComponent("Autumn Tram")}` },
  also: [],
  sensor: "X-Trans V",
  video: false,
  tones: ["#9A4B2A", "#D69A5C", "#56603F", "#EADBBE", "#2C2723"],
  parsed: { ...base, name: "Autumn Tram", sim: "Classic Negative", wb: "Colour temperature 5200K", wbShift: { r: 2, b: -4 }, grain: "Weak", colorChrome: "Strong", colorChromeBlue: "Weak", dr: "DR400", highlights: -1, shadows: 1.5, colour: 3, sharpness: -1, noiseReduction: -4, clarity: -2, notes: [["ISO", "Auto, up to 6400"], ["Exposure compensation", "+1/3 to +1"]] },
};
