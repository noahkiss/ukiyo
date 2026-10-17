import { useMemo, useState } from "react";
import { FILM_SIMS, recipes as recipeFixture, slots as slotFixture, TONE_RANGES, WHITE_BALANCES, type Recipe, type Slot } from "../fixtures/recipes";
import { useCamera } from "../lib/camera";
import { entryRecipe, filterCards, fmtDate, pickVariant, publisher, sensorOf, sortCards, useCatalog, type CatalogCard as Card, type CatalogEntry, type CatalogSort } from "../lib/catalog";
import { href } from "../lib/router";
import { useNotBuilt } from "../lib/toast";
import { CameraChip } from "../shell/CameraChip";
import { PageHead } from "../shell/PageHead";
import { ChevronDown, LinkIcon, LockIcon, PhotoIcon, SearchIcon } from "../shell/icons";
import { CatalogFilters, RecipeImport } from "./RecipeImport";

type Show = "all" | "colour" | "mono";
type Collection = "mine" | "catalog";

/** Catalogue cards shown before "Show more". */
const CATALOG_PAGE = 60;

const SORTS: Array<[CatalogSort, string]> = [
  ["popular", "Popular"],
  ["newest", "Newest"],
  ["az", "A–Z"],
];

const signed = (x: number) => (x > 0 ? "+" : "") + x;

function summary(r: Recipe): string {
  if (r.mono) return `Grain ${r.grain.toLowerCase()} · ${r.dr}`;
  const wb = r.wb.replace("Colour temperature ", "");
  if (r.wbShift.r === 0 && r.wbShift.b === 0 && r.highlights === 0 && r.shadows === 0 && r.colour === 0 && wb === "Auto" && r.dr === "DR100") return "All tones 0 · DR100";
  return `WB ${wb} R${signed(r.wbShift.r)} B${signed(r.wbShift.b)} · ${r.dr}`;
}

function diff(a: Recipe, b: Recipe): number {
  return (Object.keys(a) as Array<keyof Recipe>).filter((k) => JSON.stringify(a[k]) !== JSON.stringify(b[k])).length;
}

const NEW_C7: Recipe = { ...recipeFixture[5], id: "new", name: "Unnamed recipe", slot: "C7", tones: slotFixture[6].tones ?? [] };

