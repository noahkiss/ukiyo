import { useMemo, useState } from "react";
import { FILM_SIMS, recipes as recipeFixture, slots as slotFixture, WHITE_BALANCES, type Recipe, type Slot } from "../fixtures/recipes";
import { useCamera } from "../lib/camera";
import { href } from "../lib/router";
import { useNotBuilt } from "../lib/toast";
import { CameraChip } from "../shell/CameraChip";
import { PageHead } from "../shell/PageHead";
import { ChevronDown, LinkIcon, LockIcon, PhotoIcon, SearchIcon } from "../shell/icons";

type Show = "all" | "colour" | "mono";

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
  const saved = selectedId === "new" ? NEW_C7 : (library.find((r) => r.id === selectedId) ?? library[0]);
  const [draft, setDraft] = useState<Recipe>(saved);
  const [search, setSearch] = useState("");
  const [show, setShow] = useState<Show>("all");
  const changes = diff(saved, draft);

  const select = (id: string) => {
    setSelectedId(id);
    setDraft(id === "new" ? NEW_C7 : (library.find((r) => r.id === id) ?? library[0]));
  };

  const shown = useMemo(() => {
    const q = search.trim().toLowerCase();
    return library.filter((r) => (show === "all" || (show === "mono") === r.mono) && (!q || `${r.name} ${r.sim} ${summary(r)}`.toLowerCase().includes(q)));
  }, [library, search, show]);

  const saveToLibrary = () => {
    if (draft.id === "new") {
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
            <button type="button" className="btn" onClick={() => notBuilt("Importing a recipe")}>
              <LinkIcon />
              Import from link or text
            </button>
            <button type="button" className="btn" onClick={() => notBuilt("Reading a recipe from a photo")}>
              <PhotoIcon />
              From a photo
            </button>
          </div>
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
                </span>
              </button>
            ))}
            {shown.length === 0 && <p className="muted">No recipes match.</p>}
          </div>
        </section>

        <Editor draft={draft} saved={saved} changes={changes} setDraft={setDraft} onWrite={(slot) => notBuilt(slot ? `Writing to slot ${slot}` : "Writing to a slot")} onSave={saveToLibrary} onDiscard={() => setDraft(saved)} />
      </div>
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
  ["dr", "Dynamic range", ["DR100", "DR200", "DR400"]],
];

const SLIDERS: Array<[keyof Recipe, string, number, number, number]> = [
  ["highlights", "Highlights", -2, 4, 0.5],
  ["shadows", "Shadows", -2, 4, 0.5],
  ["colour", "Colour", -4, 4, 1],
  ["sharpness", "Sharpness", -4, 4, 1],
  ["noiseReduction", "Noise reduction", -4, 4, 1],
  ["clarity", "Clarity", -5, 5, 1],
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
  const where = draft.slot ? `In slot ${draft.slot}` : "In library";
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
      <label className="field">
        Name
        <input type="text" className="name-input" value={draft.name} onChange={(e) => set("name", e.target.value)} />
      </label>
      <label className="edit-line">
        Film simulation
        <select className="select" value={draft.sim} onChange={(e) => set("sim", e.target.value)}>
          {FILM_SIMS.map((s) => (
            <option key={s}>{s}</option>
          ))}
        </select>
      </label>
      <label className="edit-line">
        White balance
        <select className="select" value={draft.wb} onChange={(e) => set("wb", e.target.value)}>
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
        {SLIDERS.map(([k, label, min, max, step]) => {
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
          <button type="button" className="btn" onClick={onSave} disabled={changes === 0 && draft.id !== "new"}>
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
