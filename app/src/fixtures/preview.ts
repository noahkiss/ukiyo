// FIXTURE: browser preview only. Outside Tauri there is no camera, so `pnpm dev` in a browser
// shows no camera. Add `?camera=tether`, `rawconv`, `cardreader` or `none` to the URL to preview
// a camera state, and `?model=X-T3` (any model name) to preview another body's art. Never used
// inside the app.
import type { Status } from "../bindings";

const BASE: Status = {
  link: "connected",
  reason: null,
  model: "X-T5",
  firmware: "2.10",
  mode: "tether",
  capture: "stills",
  screen: "shooting",
  reopenInS: null,
  cycleMs: 42,
  sweepMs: 1800,
  opMs: 3.1,
  hot: 24,
  readable: 264,
  keepAwake: { enabled: false, previous: null },
};

export function previewStatus(): Status | null {
  if (!import.meta.env.DEV) return null;
  const q = new URLSearchParams(window.location.search);
  // `?model=X-H2S` previews another body; on its own it implies `?camera=tether`.
  const model = q.get("model");
  const base = model ? { ...BASE, model } : BASE;
  switch (q.get("camera") ?? (model ? "tether" : null)) {
    case "tether":
      return base;
    case "rawconv":
      return { ...base, mode: "rawConv" };
    case "cardreader":
      return { ...base, mode: "cardReader" };
    default:
      return null;
  }
}
