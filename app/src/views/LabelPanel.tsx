import { useMemo, useState } from "react";
import type { Applied, Confidence, LabelPatch, Plan, PropRow, Value } from "../bindings";
import { display, formValues, hex, rawText, TYPE_NAMES, valueKey, type Catalog, type Entry } from "../lib/decode";
import { CloseIcon } from "../shell/icons";

export type LabelTarget = { code: number; old?: Value | null; new?: Value | null; group?: number };

type Props = {
  target: LabelTarget;
  row: PropRow | undefined;
  entry: Entry | undefined;
  catalog: Catalog | null;
  lastMenu: string;
  connected: boolean;
  onSave: (p: LabelPatch) => void;
  writer: Writer;
  onClose: () => void;
};

/** Writes go through plans, as in the CLI: make one, show its step, then apply it. */
export type Writer = {
  plan: (code: number, v: Value) => Promise<Plan | string>;
  apply: (planId: string) => Promise<Applied | string>;
  discard: (planId: string) => void;
};

const CONFIDENCE: Array<[Confidence, string]> = [
  ["confirmed", "Confirmed"],
  ["likely", "Likely"],
  ["guess", "Guess"],
];

export function LabelPanel({ target, row, entry, catalog, lastMenu, connected, onSave, writer, onClose }: Props) {
  const [name, setName] = useState(entry?.name ?? "");
  const [menu, setMenu] = useState(entry?.menu ?? lastMenu);
  const [confidence, setConfidence] = useState<Confidence>(entry?.confidence ?? "likely");
  const [notes, setNotes] = useState(entry?.notes ?? "");

  // The values worth labelling: the change's old and new, then the current value.
  const values = useMemo(() => {
    const vs: Value[] = [];
    for (const v of [target.old, target.new, row?.value]) {
      if (v && !vs.some((x) => valueKey(x) === valueKey(v))) vs.push(v);
    }
    return vs;
  }, [target, row]);
  const [labels, setLabels] = useState<Record<string, string>>(() =>
    Object.fromEntries(values.map((v) => [valueKey(v), entry?.values?.[valueKey(v)] ?? ""])),
  );

  const menus = useMemo(() => {
    const s = new Set<string>();
    for (const e of Object.values(catalog?.properties ?? {})) if (e.menu) s.add(e.menu);
    return [...s].sort();
  }, [catalog]);

  const submit = () => {
    onSave({
      code: target.code,
      name,
      menu,
      values: Object.entries(labels).filter(([k, l]) => l.trim() !== "" || entry?.values?.[k]),
      confidence,
      notes,
    });
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || (e.target as HTMLElement).tagName === "INPUT")) {
      e.preventDefault();
      submit();
    }
  };

  return (
    <section className="card panel" role="dialog" aria-modal="false" aria-labelledby="label-title" data-component="label-panel">
      <div className="panel-head">
        <h2 id="label-title" className="mono">
          {hex(target.code)}
        </h2>
        {row && (
          <span className="muted small">
            {TYPE_NAMES[row.datatype] ?? "array"} · {row.writable ? "read and write" : "read only"}
          </span>
        )}
        <button type="button" className="icon-btn" onClick={onClose} aria-label="Close" title="Close (Esc)">
          <CloseIcon />
        </button>
      </div>
      <form
        className="panel-body"
        onKeyDown={onKey}
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <label className="field">
          <span>Label</span>
          <input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="ColorChromeEffect" />
        </label>
        <label className="field">
          <span>Menu path</span>
          <input value={menu} onChange={(e) => setMenu(e.target.value)} list="menu-paths" placeholder="IMAGE QUALITY SETTING > GRAIN EFFECT" />
          <datalist id="menu-paths">
            {menus.map((m) => (
              <option key={m} value={m} />
            ))}
          </datalist>
        </label>
        {values.length > 0 && (
          <fieldset className="field">
            <legend>Value labels</legend>
            {values.map((v) => {
              const k = valueKey(v);
              const role = target.old && valueKey(target.old) === k ? "old" : target.new && valueKey(target.new) === k ? "new" : "now";
              return (
                <label key={k} className="value-label">
                  <span className="value-raw" title={rawText(v)}>
                    <span className="muted">{role}</span> {k.length > 24 ? k.slice(0, 24) + "…" : k}
                  </span>
                  <input value={labels[k] ?? ""} onChange={(e) => setLabels({ ...labels, [k]: e.target.value })} placeholder="label" />
                </label>
              );
            })}
          </fieldset>
        )}
        <fieldset className="field">
          <legend>Confidence</legend>
          <div className="seg segmented">
            {CONFIDENCE.map(([c, label]) => (
              <label key={c} className={confidence === c ? "on" : undefined}>
                <input type="radio" name="confidence" value={c} checked={confidence === c} onChange={() => setConfidence(c)} />
                {label}
              </label>
            ))}
          </div>
        </fieldset>
        <label className="field">
          <span>Notes</span>
          <textarea rows={2} value={notes} onChange={(e) => setNotes(e.target.value)} />
        </label>
        <div className="panel-actions">
          <button type="submit" className="btn primary" data-action="save-label">
            Save label
          </button>
          <span className="muted">⌘↩</span>
        </div>
        {entry?.source && <p className="muted small">Source: {entry.source}</p>}
      </form>
      {row && row.writable && row.available && <WriteBox key={row.code} row={row} entry={entry} connected={connected} writer={writer} />}
    </section>
  );
}

