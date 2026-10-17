import { commands } from "../bindings";
import { cameraState, inTauri, MODE_NAME, useCamera } from "../lib/camera";
import { usePrefs } from "../lib/prefs";
import { href } from "../lib/router";
import { PageHead } from "../shell/PageHead";
import { WarnIcon } from "../shell/icons";
import xt5Light from "../assets/camera/xt5-three-quarter.png";
import xt5Dark from "../assets/camera/xt5-three-quarter-dark.png";

const MODES = [
  { id: "tether", name: "USB TETHER SHOOTING FIXED", tag: "Hub, Settings, Lab", text: "Read and change settings. The camera keeps working." },
  { id: "rawConv", name: "USB RAW CONV./BACKUP RESTORE", tag: "Recipes", text: "Read and write slots C1 to C7. The camera is locked until you unplug." },
  { id: "cardReader", name: "USB CARD READER", tag: "Not supported", text: "Files only. ukiyo cannot read settings in this mode." },
] as const;

/** How to set the camera's USB mode, and where the connection stands now. */
export function Connect() {
  const { status } = useCamera();
  const { theme } = usePrefs();
  const state = cameraState(status);
  const model = status?.model ?? "The camera";
  const current = state === "connected" ? status?.mode : null;

  return (
    <div className="page connect" data-component="connect" data-state={state}>
      <PageHead title="Connect your camera" sub="Set the USB mode on the camera first. ukiyo can't change it for you." />

      {state === "wrongMode" && (
        <div className="banner warn big" role="status">
          <WarnIcon />
          <span>
            <strong>{model} found in USB CARD READER mode.</strong> Unplug it, change the mode, then plug it in again.
          </span>
        </div>
      )}

      <div className="flex-row stretch">
        <div className="card connect-art">
          <div className="sun" />
          <img src={theme === "dark" ? xt5Dark : xt5Light} alt="Ink drawing of an X-T5 camera body" />
        </div>

        <ol className="steps">
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
              <span className="menu-path-keys">
                <span className="key">SET UP</span>
                <span aria-hidden="true">›</span>
                <span className="key">CONNECTION SETTING</span>
                <span aria-hidden="true">›</span>
                <span className="key">CONNECTION MODE</span>
              </span>
            </div>
          </li>
          <li className="step">
            <span className="step-n" aria-hidden="true">3</span>
            <div className="step-body grow-body">
              <span className="step-title">Pick the mode for what you want to do</span>
              {MODES.map((m) => {
                const on = current ? current === m.id : m.id === "tether";
                const cls = ["mode-card", on ? "on" : "", m.id === "cardReader" ? "unsupported" : ""].filter(Boolean).join(" ");
                return (
                  <div key={m.id} className={cls}>
                    <span className="mode-top">
                      <span className="mono mode-name">{m.name}</span>
                      <span className="mode-tag">{current === m.id ? "Set now" : m.tag}</span>
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
              <span className="small muted">ukiyo finds the camera on its own.</span>
            </div>
          </li>
        </ol>
      </div>

      <div className="card connect-status" role="status">
        {state === "connected" && status ? (
          <>
            <span className="dot big ok" aria-hidden="true" />
            <span className="grow-text">
              {model} connected in {status.mode ? MODE_NAME[status.mode] : "an unknown mode"}
            </span>
            <a className="btn raised" href={href(status.mode === "rawConv" ? "recipes" : "hub")}>
              {status.mode === "rawConv" ? "Open Recipes" : "Open Hub"}
            </a>
          </>
        ) : (
          <>
            <span className="pulse" aria-hidden="true" />
            <span className="grow-text">{state === "wrongMode" ? "Waiting for the camera in another mode" : "Waiting for a camera on USB"}</span>
            <button type="button" className="btn raised" disabled={!inTauri} onClick={() => void commands.labConnect(true)} data-action="check-again">
              Check again
            </button>
          </>
        )}
      </div>
    </div>
  );
}
