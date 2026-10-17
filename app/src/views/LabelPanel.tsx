import { useMemo, useRef, useState } from "react";
import type { Confidence, LabelPatch, PropRow, Value, WriteOutcome } from "../bindings";
import { display, formValues, hex, rawText, TYPE_NAMES, valueKey, type Catalog, type Entry } from "../lib/decode";

export type LabelTarget = { code: number; old?: Value | null; new?: Value | null; group?: number };

type Props = {
  target: LabelTarget;
  row: PropRow | undefined;
  entry: Entry | undefined;
  catalog: Catalog | null;
  lastMenu: string;
  connected: boolean;
  onSave: (p: LabelPatch) => void;
  onWrite: (code: number, v: Value) => Promise<WriteOutcome>;
  onClose: () => void;
};

const CONFIDENCE: Confidence[] = ["confirmed", "likely", "guess"];

export function LabelPanel({ target, row, entry, catalog, lastMenu, connected, onSave, onWrite, onClose }: Props) {
  const nameRef = useRef<HTMLInputElement>(null);
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
    <aside className="panel" role="dialog" aria-modal="false" aria-labelledby="label-title" onKeyDown={onKey} data-component="label-panel">
      <div className="panel-head">
        <h2 id="label-title">
          {hex(target.code)}
          {row && <span className="muted"> · {TYPE_NAMES[row.datatype] ?? "array"}{row.writable ? " · rw" : " · ro"}</span>}
        </h2>
        <button type="button" className="icon" onClick={onClose} aria-label="Close" title="Close (Esc)">
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
            <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
        </button>
      </div>
      <form
        className="panel-body"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <label className="field">
          <span>Name</span>
          <input ref={nameRef} autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="ColorChromeEffect" />
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
          <div className="segmented">
            {CONFIDENCE.map((c) => (
              <label key={c} className={confidence === c ? "on" : undefined}>
                <input type="radio" name="confidence" value={c} checked={confidence === c} onChange={() => setConfidence(c)} />
                {c}
              </label>
            ))}
          </div>
        </fieldset>
        <label className="field">
          <span>Notes</span>
          <textarea rows={2} value={notes} onChange={(e) => setNotes(e.target.value)} />
        </label>
        <div className="panel-actions">
          <button type="submit" className="primary" data-action="save-label">
            Save label
          </button>
          <span className="muted">⌘↩</span>
        </div>
        {entry?.source && <p className="muted small">Source: {entry.source}</p>}
      </form>
      {row && row.writable && row.available && <WriteBox row={row} entry={entry} connected={connected} onWrite={onWrite} />}
    </aside>
  );
}

function WriteBox({ row, entry, connected, onWrite }: { row: PropRow; entry: Entry | undefined; connected: boolean; onWrite: (code: number, v: Value) => Promise<WriteOutcome> }) {
  const options = formValues(row);
  const range = row.form.form === "range" ? row.form : null;
  const isString = row.datatype === 0xffff;
  const [choice, setChoice] = useState<string>(valueKey(row.value));
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<WriteOutcome | null>(null);

  const chosen: Value | null = (() => {
    if (options.length) return options.find((v) => valueKey(v) === choice) ?? null;
    if (isString) return { kind: "str", v: choice };
    const n = Number(choice);
    if (!Number.isInteger(n)) return null;
    if (range && range.min.kind === "int" && range.max.kind === "int" && (n < range.min.v || n > range.max.v)) return null;
    return { kind: "int", v: n };
  })();
  const same = chosen != null && valueKey(chosen) === valueKey(row.value);

  const go = async () => {
    if (!chosen) return;
    setBusy(true);
    setResult(await onWrite(row.code, chosen));
    setBusy(false);
    setConfirming(false);
  };

  if (options.length === 1 && !range) return null;
  return (
    <section className="writebox" aria-label="Write" data-component="write-box">
      <h3>Write to camera</h3>
      <div className="write-row">
        {options.length ? (
          <select value={choice} onChange={(e) => (setChoice(e.target.value), setConfirming(false))} aria-label="New value">
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
            onChange={(e) => (setChoice(e.target.value), setConfirming(false))}
            aria-label="New value"
          />
        ) : (
          <input value={choice} onChange={(e) => (setChoice(e.target.value), setConfirming(false))} aria-label="New value" />
        )}
        {!confirming ? (
          <button type="button" disabled={!connected || !chosen || same || busy} onClick={() => setConfirming(true)} data-action="write">
            Write…
          </button>
        ) : (
          <>
            <button type="button" className="primary" disabled={busy} onClick={go} data-action="confirm-write">
              Write {display(entry, row.value)} → {chosen ? display(entry, chosen) : ""}
            </button>
            <button type="button" className="quiet" onClick={() => setConfirming(false)}>
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
      {result && (
        <p className={result.ok ? "result ok" : "result bad"} role="status">
          {result.ok ? "Written" : `Not written: ${result.result}`} · read back {result.readback ? display(entry, result.readback) : "nothing"}
        </p>
      )}
    </section>
  );
}
