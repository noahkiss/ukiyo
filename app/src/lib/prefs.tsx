import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import markPrimaryLight from "../assets/brand/mark-primary-light.svg";
import markPrimaryDark from "../assets/brand/mark-primary-dark.svg";
import markAlt1Light from "../assets/brand/mark-alt-1-light.svg";
import markAlt1Dark from "../assets/brand/mark-alt-1-dark.svg";
import markAlt2Light from "../assets/brand/mark-alt-2-light.svg";
import markAlt2Dark from "../assets/brand/mark-alt-2-dark.svg";
import iconPrimaryLight from "../assets/app-icon/icon-primary-light.svg";
import iconPrimaryDark from "../assets/app-icon/icon-primary-dark.svg";
import iconAlt1Light from "../assets/app-icon/icon-alt-1-light.svg";
import iconAlt1Dark from "../assets/app-icon/icon-alt-1-dark.svg";
import iconAlt2Light from "../assets/app-icon/icon-alt-2-light.svg";
import iconAlt2Dark from "../assets/app-icon/icon-alt-2-dark.svg";

export type ThemePref = "system" | "light" | "dark";
export type Theme = "light" | "dark";

/** One pickable image, with a version for each theme. */
export type ArtChoice = { id: string; name: string; light: string; dark: string };

// Placeholder entries until the real brand marks and app icons exist. Add a choice by
// adding its two images and a line here.
export const BRAND_MARKS: ArtChoice[] = [
  { id: "primary", name: "primary", light: markPrimaryLight, dark: markPrimaryDark },
  { id: "alt-1", name: "alt-1", light: markAlt1Light, dark: markAlt1Dark },
  { id: "alt-2", name: "alt-2", light: markAlt2Light, dark: markAlt2Dark },
];

export const APP_ICONS: ArtChoice[] = [
  { id: "primary", name: "primary", light: iconPrimaryLight, dark: iconPrimaryDark },
  { id: "alt-1", name: "alt-1", light: iconAlt1Light, dark: iconAlt1Dark },
  { id: "alt-2", name: "alt-2", light: iconAlt2Light, dark: iconAlt2Dark },
];

export type Prefs = { theme: ThemePref; brandMark: string; appIcon: string };

const DEFAULTS: Prefs = { theme: "system", brandMark: "primary", appIcon: "primary" };

// No settings command covers app preferences yet (settings.json holds only keep-awake), so
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
