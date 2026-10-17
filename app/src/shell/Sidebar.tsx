import { cameraLines, useCamera } from "../lib/camera";
import { BRAND_MARKS, pick, usePrefs } from "../lib/prefs";
import { href, type Page } from "../lib/router";
import { HubIcon, LabIcon, RecipesIcon, SettingsIcon } from "./icons";

const NAV: Array<[Page, string, () => React.JSX.Element]> = [
  ["hub", "Hub", HubIcon],
  ["recipes", "Recipes", RecipesIcon],
  ["settings", "Settings", SettingsIcon],
  ["lab", "Lab", LabIcon],
];

export function Sidebar({ page }: { page: Page }) {
  const { status } = useCamera();
  const { prefs, theme } = usePrefs();
  const mark = pick(BRAND_MARKS, prefs.brandMark);
  const lines = cameraLines(status);
  return (
    <nav className="sidebar" aria-label="Main" data-component="sidebar">
      <img className="brand-mark" src={mark[theme]} alt="" width={168} height={184} />
      <div className="nav-list">
        {NAV.map(([p, label, Icon]) => (
          <a key={p} href={href(p)} className="nav-item" aria-current={p === page ? "page" : undefined}>
            <Icon />
            {label}
          </a>
        ))}
      </div>
      <div className="grow" />
      <svg className="sidebar-art" aria-hidden="true" width="200" height="56" viewBox="0 0 200 56" fill="none" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
        <path d="M30 40 L78 8 L92 18 L100 12 L148 40" />
        <path d="M70 14 l4 4 4-4 4 4" />
        <path d="M4 48 q8 -8 16 0 t16 0 t16 0 t16 0 t16 0 t16 0 t16 0 t16 0 t16 0 t16 0 t16 0 t16 0" />
        <path d="M12 54 q8 -6 16 0 t16 0 t16 0 t16 0 t16 0 t16 0 t16 0 t16 0 t16 0 t16 0 t16 0" />
        <circle className="sun" cx="164" cy="16" r="7" />
      </svg>
      <a href={href("connect")} className="camera-card" aria-current={page === "connect" ? "page" : undefined} data-component="camera-card">
        <span className={lines.ok ? "dot big ok" : "dot big"} />
        <span className="camera-card-text">
          <span className="camera-card-name">{lines.camera}</span>
          <span className="camera-card-detail">{lines.detail}</span>
        </span>
      </a>
    </nav>
  );
}
