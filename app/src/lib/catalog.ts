// The bundled recipe catalogue (catalog/recipes/catalog.json, merged from the per-source
// files by src-tauri/core/examples/recipes_refresh.rs), and the step from parsed settings to a
// Recipe. Settings and links only; each recipe credits its first publisher. The file is
// about 1 MB, so it loads in its own chunk the first time a page asks for it.
import { useEffect, useState } from "react";
import type { CatalogRecipe_Deserialize, Note, Popularity, Publication, RecipeCatalog_Deserialize, Settings_Deserialize, SourceInfo, Warning } from "../bindings";
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
  /** The settings as the core holds them, with the fields the editor does not show. */
  settings: Settings_Deserialize;
  tones: string[];
  popularity: Popularity;
};

const score = (c: { popularity: Popularity }) => c.popularity.score ?? 0;

/** One card in the library: a family of sensor versions, or a single recipe. */
export type CatalogCard = {
  id: string;
  title: string;
  /** Newest sensor first. */
  variants: CatalogEntry[];
  popularity: Popularity;
  /** In the top tenth by popularity score. */
  popular: boolean;
};

let sourcesById = new Map<string, SourceInfo>();

/** A source's display name: "Fuji X Weekly", "film.recipes". */
export const sourceName = (id: string) => sourcesById.get(id)?.name ?? id;

/** Who published it: the site, or for a compilation the creator and where it was found. */
export function publisher(p: Publication): string {
  const s = sourcesById.get(p.source);
  if (s?.compilation) return `${p.author ?? "an unknown creator"}, via ${s.name}`;
  return s?.name ?? p.source;
}

/** Camera models to their sensor generation, for picking a family's version. */
const MODEL_SENSOR: Record<string, string> = {
  "X-T5": "X-Trans V", "X-T50": "X-Trans V", "X-H2": "X-Trans V", "X-H2S": "X-Trans V", "X100VI": "X-Trans V", "X-E5": "X-Trans V", "X-M5": "X-Trans IV",
  "X-T4": "X-Trans IV", "X-T3": "X-Trans IV", "X-T30": "X-Trans IV", "X-T30 II": "X-Trans IV", "X-S10": "X-Trans IV", "X-S20": "X-Trans IV", "X-Pro3": "X-Trans IV", "X100V": "X-Trans IV", "X-E4": "X-Trans IV",
  "X-T2": "X-Trans III", "X-T20": "X-Trans III", "X-Pro2": "X-Trans III", "X-E3": "X-Trans III", "X-H1": "X-Trans III", "X100F": "X-Trans III",
  "X-T1": "X-Trans II", "X-T10": "X-Trans II", "X-E2": "X-Trans II", "X-E2S": "X-Trans II", "X100T": "X-Trans II", "X100S": "X-Trans II", "X70": "X-Trans II",
  "X-Pro1": "X-Trans I", "X-E1": "X-Trans I", "X-M1": "X-Trans I",
};

const RANK: Record<string, number> = { "X-Trans I": 1, "X-Trans II": 2, "X-Trans III": 3, "X-Trans IV": 4, "X-Trans V": 5 };

export const sensorOf = (model: string | null | undefined) => (model ? (MODEL_SENSOR[model.trim()] ?? null) : null);

/** The version to show for a camera's sensor: the exact one, else the newest the camera can
 * take, else the oldest. With no camera, the newest. */
export function pickVariant(card: CatalogCard, sensor: string | null): CatalogEntry {
  if (!sensor) return card.variants[0];
  const exact = card.variants.find((v) => v.sensor === sensor);
  if (exact) return exact;
  const rank = RANK[sensor] ?? 0;
  return card.variants.find((v) => (RANK[v.sensor ?? ""] ?? 0) <= rank) ?? card.variants[card.variants.length - 1];
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
  return { id: `cat:${r.id}`, name: r.name, credit: r.credit, also: r.alsoPublishedBy ?? [], sensor: r.sensor ?? null, video: r.video ?? false, parsed, settings: r.settings, tones: simTones(parsed.sim), popularity: r.popularity };
}

/** An entry as a library Recipe, for the editor. Not in the library until saved. The core's
 * settings ride along, so D-range priority, smooth skin and monochromatic colour survive a
 * save from the editor. */
export function entryRecipe(e: CatalogEntry): Recipe {
  const { notes, missing: _m, warnings: _w, ...rest } = e.parsed;
  return { ...rest, id: e.id, slot: null, mono: isMono(e.parsed.sim), tones: e.tones, notes, settings: e.settings };
}