export function Recipes() {
  const { status } = useCamera();
  const notBuilt = useNotBuilt();
  const [library, setLibrary] = useState<Recipe[]>(recipeFixture);
  const [selectedId, setSelectedId] = useState("r3");
  const data = useCatalog();
  const catalogById = data?.byId ?? new Map<string, CatalogEntry>();
  const find = (id: string): Recipe => {
    if (id === "new") return NEW_C7;
    const e = catalogById.get(id);
    if (e) return entryRecipe(e);
    return library.find((r) => r.id === id) ?? library[0];
  };
  const saved = find(selectedId);
  const [draft, setDraft] = useState<Recipe>(saved);
  const [search, setSearch] = useState("");
  const [show, setShow] = useState<Show>("all");
  const [importing, setImporting] = useState(false);
  const [collection, setCollection] = useState<Collection>("mine");
  const [sensor, setSensor] = useState("");
  const [sim, setSim] = useState("");
  const [source, setSource] = useState("");
  const [catalogLimit, setCatalogLimit] = useState(CATALOG_PAGE);
  const [sort, setSort] = useState<CatalogSort>("popular");
  /** A version the user opened on a card, by card id; otherwise the camera decides. */
  const [opened, setOpened] = useState<Record<string, string>>({});
  const cameraSensor = sensorOf(status?.model);
  const changes = diff(saved, draft);

  const select = (id: string) => {
    setSelectedId(id);
    setDraft(find(id));
  };

  const catalogShown = useMemo(() => (data ? sortCards(filterCards(data.cards, { q: search, sensor, sim, source, show }), sort) : []), [data, search, sensor, sim, source, show, sort]);
  const shownVariant = (c: Card) => c.variants.find((v) => v.id === opened[c.id]) ?? pickVariant(c, cameraSensor);

  const shown = useMemo(() => {
    const q = search.trim().toLowerCase();
    return library.filter((r) => (show === "all" || (show === "mono") === r.mono) && (!q || `${r.name} ${r.sim} ${summary(r)}`.toLowerCase().includes(q)));
  }, [library, search, show]);

  const saveToLibrary = () => {
    if (draft.id === "new" || catalogById.has(draft.id)) {
      const r = { ...draft, id: `r${library.length + 1}` };
      setLibrary([...library, r]);
      setSelectedId(r.id);
      setDraft(r);
    } else {
      setLibrary(library.map((r) => (r.id === draft.id ? draft : r)));
    }
  };

  return (
    <div className="page recipes" data-component="recipes">
      <PageHead title="Recipes" sub="Slots C1 to C7 on the camera, and your recipe library.">
        <CameraChip />
        <button type="button" className="btn" onClick={() => notBuilt("Reading the slots")}>
          Read all slots
        </button>
        <button type="button" className="btn" onClick={() => notBuilt("Slot sets")}>
          Slot sets
          <ChevronDown />
        </button>
      </PageHead>

      {status?.mode === "rawConv" && (
        <div className="banner warn" role="status">
          <LockIcon />
          <span>The camera is locked in this mode. Switch to Tether mode to shoot or change settings.</span>
          <a href={href("connect")}>How to switch</a>
        </div>
      )}

      <section className="slots" aria-label="Camera slots">
        {slotFixture.map((s) => {
          const r = s.recipeId ? library.find((x) => x.id === s.recipeId) : undefined;
          const id = s.recipeId ?? "new";
          const on = selectedId === id;
          const state = on && changes > 0 ? "Edited" : s.state;
          return (
            <button key={s.slot} type="button" className={s.state === "New" ? "slot new" : "slot"} aria-pressed={on} onClick={() => select(id)}>
              <span className="slot-top">
                <span className="slot-id">{s.slot}</span>
                <span className={state === "Matches" ? "slot-state" : "slot-state red"}>{state}</span>
              </span>
              <span className="slot-name">{r?.name ?? s.name}</span>
              <span className="slot-sim">{r?.sim ?? s.sim}</span>
              <Tones tones={r?.tones ?? s.tones ?? []} />
            </button>
          );
        })}
      </section>

      <div className="flex-row">
        <section className="library" aria-labelledby="lib">
          <div className="library-tools">
            <h2 id="lib" className="card-title">
              Library
            </h2>
            <div className="seg" role="group" aria-label="Collection">
              {(
                [
                  ["mine", "My recipes"],
                  ["catalog", "Catalogue"],
                ] as Array<[Collection, string]>
              ).map(([k, label]) => (
                <button key={k} type="button" aria-pressed={collection === k} onClick={() => setCollection(k)}>
                  {label}
                </button>
              ))}
            </div>
            <label className="search-box">
              <SearchIcon />
              <span className="visually-hidden">Search recipes</span>
              <input type="search" placeholder="Search recipes" value={search} onChange={(e) => setSearch(e.target.value)} />
            </label>
            <div className="seg" role="group" aria-label="Show">
              {(
                [
                  ["all", "All"],
                  ["colour", "Colour"],
                  ["mono", "Monochrome"],
                ] as Array<[Show, string]>
              ).map(([k, label]) => (
                <button key={k} type="button" aria-pressed={show === k} onClick={() => setShow(k)}>
                  {label}
                </button>
              ))}
            </div>
            <button type="button" className="btn" onClick={() => setImporting(true)} data-action="import">
              <LinkIcon />
              Import
            </button>
            <button type="button" className="btn" onClick={() => notBuilt("Reading a recipe from a photo")}>
              <PhotoIcon />
              From a photo
            </button>
          </div>
          {collection === "catalog" && (
            <div className="library-tools">
              <CatalogFilters
                source={source}
                setSource={(v) => {
                  setSource(v);
                  setCatalogLimit(CATALOG_PAGE);
                }}
                sensor={sensor}
                setSensor={(v) => {
                  setSensor(v);
                  setCatalogLimit(CATALOG_PAGE);
                }}
                sim={sim}
                setSim={(v) => {
                  setSim(v);
                  setCatalogLimit(CATALOG_PAGE);
                }}
              />
              <span className="tiny muted grow">
                {data ? `${catalogShown.length} of ${data.cards.length} recipes` : "Loading the catalogue…"}
              </span>
              <div className="seg" role="group" aria-label="Sort">
                {SORTS.map(([k, label]) => (
                  <button key={k} type="button" aria-pressed={sort === k} onClick={() => setSort(k)}>
                    {label}
                  </button>
                ))}
              </div>
            </div>
          )}
          {collection === "catalog" ? (
            <div className="library-grid">
              {catalogShown.slice(0, catalogLimit).map((c) => {
                const e = shownVariant(c);
                return (
                  <CatalogCard
                    key={c.id}
                    card={c}
                    e={e}
                    on={c.variants.some((v) => v.id === selectedId)}
                    onSelect={() => select(e.id)}
                    onOpen={(v) => {
                      setOpened({ ...opened, [c.id]: v.id });
                      select(v.id);
                    }}
                  />
                );
              })}
              {data && catalogShown.length === 0 && <p className="muted">No recipes match.</p>}
              {catalogShown.length > catalogLimit && (
                <button type="button" className="btn more-btn" onClick={() => setCatalogLimit(catalogLimit + CATALOG_PAGE)}>
                  Show more ({catalogShown.length - catalogLimit})
                </button>
              )}
            </div>
          ) : (
          <div className="library-grid">
            {shown.map((r) => (
              <button key={r.id} type="button" className="recipe-card" aria-pressed={r.id === selectedId} onClick={() => select(r.id)}>
                <Tones tones={r.tones} tall />
                <span className="recipe-body">
                  <span className="recipe-top">
                    <span className="recipe-name">{r.name}</span>
                    {r.slot && <span className="badge ink">{r.slot}</span>}
                  </span>
                  <span className="small muted">{r.sim}</span>
                  <span className="mono tiny muted">{summary(r)}</span>
                  {r.source && <span className="tiny muted">From {r.source.label}</span>}
                </span>
              </button>
            ))}
            {shown.length === 0 && <p className="muted">No recipes match.</p>}
          </div>
          )}
        </section>

        {importing && (
          <RecipeImport
            onClose={() => setImporting(false)}
            onSave={(r) => {
              const saved = { ...r, id: `r${library.length + 1}` };
              setLibrary([...library, saved]);
              setSelectedId(saved.id);
              setDraft(saved);
              setImporting(false);
            }}
            onWrite={(slot) => notBuilt(`Writing to slot ${slot}`)}
          />
        )}
        <Editor draft={draft} saved={saved} changes={changes} setDraft={setDraft} onWrite={(slot) => notBuilt(slot ? `Writing to slot ${slot}` : "Writing to a slot")} onSave={saveToLibrary} onDiscard={() => setDraft(saved)} />
      </div>
    </div>
  );
}

