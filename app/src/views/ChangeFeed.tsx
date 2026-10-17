import type { RefObject } from "react";
import type { ChangeGroup } from "../bindings";
import { display, entry, hex, rawText, time, type Catalog } from "../lib/decode";
import type { Selection } from "../App";

type Props = {
  groups: ChangeGroup[];
  catalog: Catalog | null;
  notes: Map<number, string>;
  showNoisy: boolean;
  setShowNoisy: (b: boolean) => void;
  pendingNote: string;
  setPendingNote: (s: string) => void;
  noteRef: RefObject<HTMLInputElement | null>;
  selected: Selection | null;
  onSelect: (s: Selection) => void;
  onLabel: (s: Selection) => void;
};

export function ChangeFeed(p: Props) {
  return (
    <section className="pane feed" aria-label="Changes" data-component="change-feed">
      <div className="pane-head">
        <h2>Changes</h2>
        <label className="switch">
          <input type="checkbox" checked={p.showNoisy} onChange={(e) => p.setShowNoisy(e.target.checked)} data-action="show-noisy" />
          Show noisy codes
        </label>
      </div>
      <label className="note" data-component="label-mode">
        <span>What I just changed</span>
        <input
          ref={p.noteRef}
          type="text"
          value={p.pendingNote}
          onChange={(e) => p.setPendingNote(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        />
      </label>
      <ol className="scroll groups">
        {p.groups.length === 0 && <li className="empty">No changes yet. Change a setting on the camera.</li>}
        {p.groups.map((g) => {
          const isSel = p.selected?.kind === "group" && p.selected.id === g.id;
          const note = p.notes.get(g.id);
          return (
            <li key={g.id} className={isSel ? "group sel" : "group"} data-source={g.source} data-group={g.id}>
              <div className="group-head">
                <time>{time(g.at)}</time>
                {g.source === "write" && <span className="tag">write</span>}
                {g.changes.length > 1 && <span className="muted">{g.changes.length} codes</span>}
                {note && <span className="group-note">{note}</span>}
              </div>
              <ul>
                {g.changes.map((c) => {
                  const e = entry(p.catalog, c.code);
                  const sel: Selection = { kind: "group", id: g.id, code: c.code };
                  const on = isSel && p.selected?.code === c.code;
                  return (
                    <li key={c.code} className={on ? "change on" : "change"}>
                      <button type="button" className="change-btn" onClick={() => p.onSelect(sel)} onDoubleClick={() => p.onLabel(sel)}>
                        <span className="code">{hex(c.code)}</span>
                        <span className={e?.name ? "name" : "name muted"}>{e?.name ?? "unnamed"}</span>
                        <span className="delta">
                          <span title={rawText(c.old)}>{display(e, c.old)}</span>
                          <span aria-hidden="true"> → </span>
                          <span className="new" title={rawText(c.new)}>
                            {display(e, c.new)}
                          </span>
                        </span>
                      </button>
                      <button type="button" className="quiet label-btn" onClick={() => p.onLabel(sel)} aria-label={`Label ${hex(c.code)}`}>
                        Label
                      </button>
                    </li>
                  );
                })}
              </ul>
            </li>
          );
        })}
      </ol>
      <p className="keys muted" aria-hidden="true">
        j/k move · l label · p pin · / search · n note
      </p>
    </section>
  );
}
