// The bundled recipe catalogue (catalog/recipes/catalog.json, merged from the per-source
// files by src-tauri/examples/recipes_refresh.rs), and the step from parsed settings to a
// Recipe. Settings and links only; each recipe credits its first publisher.
import raw from "../../../catalog/recipes/catalog.json?raw";
import type { CatalogRecipe_Deserialize, Note, Publication, RecipeCatalog_Deserialize, Settings_Deserialize, Warning } from "../bindings";
import type { Recipe } from "../fixtures/recipes";

/** A recipe as parsed from a post or pasted text: the slot fields, plus what a slot cannot hold. */
export type ParsedRecipe = Omit<Recipe, "id" | "slot" | "mono" | "tones"> & {
  /** Settings a post lists that are not part of a custom slot: ISO, exposure compensation. */
  notes: Array<[string, string]>;
  /** Slot fields the text does not give; they take the camera's default. */
  missing?: string[];
  /** Lines the parser could not use. */
  warnings?: Array<{ label: string; value: string; message: string }>;
};

export type CatalogEntry = {
  id: string;
  name: string;
  /** The first publication: the credit. */
  credit: Publication;
  /** Later publications of the same settings. */
  also: Publication[];
  sensor: string | null;
  video: boolean;
  parsed: ParsedRecipe;
  tones: string[];
};

export const catalog = JSON.parse(raw) as RecipeCatalog_Deserialize;

const sourcesById = new Map(catalog.sources.map((s) => [s.id, s]));

/** A source's display name: "Fuji X Weekly", "film.recipes". */
export const sourceName = (id: string) => sourcesById.get(id)?.name ?? id;

/** Who published it: the site, or for a compilation the creator and where it was found. */
export function publisher(p: Publication): string {
  const s = sourcesById.get(p.source);
  if (s?.compilation) return `${p.author ?? "an unknown creator"}, via ${s.name}`;
  return s?.name ?? p.source;
}

/** Swatch colours per film simulation family; the catalogue has no photos. */
const PALETTES: Array<[RegExp, string[]]> = [
  [/acros|monochrome/i, ["#121212", "#3B3B3B", "#777777", "#B7B7B7", "#EEEEEE"]],
  [/sepia/i, ["#2E2116", "#6B5238", "#A68B66", "#D6C3A1", "#F2E8D5"]],
  [/classic chrome/i, ["#5E7380", "#A9B4B2", "#D8D2C3", "#3E4A50", "#8C6F55"]],
  [/classic neg/i, ["#B5532E", "#E0A36A", "#4E5A3C", "#F1E2C6", "#2B2620"]],
  [/nostalgic/i, ["#C2672E", "#F0C27A", "#7A3B23", "#E9D7B4", "#3A2A22"]],
  [/bleach/i, ["#3A3D3B", "#7D8580", "#B8BDB4", "#5C625E", "#D9DBD3"]],
  [/eterna/i, ["#1F2A33", "#3D5560", "#C79A52", "#6B7D78", "#0F1418"]],
  [/velvia/i, ["#2F5A2E", "#6E8F3A", "#C8B865", "#1C2E1A", "#C0392B"]],
  [/astia/i, ["#D9A7A0", "#F2D6C9", "#8FA3B8", "#E8E0D0", "#5B6B7A"]],
  [/reala/i, ["#6F8C96", "#C9D2CC", "#3F5158", "#E8E2D4", "#A9845C"]],
  [/pro neg/i, ["#8A9599", "#C9CDC8", "#5D6A6E", "#E5E2DA", "#394246"]],
  [/provia/i, ["#3E6E9E", "#8FB3D0", "#D8D2C3", "#4E7A3C", "#B5643A"]],
];

export function simTones(sim: string): string[] {
  return PALETTES.find(([re]) => re.test(sim))?.[1] ?? ["#7A8A96", "#B9B2A4", "#5A5048", "#D9D2C2", "#2E3236"];
}

export const isMono = (sim: string) => /acros|monochrome|sepia/i.test(sim);

const oneOf = <T extends string>(v: string | null | undefined, allowed: readonly T[], fallback: T): T => (allowed.includes(v as T) ? (v as T) : fallback);

