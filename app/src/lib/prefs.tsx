import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { commands, type Prefs as FilePrefs, type PrefsPatch } from "../bindings";
import { inTauri } from "./camera";

import markStillWaterLight from "../assets/brand/mark-still-water-light.webp";
import markStillWaterDark from "../assets/brand/mark-still-water-dark.webp";
import markSunAndSealLight from "../assets/brand/mark-sun-and-seal-light.webp";
import markSunAndSealDark from "../assets/brand/mark-sun-and-seal-dark.webp";
import markFujiInMistLight from "../assets/brand/mark-fuji-in-mist-light.webp";
import markFujiInMistDark from "../assets/brand/mark-fuji-in-mist-dark.webp";
import markCloudsLight from "../assets/brand/mark-clouds-light.webp";
import markCloudsDark from "../assets/brand/mark-clouds-dark.webp";
import markFujiReflectionLight from "../assets/brand/mark-fuji-reflection-light.webp";
import markFujiReflectionDark from "../assets/brand/mark-fuji-reflection-dark.webp";
import markPineAndWaveLight from "../assets/brand/mark-pine-and-wave-light.webp";
import markPineAndWaveDark from "../assets/brand/mark-pine-and-wave-dark.webp";
import iconBrushLight from "../assets/app-icon/icon-brush-light.webp";
import iconBrushDark from "../assets/app-icon/icon-brush-dark.webp";
import iconSealLight from "../assets/app-icon/icon-seal-light.webp";
import iconSealDark from "../assets/app-icon/icon-seal-dark.webp";
import iconSunLight from "../assets/app-icon/icon-sun-light.webp";
import iconSunDark from "../assets/app-icon/icon-sun-dark.webp";
import iconApertureLight from "../assets/app-icon/icon-aperture-light.webp";
import iconApertureDark from "../assets/app-icon/icon-aperture-dark.webp";

export type ThemePref = "system" | "light" | "dark";
export type Theme = "light" | "dark";

/** One pickable image, with a version for each theme. */
export type ArtChoice = { id: string; name: string; light: string; dark: string };

// The first entry of each list is the default. Add a choice by adding its two images and a line
// here, and its id to `BRAND_MARKS` or `APP_ICONS` in `src-tauri/core/src/library/prefs.rs`.
export const BRAND_MARKS: ArtChoice[] = [
  { id: "still-water", name: "Still water", light: markStillWaterLight, dark: markStillWaterDark },
  { id: "sun-and-seal", name: "Sun and seal", light: markSunAndSealLight, dark: markSunAndSealDark },
  { id: "fuji-in-mist", name: "Fuji in mist", light: markFujiInMistLight, dark: markFujiInMistDark },
  { id: "clouds", name: "Clouds", light: markCloudsLight, dark: markCloudsDark },
  { id: "fuji-reflection", name: "Fuji reflection", light: markFujiReflectionLight, dark: markFujiReflectionDark },
  { id: "pine-and-wave", name: "Pine and wave", light: markPineAndWaveLight, dark: markPineAndWaveDark },
];

export const APP_ICONS: ArtChoice[] = [
  { id: "brush", name: "Brush 浮", light: iconBrushLight, dark: iconBrushDark },
  { id: "seal", name: "Seal", light: iconSealLight, dark: iconSealDark },
  { id: "sun", name: "Sun", light: iconSunLight, dark: iconSunDark },
  { id: "aperture", name: "Aperture", light: iconApertureLight, dark: iconApertureDark },
];

export type Prefs = { theme: ThemePref; brandMark: string; appIcon: string };

const DEFAULTS: Prefs = { theme: "system", brandMark: BRAND_MARKS[0].id, appIcon: APP_ICONS[0].id };

// In the app, the preferences live in `settings.json` (`prefs_get`, `prefs_set`), shared with
// the CLI and agents. Builds before that kept them in local storage under this key: the app
// imports them once, then removes the key. The browser preview still keeps them here.
const KEY = "ukiyo.appearance";