type CardProps = { card: Card; e: CatalogEntry; on: boolean; onSelect: () => void; onOpen: (v: CatalogEntry) => void };

/** A catalogue card: the version shown, its first publisher and a link to the post, and
 * the other sensors' versions. */
function CatalogCard({ card, e, on, onSelect, onOpen }: CardProps) {
  const r = entryRecipe(e);
  const others = card.variants.filter((v) => v.id !== e.id);
  return (
    <div className="recipe-card catalog-card">
      <button type="button" className="catalog-card-main" aria-pressed={on} onClick={onSelect}>
        <Tones tones={r.tones} tall />
        <span className="recipe-body">
          <span className="recipe-top">
            <span className="recipe-name">{card.title}</span>
            {card.popular && <span className="badge ink">Popular</span>}
          </span>
          {e.sensor && <span className="tiny muted">{e.sensor}</span>}
          <span className="small muted">
            {r.sim}
            {e.video && " · Video"}
          </span>
          <span className="mono tiny muted">{summary(r)}</span>
        </span>
      </button>
      {others.length > 0 && (
        <span className="variant-chips">
          {others.map((v) => (
            <button key={v.id} type="button" className="chip" onClick={() => onOpen(v)}>
              {v.sensor ? `${v.sensor} version` : "Other version"}
            </button>
          ))}
        </span>
      )}
      <span className="recipe-credit tiny muted">
        <span>
          From {publisher(e.credit)} · {fmtDate(e.credit.date)}
          {e.also.length > 0 && <span title={`Also published by ${e.also.map((p) => `${publisher(p)} (${p.name}, ${fmtDate(p.date)})`).join("; ")}`}> · +{e.also.length}</span>}
        </span>
        <a href={e.credit.url} target="_blank" rel="noreferrer">
          Post
        </a>
      </span>
    </div>
  );
}

function Tones({ tones, tall }: { tones: string[]; tall?: boolean }) {
  return (
    <span className={tall ? "tones tall" : "tones"} aria-hidden="true">
      {tones.map((c, i) => (
        <span key={i} style={{ background: c }} />
      ))}
    </span>
  );
}

const SEGS: Array<[keyof Recipe, string, string[]]> = [
  ["grain", "Grain", ["Off", "Weak", "Strong"]],
  ["grainSize", "Grain size", ["Small", "Large"]],
  ["colorChrome", "Color Chrome effect", ["Off", "Weak", "Strong"]],
  ["colorChromeBlue", "Color Chrome FX Blue", ["Off", "Weak", "Strong"]],
  ["dr", "Dynamic range", ["DR100", "DR200", "DR400", "Auto"]],
];

