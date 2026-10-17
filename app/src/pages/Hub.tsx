import { useCallback, useEffect, useMemo, useState } from "react";
import { commands, type HistoryView } from "../bindings";
import * as fx from "../fixtures/hub";
import type { DayShots, OwnerDetails } from "../fixtures/hub";
import { inTauri, useCamera } from "../lib/camera";
import { display, entry } from "../lib/decode";
import { codeOf, useLive, type Live } from "../lib/live";
import { writeNow } from "../lib/plans";
import { usePrefs } from "../lib/prefs";
import { useToast } from "../lib/toast";
import { CameraChip } from "../shell/CameraChip";
import { ModeNotice } from "../shell/ModeNotice";
import { PageHead } from "../shell/PageHead";
import { RefreshIcon } from "../shell/icons";
import { cameraArt } from "../assets/camera";

const n = (x: number) => x.toLocaleString("en-US");

/** Everything the Hub shows: from the camera and its history in the app, from the fixtures
 * in the browser preview. A missing value is `null`, never invented. */
type HubData = {
  readAt: string | null;
  model: string | null;
  firmware: string | null;
  serialTail: string | null;
  lens: string | null;
  battery: string | null;
  batteryPct: number | null;
  cards: fx.HubCamera["cards"] | null;
  /** Shots left on the card, as the camera reports it. */
  shotsLeft: string | null;
  shots: { total: number; sinceLast: number | null; lastConnect: string | null; mechanical: number | null; electronic: number | null } | null;
  firstConnected: string | null;
  perDay: DayShots[];
  perMonth: Array<{ month: string; shots: number }>;
  lenses: fx.LensSeen[];
  connects: Array<{ when: string; shots: number | null }>;
  owner: OwnerDetails | null;
};

const fmtDate = (ms: number) => new Date(ms).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
const fmtShort = (ms: number) => new Date(ms).toLocaleDateString("en-US", { month: "short", day: "numeric" });
const fmtWhen = (ms: number) => {
  const d = new Date(ms);
  const time = d.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
  return d.toDateString() === new Date().toDateString() ? `Today, ${time}` : `${fmtShort(ms)}, ${time}`;
};
const isoDay = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

function fixtureData(): HubData {
  const c = fx.hubCamera;
  const s = fx.shotCounts;
  return {
    readAt: fx.hubReadAt,
    model: c.model,
    firmware: c.firmware,
    serialTail: c.serialTail,
    lens: c.lens,
    battery: `${c.batteryPct}%`,
    batteryPct: c.batteryPct,
    cards: c.cards,
    shotsLeft: null,
    shots: { total: s.total, sinceLast: s.sinceLast, lastConnect: s.lastConnect, mechanical: s.mechanical, electronic: s.electronic },
    firstConnected: s.firstConnected,
    perDay: fx.shotsPerDay,
    perMonth: fx.shotsPerMonth,
    lenses: fx.lensesSeen,
    connects: fx.connectLog,
    owner: fx.ownerDetails,
  };
}

/** The last 30 days, oldest first, with a zero for a day with no reading. */
function last30(perDay: HistoryView["per_day"]): DayShots[] {
  const by = new Map(perDay.map((d) => [d.date, d.shots]));
  const today = new Date();
  return Array.from({ length: 30 }, (_, i) => {
    const d = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 29 + i);
    const date = isoDay(d);
    return { date, shots: by.get(date) ?? 0 };
  });
}

/** The last 12 months, oldest first. */
function last12(perMonth: HistoryView["per_month"]): Array<{ month: string; shots: number }> {
  const by = new Map(perMonth.map((m) => [m.month, m.shots]));
  const today = new Date();
  return Array.from({ length: 12 }, (_, i) => {
    const d = new Date(today.getFullYear(), today.getMonth() - 11 + i, 1);
    const month = isoDay(d).slice(0, 7);
    return { month, shots: by.get(month) ?? 0 };
  });
}

const OWNER_NAMES: Record<keyof OwnerDetails, string> = { copyright: "Copyright", author: "CopyrightAuthor", prefix: "FileNamePrefix1", deviceName: "DeviceName" };

