import { memo, useMemo, type RefObject } from "react";
import type { PropRow } from "../bindings";
import { display, entry, hex, rawText, time, type Catalog } from "../lib/decode";
import type { Selection } from "../App";

export type Filter = "all" | "unnamed" | "changed" | "pinned";

const FILTERS: Array<[Filter, string]> = [
  ["all", "All"],
  ["unnamed", "Unnamed"],
  ["changed", "Changed"],
  ["pinned", "Pinned"],
];

/** A changed row stays tinted this long. */
const TINT_MS = 20_000;

type Props = {
  rows: Map<number, PropRow>;
  catalog: Catalog | null;
  filter: Filter;
  setFilter: (f: Filter) => void;
  search: string;
  setSearch: (s: string) => void;
  searchRef: RefObject<HTMLInputElement | null>;
  pinned: Set<number>;
  /** Codes whose changes neither flash nor tint (the hidden noisy codes). */
  quiet: Set<number>;
  onTogglePin: (code: number) => void;
  selected: Selection | null;
  onSelect: (s: Selection) => void;
  onLabel: (code: number) => void;
  now: number;
};

export function PropTable(p: Props) {
  const list = useMemo(() => {
    const q = p.search.trim().toLowerCase();
    return [...p.rows.values()].filter((r) => {
      const e = entry(p.catalog, r.code);
      if (p.filter === "unnamed" && e?.name) return false;
      if (p.filter === "changed" && r.changedAt == null) return false;
      if (p.filter === "pinned" && !p.pinned.has(r.code)) return false;
      if (!q) return true;
      return [hex(r.code), e?.name ?? "", e?.menu ?? "", display(e, r.value)].some((s) => s.toLowerCase().includes(q));
    });
  }, [p.rows, p.catalog, p.filter, p.search, p.pinned]);

  const counts = useMemo(() => {
    const all = [...p.rows.values()];
    return {
      all: all.length,
      unnamed: all.filter((r) => !entry(p.catalog, r.code)?.name).length,
      changed: all.filter((r) => r.changedAt != null).length,
      pinned: all.filter((r) => p.pinned.has(r.code)).length,
    };
  }, [p.rows, p.catalog, p.pinned]);

  return (
    <section className="pane props" aria-label="Properties" data-component="prop-table">
      <div className="pane-head">
        <div className="tabs" role="tablist">
          {FILTERS.map(([f, label]) => (
            <button
              key={f}
              type="button"
              role="tab"
              aria-selected={p.filter === f}
              className={p.filter === f ? "tab on" : "tab"}
              onClick={() => p.setFilter(f)}
              data-action={`filter-${f}`}
            >
              {label} <span className="count">{counts[f]}</span>
            </button>
          ))}
        </div>
        <label className="search">
          <span className="visually-hidden">Search properties</span>
          <input ref={p.searchRef} type="search" placeholder="Search" value={p.search} onChange={(e) => p.setSearch(e.target.value)} />
        </label>
      </div>
      <div className="scroll">
        {p.rows.size === 0 ? (
          <p className="empty">No properties yet. Connect the camera in USB TETHER SHOOTING FIXED mode.</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th className="pin-col">
                  <span className="visually-hidden">Pinned</span>
                </th>
                <th>Code</th>
                <th>Name</th>
                <th>Value</th>
                <th className="num">Changed</th>
              </tr>
            </thead>
            <tbody>
              {list.map((r) => (
                <Row
                  key={`${r.code}:${r.changedAt ?? 0}`}
                  row={r}
                  catalog={p.catalog}
                  pinned={p.pinned.has(r.code)}
                  selected={p.selected?.code === r.code}
                  tinted={!p.quiet.has(r.code) && r.changedAt != null && p.now - r.changedAt < TINT_MS}
                  quiet={p.quiet.has(r.code)}
                  onTogglePin={p.onTogglePin}
                  onSelect={p.onSelect}
                  onLabel={p.onLabel}
                />
              ))}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
}

type RowProps = {
  row: PropRow;
  catalog: Catalog | null;
  pinned: boolean;
  selected: boolean;
  tinted: boolean;
  quiet: boolean;
  onTogglePin: (code: number) => void;
  onSelect: (s: Selection) => void;
  onLabel: (code: number) => void;
};

const Row = memo(function Row({ row, catalog, pinned, selected, tinted, quiet, onTogglePin, onSelect, onLabel }: RowProps) {
  const e = entry(catalog, row.code);
  // A fresh change remounts the row (its key holds changedAt), which replays the flash.
  const fresh = !quiet && row.changedAt != null && Date.now() - row.changedAt < 1500;
  const cls = [fresh ? "flash" : "", tinted ? "tint" : "", selected ? "sel" : "", row.available ? "" : "unavailable"].filter(Boolean).join(" ");
  return (
    <tr className={cls} onClick={() => onSelect({ kind: "row", code: row.code })} onDoubleClick={() => onLabel(row.code)} data-code={hex(row.code)}>
      <td className="pin-col">
        <button
          type="button"
          className={pinned ? "pin on" : "pin"}
          aria-pressed={pinned}
          aria-label={pinned ? `Unpin ${hex(row.code)}` : `Pin ${hex(row.code)}`}
          title={pinned ? "Unpin" : "Pin"}
          onClick={(ev) => {
            ev.stopPropagation();
            onTogglePin(row.code);
          }}
        >
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <path d="M5 2h6l-1 4 2 3H4l2-3zM8 9v5" fill={pinned ? "currentColor" : "none"} stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
          </svg>
        </button>
      </td>
      <td className="code">{hex(row.code)}</td>
      <td className="name">
        {e?.name ? <span>{e.name}</span> : <span className="muted">unnamed</span>}
        {e?.confidence && e.confidence !== "confirmed" && <span className={`conf ${e.confidence}`}>{e.confidence}</span>}
        {e?.menu && <span className="menu-path">{e.menu}</span>}
      </td>
      <td className="value" title={row.available ? rawText(row.value) : (row.error ?? "not readable")}>
        {row.available ? display(e, row.value) : <span className="muted">{row.error ?? "not readable"}</span>}
        {row.writable && row.available && <span className="rw" title="Writable">rw</span>}
      </td>
      <td className="num muted">{time(row.changedAt)}</td>
    </tr>
  );
});
