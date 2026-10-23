import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { commands, type Diff, type Hit as LibHit, type Plan, type PropRow, type UpdateStatus, type Value } from "../bindings";
import { inTauri, useCamera } from "../lib/camera";
import { display, entry, hex, type Catalog } from "../lib/decode";
import { useLive, rawOf } from "../lib/live";
import { makePlan, writeNow } from "../lib/plans";
import { PlanCard } from "../views/PlanCard";
import { menus as menuFixture, presets as presetFixture, searchWords, snapshots as snapshotFixture, type Menu, type SettingRow } from "../fixtures/settings";
import { APP_ICONS, BRAND_MARKS, usePrefs, type ArtChoice, type ThemePref } from "../lib/prefs";
import { href } from "../lib/router";
import { useNotBuilt, useToast } from "../lib/toast";
import { CameraChip } from "../shell/CameraChip";
import { ModeNotice } from "../shell/ModeNotice";
import { PageHead } from "../shell/PageHead";
import { CameraIcon, CloseIcon, SearchIcon, StarIcon } from "../shell/icons";

const FAVOURITES = "favourites";
const APPEARANCE = "appearance";
const CONNECTION = "connection";

type Hit = { row: SettingRow; menu: string; score: number };

/** A row on the page. From the camera it carries its code, the raw value of each option, and
 * the words a search matches. */
type Line = SettingRow & { code?: number; raws?: Record<string, string>; words?: string; readOnly?: boolean; alt?: string };
type Section = Omit<Menu, "rows"> & { rows: Line[] };
/** A preset or snapshot in the aside. */
type Item = { id: string; name: string; meta: string };

// ---- the camera's settings, from the catalog ----

/** `IMAGE QUALITY SETTING` as `Image quality setting`; `AutoPowerOff` as `Auto power off`. */
function sentence(s: string): string {
  const spaced = /[a-z]/.test(s) ? s.replace(/([a-z0-9])([A-Z])/g, "$1 $2") : s;
  const lower = spaced
    .toLowerCase()
    .split(" ")
    .map((w) => (ACRONYMS.has(w) || (w.includes("/") && w.length <= 5) ? w.toUpperCase() : w))
    .join(" ");
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}

const ACRONYMS = new Set(["af", "mf", "iso", "nr", "raw", "usb", "dr", "wb", "is", "hdr", "lcd", "evf", "ae", "fps"]);

const slugOf = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");

/** The camera's menu order; any other menu follows in name order, then the dials. */
const ORDER = ["IMAGE QUALITY SETTING", "AF/MF SETTING", "SHOOTING SETTING", "FLASH SETTING", "MOVIE SETTING", "SET UP"];
const DIALS = "Dials and switches";

/** The values to offer: an enum's, or a short range's steps. */
function choices(r: PropRow): Value[] {
  if (r.form.form === "enum") return r.form.values;
  if (r.form.form === "range" && r.form.min.kind === "int" && r.form.max.kind === "int" && r.form.step.kind === "int" && r.form.step.v > 0) {
    const { v: min } = r.form.min;
    const { v: max } = r.form.max;
    const { v: step } = r.form.step;
    if ((max - min) / step <= 60) return Array.from({ length: Math.floor((max - min) / step) + 1 }, (_, i) => ({ kind: "int", v: min + i * step }) as Value);
  }
  return [];
}

function lineOf(r: PropRow, cat: Catalog | null, favs: Set<number>): Line {
  const e = entry(cat, r.code);
  const parts = (e?.menu ?? "").split(">").map((p) => p.trim()).filter(Boolean);
  const leaf = parts.length > 1 ? parts[parts.length - 1] : null;
  const name = leaf ? sentence(leaf) : e?.name ? sentence(e.name) : hex(r.code);
  const value = display(e, r.value);
  const raws: Record<string, string> = {};
  for (const v of choices(r)) {
    const raw = rawOf(v);
    if (raw != null) raws[display(e, v)] = raw;
  }
  const options = Object.keys(raws);
  if (!options.includes(value)) options.unshift(value);
  const readOnly = !r.writable || !r.available || Object.keys(raws).length === 0;
  return {
    id: hex(r.code),
    code: r.code,
    name,
    cam: parts.length > 1 ? parts.slice(1).join(" > ") : (e?.menu ?? e?.name ?? ""),
    kind: "select",
    value,
    options,
    fav: favs.has(r.code),
    raws,
    readOnly,
    alt: e?.name ? sentence(e.name) : undefined,
    words: `${e?.name ? sentence(e.name) : ""} ${e?.menu ?? ""} ${e?.notes ?? ""}`,
  };
}