function ownerOf(live: Live): OwnerDetails | null {
  const out: Partial<OwnerDetails> = {};
  for (const [k, name] of Object.entries(OWNER_NAMES) as Array<[keyof OwnerDetails, string]>) {
    const code = codeOf(live, name);
    const v = code == null ? null : live.rows.get(code);
    if (!v?.available || v.value?.kind !== "str") return null;
    out[k] = v.value.v;
  }
  return out as OwnerDetails;
}

function liveData(status: ReturnType<typeof useCamera>["status"], live: Live, hist: HistoryView | null): HubData {
  const connected = status?.link === "connected";
  const read = (name: string) => {
    const code = codeOf(live, name);
    const r = code == null ? undefined : live.rows.get(code);
    return connected && r?.available && r.value ? display(entry(live.catalog, r.code), r.value) : null;
  };
  const battery = read("BatteryLevel");
  const pct = battery?.match(/^(\d+)\s*%$/);
  const prev = hist?.connects.find((c, i) => i > 0 && c.shots != null);
  return {
    readAt: connected && live.readAt ? new Date(live.readAt).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" }) : null,
    model: status?.model ?? hist?.model ?? null,
    firmware: status?.firmware ?? hist?.connects[0]?.firmware ?? null,
    serialTail: null,
    lens: null,
    battery,
    batteryPct: pct ? Number(pct[1]) : null,
    cards: null,
    shotsLeft: read("CaptureRemaining"),
    shots:
      hist?.shots != null
        ? { total: hist.shots, sinceLast: prev?.shots != null ? hist.shots - prev.shots : null, lastConnect: prev?.at != null ? fmtShort(prev.at) : null, mechanical: null, electronic: null }
        : null,
    firstConnected: hist?.first_connected != null ? fmtDate(hist.first_connected) : null,
    perDay: hist ? last30(hist.per_day) : [],
    perMonth: hist ? last12(hist.per_month) : [],
    lenses: (hist?.lenses ?? []).map((l) => ({ name: l.name, seen: l.last_seen != null ? fmtShort(l.last_seen) : "", attached: false })),
    connects: (hist?.connects ?? []).slice(0, 8).map((c) => ({ when: c.at != null ? fmtWhen(c.at) : "", shots: c.shots })),
    owner: connected ? ownerOf(live) : null,
  };
}

/** This camera's history; with no camera, the one seen last. */
function useHistory(cameraId: string | null | undefined, link: string | undefined) {
  const [hist, setHist] = useState<HistoryView | null>(null);
  const load = useCallback(async () => {
    const r = await commands.libraryRead("history", null);
    if (r.status !== "ok" || r.data.kind !== "history") return;
    const all = r.data.item;
    const mine = cameraId ? all.find((h) => h.camera === cameraId) : undefined;
    setHist(mine ?? [...all].sort((a, b) => (b.last_connected ?? 0) - (a.last_connected ?? 0))[0] ?? null);
  }, [cameraId]);
  useEffect(() => {
    if (inTauri) void load();
  }, [load, link]);
  return { hist, reload: load };
}

export function Hub() {
  const { status } = useCamera();
  const live = useLive();
  const { hist, reload } = useHistory(status?.cameraId, status?.link);
  const d = useMemo(() => (inTauri ? liveData(status, live, hist) : fixtureData()), [status, live, hist]);
  const readAgain = () => {
    live.reload();
    void reload();
  };
  return (
    <div className="page hub" data-component="hub">
      <PageHead title="Hub" sub={d.readAt ? `Read from the camera ${inTauri ? "at " : ""}${d.readAt}.` : "No camera connected."}>
        <CameraChip />
        <button type="button" className="btn" onClick={readAgain}>
          <RefreshIcon />
          Read again
        </button>
      </PageHead>
      <ModeNotice needs="tether" what="Reading the camera's details" />
      <KeepAwakeLine />
      <CameraCard d={d} />
      <div className="flex-row stretch">
        <ShotHistory perDay={d.perDay} perMonth={d.perMonth} />
        <section className="card pad lenses" aria-labelledby="lenses">
          <h2 id="lenses" className="card-title">
            Lenses seen
          </h2>
          <ul className="rows">
            {d.lenses.map((l) => (
              <li key={l.name} className="row-line">
                <span className="strong">{l.name}</span>
                <span className={l.attached ? "small red" : "small muted"}>{l.seen}</span>
              </li>
            ))}
          </ul>
          {d.lenses.length === 0 && <p className="small muted">No lenses recorded yet.</p>}
        </section>
      </div>
      <div className="flex-row stretch">
        <Owner saved={d.owner} onWritten={live.reload} />
        <section className="card pad connect-log" aria-labelledby="log">
          <h2 id="log" className="card-title">
            Connect log
          </h2>
          <ul className="rows">
            {d.connects.map((c, i) => (
              <li key={`${c.when}-${i}`} className="row-line">
                <span>{c.when}</span>
                <span className="mono small muted">{c.shots != null ? n(c.shots) : "—"}</span>
              </li>
            ))}
          </ul>
          {d.connects.length === 0 && <p className="small muted">No connects recorded yet.</p>}
        </section>
      </div>
    </div>
  );
}

