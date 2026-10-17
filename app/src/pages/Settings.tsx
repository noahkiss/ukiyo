import { useEffect, useMemo, useRef, useState } from "react";
import { menus as menuFixture, presets, searchWords, snapshots, type SettingRow } from "../fixtures/settings";
import { APP_ICONS, BRAND_MARKS, usePrefs, type ArtChoice, type ThemePref } from "../lib/prefs";
import { href } from "../lib/router";
import { useNotBuilt } from "../lib/toast";
import { CameraChip } from "../shell/CameraChip";
import { PageHead } from "../shell/PageHead";
import { CameraIcon, SearchIcon, StarIcon } from "../shell/icons";

const FAVOURITES = "favourites";
const APPEARANCE = "appearance";

type Hit = { row: SettingRow; menu: string; score: number };

export function Settings({ sub }: { sub: string | null }) {
  const notBuilt = useNotBuilt();
  const [menus, setMenus] = useState(menuFixture);
  const [query, setQuery] = useState("");
  const [focusRow, setFocusRow] = useState<string | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const section = sub ?? "af-mf";

  // ⌘K jumps to the search box.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (focusRow) document.getElementById(`row-${focusRow}`)?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [focusRow, section]);

  const allRows = useMemo(() => menus.flatMap((m) => m.rows.map((row) => ({ row, menu: m.id }))), [menus]);
  const favs = allRows.filter((r) => r.row.fav);

  const hits: Hit[] = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    return allRows
      .map(({ row, menu }) => {
        const hay = `${row.name} ${row.cam} ${searchWords[row.id] ?? ""}`.toLowerCase();
        return { row, menu, score: words.filter((w) => hay.includes(w)).length };
      })
      .filter((h) => h.score > 0)
      .sort((a, b) => b.score - a.score);
  }, [allRows, query]);
  const hitIds = new Set(hits.map((h) => h.row.id));

  const update = (id: string, patch: Partial<SettingRow>, write: boolean) => {
    setMenus((ms) => ms.map((m) => ({ ...m, rows: m.rows.map((r) => (r.id === id ? ({ ...r, ...patch } as SettingRow) : r)) })));
    if (write) notBuilt("Writing settings to the camera");
  };

  const menu = menus.find((m) => m.id === section);
  const listed = section === FAVOURITES ? favs.map((f) => f.row) : (menu?.rows ?? []);

  return (
    <div className="page settings" data-component="settings">
      <PageHead title="Settings" sub={section === APPEARANCE ? "How ukiyo looks on this Mac." : "The camera's menus. A change writes to the camera at once."}>
        <CameraChip />
        <button type="button" className="btn" onClick={() => notBuilt("Snapshots")}>
          <CameraIcon />
          Take snapshot
        </button>
      </PageHead>

      <div className="settings-search">
        <label className="search-box big">
          <SearchIcon size={20} />
          <span className="visually-hidden">Search settings</span>
          <input ref={searchRef} type="search" placeholder="Search by name or by what it does" value={query} onChange={(e) => setQuery(e.target.value)} />
          <kbd>⌘K</kbd>
        </label>
        {query.trim() && (
          <div className="hits">
            <span className="muted">{hits.length === 1 ? "1 match" : `${hits.length} matches`}</span>
            {hits.map((h, i) => (
              <a
                key={h.row.id}
                href={href("settings", h.menu)}
                className={i === 0 ? "hit-chip first" : "hit-chip"}
                onClick={() => setFocusRow(h.row.id)}
              >
                {h.row.name}
              </a>
            ))}
          </div>
        )}
      </div>

      <div className="flex-row">
        <nav className="menu-tree" aria-label="Camera menus">
          {menus.map((m) => (
            <div key={m.id} className="tree-group">
              <a href={href("settings", m.id)} className="tree-item" aria-current={section === m.id ? "page" : undefined} onClick={() => setFocusRow(null)}>
                <span>{m.label}</span>
                <span className="tree-count">{m.rows.length}</span>
              </a>
              {m.children?.map((c) => (
                <a key={c.id} href={href("settings", m.id)} className="tree-item child" onClick={() => setFocusRow(c.id)}>
                  {c.label}
                </a>
              ))}
            </div>
          ))}
          <a href={href("settings", FAVOURITES)} className="tree-item" aria-current={section === FAVOURITES ? "page" : undefined}>
            <span>Favourites</span>
            <span className="tree-count">{favs.length}</span>
          </a>
          <div className="tree-sep">This app</div>
          <a href={href("settings", APPEARANCE)} className="tree-item" aria-current={section === APPEARANCE ? "page" : undefined}>
            <span>Appearance</span>
          </a>
        </nav>

        {section === APPEARANCE ? (
          <Appearance />
        ) : (
          <section className="card settings-list" aria-labelledby="menu-title">
            <div className="settings-list-head">
              <h2 id="menu-title">{section === FAVOURITES ? "Favourites" : (menu?.label ?? "")}</h2>
              <span className="mono tiny muted">
                {section === FAVOURITES ? "" : `${menu?.cam} · ${menu?.rows.length}`}
              </span>
            </div>
            {listed.map((r) => (
              <SettingLine key={r.id} row={r} hit={hitIds.has(r.id) || focusRow === r.id} onChange={update} />
            ))}
            {listed.length === 0 && <p className="empty">No favourites yet. Star a setting to add it.</p>}
          </section>
        )}

        <aside className="settings-aside">
          <section className="card pad-s" aria-labelledby="presets">
            <div className="card-head">
              <h2 id="presets" className="card-title small-title">
                Presets
              </h2>
              <button type="button" className="btn text small" onClick={() => notBuilt("Presets")}>
                Save current
              </button>
            </div>
            <div className="rows">
              {presets.map((p) => (
                <div key={p.name} className="row-line">
                  <span className="stack">
                    <span className="strong">{p.name}</span>
                    <span className="tiny muted">{p.meta}</span>
                  </span>
                  <button type="button" className="btn small raised" onClick={() => notBuilt("Applying presets")}>
                    Apply
                  </button>
                </div>
              ))}
            </div>
          </section>

          <section className="card pad-s" aria-labelledby="favs">
            <h2 id="favs" className="card-title small-title">
              Favourites
            </h2>
            <div className="rows">
              {favs.map(({ row }) => (
                <label key={row.id} className="row-line small">
                  <span>{row.name}</span>
                  {row.kind === "select" ? (
                    <select className="select narrow" value={row.value} onChange={(e) => update(row.id, { value: e.target.value }, true)}>
                      {row.options.map((o) => (
                        <option key={o}>{o}</option>
                      ))}
                    </select>
                  ) : (
                    <button type="button" role="switch" className="toggle" aria-checked={row.value} onClick={() => update(row.id, { value: !row.value }, true)} />
                  )}
                </label>
              ))}
            </div>
          </section>

          <section className="card pad-s" aria-labelledby="snaps">
            <div className="card-head">
              <h2 id="snaps" className="card-title small-title">
                Snapshots
              </h2>
              <button type="button" className="btn text small" onClick={() => notBuilt("Comparing snapshots")}>
                Compare two
              </button>
            </div>
            <div className="rows">
              {snapshots.map((s) => (
                <div key={s.name} className="row-line small">
                  <span>{s.name}</span>
                  <span className="muted">{s.when}</span>
                </div>
              ))}
            </div>
          </section>
        </aside>
      </div>
    </div>
  );
}

