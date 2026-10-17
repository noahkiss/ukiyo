import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { commands, events, type ChangeGroup, type FakeAction, type LabelPatch, type PropRow, type Value } from "../bindings";
import { entry, type Catalog } from "../lib/decode";
import { inTauri, useCamera } from "../lib/camera";
import { useToast } from "../lib/toast";
import { ModeNotice } from "../shell/ModeNotice";
import { PageHead } from "../shell/PageHead";
import { StatusActions, StatusFacts } from "../views/StatusBar";
import { FakeBar } from "../views/FakeBar";
import { PropTable, type Filter } from "../views/PropTable";
import { ChangeFeed } from "../views/ChangeFeed";
import { LabelPanel, type LabelTarget, type Writer } from "../views/LabelPanel";

const PINNED_KEY = "ukiyo.pinned";
const MENU_KEY = "ukiyo.lastMenu";

function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? (JSON.parse(v) as T) : fallback;
  } catch {
    return fallback;
  }
}

function save(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* storage unavailable */
  }
}

export type Selection = { kind: "row"; code: number } | { kind: "group"; id: number; code: number };

/** The live property mapper. It stays mounted while other pages show, so the session's feed
 * and notes survive navigation; `active` gates its keyboard shortcuts. */
export function Lab({ active }: { active: boolean }) {
  const { status } = useCamera();
  const [rows, setRows] = useState<Map<number, PropRow>>(new Map());
  const [groups, setGroups] = useState<ChangeGroup[]>([]);
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [catalogPath, setCatalogPath] = useState<string | null>(null);
  const [fake, setFake] = useState(false);
  const [noisy, setNoisy] = useState<Set<number>>(new Set());
  const [filter, setFilter] = useState<Filter>("all");
  const [search, setSearch] = useState("");
  const [pinned, setPinned] = useState<Set<number>>(() => new Set(load<number[]>(PINNED_KEY, [])));
  const [showNoisy, setShowNoisy] = useState(false);
  const [notes, setNotes] = useState<Map<number, string>>(new Map());
  const [pendingNote, setPendingNote] = useState("");
  const [selected, setSelected] = useState<Selection | null>(null);
  const [target, setTarget] = useState<LabelTarget | null>(null);
  const [lastMenu, setLastMenu] = useState<string>(() => load(MENU_KEY, ""));
  const [now, setNow] = useState(Date.now());
  const searchRef = useRef<HTMLInputElement>(null);
  const noteRef = useRef<HTMLInputElement>(null);
  const pendingRef = useRef("");
  const noisyRef = useRef<Set<number>>(new Set());
  pendingRef.current = pendingNote;
  noisyRef.current = noisy;

  const flash = useToast();

  useEffect(() => {
    let cancelled = false;
    const un: Array<() => void> = [];
    (async () => {
      if (!inTauri) return;
      const s = await commands.labSnapshot();
      if (cancelled) return;
      setRows(new Map(s.rows.map((r) => [r.code, r])));
      setGroups(s.groups);
      setCatalog(s.catalog);
      setCatalogPath(s.catalogPath);
      setFake(s.fake);
      setNoisy(new Set(s.noisy));
      un.push(await events.labProps.listen((e) => setRows(new Map(e.payload.map((r) => [r.code, r])))));
      un.push(await events.catalogChanged.listen((e) => setCatalog(e.payload as Catalog | null)));
      un.push(
        await events.labChanges.listen((e) => {
          const g = e.payload;
          setRows((prev) => {
            const next = new Map(prev);
            for (const c of g.changes) {
              const r = next.get(c.code);
              if (r) next.set(c.code, { ...r, value: c.new, changedAt: g.at });
            }
            return next;
          });
          setGroups((prev) => [g, ...prev].slice(0, 1000));
          // "What I just changed" attaches to the next camera change that is not noise.
          const text = pendingRef.current.trim();
          if (text && g.source === "camera" && g.changes.some((c) => !noisyRef.current.has(c.code))) {
            setNotes((prev) => new Map(prev).set(g.id, text));
            setPendingNote("");
            void commands.labNote(g.id, text);
          }
        }),
      );
    })();
    return () => {
      cancelled = true;
      un.forEach((f) => f());
    };
  }, []);

  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, []);

  useEffect(() => {
    save(PINNED_KEY, [...pinned]);
    if (inTauri) void commands.labPin([...pinned]);
  }, [pinned]);

  const togglePin = useCallback((code: number) => {
    setPinned((prev) => {
      const next = new Set(prev);
      if (next.has(code)) next.delete(code);
      else next.add(code);
      return next;
    });
  }, []);

  const visibleGroups = useMemo(
    () =>
      groups
        .map((g) => (showNoisy ? g : { ...g, changes: g.changes.filter((c) => !noisy.has(c.code)) }))
        .filter((g) => g.changes.length > 0),
    [groups, noisy, showNoisy],
  );

  const openLabel = useCallback(
    (sel: Selection | null) => {
      if (!sel) return;
      if (sel.kind === "row") {
        setTarget({ code: sel.code });
        return;
      }
      const g = groups.find((x) => x.id === sel.id);
      const c = g?.changes.find((x) => x.code === sel.code) ?? g?.changes[0];
      if (c) setTarget({ code: c.code, old: c.old, new: c.new, group: g?.id });
    },
    [groups],
  );

  const saveLabel = useCallback(
    async (patch: LabelPatch) => {
      const r = await commands.labLabel(patch);
      if (r.status === "ok") {
        setCatalog(r.data);
        if (patch.menu) {
          setLastMenu(patch.menu);
          save(MENU_KEY, patch.menu);
        }
        setTarget(null);
        flash("Saved to the catalog");
      } else {
        flash(r.error);
      }
    },
    [flash],
  );

  // The Lab is the mapping tool: its plans may write codes the catalog has not confirmed.
  const writer = useMemo<Writer>(
    () => ({
      plan: async (code: number, value: Value) => {
        const raw = value.kind === "int" ? String(value.v) : value.kind === "str" ? value.v : null;
        if (raw === null) return "This value type cannot be written.";
        const r = await commands.planCreate({ [`0x${code.toString(16).padStart(4, "0")}`]: raw }, true);
        return r.status === "ok" ? r.data : r.error;
      },
      apply: async (planId: string) => {
        const r = await commands.planApply(planId);
        return r.status === "ok" ? r.data : r.error;
      },
      discard: (planId: string) => void commands.planDiscard(planId),
    }),
    [],
  );

  const exportDump = useCallback(async () => {
    const r = await commands.labExport();
    flash(r.status === "ok" ? `Exported to ${r.data}` : r.error);
  }, [flash]);

  const fakeAction = useCallback((a: FakeAction) => void commands.labFake(a), []);

  // Keyboard first: / search, n note, j/k move in the feed, l or Enter label, p pin, Esc close.
  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement;
      const typing = el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT";
      if (e.key === "Escape") {
        if (target) setTarget(null);
        else if (typing) el.blur();
        return;
      }
      if (typing || target || e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === "/") {
        e.preventDefault();
        searchRef.current?.focus();
      } else if (e.key === "n") {
        e.preventDefault();
        noteRef.current?.focus();
      } else if (e.key === "j" || e.key === "k") {
        e.preventDefault();
        if (visibleGroups.length === 0) return;
        const i = selected?.kind === "group" ? visibleGroups.findIndex((g) => g.id === selected.id) : -1;
        const j = Math.max(0, Math.min(visibleGroups.length - 1, i + (e.key === "j" ? 1 : -1)));
        const g = visibleGroups[i < 0 ? 0 : j];
        setSelected({ kind: "group", id: g.id, code: g.changes[0].code });
      } else if (e.key === "l" || e.key === "Enter") {
        e.preventDefault();
        openLabel(selected ?? (visibleGroups[0] ? { kind: "group", id: visibleGroups[0].id, code: visibleGroups[0].changes[0].code } : null));
      } else if (e.key === "p" && selected) {
        togglePin(selected.code);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, target, selected, visibleGroups, openLabel, togglePin]);

  const targetRow = target ? rows.get(target.code) : undefined;

  return (
    <div className="page lab" data-component="lab" hidden={!active}>
      <PageHead title="Lab" sub="Every camera property, read live. Change something on the camera and watch it here.">
        <StatusActions status={status} onExport={exportDump} />
      </PageHead>
      <StatusFacts status={status} catalogPath={catalogPath} />
      <ModeNotice needs="tether" what="Writing and the full property list" />
      {fake && <FakeBar onAction={fakeAction} />}
      <div className="lab-body">
        <PropTable
          rows={rows}
          catalog={catalog}
          filter={filter}
          setFilter={setFilter}
          search={search}
          setSearch={setSearch}
          searchRef={searchRef}
          pinned={pinned}
          quiet={showNoisy ? new Set() : noisy}
          onTogglePin={togglePin}
          selected={selected}
          onSelect={setSelected}
          onLabel={(code) => openLabel({ kind: "row", code })}
          now={now}
        />
        <aside className="lab-aside">
          {target && (
            <LabelPanel
              key={`${target.code}:${target.group ?? "row"}`}
              target={target}
              row={targetRow}
              entry={entry(catalog, target.code)}
              catalog={catalog}
              lastMenu={lastMenu}
              connected={status?.link === "connected" && status.screen !== "menu"}
              onSave={saveLabel}
              writer={writer}
              onClose={() => setTarget(null)}
            />
          )}
          <ChangeFeed
            groups={visibleGroups}
            catalog={catalog}
            notes={notes}
            showNoisy={showNoisy}
            setShowNoisy={setShowNoisy}
            pendingNote={pendingNote}
            setPendingNote={setPendingNote}
            noteRef={noteRef}
            selected={selected}
            onSelect={setSelected}
            onLabel={openLabel}
          />
        </aside>
      </div>
    </div>
  );
}
