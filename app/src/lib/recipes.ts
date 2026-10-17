// The recipe library and the camera's slots, through `ukiyo serve`: library recipes
// (`recipe:<slug>`), slot reads (USB RAW CONV. mode) and the editor's Recipe shape.
import { commands, type Plan, type Recipe_Serialize, type Settings_Deserialize, type Settings_Serialize, type SlotView_Serialize } from "../bindings";
import type { Recipe, Slot } from "../fixtures/recipes";
import { isMono, simTones, toParsed } from "./catalog";

export const SLOTS: Slot[] = ["C1", "C2", "C3", "C4", "C5", "C6", "C7"];

const isSlot = (s: string): s is Slot => (SLOTS as string[]).includes(s);

/** Core settings as the editor's Recipe. The settings stay on it, so fields the editor does
 * not show (smooth skin, D-range priority, monochromatic colour) survive a save. */
export function settingsRecipe(id: string, name: string, s: Settings_Serialize, extra: Partial<Recipe> = {}): Recipe {
  const { notes, missing: _m, warnings: _w, ...rest } = toParsed(name, s, [], [], extra.source);
  return { ...rest, id, slot: null, mono: isMono(rest.sim), tones: simTones(rest.sim), notes, settings: s, ...extra };
}

export function libraryRecipe(r: Recipe_Serialize): Recipe {
  const source = r.source?.url || r.source?.credit ? { label: r.source?.credit ?? "Imported", url: r.source?.url ?? null } : undefined;
  return settingsRecipe(r.id, r.name, r.settings, { source, notes: (r.notes ?? []).map((n) => [n.label, n.value] as [string, string]) });
}

/** The editor's fields over the recipe's own settings. */
export function recipeSettings(r: Recipe): Settings_Serialize {
  const wb = /^Colour temperature (\d+)K$/.exec(r.wb);
  const base: Settings_Serialize = { ...(r.settings ?? {}) };
  return {
    ...base,
    filmSimulation: r.sim,
    whiteBalance: wb ? "Kelvin" : r.wb,
    colorTemp: wb ? Number(wb[1]) : null,
    wbShiftR: r.wbShift.r,
    wbShiftB: r.wbShift.b,
    grainEffect: r.grain,
    grainSize: r.grain === "Off" ? null : r.grainSize,
    colorChrome: r.colorChrome,
    colorChromeFxBlue: r.colorChromeBlue,
    dynamicRange: r.dr,
    highlightTone: r.highlights,
    shadowTone: r.shadows,
    color: r.colour,
    sharpness: r.sharpness,
    highIsoNr: r.noiseReduction,
    clarity: r.clarity,
  };
}

const err = <T,>(r: { status: "ok"; data: T } | { status: "error"; error: string }): T => {
  if (r.status === "error") throw new Error(r.error);
  return r.data;
};

/** Every library recipe, newest first. */
export async function loadLibrary(): Promise<Recipe[]> {
  const hits = err(await commands.librarySearch({ kind: "recipe", limit: 1000 }));
  const out: Recipe[] = [];
  for (const h of hits.items) {
    const r = await commands.libraryRead(h.id, null);
    if (r.status === "ok" && r.data.kind === "recipe") out.push(libraryRecipe(r.data.item));
  }
  return out;
}

/** Saves the editor's recipe under its name; returns the library id and any warnings. */
export async function saveRecipe(r: Recipe): Promise<{ id: string; warnings: string[] }> {
  const notes = (r.notes ?? []).map(([label, value]) => ({ label, value }));
  const source = r.source ? { url: r.source.url, credit: r.source.label, from: r.id.startsWith("cat:") ? r.id : null } : null;
  const s = err(await commands.recipeSave(r.name.trim(), { settings: recipeSettings(r) as Settings_Deserialize, set: null, notes, sensor: null, source, comment: null }, null));
  return { id: s.id, warnings: s.warnings };
}

export async function deleteRecipe(id: string): Promise<void> {
  err(await commands.libraryDelete(id));
}

export type SlotContent = { slot: Slot; view: SlotView_Serialize; recipe: Recipe };

/** Reads every slot from the camera (RAW CONV. mode). */
export async function readSlots(): Promise<SlotContent[]> {
  const r = err(await commands.libraryRead("slots", null));
  if (r.kind !== "slots") return [];
  return r.item
    .filter((v) => isSlot(v.slot))
    .map((v) => ({ slot: v.slot as Slot, view: v, recipe: settingsRecipe(v.id, v.name || "Unnamed", v.settings, { slot: v.slot as Slot }) }));
}

/** A plan that writes a recipe (`recipe:<slug>` or `cat:<id>`) to a slot. */
export async function slotPlan(recipeId: string, slot: Slot, allowUnconfirmed: boolean): Promise<Plan> {
  return err(await commands.planCreate({}, allowUnconfirmed, recipeId, slot));
}
