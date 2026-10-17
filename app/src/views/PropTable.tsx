import { memo, useMemo, type RefObject } from "react";
import type { PropRow } from "../bindings";
import { display, entry, hex, rawText, time, type Catalog } from "../lib/decode";
import type { Selection } from "../pages/Lab";
import { PinIcon, SearchIcon } from "../shell/icons";

export type Filter = "all" | "changed" | "unnamed" | "writable" | "pinned";

const FILTERS: Array<[Filter, string]> = [
  ["all", "All"],
  ["changed", "Changed"],
  ["unnamed", "Unlabelled"],
  ["writable", "Writable"],
  ["pinned", "Pinned"],
];

const CONFIDENCE: Record<string, [string, string]> = {
  confirmed: ["Confirmed", "badge"],
  likely: ["Likely", "badge warn"],
  guess: ["Guess", "badge warn"],
};

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

/** The raw value, short: hex for numbers, the type for others. */
function rawShort(v: PropRow["value"]): string {
  if (!v) return "";
  if (v.kind === "int") return "0x" + (v.v < 0 ? v.v + 2 ** 32 : v.v).toString(16).toUpperCase().padStart(4, "0");
  if (v.kind === "str") return "string";
  if (v.kind === "array") return `array[${v.v.length}]`;
  return v.v.length > 12 ? v.v.slice(0, 12) + "…" : v.v;
}

export function PropTable(p: Props) {
  const list = useMemo(() => {
    const q = p.search.trim().toLowerCase();
    return [...p.rows.values()].filter((r) => {
      const e = entry(p.catalog, r.code);
      if (p.filter === "unnamed" && e?.name) return false;
      if (p.filter === "changed" && r.changedAt == null) return false;
      if (p.filter === "pinned" && !p.pinned.has(r.code)) return false;
      if (p.filter === "writable" && !r.writable) return false;
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
      writable: all.filter((r) => r.writable).length,
    };
  }, [p.rows, p.catalog, p.pinned]);

  return (
    <>
      <div className="lab-tools">
        <label className="search-box">
          <SearchIcon />
          <span className="visually-hidden">Filter properties</span>
          <input ref={p.searchRef} type="search" placeholder="Filter by code, name or value" value={p.search} onChange={(e) => p.setSearch(e.target.value)} />
        </label>
        <div className="seg" role="group" aria-label="Show">
          {FILTERS.map(([f, label]) => (
            <button key={f} type="button" aria-pressed={p.filter === f} onClick={() => p.setFilter(f)} data-action={`filter-${f}`}>
              {label} <span className="n">{counts[f]}</span>
            </button>
          ))}
        </div>
      </div>
      <section className="card lab-table" aria-label="Properties" data-component="prop-table">
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
                <th>Label</th>
                <th>Value</th>
                <th>Raw</th>
                <th>Status</th>
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
        {p.rows.size > 0 && (
          <p className="table-foot">
            Showing {list.length} of {p.rows.size}
          </p>
        )}
      </section>
    </>
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
  const conf = e?.name ? CONFIDENCE[e.confidence ?? ""] : undefined;
  const status = conf ? <span className={conf[1]}>{conf[0]}</span> : <span className="badge dashed">Unlabelled</span>;
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
          <PinIcon filled={pinned} />
        </button>
      </td>
      <td className="code">
        {hex(row.code)}
        {row.writable && row.available && (
          <span className="rw" title="Writable">
            rw
          </span>
        )}
      </td>
      <td className="name">
        {e?.name ? <span>{e.name}</span> : <span className="muted">Unlabelled</span>}
        {e?.menu && <span className="menu-path">{e.menu}</span>}
      </td>
      <td className="value" title={row.available ? rawText(row.value) : (row.error ?? "not readable")}>
        {row.available ? display(e, row.value) : <span className="muted">{row.error ?? "not readable"}</span>}
      </td>
      <td className="raw">{row.available ? rawShort(row.value) : ""}</td>
      <td>{status}</td>
      <td className="num when">{time(row.changedAt) || "—"}</td>
    </tr>
  );
});
