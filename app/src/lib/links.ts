import { openUrl } from "@tauri-apps/plugin-opener";
import { inTauri } from "./camera";

/** In the app, open `target="_blank"` links in the default browser. A plain browser follows them itself. */
export function openExternalLinks() {
  if (!inTauri) return;
  document.addEventListener("click", (e) => {
    const a = e.target instanceof Element ? e.target.closest<HTMLAnchorElement>('a[target="_blank"][href]') : null;
    if (!a) return;
    e.preventDefault();
    // The capability allows https on the catalogue's hosts; the normalised href matches its patterns.
    openUrl(new URL(a.href).href).catch((err) => console.warn("ukiyo: link not opened:", a.href, err));
  });
}