function SettingLine({ row, hit, onChange }: { row: SettingRow; hit: boolean; onChange: (id: string, p: Partial<SettingRow>, write: boolean) => void }) {
  return (
    <div id={`row-${row.id}`} className={hit ? "setting hit" : "setting"}>
      <div className="setting-name">
        <span>{row.name}</span>
        <span className="mono tiny muted">{row.cam}</span>
      </div>
      {row.kind === "select" ? (
        <select className="select wide" aria-label={row.name} value={row.value} onChange={(e) => onChange(row.id, { value: e.target.value }, true)}>
          {row.options.map((o) => (
            <option key={o}>{o}</option>
          ))}
        </select>
      ) : (
        <button type="button" role="switch" className="toggle" aria-checked={row.value} aria-label={row.name} onClick={() => onChange(row.id, { value: !row.value }, true)} />
      )}
      <button type="button" className={row.fav ? "icon-btn star on" : "icon-btn star"} aria-pressed={!!row.fav} aria-label={`Favourite ${row.name}`} onClick={() => onChange(row.id, { fav: !row.fav }, false)}>
        <StarIcon filled={!!row.fav} />
      </button>
    </div>
  );
}

const THEMES: Array<[ThemePref, string]> = [
  ["system", "Match the system"],
  ["light", "Light"],
  ["dark", "Dark"],
];

function Appearance() {
  const { prefs, setPrefs } = usePrefs();
  return (
    <section className="card settings-list appearance" aria-labelledby="appearance-title" data-component="appearance">
      <div className="settings-list-head">
        <h2 id="appearance-title">Appearance</h2>
      </div>
      <div className="appearance-block">
        <h3 id="theme-label">Theme</h3>
        <div className="seg" role="group" aria-labelledby="theme-label">
          {THEMES.map(([t, label]) => (
            <button key={t} type="button" aria-pressed={prefs.theme === t} onClick={() => setPrefs({ theme: t })}>
              {label}
            </button>
          ))}
        </div>
      </div>
      <ArtPicker title="Sidebar brand mark" choices={BRAND_MARKS} value={prefs.brandMark} onPick={(id) => setPrefs({ brandMark: id })} tall />
      <ArtPicker title="App icon" note="The Dock icon does not change yet." choices={APP_ICONS} value={prefs.appIcon} onPick={(id) => setPrefs({ appIcon: id })} />
    </section>
  );
}

function ArtPicker({ title, note, choices, value, onPick, tall }: { title: string; note?: string; choices: ArtChoice[]; value: string; onPick: (id: string) => void; tall?: boolean }) {
  const id = title.toLowerCase().replace(/\s+/g, "-");
  return (
    <div className="appearance-block">
      <h3 id={id}>{title}</h3>
      {note && <p className="small muted">{note}</p>}
      <div className="art-choices" role="radiogroup" aria-labelledby={id}>
        {choices.map((c) => (
          <button key={c.id} type="button" role="radio" aria-checked={value === c.id} className="art-choice" onClick={() => onPick(c.id)}>
            <span className={tall ? "art-pair tall" : "art-pair"}>
              <span className="art-tile light">
                <img src={c.light} alt="" />
              </span>
              <span className="art-tile dark">
                <img src={c.dark} alt="" />
              </span>
            </span>
            <span className="art-name">
              <span className="radio-dot" aria-hidden="true" />
              {c.name}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}
