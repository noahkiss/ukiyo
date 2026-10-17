// FIXTURE: Fuji X Weekly search results and parsed recipes, until the RSS fetch and the
// recipe parser exist. Titles are made up; links go to the site's own search for the title.
import type { Recipe } from "./recipes";

/** A recipe as parsed from a post: the slot fields, plus what a slot cannot hold. */
export type ParsedRecipe = Omit<Recipe, "id" | "slot" | "mono" | "tones"> & {
  /** Settings a post lists that are not part of a custom slot: ISO, exposure compensation. */
  notes: Array<[string, string]>;
};

export type FxwResult = {
  id: string;
  title: string;
  date: string;
  /** The sensor generation the post targets. */
  sensor: string;
  url: string;
  parsed: ParsedRecipe;
  tones: string[];
};

const search = (title: string) => `https://fujixweekly.com/?s=${encodeURIComponent(title)}`;

const base = { wbShift: { r: 0, b: 0 }, grain: "Off", grainSize: "Small", colorChrome: "Off", colorChromeBlue: "Off", dr: "DR100", highlights: 0, shadows: 0, colour: 0, sharpness: 0, noiseReduction: 0, clarity: 0 } as const;

export const fxwResults: FxwResult[] = [
  {
    id: "f1",
    title: "Autumn Tram",
    date: "2026-10-09",
    sensor: "X-Trans V",
    url: search("Autumn Tram"),
    tones: ["#9A4B2A", "#D69A5C", "#56603F", "#EADBBE", "#2C2723"],
    parsed: { ...base, name: "Autumn Tram", sim: "Classic Negative", wb: "Colour temperature 5200K", wbShift: { r: 2, b: -4 }, grain: "Weak", colorChrome: "Strong", colorChromeBlue: "Weak", dr: "DR400", highlights: -1, shadows: 1.5, colour: 3, sharpness: -1, noiseReduction: -4, clarity: -2, notes: [["ISO", "Auto, up to 6400"], ["Exposure compensation", "+1/3 to +1"]] },
  },
  {
    id: "f2",
    title: "Coastal Ace",
    date: "2026-09-27",
    sensor: "X-Trans V",
    url: search("Coastal Ace"),
    tones: ["#6F8C96", "#C9D2CC", "#3F5158", "#E8E2D4", "#A9845C"],
    parsed: { ...base, name: "Coastal Ace", sim: "Reala Ace", wb: "Daylight", wbShift: { r: 1, b: -2 }, dr: "DR200", highlights: 0, shadows: -0.5, colour: 1, sharpness: 0, noiseReduction: -4, notes: [["ISO", "Auto, up to 3200"], ["Exposure compensation", "0"]] },
  },
  {
    id: "f3",
    title: "Night Market Film",
    date: "2026-09-12",
    sensor: "X-Trans IV",
    url: search("Night Market Film"),
    tones: ["#1E2730", "#C28B4A", "#4C6670", "#E2B87A", "#0E1215"],
    parsed: { ...base, name: "Night Market Film", sim: "Eterna", wb: "Colour temperature 3800K", wbShift: { r: 0, b: 2 }, grain: "Strong", grainSize: "Large", dr: "DR400", highlights: 4.5, shadows: 2, colour: -1, sharpness: -2, noiseReduction: -4, notes: [["ISO", "Auto, up to 12800"], ["Exposure compensation", "-1/3"]] },
  },
  {
    id: "f4",
    title: "Silver Rain",
    date: "2026-08-30",
    sensor: "X-Trans V",
    url: search("Silver Rain"),
    tones: ["#121212", "#3B3B3B", "#7A7A7A", "#B7B7B7", "#EEEEEE"],
    parsed: { ...base, name: "Silver Rain", sim: "Acros + R filter", wb: "Auto", grain: "Strong", dr: "DR200", highlights: 1, shadows: 2, sharpness: 1, noiseReduction: -4, notes: [["ISO", "Auto, up to 12800"], ["Exposure compensation", "+1/3"]] },
  },
  {
    id: "f5",
    title: "Faded Summer Print",
    date: "2026-08-14",
    sensor: "X-Trans III",
    url: search("Faded Summer Print"),
    tones: ["#C9925F", "#EFD2A2", "#7D5A3C", "#F4E9D2", "#4A3B2E"],
    parsed: { ...base, name: "Faded Summer Print", sim: "Classic Chrome", wb: "Shade", wbShift: { r: 3, b: -5 }, dr: "DR200", highlights: -1, shadows: -1, colour: 2, sharpness: -1, noiseReduction: -3, notes: [["ISO", "Auto, up to 6400"], ["Exposure compensation", "+2/3"]] },
  },
];

/** What a pasted link or pasted text parses to, for now: the first result. */
export const fxwPasteResult = fxwResults[0];
