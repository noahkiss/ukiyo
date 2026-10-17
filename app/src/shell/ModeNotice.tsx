import { useEffect, useRef, useState, type ReactNode } from "react";
import type { UsbMode } from "../bindings";
import { cameraState, MODE_NAME, useCamera } from "../lib/camera";
import { CloseIcon, WarnIcon } from "./icons";

/** A mode a feature can need. Card reader mode has nothing ukiyo can use. */
export type NeededMode = "tether" | "rawConv";

/** The three modes as the camera's menu names them, and what each one gives ukiyo. */
export const MODES = [
  { id: "tether", name: "USB TETHER SHOOTING FIXED", tag: "Hub, Settings, Lab", text: "Read and change settings. The camera keeps working." },
  { id: "rawConv", name: "USB RAW CONV./BACKUP RESTORE", tag: "Recipes", text: "Read and write slots C1 to C7. The camera is locked until you unplug." },
  { id: "cardReader", name: "USB CARD READER", tag: "Not supported", text: "Files only. Ukiyo cannot read settings in this mode." },
] as const;

const NETWORK_MENU = ["NETWORK/USB SETTING", "CONNECTION MODE"];
const SETUP_MENU = ["SET UP", "CONNECTION SETTING", "CONNECTION MODE"];
const PC_MENU = ["SET UP", "CONNECTION SETTING", "PC CONNECTION MODE"];

/**
 * Where CONNECTION MODE sits in a model's menu, from its owner's manual (`catalog/menus/`).
 * The same table as `connection_menu` in `src-tauri/core/src/modes.rs`, which a test checks
 * against the manuals. `null` for a body Ukiyo has no manual entry for.
 */
export function connectionMenu(model: string | null | undefined): string[] | null {
  const slug = (model ?? "").trim().toLowerCase().replace(/ /g, "-");
  if (["x-t5", "x-t50", "x-e5", "x-m5", "x-s20", "x100vi"].includes(slug)) return NETWORK_MENU;
  if (["x-t3", "x-t4", "x-t30", "x-t30-ii", "x-s10", "x-e4", "x100v"].includes(slug)) return SETUP_MENU;
  if (["x-t2", "x-h1", "x-pro2", "x-pro3", "x100f"].includes(slug)) return PC_MENU;
  return null;
}

function MenuKeys({ path }: { path: string[] }) {
  return (
    <span className="menu-path-keys">
      {path.map((k, i) => (
        <span key={k} className="menu-path-part">
          {i > 0 && <span aria-hidden="true">›</span>}
          <span className="key">{k}</span>
        </span>
      ))}
    </span>
  );
}

/**
 * The four steps to change the camera's USB mode. The Connect page and the walkthrough share them.
 * `target` marks the mode to pick; `current` marks the mode the camera is in now. With `named`,
 * step 3 names the target mode. Step 2 shows the connected model's menu path, or both common
 * paths with no known model.
 */
export function SwitchSteps({ target, current, named = false }: { target: UsbMode | null; current: UsbMode | null; named?: boolean }) {
  const pick = named ? MODES.find((m) => m.id === target)?.name : undefined;
  const { status } = useCamera();
  const path = connectionMenu(status?.model);
  return (
    <ol className="steps" data-component="switch-steps">
      <li className="step">
        <span className="step-n" aria-hidden="true">1</span>
        <div className="step-body">
          <span className="step-title">Unplug the USB cable</span>
          <span className="small muted">The camera locks this menu while the cable is in.</span>
        </div>
      </li>
      <li className="step">
        <span className="step-n" aria-hidden="true">2</span>
        <div className="step-body">
          <span className="step-title">On the camera, open this menu</span>
          <MenuKeys path={path ?? NETWORK_MENU} />
          {!path && (
            <>
              <span className="small muted">On the X-T4 and older:</span>
              <MenuKeys path={SETUP_MENU} />
            </>
          )}
        </div>
      </li>
      <li className="step">
        <span className="step-n" aria-hidden="true">3</span>
        <div className="step-body grow-body">
          <span className="step-title">{pick ? `Pick ${pick}` : "Pick the mode for what you want to do"}</span>
          {MODES.map((m) => {
            const on = target === m.id;
            const cls = ["mode-card", on ? "on" : "", m.id === "cardReader" ? "unsupported" : ""].filter(Boolean).join(" ");
            const tag = current === m.id ? "Set now" : m.tag;
            return (
              <div key={m.id} className={cls} data-mode={m.id}>
                <span className="mode-top">
                  <span className="mono mode-name">{m.name}</span>
                  <span className="mode-tag">{tag}</span>
                </span>
                <span className="small">{m.text}</span>
              </div>
            );
          })}
        </div>
      </li>
      <li className="step">
        <span className="step-n" aria-hidden="true">4</span>
        <div className="step-body">
          <span className="step-title">Plug the cable into this Mac</span>
          <span className="small muted">Ukiyo finds the camera on its own.</span>
        </div>
      </li>
    </ol>
  );
}

