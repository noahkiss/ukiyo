import { useEffect, useMemo, useRef, useState } from "react";
import { fxwPasteResult, fxwResults, type FxwResult, type ParsedRecipe } from "../fixtures/fxw";
import { FILM_SIMS, TONE_RANGES, WB_SHIFT_RANGE, WHITE_BALANCES, type Recipe, type Slot } from "../fixtures/recipes";
import { CloseIcon, SearchIcon } from "../shell/icons";

type Tab = "search" | "link" | "text";

const TABS: Array<[Tab, string]> = [
  ["search", "Search Fuji X Weekly"],
  ["link", "Paste a link"],
  ["text", "Paste text"],
];

const SLOTS: Slot[] = ["C1", "C2", "C3", "C4", "C5", "C6", "C7"];

/** The body the review checks against. One model for now. */
const BODY = "X-T5";

/** A recipe ready for review: the parse, where it came from, and its swatch. */
type Candidate = { parsed: ParsedRecipe; source: { label: string; url: string | null }; tones: string[] };

type Props = {
  onClose: () => void;
  onSave: (r: Recipe) => void;
  onWrite: (slot: Slot) => void;
};

/** Import a recipe from Fuji X Weekly, a link or pasted text, then review it. */
export function RecipeImport({ onClose, onSave, onWrite }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const [tab, setTab] = useState<Tab>("search");
  const [candidate, setCandidate] = useState<Candidate | null>(null);

  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);

  // showModal focuses the first control (Close); start in the tab's text box instead.
  useEffect(() => {
    if (!candidate) ref.current?.querySelector<HTMLElement>(".sheet-body input, .sheet-body textarea")?.focus();
  }, [tab, candidate]);

  const fromResult = (r: FxwResult): Candidate => ({ parsed: r.parsed, source: { label: "Fuji X Weekly", url: r.url }, tones: r.tones });

  return (
    <dialog ref={ref} className="sheet" aria-labelledby="import-title" onClose={onClose} onCancel={onClose} data-component="recipe-import">
      <div className="sheet-head">
        <h2 id="import-title" className="card-title">
          {candidate ? "Review recipe" : "Import a recipe"}
        </h2>
        <button type="button" className="icon-btn" aria-label="Close" onClick={onClose}>
          <CloseIcon />
        </button>
      </div>
      {candidate ? (
        <Review c={candidate} onBack={() => setCandidate(null)} onSave={onSave} onWrite={onWrite} />
      ) : (
        <>
          <div className="seg" role="tablist" aria-label="Import from">
            {TABS.map(([t, label]) => (
              <button key={t} type="button" role="tab" aria-selected={tab === t} aria-pressed={tab === t} onClick={() => setTab(t)}>
                {label}
              </button>
            ))}
          </div>
          {tab === "search" && <SearchTab onPick={(r) => setCandidate(fromResult(r))} />}
          {tab === "link" && <LinkTab onRead={(url) => setCandidate({ ...fromResult(fxwPasteResult), source: { label: "Fuji X Weekly", url } })} />}
          {tab === "text" && <TextTab onRead={() => setCandidate({ ...fromResult(fxwPasteResult), source: { label: "Pasted text", url: null } })} />}
        </>
      )}
    </dialog>
  );
}

const fmtDate = (iso: string) => new Date(iso + "T12:00:00").toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });

function SearchTab({ onPick }: { onPick: (r: FxwResult) => void }) {
  const [q, setQ] = useState("");
  // FIXTURE: filters the sample results; the RSS search replaces this.
  const results = useMemo(() => {
    const s = q.trim().toLowerCase();
    return fxwResults.filter((r) => !s || `${r.title} ${r.parsed.sim} ${r.sensor}`.toLowerCase().includes(s));
  }, [q]);
  return (
    <div className="sheet-body">
      <label className="search-box">
        <SearchIcon />
        <span className="visually-hidden">Search Fuji X Weekly</span>
        <input type="search" placeholder="Recipe name, film simulation or sensor" value={q} onChange={(e) => setQ(e.target.value)} />
      </label>
      <ul className="rows results">
        {results.map((r) => (
          <li key={r.id} className="result-line">
            <span className="result-swatch tones" aria-hidden="true">
              {r.tones.map((c, i) => (
                <span key={i} style={{ background: c }} />
              ))}
            </span>
            <span className="stack result-text">
              <span className="strong">{r.title}</span>
              <span className="small muted">
                {fmtDate(r.date)} · {r.parsed.sim}
              </span>
            </span>
            <span className={r.sensor === "X-Trans V" ? "badge ink" : "badge"}>{r.sensor}</span>
            <a className="small" href={r.url} target="_blank" rel="noreferrer">
              Post
            </a>
            <button type="button" className="btn small raised" onClick={() => onPick(r)}>
              Review
            </button>
          </li>
        ))}
        {results.length === 0 && <li className="empty">No recipes match.</li>}
      </ul>
    </div>
  );
}

function LinkTab({ onRead }: { onRead: (url: string) => void }) {
  const [url, setUrl] = useState("");
  const ok = /^https?:\/\/\S+$/.test(url.trim());
  return (
    <form
      className="sheet-body"
      onSubmit={(e) => {
        e.preventDefault();
        if (ok) onRead(url.trim());
      }}
    >
      <label className="field">
        Link to a recipe post
        <input type="url" placeholder="https://fujixweekly.com/…" value={url} onChange={(e) => setUrl(e.target.value)} />
      </label>
      <div className="actions-line">
        <button type="submit" className="btn primary" disabled={!ok}>
          Read recipe
        </button>
      </div>
    </form>
  );
}