const SLIDERS: Array<[keyof typeof TONE_RANGES, string]> = [
  ["highlights", "Highlights"],
  ["shadows", "Shadows"],
  ["colour", "Colour"],
  ["sharpness", "Sharpness"],
  ["noiseReduction", "Noise reduction"],
  ["clarity", "Clarity"],
];

type EditorProps = {
  draft: Recipe;
  saved: Recipe;
  changes: number;
  setDraft: (r: Recipe) => void;
  onWrite: (slot: Slot | null) => void;
  onSave: () => void;
  onDiscard: () => void;
};

function Editor({ draft, saved, changes, setDraft, onWrite, onSave, onDiscard }: EditorProps) {
  const set = <K extends keyof Recipe>(k: K, v: Recipe[K]) => setDraft({ ...draft, [k]: v });
  const where = draft.slot ? `In slot ${draft.slot}` : draft.id.startsWith("cat:") ? "From the catalogue" : "In library";
  return (
    <aside className="card editor" aria-labelledby="ed">
      <div className="editor-head">
        <h2 id="ed" className="eyebrow red">
          Edit recipe
        </h2>
        <span className="small muted">
          {where} · {changes === 0 ? "no changes" : `${changes} ${changes === 1 ? "change" : "changes"}`}
        </span>
      </div>
      {draft.source && (
        <p className="small muted">
          Source:{" "}
          {draft.source.url ? (
            <a href={draft.source.url} target="_blank" rel="noreferrer">
              {draft.source.label}
            </a>
          ) : (
            draft.source.label
          )}
        </p>
      )}
      {draft.notes && draft.notes.length > 0 && (
        <p className="small muted">{draft.notes.map(([k, v]) => `${k}: ${v}`).join(" · ")}</p>
      )}
      <label className="field">
        Name
        <input type="text" className="name-input" value={draft.name} onChange={(e) => set("name", e.target.value)} />
      </label>
      <label className="edit-line">
        Film simulation
        <select className="select" value={draft.sim} onChange={(e) => set("sim", e.target.value)}>
          {!FILM_SIMS.includes(draft.sim) && <option value={draft.sim}>{draft.sim} (not on the X-T5)</option>}
          {FILM_SIMS.map((s) => (
            <option key={s}>{s}</option>
          ))}
        </select>
      </label>
      <label className="edit-line">
        White balance
        <select className="select" value={draft.wb} onChange={(e) => set("wb", e.target.value)}>
          {!WHITE_BALANCES.includes(draft.wb) && <option value={draft.wb}>{draft.wb}</option>}
          {WHITE_BALANCES.map((s) => (
            <option key={s}>{s}</option>
          ))}
        </select>
      </label>
      <div className="edit-line">
        <span>White balance shift</span>
        <span className="wb-shift">
          {(["r", "b"] as const).map((k) => (
            <label key={k}>
              {k.toUpperCase()}
              <input type="number" min={-9} max={9} value={draft.wbShift[k]} onChange={(e) => set("wbShift", { ...draft.wbShift, [k]: Number(e.target.value) })} />
            </label>
          ))}
        </span>
      </div>
      {SEGS.map(([k, label, opts]) => (
        <div key={k} className="edit-line">
          <span>{label}</span>
          <span className="seg compact" role="group" aria-label={label}>
            {opts.map((o) => (
              <button key={o} type="button" aria-pressed={draft[k] === o} onClick={() => setDraft({ ...draft, [k]: o })}>
                {o}
              </button>
            ))}
          </span>
        </div>
      ))}
      <div className="sliders">
        {SLIDERS.map(([k, label]) => {
          const [min, max, step] = TONE_RANGES[k];
          const v = draft[k] as number;
          return (
            <label key={k} className="slider">
              <span>{label}</span>
              <input type="range" min={min} max={max} step={step} value={v} onChange={(e) => setDraft({ ...draft, [k]: Number(e.target.value) })} />
              <span className={v !== saved[k] ? "slider-val red" : "slider-val"}>{signed(v)}</span>
            </label>
          );
        })}
      </div>
      <div className="editor-actions">
        <button type="button" className="btn primary tall" onClick={() => onWrite(draft.slot)}>
          {draft.slot ? `Write to slot ${draft.slot}` : "Write to a slot"}
        </button>
        <div className="actions-pair">
          <button type="button" className="btn" onClick={onSave} disabled={changes === 0 && draft.id !== "new" && !draft.id.startsWith("cat:")}>
            Save to library
          </button>
          <button type="button" className="btn text" onClick={onDiscard} disabled={changes === 0}>
            Discard changes
          </button>
        </div>
      </div>
    </aside>
  );
}