function loadLocal(): Partial<Prefs> | null {
  try {
    const v = localStorage.getItem(KEY);
    return v ? (JSON.parse(v) as Partial<Prefs>) : null;
  } catch {
    return null;
  }
}

function fromFile(p: FilePrefs): Prefs {
  return { theme: p.theme ?? DEFAULTS.theme, brandMark: p.brand_mark ?? DEFAULTS.brandMark, appIcon: p.app_icon ?? DEFAULTS.appIcon };
}

function toPatch(p: Partial<Prefs>): PrefsPatch {
  const known = (list: ArtChoice[], id: string | undefined) => (id && list.some((c) => c.id === id) ? id : undefined);
  const theme = p.theme && ["system", "light", "dark"].includes(p.theme) ? p.theme : undefined;
  return { theme, brand_mark: known(BRAND_MARKS, p.brandMark), app_icon: known(APP_ICONS, p.appIcon) };
}

/** Reads `settings.json`, importing old local-storage values first if there are any. */
async function loadFile(): Promise<Prefs | null> {
  const old = loadLocal();
  if (old) {
    const r = await commands.prefsSet(toPatch(old));
    if (r.status === "ok") {
      try {
        localStorage.removeItem(KEY);
      } catch {
        /* storage unavailable */
      }
      return fromFile(r.data);
    }
  }
  try {
    return fromFile(await commands.prefsGet());
  } catch {
    return null;
  }
}

type Ctx = {
  prefs: Prefs;
  theme: Theme;
  setPrefs: (p: Partial<Prefs>) => void;
};

const PrefsContext = createContext<Ctx | null>(null);

const darkQuery = () => window.matchMedia("(prefers-color-scheme: dark)");

export function PrefsProvider({ children }: { children: ReactNode }) {
  // Old local-storage values paint the first frame until the file answers.
  const [prefs, setState] = useState<Prefs>(() => ({ ...DEFAULTS, ...loadLocal() }));
  const [systemDark, setSystemDark] = useState(() => darkQuery().matches);

  useEffect(() => {
    const q = darkQuery();
    const on = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    q.addEventListener("change", on);
    return () => q.removeEventListener("change", on);
  }, []);

  // The CLI or an agent may change the file: read it again when the window gets focus.
  useEffect(() => {
    if (!inTauri) return;
    let cancelled = false;
    const read = () => void loadFile().then((p) => !cancelled && p && setState(p));
    read();
    window.addEventListener("focus", read);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", read);
    };
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    if (prefs.theme === "system") delete root.dataset.theme;
    else root.dataset.theme = prefs.theme;
  }, [prefs.theme]);

  const setPrefs = useCallback((p: Partial<Prefs>) => {
    setState((prev) => {
      const next = { ...prev, ...p };
      if (!inTauri) {
        try {
          localStorage.setItem(KEY, JSON.stringify(next));
        } catch {
          /* storage unavailable */
        }
      }
      return next;
    });
    if (inTauri) {
      void commands.prefsSet(toPatch(p)).then((r) => {
        if (r.status === "ok") setState(fromFile(r.data));
        else console.error("ukiyo: prefs_set:", r.error);
      });
    }
  }, []);

  const theme: Theme = prefs.theme === "system" ? (systemDark ? "dark" : "light") : prefs.theme;
  const value = useMemo(() => ({ prefs, theme, setPrefs }), [prefs, theme, setPrefs]);
  return <PrefsContext.Provider value={value}>{children}</PrefsContext.Provider>;
}

export function usePrefs(): Ctx {
  const c = useContext(PrefsContext);
  if (!c) throw new Error("usePrefs outside PrefsProvider");
  return c;
}

export function pick(list: ArtChoice[], id: string): ArtChoice {
  return list.find((c) => c.id === id) ?? list[0];
}