function TextTab({ onRead }: { onRead: () => void }) {
  const [text, setText] = useState("");
  return (
    <form
      className="sheet-body"
      onSubmit={(e) => {
        e.preventDefault();
        if (text.trim()) onRead();
      }}
    >
      <label className="field">
        Recipe text
        <textarea rows={9} placeholder={"Film Simulation: Classic Negative\nGrain Effect: Weak, Small\nWhite Balance: 5200K, +2 Red & -4 Blue\n…"} value={text} onChange={(e) => setText(e.target.value)} />
      </label>
      <div className="actions-line">
        <button type="submit" className="btn primary" disabled={!text.trim()}>
          Read recipe
        </button>
      </div>
    </form>
  );
}

const signed = (x: number) => (x > 0 ? "+" : "") + x;

type Field = { label: string; value: string; problem?: string };

/** Each parsed field, flagged when the body cannot take it. */
function fields(p: ParsedRecipe): Field[] {
  const range = (label: string, v: number, [min, max, step]: readonly [number, number, number]): Field => ({
    label,
    value: signed(v),
    problem: v < min || v > max || Math.abs(v / step - Math.round(v / step)) > 1e-9 ? `${BODY} takes ${signed(min)} to ${signed(max)} in steps of ${step}` : undefined,
  });
  const [wbMin, wbMax] = WB_SHIFT_RANGE;
  const shiftBad = [p.wbShift.r, p.wbShift.b].some((x) => x < wbMin || x > wbMax);
  return [
    { label: "Film simulation", value: p.sim, problem: FILM_SIMS.includes(p.sim) ? undefined : `Not on the ${BODY}` },
    { label: "White balance", value: p.wb, problem: WHITE_BALANCES.includes(p.wb) ? undefined : `Not on the ${BODY}` },
    { label: "White balance shift", value: `R ${signed(p.wbShift.r)} · B ${signed(p.wbShift.b)}`, problem: shiftBad ? `${BODY} takes ${wbMin} to +${wbMax}` : undefined },
    { label: "Grain", value: p.grain === "Off" ? "Off" : `${p.grain}, ${p.grainSize}` },
    { label: "Color Chrome effect", value: p.colorChrome },
    { label: "Color Chrome FX Blue", value: p.colorChromeBlue },
    { label: "Dynamic range", value: p.dr },
    range("Highlights", p.highlights, TONE_RANGES.highlights),
    range("Shadows", p.shadows, TONE_RANGES.shadows),
    range("Colour", p.colour, TONE_RANGES.colour),
    range("Sharpness", p.sharpness, TONE_RANGES.sharpness),
    range("Noise reduction", p.noiseReduction, TONE_RANGES.noiseReduction),
    range("Clarity", p.clarity, TONE_RANGES.clarity),
  ];
}

function Review({ c, onBack, onSave, onWrite }: { c: Candidate; onBack: () => void; onSave: (r: Recipe) => void; onWrite: (s: Slot) => void }) {
  const [name, setName] = useState(c.parsed.name);
  const [slot, setSlot] = useState<Slot | "">("");
  const list = fields(c.parsed);
  const problems = list.filter((f) => f.problem).length;
  const { notes, ...rest } = c.parsed;
  const recipe: Recipe = {
    ...rest,
    name,
    id: "import",
    slot: null,
    mono: /acros|monochrome|sepia/i.test(c.parsed.sim),
    tones: c.tones,
    source: c.source,
    notes,
  };

  return (
    <div className="sheet-body">
      <div className="review-top">
        <span className="tones review-swatch" aria-hidden="true">
          {c.tones.map((t, i) => (
            <span key={i} style={{ background: t }} />
          ))}
        </span>
        <label className="field grow-field">
          Name
          <input type="text" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
      </div>
      <p className="small muted">
        Source:{" "}
        {c.source.url ? (
          <a href={c.source.url} target="_blank" rel="noreferrer">
            {c.source.label}
          </a>
        ) : (
          c.source.label
        )}
      </p>
      {problems > 0 && (
        <div className="banner warn" role="status">
          <span>
            {problems === 1 ? "1 field does not" : `${problems} fields do not`} fit the {BODY}. Save to the library and fix {problems === 1 ? "it" : "them"} in the editor before writing to a slot.
          </span>
        </div>
      )}
      <dl className="review-fields">
        {list.map((f) => (
          <div key={f.label} className={f.problem ? "review-field bad" : "review-field"}>
            <dt>{f.label}</dt>
            <dd>
              <span className={f.problem ? "red strong" : undefined}>{f.value}</span>
              {f.problem && <span className="tiny review-problem">{f.problem}</span>}
            </dd>
          </div>
        ))}
        {notes.map(([k, v]) => (
          <div key={k} className="review-field">
            <dt>{k}</dt>
            <dd>
              {v}
              <span className="tiny muted review-problem">Kept as a note</span>
            </dd>
          </div>
        ))}
      </dl>
      <div className="sheet-actions">
        <button type="button" className="btn text" onClick={onBack}>
          Back
        </button>
        <span className="grow" />
        <label className="slot-pick">
          <span className="visually-hidden">Slot</span>
          <select className="select narrow" value={slot} onChange={(e) => setSlot(e.target.value as Slot)} disabled={problems > 0}>
            <option value="">Write to slot…</option>
            {SLOTS.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </label>
        <button type="button" className="btn" disabled={!slot || problems > 0} onClick={() => slot && onWrite(slot)}>
          Write
        </button>
        <button type="button" className="btn primary" disabled={!name.trim()} onClick={() => onSave(recipe)}>
          Save to library
        </button>
      </div>
    </div>
  );
}