/** Parsed settings to the Recipe fields. Absent settings take the camera default and are listed in `missing`. */
export function toParsed(name: string, s: Settings_Deserialize, notes: Note[] = [], warnings: Warning[] = [], source: Recipe["source"]): ParsedRecipe {
  const missing: string[] = [];
  const need = <T,>(v: T | null | undefined, label: string, fallback: T): T => {
    if (v === null || v === undefined) {
      missing.push(label);
      return fallback;
    }
    return v;
  };
  const wbBase = need(s.whiteBalance, "White balance", "Auto");
  const wb = wbBase === "Kelvin" ? `Colour temperature ${s.colorTemp ?? 5500}K` : wbBase;
  const strengths = ["Off", "Weak", "Strong"] as const;
  return {
    name,
    sim: need(s.filmSimulation, "Film simulation", "Provia"),
    wb,
    wbShift: { r: s.wbShiftR ?? 0, b: s.wbShiftB ?? 0 },
    grain: oneOf(need(s.grainEffect, "Grain", "Off"), strengths, "Off"),
    grainSize: oneOf(s.grainSize, ["Small", "Large"] as const, "Small"),
    colorChrome: oneOf(need(s.colorChrome, "Color Chrome effect", "Off"), strengths, "Off"),
    colorChromeBlue: oneOf(need(s.colorChromeFxBlue, "Color Chrome FX Blue", "Off"), strengths, "Off"),
    dr: oneOf(need(s.dynamicRange, "Dynamic range", "DR100"), ["DR100", "DR200", "DR400", "Auto"] as const, "DR100"),
    highlights: need(s.highlightTone, "Highlights", 0),
    shadows: need(s.shadowTone, "Shadows", 0),
    colour: need(s.color, "Colour", 0),
    sharpness: need(s.sharpness, "Sharpness", 0),
    noiseReduction: need(s.highIsoNr, "Noise reduction", 0),
    clarity: need(s.clarity, "Clarity", 0),
    source,
    notes: notes.map((n) => [n.label, n.value]),
    missing,
    warnings: warnings.map((w) => ({ label: w.label, value: w.value, message: w.message })),
  };
}

function entry(r: CatalogRecipe_Deserialize): CatalogEntry {
  const parsed = toParsed(r.name, r.settings, r.notes ?? [], r.warnings ?? [], { label: publisher(r.credit), url: r.credit.url });
  return { id: `cat:${r.id}`, name: r.name, credit: r.credit, also: r.alsoPublishedBy ?? [], sensor: r.sensor ?? null, video: r.video ?? false, parsed, tones: simTones(parsed.sim) };
}

export const catalogEntries: CatalogEntry[] = catalog.recipes.map(entry);

/** An entry as a library Recipe, for the editor. Not in the library until saved. */
export function entryRecipe(e: CatalogEntry): Recipe {
  const { notes, missing: _m, warnings: _w, ...rest } = e.parsed;
  return { ...rest, id: e.id, slot: null, mono: isMono(e.parsed.sim), tones: e.tones, notes };
}

const SENSOR_ORDER = ["X-Trans V", "X-Trans IV", "X-Trans III", "X-Trans II", "X-Trans I", "Bayer", "GFX"];

/** Sensors in the catalogue, newest first. */
export const catalogSensors = SENSOR_ORDER.filter((s) => catalogEntries.some((e) => e.sensor === s));

/** Sources in the catalogue, as [id, name]. */
export const catalogSources: Array<[string, string]> = catalog.sources.map((s) => [s.id, s.name]);

/** Film simulations in the catalogue, most used first. */
export const catalogSims = [...catalogEntries.reduce((m, e) => m.set(e.parsed.sim, (m.get(e.parsed.sim) ?? 0) + 1), new Map<string, number>())].sort((a, b) => b[1] - a[1]).map(([s]) => s);

export type CatalogFilter = { q: string; sensor: string; sim: string; source: string; show?: "all" | "colour" | "mono" };

/** Name search plus sensor, film simulation and source filters. "" means any. A source
 * matches the credit or any later publication. */
export function filterEntries(entries: CatalogEntry[], f: CatalogFilter): CatalogEntry[] {
  const words = f.q.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return entries.filter((e) => {
    if (f.sensor && e.sensor !== f.sensor) return false;
    if (f.sim && e.parsed.sim !== f.sim) return false;
    if (f.source && e.credit.source !== f.source && !e.also.some((p) => p.source === f.source)) return false;
    if (f.show === "mono" && !isMono(e.parsed.sim)) return false;
    if (f.show === "colour" && isMono(e.parsed.sim)) return false;
    const hay = `${e.name} ${e.also.map((p) => p.name).join(" ")} ${e.parsed.sim} ${e.sensor ?? ""}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

export const fmtDate = (iso: string) => (iso ? new Date(iso + "T12:00:00").toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" }) : "date unknown");