/** Why a plan has no step: what blocked or skipped it. */
function planProblem(p: Plan): string | null {
  if (p.steps.length) return null;
  const left = p.blocked[0] ?? p.skipped[0];
  return left ? left.message : "Nothing to write.";
}

function WriteBox({ row, entry, connected, writer }: { row: PropRow; entry: Entry | undefined; connected: boolean; writer: Writer }) {
  const options = formValues(row);
  const range = row.form.form === "range" ? row.form : null;
  const isString = row.datatype === 0xffff;
  const [choice, setChoice] = useState<string>(valueKey(row.value));
  const [plan, setPlan] = useState<Plan | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  const chosen: Value | null = (() => {
    if (options.length) return options.find((v) => valueKey(v) === choice) ?? null;
    if (isString) return { kind: "str", v: choice };
    const n = Number(choice);
    if (!Number.isInteger(n)) return null;
    if (range && range.min.kind === "int" && range.max.kind === "int" && (n < range.min.v || n > range.max.v)) return null;
    return { kind: "int", v: n };
  })();
  const same = chosen != null && valueKey(chosen) === valueKey(row.value);

  const cancel = () => {
    if (plan) writer.discard(plan.plan_id);
    setPlan(null);
  };

  const makePlan = async () => {
    if (!chosen) return;
    setBusy(true);
    setResult(null);
    const p = await writer.plan(row.code, chosen);
    setBusy(false);
    if (typeof p === "string") return setResult({ ok: false, text: p });
    const problem = planProblem(p);
    if (problem) {
      writer.discard(p.plan_id);
      return setResult({ ok: false, text: problem });
    }
    setPlan(p);
  };

  const go = async () => {
    if (!plan) return;
    setBusy(true);
    const a = await writer.apply(plan.plan_id);
    setBusy(false);
    setPlan(null);
    if (typeof a === "string") return setResult({ ok: false, text: a });
    const s = a.steps[0];
    if (!s) return setResult({ ok: false, text: "Nothing written" });
    const back = `read back ${s.readback ?? "nothing"}`;
    const menu = s.closed_menu ? "Closed the camera menu to apply the change · " : "";
    setResult(s.ok ? { ok: true, text: `${menu}Written · ${back}` } : { ok: false, text: `Not written: ${s.error ?? s.result} (${s.result}) · ${back}` });
  };

  const choose = (v: string) => {
    setChoice(v);
    cancel();
  };

  if (options.length === 1 && !range) return null;
  return (
    <section className="writebox" aria-label="Write" data-component="write-box">
      <h3>Write a value</h3>
      <div className="write-row">
        {options.length ? (
          <select value={choice} onChange={(e) => choose(e.target.value)} aria-label="New value">
            {options.map((v) => (
              <option key={valueKey(v)} value={valueKey(v)}>
                {display(entry, v)}
                {entry?.values?.[valueKey(v)] ? ` (${valueKey(v)})` : ""}
              </option>
            ))}
          </select>
        ) : range && range.min.kind === "int" && range.max.kind === "int" && range.step.kind === "int" ? (
          <input
            type="number"
            min={range.min.v}
            max={range.max.v}
            step={range.step.v || 1}
            value={choice}
            onChange={(e) => choose(e.target.value)}
            aria-label="New value"
          />
        ) : (
          <input value={choice} onChange={(e) => choose(e.target.value)} aria-label="New value" />
        )}
        {!plan ? (
          <button type="button" className="btn primary" disabled={!connected || !chosen || same || busy} onClick={makePlan} data-action="write">
            Write…
          </button>
        ) : (
          <>
            <button type="button" className="btn primary" disabled={busy} onClick={go} data-action="confirm-write">
              Write {plan.steps[0].before} → {plan.steps[0].after}
            </button>
            <button type="button" className="btn text" onClick={cancel}>
              Cancel
            </button>
          </>
        )}
      </div>
      {range && range.min.kind === "int" && range.max.kind === "int" && (
        <p className="muted small">
          {range.min.v} to {range.max.v}
          {range.step.kind === "int" && range.step.v > 1 ? `, step ${range.step.v}` : ""}
        </p>
      )}
      {plan && plan.steps[0].gate !== "ok" && <p className="muted small">Not confirmed in the catalog.</p>}
      {result && (
        <p className={result.ok ? "result ok" : "result bad"} role="status">
          {result.text}
        </p>
      )}
    </section>
  );
}
