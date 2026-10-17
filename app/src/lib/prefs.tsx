import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

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

// The first entry of each list is the default. Add a choice by adding its two images and a line here.
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

// No settings command covers app preferences yet (settings.json holds only the camera connection settings), so
// they live in local storage.
const KEY = "ukiyo.appearance";

function loadPrefs(): Prefs {
  try {
    const v = localStorage.getItem(KEY);
    return v ? { ...DEFAULTS, ...(JSON.parse(v) as Partial<Prefs>) } : DEFAULTS;
  } catch {
    return DEFAULTS;
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
  const [prefs, setState] = useState<Prefs>(loadPrefs);
  const [systemDark, setSystemDark] = useState(() => darkQuery().matches);

  useEffect(() => {
    const q = darkQuery();
    const on = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    q.addEventListener("change", on);
    return () => q.removeEventListener("change", on);
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    if (prefs.theme === "system") delete root.dataset.theme;
    else root.dataset.theme = prefs.theme;
  }, [prefs.theme]);

  const setPrefs = useCallback((p: Partial<Prefs>) => {
    setState((prev) => {
      const next = { ...prev, ...p };
      try {
        localStorage.setItem(KEY, JSON.stringify(next));
      } catch {
        /* storage unavailable */
      }
      return next;
    });
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