const SENSOR_ORDER = ["X-Trans V", "X-Trans IV", "X-Trans III", "X-Trans II", "X-Trans I", "Bayer", "GFX"];

export type CatalogData = {
  catalog: RecipeCatalog_Deserialize;
  entries: CatalogEntry[];
  byId: Map<string, CatalogEntry>;
  /** Popular first. */
  cards: CatalogCard[];
  /** Sensors in the catalogue, newest first. */
  sensors: string[];
  /** Sources, as [id, name]. */
  sources: Array<[string, string]>;
  /** Film simulations, most used first. */
  sims: string[];
};

function build(catalog: RecipeCatalog_Deserialize): CatalogData {
  sourcesById = new Map(catalog.sources.map((s) => [s.id, s]));
  const entries = catalog.recipes.map(entry);
  const byId = new Map(entries.map((e) => [e.id, e]));
  const inFamily = new Set<string>();
  const cards: CatalogCard[] = [];
  for (const f of catalog.families ?? []) {
    const variants = f.variants.map((v) => byId.get(`cat:${v.recipe}`)).filter((e): e is CatalogEntry => !!e);
    if (variants.length === 0) continue;
    variants.forEach((v) => inFamily.add(v.id));
    cards.push({ id: f.id, title: f.title, variants, popularity: f.popularity, popular: false });
  }
  for (const e of entries) if (!inFamily.has(e.id)) cards.push({ id: e.id, title: e.name, variants: [e], popularity: e.popularity, popular: false });
  cards.sort((a, b) => score(b) - score(a));
  const cut = Math.ceil(cards.length / 10);
  cards.forEach((c, i) => (c.popular = i < cut && score(c) > 0));
  const sims = [...entries.reduce((m, e) => m.set(e.parsed.sim, (m.get(e.parsed.sim) ?? 0) + 1), new Map<string, number>())].sort((a, b) => b[1] - a[1]).map(([s]) => s);
  return {
    catalog,
    entries,
    byId,
    cards,
    sensors: SENSOR_ORDER.filter((s) => entries.some((e) => e.sensor === s)),
    sources: catalog.sources.map((s) => [s.id, s.name]),
    sims,
  };
}

let loading: Promise<CatalogData> | null = null;
let loaded: CatalogData | null = null;

export function loadCatalog(): Promise<CatalogData> {
  loading ??= import("../../../catalog/recipes/catalog.json?raw").then((m) => (loaded = build(JSON.parse(m.default) as RecipeCatalog_Deserialize)));
  return loading;
}

/** The catalogue, or null while it loads. */
export function useCatalog(): CatalogData | null {
  const [data, setData] = useState(loaded);
  useEffect(() => {
    if (!data) loadCatalog().then(setData, (e) => console.error("catalogue", e));
  }, [data]);
  return data;
}

export type CatalogFilter = { q: string; sensor: string; sim: string; source: string; show?: "all" | "colour" | "mono" };

function matches(e: CatalogEntry, f: CatalogFilter, words: string[]): boolean {
  if (f.sensor && e.sensor !== f.sensor) return false;
  if (f.sim && e.parsed.sim !== f.sim) return false;
  if (f.source && e.credit.source !== f.source && !e.also.some((p) => p.source === f.source)) return false;
  if (f.show === "mono" && !isMono(e.parsed.sim)) return false;
  if (f.show === "colour" && isMono(e.parsed.sim)) return false;
  const hay = `${e.name} ${e.also.map((p) => p.name).join(" ")} ${e.parsed.sim} ${e.sensor ?? ""}`.toLowerCase();
  return words.every((w) => hay.includes(w));
}

/** Name search plus sensor, film simulation and source filters. "" means any. A source
 * matches the credit or any later publication; a card matches when any version does. */
export function filterCards(cards: CatalogCard[], f: CatalogFilter): CatalogCard[] {
  const words = f.q.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return cards.filter((c) => c.variants.some((e) => matches(e, f, words)));
}

export type CatalogSort = "popular" | "newest" | "az";

export function sortCards(cards: CatalogCard[], by: CatalogSort): CatalogCard[] {
  const out = [...cards];
  if (by === "popular") out.sort((a, b) => score(b) - score(a));
  if (by === "newest") out.sort((a, b) => (b.variants[0].credit.date || "").localeCompare(a.variants[0].credit.date || ""));
  if (by === "az") out.sort((a, b) => a.title.localeCompare(b.title));
  return out;
}

export const fmtDate = (iso: string) => (iso ? new Date(iso + "T12:00:00").toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" }) : "date unknown");