/** Every setting the catalog places in a menu, grouped by its top menu. */
function sectionsOf(rows: Map<number, PropRow>, cat: Catalog | null, favs: Set<number>): Section[] {
  const by = new Map<string, { cam: string; label: string; lines: Line[]; subs: Map<string, string> }>();
  for (const r of [...rows.values()].sort((a, b) => a.code - b.code)) {
    const menu = entry(cat, r.code)?.menu;
    if (!menu) continue;
    const parts = menu.split(">").map((p) => p.trim());
    const isMenu = parts.length > 1 && parts[0] === parts[0].toUpperCase();
    const top = isMenu ? parts[0] : DIALS;
    const g = by.get(top) ?? { cam: isMenu ? top : "", label: isMenu ? sentence(top.replace(/ SETTINGS?$/, "")) : DIALS, lines: [] as Line[], subs: new Map<string, string>() };
    const line = lineOf(r, cat, favs);
    g.lines.push(line);
    if (isMenu && parts.length > 2 && !g.subs.has(parts[1])) g.subs.set(parts[1], line.id);
    by.set(top, g);
  }
  // Codes that share a menu item (the white balance shifts) go by their catalog names.
  for (const g of by.values()) {
    const seen = new Map<string, number>();
    for (const l of g.lines) seen.set(l.name, (seen.get(l.name) ?? 0) + 1);
    for (const l of g.lines) if ((seen.get(l.name) ?? 0) > 1 && l.alt) l.name = l.alt;
  }
  const rank = (t: string) => (t === DIALS ? 1000 : ORDER.includes(t) ? ORDER.indexOf(t) : 100);
  return [...by.entries()]
    .sort(([a], [b]) => rank(a) - rank(b) || a.localeCompare(b))
    .map(([top, g]) => ({
      id: slugOf(top),
      label: g.label,
      cam: g.cam,
      children: g.subs.size > 1 ? [...g.subs.entries()].map(([label, id]) => ({ id, label: sentence(label) })) : undefined,
      rows: g.lines,
    }));
}

// ---- the library: presets, snapshots, favourites ----

const fmtWhen = (ms: number | null) => {
  if (ms == null) return "";
  const d = new Date(ms);
  return d.toDateString() === new Date().toDateString()
    ? `Today ${d.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" })}`
    : d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
};

function useLibrary() {
  const [presets, setPresets] = useState<Item[]>([]);
  const [snapshots, setSnapshots] = useState<Item[]>([]);
  const [favs, setFavs] = useState<Set<number>>(new Set());
  const reload = useCallback(async () => {
    const items = (h: LibHit[], meta: (h: LibHit) => string) => h.map((x) => ({ id: x.id, name: x.name, meta: meta(x) }));
    const [p, s, f] = await Promise.all([
      commands.librarySearch({ kind: "preset", limit: 200 }),
      commands.librarySearch({ kind: "snapshot", limit: 200 }),
      commands.libraryRead("favourites", null),
    ]);
    if (p.status === "ok") setPresets(items(p.data.items, (x) => x.summary));
    if (s.status === "ok") setSnapshots(items(s.data.items, (x) => fmtWhen(x.at)));
    if (f.status === "ok" && f.data.kind === "favourites") setFavs(new Set((f.data.item.items ?? []).map((x) => parseInt(x.code, 16))));
  }, []);
  useEffect(() => {
    if (inTauri) void reload();
  }, [reload]);
  return { presets, snapshots, favs, setFavs, reload };
}

// ---- the page ----