function KeepAwakeLine() {
  const { status } = useCamera();
  if (!status?.keepAwake.active) return null;
  return (
    <p className="small muted keep-awake-line" data-component="keep-awake-line">
      Keeping the camera awake while you use Ukiyo.
    </p>
  );
}

function CameraCard({ d }: { d: HubData }) {
  const { theme } = usePrefs();
  const art = cameraArt(d.model ?? "");
  const s = d.shots;
  return (
    <section className="card camera" aria-label="Camera">
      <div className="camera-art">
        <div className="sun" />
        <img src={theme === "dark" ? art.dark : art.light} alt={art.alt} />
      </div>
      <div className="camera-facts">
        <div>
          <div className="big-name">{d.model ?? "No camera"}</div>
          <div className="small muted facts-line">
            {d.firmware && <>Firmware {d.firmware}</>}
            {d.serialTail && (
              <>
                {" "}
                · Body serial <span className="mono">••••{d.serialTail}</span>
              </>
            )}
          </div>
        </div>
        <dl className="facts">
          <div className="fact">
            <dt>Lens attached</dt>
            <dd className={d.lens ? undefined : "muted"}>{d.lens ?? "Not read"}</dd>
          </div>
          <div className="fact col">
            <div className="fact-row">
              <dt>Battery</dt>
              <dd className={d.battery ? undefined : "muted"}>{d.battery ?? "—"}</dd>
            </div>
            {d.batteryPct != null && <Meter pct={d.batteryPct} label="Battery" />}
          </div>
          {d.cards?.map((card) =>
            "empty" in card ? (
              <div className="fact" key={card.slot}>
                <dt>Card slot {card.slot}</dt>
                <dd className="muted">No card</dd>
              </div>
            ) : (
              <div className="fact col" key={card.slot}>
                <div className="fact-row">
                  <dt>Card slot {card.slot}</dt>
                  <dd>
                    {card.freeGb} GB free of {card.totalGb} GB
                  </dd>
                </div>
                <Meter pct={(1 - card.freeGb / card.totalGb) * 100} label={`Card slot ${card.slot} used`} />
              </div>
            ),
          )}
          {!d.cards && (
            <div className="fact">
              <dt>Shots left on the card</dt>
              <dd className={d.shotsLeft ? undefined : "muted"}>{d.shotsLeft ?? "—"}</dd>
            </div>
          )}
        </dl>
      </div>
      <div className="camera-shots">
        <div className="small muted">Shots taken</div>
        <div className="big-number">{s ? n(s.total) : "—"}</div>
        {s?.sinceLast != null && (
          <div className="small">
            <span className="red strong">+{n(s.sinceLast)}</span> since last connect{s.lastConnect && ` on ${s.lastConnect}`}
          </div>
        )}
        {!s && <div className="small muted">Counted in tether mode with the STILL switch.</div>}
        <div className="grow" />
        <dl className="mini-facts">
          {s?.mechanical != null && (
            <div>
              <dt>Mechanical shutter</dt>
              <dd className="mono">{n(s.mechanical)}</dd>
            </div>
          )}
          {s?.electronic != null && (
            <div>
              <dt>Electronic shutter</dt>
              <dd className="mono">{n(s.electronic)}</dd>
            </div>
          )}
          <div>
            <dt>First connected</dt>
            <dd>{d.firstConnected ?? "—"}</dd>
          </div>
        </dl>
      </div>
    </section>
  );
}

