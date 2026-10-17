import { useEffect, useState } from "react";

export type Page = "hub" | "recipes" | "settings" | "lab" | "connect";

const PAGES: Page[] = ["hub", "recipes", "settings", "lab", "connect"];

export type Route = { page: Page; sub: string | null };

/** `#/settings/appearance` → { page: "settings", sub: "appearance" }. */
function parse(hash: string): Route {
  const [page, sub] = hash.replace(/^#\/?/, "").split("/");
  return PAGES.includes(page as Page) ? { page: page as Page, sub: sub || null } : { page: "hub", sub: null };
}

export const href = (page: Page, sub?: string) => `#/${page}${sub ? "/" + sub : ""}`;

export function useRoute(): Route {
  const [route, setRoute] = useState(() => parse(window.location.hash));
  useEffect(() => {
    const on = () => setRoute(parse(window.location.hash));
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return route;
}
