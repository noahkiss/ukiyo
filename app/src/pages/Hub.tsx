import { useMemo, useState } from "react";
import { connectLog, hubCamera, hubReadAt, lensesSeen, ownerDetails, shotCounts, shotsPerDay, shotsPerMonth, type OwnerDetails } from "../fixtures/hub";
import { usePrefs } from "../lib/prefs";
import { useNotBuilt } from "../lib/toast";
import { CameraChip } from "../shell/CameraChip";
import { ModeNotice } from "../shell/ModeNotice";
import { PageHead } from "../shell/PageHead";
import { RefreshIcon } from "../shell/icons";
import { cameraArt } from "../assets/camera";
import { useCamera } from "../lib/camera";

const n = (x: number) => x.toLocaleString("en-US");

export function Hub() {
  const notBuilt = useNotBuilt();
  return (
    <div className="page hub" data-component="hub">
      <PageHead title="Hub" sub={`Read from the camera ${hubReadAt}.`}>
        <CameraChip />
        <button type="button" className="btn" onClick={() => notBuilt("Reading from the camera")}>
          <RefreshIcon />
          Read again
        </button>
      </PageHead>
      <ModeNotice needs="tether" what="Reading the camera's details" />
      <KeepAwakeLine />
      <CameraCard />
      <div className="flex-row stretch">
        <ShotHistory />
        <section className="card pad lenses" aria-labelledby="lenses">
          <h2 id="lenses" className="card-title">
            Lenses seen
          </h2>
          <ul className="rows">
            {lensesSeen.map((l) => (
              <li key={l.name} className="row-line">
                <span className="strong">{l.name}</span>
                <span className={l.attached ? "small red" : "small muted"}>{l.seen}</span>
              </li>
            ))}
          </ul>
        </section>
      </div>
      <div className="flex-row stretch">
        <Owner />
        <section className="card pad connect-log" aria-labelledby="log">
          <h2 id="log" className="card-title">
            Connect log
          </h2>
          <ul className="rows">
            {connectLog.map((c) => (
              <li key={c.when} className="row-line">
                <span>{c.when}</span>
                <span className="mono small muted">{n(c.shots)}</span>
              </li>
            ))}
          </ul>
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

function CameraCard() {
  const { theme } = usePrefs();
  const { status } = useCamera();
  const c = hubCamera;
  const model = status?.model ?? c.model;
  const art = cameraArt(model);
  return (
    <section className="card camera" aria-label="Camera">
      <div className="camera-art">
        <div className="sun" />
        <img src={theme === "dark" ? art.dark : art.light} alt={art.alt} />
      </div>
      <div className="camera-facts">
        <div>
          <div className="big-name">{model}</div>
          <div className="small muted facts-line">
            Firmware {c.firmware} · Body serial <span className="mono">••••{c.serialTail}</span>
          </div>
        </div>
        <dl className="facts">
          <div className="fact">
            <dt>Lens attached</dt>
            <dd>{c.lens}</dd>
          </div>
          <div className="fact col">
            <div className="fact-row">
              <dt>Battery</dt>
              <dd>{c.batteryPct}%</dd>
            </div>
            <Meter pct={c.batteryPct} label="Battery" />
          </div>
          {c.cards.map((card) =>
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
        </dl>
      </div>
      <div className="camera-shots">
        <div className="small muted">Shots taken</div>
        <div className="big-number">{n(shotCounts.total)}</div>
        <div className="small">
          <span className="red strong">+{shotCounts.sinceLast}</span> since last connect on {shotCounts.lastConnect}
        </div>
        <div className="grow" />
        <dl className="mini-facts">
          <div>
            <dt>Mechanical shutter</dt>
            <dd className="mono">{n(shotCounts.mechanical)}</dd>
          </div>
          <div>
            <dt>Electronic shutter</dt>
            <dd className="mono">{n(shotCounts.electronic)}</dd>
          </div>
          <div>
            <dt>First connected</dt>
            <dd>{shotCounts.firstConnected}</dd>
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

function ShotHistory() {
  const [range, setRange] = useState<Range>("day");
  const [since, setSince] = useState(shotsPerDay[0].date);

  const view = useMemo(() => {
    if (range === "month") {
      const bars = shotsPerMonth.map((m) => ({ key: m.month, label: fmtMonth(m.month), shots: m.shots }));
      const total = bars.reduce((a, b) => a + b.shots, 0);
      const best = bars.reduce((a, b) => (b.shots > a.shots ? b : a));
      return {
        bars,
        stats: [
          ["Last 12 months", n(total)],
          ["Monthly average", n(Math.round(total / bars.length))],
          ["Busiest month", n(best.shots), best.label],
        ] as Array<[string, string, string?]>,
        ticks: [bars[0].label, bars[3].label, bars[6].label, bars[9].label, "This month"],
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
        ["Busiest day", best ? n(best.shots) : "—", best?.label],
      ] as Array<[string, string, string?]>,
      ticks,
      unit: "day",
    };
  }, [range, since]);

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
          <input type="date" value={since} min={shotsPerDay[0].date} max={shotsPerDay[shotsPerDay.length - 1].date} onChange={(e) => e.target.value && setSince(e.target.value)} />
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

function Owner() {
  const notBuilt = useNotBuilt();
  const saved = ownerDetails;
  const [draft, setDraft] = useState<OwnerDetails>(ownerDetails);
  const changes = (Object.keys(draft) as Array<keyof OwnerDetails>).filter((k) => draft[k] !== saved[k]).length;
  const set = (k: keyof OwnerDetails) => (e: React.ChangeEvent<HTMLInputElement>) => setDraft({ ...draft, [k]: e.target.value });
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
          <input type="text" value={draft.copyright} onChange={set("copyright")} />
        </label>
        <label className="field">
          Author
          <input type="text" value={draft.author} onChange={set("author")} />
        </label>
        <label className={draft.prefix !== saved.prefix ? "field changed" : "field"}>
          File name prefix, 4 characters
          <input type="text" className="mono" value={draft.prefix} maxLength={4} onChange={set("prefix")} />
        </label>
        <label className="field">
          Device name
          <input type="text" value={draft.deviceName} onChange={set("deviceName")} />
        </label>
      </div>
      <div className="actions-line">
        <button
          type="button"
          className="btn primary"
          disabled={changes === 0}
          onClick={() => notBuilt("Writing owner details to the camera")}
        >
          Write to camera
        </button>
        <button type="button" className="btn text" disabled={changes === 0} onClick={() => setDraft(saved)}>
          Revert
        </button>
        <span className="small muted">{changes === 0 ? "No changes" : `${changes} ${changes === 1 ? "change" : "changes"} not written`}</span>
      </div>
    </section>
  );
}
