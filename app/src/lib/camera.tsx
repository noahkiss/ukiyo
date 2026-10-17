import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { commands, events, type Status, type UsbMode } from "../bindings";
import { previewStatus } from "../fixtures/preview";

export const inTauri = "__TAURI_INTERNALS__" in window;

/** What every page needs to know about the camera: the Lab's live status. */
export type Camera = { status: Status | null; fake: boolean };

const CameraContext = createContext<Camera>({ status: null, fake: false });

export function CameraProvider({ children }: { children: ReactNode }) {
  const [cam, setCam] = useState<Camera>(() => ({ status: inTauri ? null : previewStatus(), fake: false }));

  useEffect(() => {
    if (!inTauri) return;
    let cancelled = false;
    let un: (() => void) | undefined;
    (async () => {
      const s = await commands.labSnapshot();
      if (cancelled) return;
      setCam({ status: s.status, fake: s.fake });
      un = await events.labStatus.listen((e) => setCam((c) => ({ ...c, status: e.payload })));
      if (cancelled) un();
    })();
    return () => {
      cancelled = true;
      un?.();
    };
  }, []);

  // Input in the window tells the owner Ukiyo is in use, for keep-awake: at most every 30 s.
  useEffect(() => {
    if (!inTauri) return;
    let last = 0;
    const ping = () => {
      const now = Date.now();
      if (now - last < 30_000) return;
      last = now;
      void commands.activity();
    };
    const kinds = ["pointerdown", "keydown", "wheel"] as const;
    for (const k of kinds) window.addEventListener(k, ping, { passive: true });
    return () => {
      for (const k of kinds) window.removeEventListener(k, ping);
    };
  }, []);

  return <CameraContext.Provider value={cam}>{children}</CameraContext.Provider>;
}

export const useCamera = () => useContext(CameraContext);

export const MODE_NAME: Record<UsbMode, string> = {
  tether: "Tether mode",
  rawConv: "RAW CONV. mode",
  cardReader: "Card reader mode",
  unknown: "Unknown mode",
};

export type CameraState = "none" | "wrongMode" | "connected";

/** Where the camera stands, as the Connect page and the sidebar show it. */
export function cameraState(s: Status | null): CameraState {
  if (!s) return "none";
  if (s.mode === "cardReader") return "wrongMode";
  return s.link === "connected" || s.link === "busy" ? "connected" : "none";
}

/** The two lines of the sidebar's camera card, and the header chip. */
export function cameraLines(s: Status | null): { camera: string; detail: string; ok: boolean } {
  const st = cameraState(s);
  if (!s || (st === "none" && !s.model)) return { camera: "No camera", detail: waitText(s), ok: false };
  const model = s.model ?? "Camera";
  if (st === "wrongMode") return { camera: model, detail: "Card reader mode", ok: false };
  if (st === "connected") return { camera: model, detail: s.mode ? MODE_NAME[s.mode] : "Connected", ok: true };
  return { camera: model, detail: waitText(s), ok: false };
}

function waitText(s: Status | null): string {
  switch (s?.link) {
    case "asleep":
      return "Camera powered off";
    case "stopped":
      return "Disconnected";
    case "connecting":
      return "Connecting";
    case "waiting":
      return "Waiting to reconnect";
    default:
      return "Waiting for USB";
  }
}
