import { commands, type Status } from "../bindings";

const MODE: Record<string, string> = {
  tether: "USB tether",
  rawConv: "RAW CONV.",
  cardReader: "Card reader",
  unknown: "Unknown mode",
};

const SCREEN: Record<string, string> = {
  shooting: "Shooting",
  playback: "Playback",
  menu: "Camera in menu",
  unknown: "Screen unknown",
};

const AUTO_OFF: Record<number, string> = { 1: "5 MIN", 2: "2 MIN", 3: "OFF", 4: "1 MIN", 5: "30 SEC", 6: "15 SEC" };

function linkText(s: Status): string {
  const wait = s.reopenInS != null ? ` · reconnect in ${Math.ceil(s.reopenInS)} s` : "";
  switch (s.link) {
    case "connected":
      return "Connected";
    case "connecting":
      return "Connecting";
    case "searching":
      return s.reason === "unplugged" ? "Unplugged · looking for the camera" : "Looking for the camera";
    case "waiting":
      return `Waiting${s.reason ? ` (${s.reason})` : ""}${wait}`;
    case "busy":
      return "Camera busy";
    case "asleep":
      return `Camera powered off${wait}`;
    case "stopped":
      return "Disconnected";
  }
}

function ms(n: number | null): string {
  if (n == null || n === 0) return "—";
  return n >= 1000 ? `${(n / 1000).toFixed(1)} s` : `${n.toFixed(n < 10 ? 1 : 0)} ms`;
}

export function StatusBar({ status, catalogPath, onExport }: { status: Status | null; catalogPath: string | null; onExport: () => void }) {
  if (!status) return <header className="status" data-component="status-bar" />;
  const connected = status.link === "connected" || status.link === "busy";
  const ka = status.keepAwake;
  return (
    <header className="status" data-component="status-bar" data-state={status.link}>
      <div className="status-facts">
        <span className="link" data-state={status.link}>
          {linkText(status)}
        </span>
        {status.model && (
          <span title={catalogPath ? `Catalog: ${catalogPath}` : undefined}>
            {status.model}
            {status.firmware ? ` · fw ${status.firmware}` : ""}
          </span>
        )}
        {connected && status.mode && <span>{MODE[status.mode]}</span>}
        {connected && status.capture && <span>{status.capture === "stills" ? "Stills" : "Movie"}</span>}
        {connected && status.screen && (
          <span className={status.screen === "menu" ? "warn" : undefined} data-state={status.screen}>
            {SCREEN[status.screen]}
          </span>
        )}
        {connected && (
          <span className="timing" title={`${status.hot} hot codes, ${status.readable} readable`}>
            cycle {ms(status.cycleMs)} · sweep {ms(status.sweepMs)} · {ms(status.opMs)}/op
          </span>
        )}
      </div>
      <div className="status-actions">
        <label className="switch">
          <input type="checkbox" checked={ka.enabled} onChange={(e) => void commands.labKeepAwake(e.target.checked)} data-action="keep-awake" />
          Keep camera awake
          {ka.previous != null && <span className="muted"> · auto power off was {AUTO_OFF[ka.previous] ?? ka.previous}</span>}
        </label>
        <button type="button" onClick={onExport} disabled={!connected} data-action="export">
          Export dump
        </button>
        {status.link === "stopped" ? (
          <button type="button" onClick={() => void commands.labConnect(true)} data-action="connect">
            Connect
          </button>
        ) : (
          <button type="button" onClick={() => void commands.labConnect(false)} data-action="disconnect">
            Disconnect
          </button>
        )}
      </div>
    </header>
  );
}