function Meter({ pct, label }: { pct: number; label: string }) {
  return (
    <div className="meter" role="meter" aria-label={label} aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100}>
      <div style={{ width: `${pct}%` }} />
    </div>
  );
}

type Range = "day" | "month" | "since";

const fmtDay = (iso: string) => new Date(iso + "T12:00:00").toLocaleDateString("en-US", { month: "short", day: "numeric" });
const fmtMonth = (ym: string) => new Date(ym + "-15T12:00:00").toLocaleDateString("en-US", { month: "short", year: "2-digit" });

function ShotHistory({ perDay: shotsPerDay, perMonth: shotsPerMonth }: { perDay: DayShots[]; perMonth: Array<{ month: string; shots: number }> }) {
  const [range, setRange] = useState<Range>("day");
  const [since, setSince] = useState(shotsPerDay[0]?.date ?? "");

  const view = useMemo(() => {
    if (range === "month") {
      const bars = shotsPerMonth.map((m) => ({ key: m.month, label: fmtMonth(m.month), shots: m.shots }));
      const total = bars.reduce((a, b) => a + b.shots, 0);
      const best = bars.length ? bars.reduce((a, b) => (b.shots > a.shots ? b : a)) : { label: "", shots: 0 };
      return {
        bars,
        stats: [
          ["Last 12 months", n(total)],
          ["Monthly average", n(Math.round(total / bars.length))],
          ["Busiest month", best.shots ? n(best.shots) : "—", best.shots ? best.label : undefined],
        ] as Array<[string, string, string?]>,
        ticks: bars.length >= 12 ? [bars[0].label, bars[3].label, bars[6].label, bars[9].label, "This month"] : bars.map((b) => b.label),
        unit: "month",
      };
    }
    const days = range === "since" ? shotsPerDay.filter((d) => d.date >= since) : shotsPerDay;
    const bars = days.map((d) => ({ key: d.date, label: fmtDay(d.date), shots: d.shots }));
    const total = bars.reduce((a, b) => a + b.shots, 0);
    const best = bars.length ? bars.reduce((a, b) => (b.shots > a.shots ? b : a)) : null;
    const step = Math.max(1, Math.floor((bars.length - 1) / 4));
    const ticks = bars.length > 1 ? [0, step, step * 2, step * 3].filter((i) => i < bars.length - 1).map((i) => bars[i].label).concat("Today") : bars.map((b) => b.label);
    return {
      bars,
      stats: [
        [range === "since" ? `Since ${fmtDay(since)}` : "Last 30 days", n(total)],
        ["Daily average", n(bars.length ? Math.round(total / bars.length) : 0)],
        ["Busiest day", best?.shots ? n(best.shots) : "—", best?.shots ? best.label : undefined],
      ] as Array<[string, string, string?]>,
      ticks,
      unit: "day",
    };
  }, [range, since, shotsPerDay, shotsPerMonth]);

  const max = Math.max(1, ...view.bars.map((b) => b.shots));
  const top = niceTop(max);
  const best = view.stats[2];

  return (
    <section className="card pad history" aria-labelledby="hist">
      <div className="card-head">
        <h2 id="hist" className="card-title">
          Shot history
        </h2>
        <div className="seg" role="group" aria-label="Group shots by">
          {(
            [
              ["day", "Per day"],
              ["month", "Per month"],
              ["since", "Since a date"],
            ] as Array<[Range, string]>
          ).map(([r, label]) => (
            <button key={r} type="button" aria-pressed={range === r} onClick={() => setRange(r)}>
              {label}
            </button>
          ))}
        </div>
      </div>
      {range === "since" && (
        <label className="field inline-field">
          From
          <input type="date" value={since} min={shotsPerDay[0]?.date} max={shotsPerDay[shotsPerDay.length - 1]?.date} onChange={(e) => e.target.value && setSince(e.target.value)} />
        </label>
      )}
      <div className="stats">
        {view.stats.map(([label, value, note]) => (
          <div key={label}>
            <div className="small muted">{label}</div>
            <div className="stat">
              {value} {note && <span className="stat-note">{note}</span>}
            </div>
          </div>
        ))}
      </div>
      <div className="chart" role="img" aria-label={`Bar chart of shots per ${view.unit}. Busiest ${view.unit} ${best[2] ?? ""} with ${best[1]} shots.`}>
        <div className="chart-y" aria-hidden="true">
          {[top, (top * 2) / 3, top / 3, 0].map((v) => (
            <span key={v}>{n(Math.round(v))}</span>
          ))}
        </div>
        <div className="chart-plot">
          <div className="chart-area">
            <div className="grid-line" style={{ top: 0 }} />
            <div className="grid-line" style={{ top: "33.3%" }} />
            <div className="grid-line" style={{ top: "66.6%" }} />
            <div className="bars">
              {view.bars.map((b) => (
                <div key={b.key} className={b.shots === 0 ? "bar zero" : "bar"} title={`${b.label}: ${n(b.shots)} shots`} style={{ height: b.shots === 0 ? 2 : `${(b.shots / top) * 100}%` }} />
              ))}
            </div>
          </div>
          <div className="chart-x" aria-hidden="true">
            {view.ticks.map((t) => (
              <span key={t}>{t}</span>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

/** An axis top whose thirds are round numbers: 180 for a largest value of 168. */
function niceTop(max: number): number {
  const third = max / 3;
  const mag = 10 ** Math.floor(Math.log10(third));
  for (const m of [1, 1.5, 2, 2.5, 3, 4, 5, 6, 7.5, 8, 10]) if (m * mag >= third) return 3 * m * mag;
  return 30 * mag;
}

function Owner({ saved, onWritten }: { saved: OwnerDetails | null; onWritten: () => void }) {
  const flash = useToast();
  const blank: OwnerDetails = { copyright: "", author: "", prefix: "", deviceName: "" };
  const base = saved ?? blank;
  const [draft, setDraft] = useState<OwnerDetails>(base);
  const [busy, setBusy] = useState(false);
  // A new reading of the camera replaces the draft.
  const savedKey = JSON.stringify(saved);
  useEffect(() => setDraft(saved ?? blank), [savedKey]); // eslint-disable-line react-hooks/exhaustive-deps
  const changed = (Object.keys(draft) as Array<keyof OwnerDetails>).filter((k) => draft[k] !== base[k]);
  const changes = changed.length;
  const set = (k: keyof OwnerDetails) => (e: React.ChangeEvent<HTMLInputElement>) => setDraft({ ...draft, [k]: e.target.value });
  const write = async () => {
    if (!inTauri) {
      flash("Writing owner details to the camera is not built yet.");
      return;
    }
    setBusy(true);
    const msg = await writeNow(Object.fromEntries(changed.map((k) => [OWNER_NAMES[k], draft[k]])));
    setBusy(false);
    flash(msg);
    onWritten();
  };
  return (
    <section className="card pad owner" aria-labelledby="owner">
      <div className="card-head baseline">
        <h2 id="owner" className="card-title">
          Owner details
        </h2>
        <span className="small muted">Stored on the camera and written into every photo</span>
      </div>
      <div className="owner-grid">
        <label className="field">
          Copyright
          <input type="text" value={draft.copyright} disabled={!saved} onChange={set("copyright")} />
        </label>
        <label className="field">
          Author
          <input type="text" value={draft.author} disabled={!saved} onChange={set("author")} />
        </label>
        <label className={draft.prefix !== base.prefix ? "field changed" : "field"}>
          File name prefix, 4 characters
          <input type="text" className="mono" value={draft.prefix} maxLength={4} disabled={!saved} onChange={set("prefix")} />
        </label>
        <label className="field">
          Device name
          <input type="text" value={draft.deviceName} disabled={!saved} onChange={set("deviceName")} />
        </label>
      </div>
      <div className="actions-line">
        <button type="button" className="btn primary" disabled={changes === 0 || busy} onClick={() => void write()}>
          Write to camera
        </button>
        <button type="button" className="btn text" disabled={changes === 0} onClick={() => setDraft(base)}>
          Revert
        </button>
        <span className="small muted">{!saved ? "Not read: connect the camera in tether mode" : changes === 0 ? "No changes" : `${changes} ${changes === 1 ? "change" : "changes"} not written`}</span>
      </div>
    </section>
  );
}