export function Settings({ sub }: { sub: string | null }) {
  const notBuilt = useNotBuilt();
  const flash = useToast();
  const live = useLive();
  const lib = useLibrary();
  const [fixture, setFixture] = useState<Section[]>(menuFixture);
  const [query, setQuery] = useState("");
  const [focusRow, setFocusRow] = useState<string | null>(null);
  const [pending, setPending] = useState<{ title: string; plan: Plan } | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const menus: Section[] = useMemo(() => (inTauri ? sectionsOf(live.rows, live.catalog, lib.favs) : fixture), [live.rows, live.catalog, lib.favs, fixture]);
  const section = sub ?? menus[0]?.id ?? FAVOURITES;

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
  // A favourite with no menu still shows under Favourites.
  const favRows: Line[] = useMemo(() => {
    if (!inTauri) return allRows.filter((r) => r.row.fav).map((r) => r.row);
    return [...lib.favs].flatMap((code) => {
      const r = live.rows.get(code);
      return r ? [lineOf(r, live.catalog, lib.favs)] : [];
    });
  }, [allRows, lib.favs, live.rows, live.catalog]);

  const hits: Hit[] = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    return allRows
      .map(({ row, menu }) => {
        const hay = `${row.name} ${row.cam} ${(row as Line).words ?? ""} ${searchWords[row.id] ?? ""}`.toLowerCase();
        return { row, menu, score: words.filter((w) => hay.includes(w)).length };
      })
      .filter((h) => h.score > 0)
      .sort((a, b) => b.score - a.score);
  }, [allRows, query]);
  const hitIds = new Set(hits.map((h) => h.row.id));

  const update = async (line: Line, patch: Partial<SettingRow>) => {
    if (!inTauri) {
      setFixture((ms) => ms.map((m) => ({ ...m, rows: m.rows.map((r) => (r.id === line.id ? ({ ...r, ...patch } as Line) : r)) })));
      if (!("fav" in patch)) notBuilt("Writing settings to the camera");
      return;
    }
    if ("fav" in patch && line.code != null) {
      const code = line.code;
      const r = patch.fav ? await commands.librarySave("favourite", hex(code), null, null) : await commands.libraryDelete(`favourite:${hex(code)}`);
      if (r.status !== "ok") flash(r.error);
      void lib.reload();
      return;
    }
    if ("value" in patch && line.code != null) {
      const raw = line.raws?.[String(patch.value)];
      if (raw == null) return;
      flash(await writeNow({ [hex(line.code)]: raw }));
    }
  };

  const plan = async (title: string, from: string) => {
    const p = await makePlan({}, from);
    if (typeof p === "string") flash(p);
    else setPending({ title, plan: p });
  };

  const snapshot = async () => {
    if (!inTauri) return notBuilt("Snapshots");
    const d = new Date();
    const name = `Snapshot ${d.toLocaleDateString("en-CA")} ${d.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`;
    const r = await commands.librarySave("snapshot", name, null, null);
    flash(r.status === "ok" ? `Saved ${name}.` : r.error);
    void lib.reload();
  };

  const menu = menus.find((m) => m.id === section);
  const listed: Line[] = section === FAVOURITES ? favRows : (menu?.rows ?? []);
  const presets: Item[] = inTauri ? lib.presets : presetFixture.map((p) => ({ id: p.name, name: p.name, meta: p.meta }));
  const snapshots: Item[] = inTauri ? lib.snapshots : snapshotFixture.map((s) => ({ id: s.name, name: s.name, meta: s.when }));

  return (
    <div className="page settings" data-component="settings">
      <PageHead
        title="Settings"
        sub={
          section === APPEARANCE ? "How Ukiyo looks on this Mac." : section === CONNECTION ? "How Ukiyo treats the connected camera, and Ukiyo's own updates." : "The camera's menus. A change writes to the camera at once."
        }
      >
        <CameraChip />
        <button type="button" className="btn" onClick={() => void snapshot()}>
          <CameraIcon />
          Take snapshot
        </button>
      </PageHead>
      {section !== APPEARANCE && section !== CONNECTION && <ModeNotice needs="tether" what="Reading and changing settings" />}

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
              <a key={h.row.id} href={href("settings", h.menu)} className={i === 0 ? "hit-chip first" : "hit-chip"} onClick={() => setFocusRow(h.row.id)}>
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
            <span className="tree-count">{favRows.length}</span>
          </a>
          <div className="tree-sep">This app</div>
          <a href={href("settings", APPEARANCE)} className="tree-item" aria-current={section === APPEARANCE ? "page" : undefined}>
            <span>Appearance</span>
          </a>
          <a href={href("settings", CONNECTION)} className="tree-item" aria-current={section === CONNECTION ? "page" : undefined}>
            <span>Camera connection</span>
          </a>
        </nav>

        {section === APPEARANCE ? (
          <Appearance />
        ) : section === CONNECTION ? (
          <>
            <Connection />
            <Updates />
          </>
        ) : (
          <section className="card settings-list" aria-labelledby="menu-title">
            <div className="settings-list-head">
              <h2 id="menu-title">{section === FAVOURITES ? "Favourites" : (menu?.label ?? "")}</h2>
              <span className="mono tiny muted">{section === FAVOURITES ? "" : `${menu?.cam ? `${menu.cam} · ` : ""}${menu?.rows.length ?? 0}`}</span>
            </div>
            {listed.map((r) => (
              <SettingLine key={r.id} row={r} hit={hitIds.has(r.id) || focusRow === r.id} onChange={(id, p) => void update(listed.find((x) => x.id === id) ?? r, p)} />
            ))}
            {listed.length === 0 && (
              <p className="empty">{section === FAVOURITES && lib.favs.size === 0 ? "No favourites yet. Star a setting to add it." : "No settings to show. Connect the camera in tether mode."}</p>
            )}
          </section>
        )}

        <aside className="settings-aside">
          {pending && <PlanCard title={pending.title} plan={pending.plan} onDone={() => setPending(null)} />}
          <Presets presets={presets} favs={favRows} onApply={(p) => (inTauri ? void plan(`Apply ${p.name}`, p.id) : notBuilt("Applying presets"))} onChanged={() => void lib.reload()} />

          <section className="card pad-s" aria-labelledby="favs">
            <h2 id="favs" className="card-title small-title">
              Favourites
            </h2>
            <div className="rows">
              {favRows.map((row) => (
                <label key={row.id} className="row-line small">
                  <span>{row.name}</span>
                  {row.kind === "select" ? (
                    <select className="select narrow" value={row.value} disabled={row.readOnly} onChange={(e) => void update(row, { value: e.target.value })}>
                      {row.options.map((o) => (
                        <option key={o}>{o}</option>
                      ))}
                    </select>
                  ) : (
                    <button type="button" role="switch" className="toggle" aria-checked={row.value} onClick={() => void update(row, { value: !row.value })} />
                  )}
                </label>
              ))}
            </div>
          </section>

          <Snapshots snapshots={snapshots} onRestore={(s) => (inTauri ? void plan(`Restore ${s.name}`, s.id) : notBuilt("Restoring snapshots"))} onChanged={() => void lib.reload()} />
        </aside>
      </div>
    </div>
  );
}

