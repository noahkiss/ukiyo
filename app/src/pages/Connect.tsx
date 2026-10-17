import { commands } from "../bindings";
import { cameraState, inTauri, MODE_NAME, useCamera } from "../lib/camera";
import { usePrefs } from "../lib/prefs";
import { href } from "../lib/router";
import { PageHead } from "../shell/PageHead";
import { WarnIcon } from "../shell/icons";
import { SwitchSteps } from "../shell/ModeNotice";
import { cameraArt } from "../assets/camera";

/** How to set the camera's USB mode, and where the connection stands now. */
export function Connect() {
  const { status } = useCamera();
  const { theme } = usePrefs();
  const state = cameraState(status);
  const model = status?.model ?? "The camera";
  const current = state === "connected" ? status?.mode : null;
  // No camera yet: the X-T5, as before. A connected model gets its own drawing.
  const art = cameraArt(status?.model ?? "X-T5");

  return (
    <div className="page connect" data-component="connect" data-state={state}>
      <PageHead title="Connect your camera" sub="Set the USB mode on the camera first. Ukiyo can't change it for you." />

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
          <img src={theme === "dark" ? art.dark : art.light} alt={art.alt} />
        </div>

        <SwitchSteps target={current ?? "tether"} current={current ?? null} />
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