/** The walkthrough in a sheet, with the camera's live mode at the bottom. */
export function SwitchDialog({ target, onClose }: { target: NeededMode; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const { status } = useCamera();
  const state = cameraState(status);
  const current = state === "none" ? null : (status?.mode ?? null);
  const done = state === "connected" && current === target;

  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);

  return (
    <dialog ref={ref} className="sheet switch-sheet" aria-labelledby="switch-title" onClose={onClose} onCancel={onClose} data-component="switch-mode" data-state={done ? "done" : "waiting"}>
      <div className="sheet-head">
        <h2 id="switch-title" className="card-title">
          Switch to {MODE_NAME[target]}
        </h2>
        <button type="button" className="icon-btn" aria-label="Close" onClick={onClose}>
          <CloseIcon />
        </button>
      </div>
      <SwitchSteps target={target} current={current} named />
      <div className="switch-status" role="status">
        {done ? (
          <>
            <span className="dot big ok" aria-hidden="true" />
            <span className="grow-text">
              {status?.model ?? "The camera"} is in {MODE_NAME[target]}.
            </span>
            <button type="button" className="btn primary" onClick={onClose} data-action="switch-done">
              Done
            </button>
          </>
        ) : (
          <>
            <span className="pulse" aria-hidden="true" />
            <span className="grow-text">{current ? `The camera is in ${MODE_NAME[current]}.` : "Waiting for a camera on USB"}</span>
          </>
        )}
      </div>
    </dialog>
  );
}

/** A "How to switch" control that opens the walkthrough for `target`. */
export function HowToSwitch({ target, children = "How to switch" }: { target: NeededMode; children?: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" className="link-btn" onClick={() => setOpen(true)} data-action="how-to-switch" data-mode={target}>
        {children}
      </button>
      {open && <SwitchDialog target={target} onClose={() => setOpen(false)} />}
    </>
  );
}

/**
 * Shows when a camera is connected in a mode that `what` cannot use. Renders nothing with no
 * camera, or in the needed mode. In tether mode it shows what the person must do at the camera
 * first instead: close a menu (D22E 3), leave playback (D22E 2), or, unless `stills` is false,
 * set the STILL/MOVIE switch to STILL (D307). These clear when the status changes back. The
 * same notices reach the CLI and MCP as `notices` in `status` (`read.rs`).
 */
export function ModeNotice({ needs, what, stills = true }: { needs: NeededMode; what: string; stills?: boolean }) {
  const { status } = useCamera();
  const state = cameraState(status);
  const mode = status?.mode;
  if (state === "none" || !mode || mode === "unknown") return null;
  if (mode === needs) return mode === "tether" ? <CameraNotices stills={stills} /> : null;
  return (
    <div className="banner warn" role="status" data-component="mode-notice" data-needs={needs} data-mode={mode}>
      <WarnIcon />
      <span>
        <strong>{what} needs {MODE_NAME[needs]}.</strong> The camera is in {MODE_NAME[mode]}.
      </span>
      <HowToSwitch target={needs} />
    </div>
  );
}

/** Camera screens and the STILL/MOVIE switch, in tether mode. */
function CameraNotices({ stills }: { stills: boolean }) {
  const { status } = useCamera();
  if (status?.link !== "connected") return null;
  const screen =
    status.screen === "menu" ? (
      <>
        <strong>Close the camera menu.</strong> Half-press the shutter button on the camera.
      </>
    ) : status.screen === "playback" ? (
      <>
        <strong>Leave playback.</strong> Press the playback button or half-press the shutter button on the camera.
      </>
    ) : null;
  const movie = stills && status.capture === "movie";
  if (!screen && !movie) return null;
  return (
    <>
      {screen && (
        <div className="banner warn" role="status" data-component="screen-notice" data-screen={status.screen}>
          <WarnIcon />
          <span>{screen}</span>
        </div>
      )}
      {movie && (
        <div className="banner warn" role="status" data-component="movie-notice">
          <WarnIcon />
          <span>
            <strong>Set the STILL/MOVIE switch to STILL</strong> for stills settings and recipes.
          </span>
        </div>
      )}
    </>
  );
}