/** A plan from a preset or a snapshot, shown before anything is written. */
function Presets({ presets, favs, onApply, onChanged }: { presets: Item[]; favs: Line[]; onApply: (p: Item) => void; onChanged: () => void }) {
  const flash = useToast();
  const notBuilt = useNotBuilt();
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const save = async () => {
    const n = name.trim();
    if (!n) return;
    const r = await commands.librarySave("preset", n, { set: null, settings: favs.map((f) => f.id), notes: null }, null);
    flash(r.status === "ok" ? `Saved preset ${n}.${r.data.warnings.length ? ` ${r.data.warnings[0]}` : ""}` : r.error);
    setNaming(false);
    setName("");
    onChanged();
  };
  const remove = async (p: Item) => {
    const r = await commands.libraryDelete(p.id);
    flash(r.status === "ok" ? `Moved ${p.name} to the trash.` : r.error);
    onChanged();
  };
  return (
    <section className="card pad-s" aria-labelledby="presets">
      <div className="card-head">
        <h2 id="presets" className="card-title small-title">
          Presets
        </h2>
        <button type="button" className="btn text small" disabled={inTauri && favs.length === 0} title="Saves the current values of your favourites" onClick={() => (inTauri ? setNaming(true) : notBuilt("Presets"))}>
          Save current
        </button>
      </div>
      {naming && (
        <form
          className="actions-line"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <input className="input" type="text" placeholder="Preset name" aria-label="Preset name" autoFocus value={name} onChange={(e) => setName(e.target.value)} />
          <button type="submit" className="btn small raised" disabled={!name.trim()}>
            Save
          </button>
          <button type="button" className="btn text small" onClick={() => setNaming(false)}>
            Cancel
          </button>
        </form>
      )}
      <div className="rows">
        {presets.map((p) => (
          <div key={p.id} className="row-line">
            <span className="stack">
              <span className="strong">{p.name}</span>
              <span className="tiny muted">{p.meta}</span>
            </span>
            <span className="row-actions">
              <button type="button" className="btn small raised" onClick={() => onApply(p)}>
                Apply
              </button>
              {inTauri && (
                <button type="button" className="icon-btn" aria-label={`Delete ${p.name}`} onClick={() => void remove(p)}>
                  <CloseIcon />
                </button>
              )}
            </span>
          </div>
        ))}
      </div>
      {presets.length === 0 && <p className="tiny muted">No presets yet.</p>}
    </section>
  );
}

