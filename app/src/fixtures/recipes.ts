// FIXTURE: sample recipes and slot contents until RAW CONV. slot reads and a library file exist.

export type Slot = "C1" | "C2" | "C3" | "C4" | "C5" | "C6" | "C7";

export type Recipe = {
  id: string;
  name: string;
  sim: string;
  mono: boolean;
  /** The slot this recipe is in on the camera, if any. */
  slot: Slot | null;
  /** Five colours for the swatch strip. */
  tones: string[];
  wb: string;
  wbShift: { r: number; b: number };
  grain: "Off" | "Weak" | "Strong";
  grainSize: "Small" | "Large";
  colorChrome: "Off" | "Weak" | "Strong";
  colorChromeBlue: "Off" | "Weak" | "Strong";
  dr: "DR100" | "DR200" | "DR400";
  highlights: number;
  shadows: number;
  colour: number;
  sharpness: number;
  noiseReduction: number;
  clarity: number;
  /** Where an imported recipe came from. */
  source?: { label: string; url: string | null };
  /** Settings from the source that a slot cannot hold, such as ISO. */
  notes?: Array<[string, string]>;
};

export type SlotState = { slot: Slot; recipeId: string | null; state: "Matches" | "Edited" | "New"; name?: string; sim?: string; tones?: string[] };

const base = { wbShift: { r: 0, b: 0 }, grain: "Off", grainSize: "Small", colorChrome: "Off", colorChromeBlue: "Off", dr: "DR100", highlights: 0, shadows: 0, colour: 0, sharpness: 0, noiseReduction: 0, clarity: 0 } as const;

export const recipes: Recipe[] = [
  { ...base, id: "r1", name: "Harbour Morning", sim: "Classic Chrome", mono: false, slot: "C1", wb: "Auto", wbShift: { r: 1, b: -2 }, dr: "DR200", tones: ["#5E7380", "#A9B4B2", "#D8D2C3", "#3E4A50", "#8C6F55"] },
  { ...base, id: "r2", name: "Night Tram", sim: "Eterna", mono: false, slot: "C2", wb: "Colour temperature 3800K", wbShift: { r: 0, b: 1 }, dr: "DR400", tones: ["#1F2A33", "#3D5560", "#C79A52", "#6B7D78", "#0F1418"] },
  { ...base, id: "r3", name: "Late Persimmon", sim: "Classic Negative", mono: false, slot: "C3", wb: "Colour temperature 5200K", wbShift: { r: 2, b: -3 }, grain: "Weak", colorChrome: "Strong", colorChromeBlue: "Weak", dr: "DR400", highlights: -1, shadows: 1, colour: 2, sharpness: -1, noiseReduction: -4, tones: ["#B5532E", "#E0A36A", "#4E5A3C", "#F1E2C6", "#2B2620"] },
  { ...base, id: "r4", name: "Ink Rain", sim: "Acros + R filter", mono: true, slot: "C4", wb: "Auto", grain: "Strong", dr: "DR200", tones: ["#111111", "#3A3A3A", "#777777", "#B5B5B5", "#EDEDED"] },
  { ...base, id: "r5", name: "Paper Lantern", sim: "Nostalgic Neg.", mono: false, slot: "C5", wb: "Colour temperature 4600K", wbShift: { r: 3, b: -4 }, dr: "DR400", tones: ["#C2672E", "#F0C27A", "#7A3B23", "#E9D7B4", "#3A2A22"] },
  { ...base, id: "r6", name: "Neutral Calibration", sim: "Provia", mono: false, slot: "C6", wb: "Auto", tones: ["#808080", "#A0A0A0", "#606060", "#C0C0C0", "#404040"] },
  { ...base, id: "r7", name: "Grey Coast", sim: "Pro Neg. Std", mono: false, slot: null, wb: "Daylight", wbShift: { r: -1, b: 1 }, dr: "DR200", tones: ["#8A9599", "#C9CDC8", "#5D6A6E", "#E5E2DA", "#394246"] },
  { ...base, id: "r8", name: "Cedar Path", sim: "Velvia", mono: false, slot: null, wb: "Auto", wbShift: { r: 0, b: -1 }, tones: ["#2F5A2E", "#6E8F3A", "#C8B865", "#1C2E1A", "#8FA77A"] },
];

export const slots: SlotState[] = [
  { slot: "C1", recipeId: "r1", state: "Matches" },
  { slot: "C2", recipeId: "r2", state: "Matches" },
  { slot: "C3", recipeId: "r3", state: "Matches" },
  { slot: "C4", recipeId: "r4", state: "Matches" },
  { slot: "C5", recipeId: "r5", state: "Matches" },
  { slot: "C6", recipeId: "r6", state: "Matches" },
  { slot: "C7", recipeId: null, state: "New", name: "Unnamed recipe", sim: "Provia · not in library", tones: ["#7A8A96", "#B9B2A4", "#5A5048", "#D9D2C2", "#2E3236"] },
];

/** Film simulations the X-T5 offers. */
export const FILM_SIMS = ["Provia", "Velvia", "Astia", "Classic Chrome", "Pro Neg. Hi", "Pro Neg. Std", "Classic Negative", "Nostalgic Neg.", "Eterna", "Eterna Bleach Bypass", "Acros", "Acros + R filter", "Monochrome", "Sepia"];

export const WHITE_BALANCES = ["Auto", "Daylight", "Shade", "Colour temperature 3800K", "Colour temperature 4600K", "Colour temperature 5200K"];

/** The X-T5's range for each tone setting: min, max, step. */
export const TONE_RANGES = {
  highlights: [-2, 4, 0.5],
  shadows: [-2, 4, 0.5],
  colour: [-4, 4, 1],
  sharpness: [-4, 4, 1],
  noiseReduction: [-4, 4, 1],
  clarity: [-5, 5, 1],
} as const;

export const WB_SHIFT_RANGE = [-9, 9] as const;
