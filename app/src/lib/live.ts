import { useCallback, useEffect, useState } from "react";
import { commands, events, type PropRow, type Value } from "../bindings";
import { inTauri } from "./camera";
import { hex, type Catalog } from "./decode";

/** The camera's properties and the catalog that names them, kept live from `serve`. */
export type Live = { rows: Map<number, PropRow>; catalog: Catalog | null; readAt: number | null; reload: () => void };

export function useLive(): Live {
  const [rows, setRows] = useState<Map<number, PropRow>>(new Map());
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [readAt, setReadAt] = useState<number | null>(null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    if (!inTauri) return;
    let cancelled = false;
    const un: Array<() => void> = [];
    (async () => {
      const s = await commands.labSnapshot();
      if (cancelled) return;
      setRows(new Map(s.rows.map((r) => [r.code, r])));
      setCatalog(s.catalog);
      setReadAt(Date.now());
      un.push(
        await events.labProps.listen((e) => {
          setRows(new Map(e.payload.map((r) => [r.code, r])));
          setReadAt(Date.now());
        }),
      );
      un.push(await events.catalogChanged.listen((e) => setCatalog(e.payload as Catalog | null)));
      un.push(
        await events.labChanges.listen((e) => {
          setRows((prev) => {
            const next = new Map(prev);
            for (const c of e.payload.changes) {
              const r = next.get(c.code);
              if (r) next.set(c.code, { ...r, value: c.new, changedAt: e.payload.at });
            }
            return next;
          });
          setReadAt(Date.now());
        }),
      );
      if (cancelled) un.forEach((f) => f());
    })();
    return () => {
      cancelled = true;
      un.forEach((f) => f());
    };
  }, [tick]);

  const reload = useCallback(() => setTick((t) => t + 1), []);
  return { rows, catalog, readAt, reload };
}

/** The code the catalog gives this name, if the camera has it. */
export function codeOf(live: Pick<Live, "rows" | "catalog">, name: string): number | null {
  for (const [k, e] of Object.entries(live.catalog?.properties ?? {})) {
    if (e.name !== name) continue;
    const code = parseInt(k, 16);
    if (live.rows.has(code)) return code;
  }
  return null;
}

/** A value as a plan takes it: the raw number or the text. */
export function rawOf(v: Value): string | null {
  return v.kind === "int" ? String(v.v) : v.kind === "str" ? v.v : null;
}

export { hex };