function Snapshots({ snapshots, onRestore, onChanged }: { snapshots: Item[]; onRestore: (s: Item) => void; onChanged: () => void }) {
  const flash = useToast();
  const notBuilt = useNotBuilt();
  const [comparing, setComparing] = useState(false);
  const [picked, setPicked] = useState<string[]>([]);
  const [diff, setDiff] = useState<Diff | null>(null);

  useEffect(() => {
    if (picked.length !== 2) return setDiff(null);
    void commands.libraryRead(picked[0], picked[1]).then((r) => {
      if (r.status === "ok" && r.data.kind === "diff") setDiff(r.data.item);
      else if (r.status !== "ok") flash(r.error);
    });
  }, [picked, flash]);

  const toggle = (id: string) => setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id].slice(-2)));
  const remove = async (s: Item) => {
    const r = await commands.libraryDelete(s.id);
    flash(r.status === "ok" ? `Moved ${s.name} to the trash.` : r.error);
    setPicked((p) => p.filter((x) => x !== s.id));
    onChanged();
  };
  const label = (l: string | null, v: unknown) => l ?? String(v);

  return (
    <section className="card pad-s" aria-labelledby="snaps">
      <div className="card-head">
        <h2 id="snaps" className="card-title small-title">
          Snapshots
        </h2>
        <button
          type="button"
          className="btn text small"
          aria-pressed={comparing}
          disabled={inTauri && snapshots.length < 2}
          onClick={() => {
            if (!inTauri) return notBuilt("Comparing snapshots");
            setComparing(!comparing);
            setPicked([]);
          }}
        >
          {comparing ? "Done" : "Compare two"}
        </button>
      </div>
      <div className="rows">
        {snapshots.map((s) => (
          <div key={s.id} className="row-line small">
            {comparing ? (
              <label className="inline-field">
                <input type="checkbox" checked={picked.includes(s.id)} onChange={() => toggle(s.id)} />
                {s.name}
              </label>
            ) : (
              <span className="stack">
                <span>{s.name}</span>
                {inTauri && <span className="tiny muted">{s.meta}</span>}
              </span>
            )}
            {inTauri && !comparing ? (
              <span className="row-actions">
                <button type="button" className="btn small raised" onClick={() => onRestore(s)}>
                  Restore
                </button>
                <button type="button" className="icon-btn" aria-label={`Delete ${s.name}`} onClick={() => void remove(s)}>
                  <CloseIcon />
                </button>
              </span>
            ) : (
              !comparing && <span className="muted">{s.meta}</span>
            )}
          </div>
        ))}
      </div>
      {snapshots.length === 0 && <p className="tiny muted">No snapshots yet.</p>}
      {comparing && diff && (
        <div className="rows" data-component="snapshot-diff">
          <p className="tiny muted">
            {diff.changed.length + diff.added.length + diff.removed.length} differ · {diff.same} alike
          </p>
          {diff.changed.map((c) => (
            <div key={c.code} className="row-line small">
              <span>{c.name ? sentence(c.name) : c.code}</span>
              <span className="muted">
                {label(c.a_label, c.a)} → {label(c.b_label, c.b)}
              </span>
            </div>
          ))}
          {diff.removed.map((v) => (
            <div key={v.code} className="row-line small">
              <span>{v.name ? sentence(v.name) : v.code}</span>
              <span className="muted">only in the first</span>
            </div>
          ))}
          {diff.added.map((v) => (
            <div key={v.code} className="row-line small">
              <span>{v.name ? sentence(v.name) : v.code}</span>
              <span className="muted">only in the second</span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function SettingLine({ row, hit, onChange }: { row: Line; hit: boolean; onChange: (id: string, p: Partial<SettingRow>) => void }) {
  return (
    <div id={`row-${row.id}`} className={hit ? "setting hit" : "setting"}>
      <div className="setting-name">
        <span>{row.name}</span>
        <span className="mono tiny muted">{row.cam}</span>
      </div>
      {row.kind === "select" ? (
        <select className="select wide" aria-label={row.name} value={row.value} disabled={row.readOnly} onChange={(e) => onChange(row.id, { value: e.target.value })}>
          {row.options.map((o) => (
            <option key={o}>{o}</option>
          ))}
        </select>
      ) : (
        <button type="button" role="switch" className="toggle" aria-checked={row.value} aria-label={row.name} onClick={() => onChange(row.id, { value: !row.value })} />
      )}
      <button type="button" className={row.fav ? "icon-btn star on" : "icon-btn star"} aria-pressed={!!row.fav} aria-label={`Favourite ${row.name}`} onClick={() => onChange(row.id, { fav: !row.fav })}>
        <StarIcon filled={!!row.fav} />
      </button>
    </div>
  );
}

function Connection() {
  const { status } = useCamera();
  const on = status?.keepAwake.enabled ?? false;
  return (
    <section className="card settings-list" aria-labelledby="connection-title" data-component="connection">
      <div className="settings-list-head">
        <h2 id="connection-title">Camera connection</h2>
      </div>
      <div className="setting">
        <div className="setting-name">
          <span id="keep-awake-label">Keep the camera awake while you use Ukiyo</span>
          <span className="tiny muted">In use means a click or key press in Ukiyo, or a CLI or agent request, in the last 5 minutes.</span>
        </div>
        <button
          type="button"
          role="switch"
          className="toggle"
          aria-checked={on}
          aria-labelledby="keep-awake-label"
          disabled={!inTauri}
          data-action="keep-awake"
          onClick={() => void commands.labKeepAwake(!on)}
        />
      </div>
    </section>
  );
}

function Updates() {
  const [s, setS] = useState<UpdateStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (inTauri) void commands.updateStatus().then(setS);
  }, []);
  const check = async () => {
    setBusy(true);
    setError(null);
    setS(await commands.updateCheck());
    setBusy(false);
  };
  const install = async () => {
    setBusy(true);
    setError(null);
    const r = await commands.updateInstall();
    if (r.status === "error") setError(r.error);
    setBusy(false);
  };
  const line = !s
    ? "Updates work in the app only."
    : !s.enabled
      ? s.reason
      : s.error
        ? s.error
        : s.available
          ? `Version ${s.available} is ready to install.`
          : s.checked
            ? "Ukiyo is up to date."
            : "Not checked yet.";
  return (
    <section className="card settings-list" aria-labelledby="updates-title" data-component="updates">
      <div className="settings-list-head">
        <h2 id="updates-title">Updates</h2>
        <span className="mono tiny muted">{s ? `Version ${s.current}` : ""}</span>
      </div>
      <div className="setting">
        <div className="setting-name">
          <span>{line}</span>
          {error && <span className="tiny muted">{error}</span>}
        </div>
        {s?.available ? (
          <button type="button" className="btn small raised" disabled={busy} data-action="update-install" onClick={() => void install()}>
            Install and restart
          </button>
        ) : (
          <button type="button" className="btn small" disabled={!s?.enabled || busy} data-action="update-check" onClick={() => void check()}>
            Check for updates
          </button>
        )}
      </div>
    </section>
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
