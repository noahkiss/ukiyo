import { commands } from "../bindings";
import { cameraState, inTauri, MODE_NAME, useCamera } from "../lib/camera";
import { usePrefs } from "../lib/prefs";
import { href } from "../lib/router";
import { PageHead } from "../shell/PageHead";
import { WarnIcon } from "../shell/icons";
import { SwitchSteps } from "../shell/ModeNotice";
import { cameraArt } from "../assets/camera";
import wifiStartingLight from "../assets/status/wifi-status-starting-light.webp";
import wifiStartingDark from "../assets/status/wifi-status-starting-dark.webp";
import wifiReadyLight from "../assets/status/wifi-status-ready-light.webp";
import wifiReadyDark from "../assets/status/wifi-status-ready-dark.webp";
import wifiConnectedLight from "../assets/status/wifi-status-connected-light.webp";
import wifiConnectedDark from "../assets/status/wifi-status-connected-dark.webp";

/** The camera's Wi-Fi status icon, as it shows at the top left of the camera screen. */
const WIFI_STATES = [
  { id: "starting", light: wifiStartingLight, dark: wifiStartingDark, color: "Red", text: "Wi-Fi is starting, or not connected to a network yet." },
  { id: "ready", light: wifiReadyLight, dark: wifiReadyDark, color: "Yellow", text: "Connected to the network. No app is using the camera." },
  { id: "connected", light: wifiConnectedLight, dark: wifiConnectedDark, color: "Green", text: "An app is connected to the camera." },
];

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

      <section className="card wifi-status" data-component="wifi-status">
        <h2 className="card-title">Wi-Fi status icon</h2>
        <p className="small muted">The camera shows it at the top left of the screen.</p>
        <ul className="wifi-states">
          {WIFI_STATES.map((w) => (
            <li key={w.id} className="wifi-state" data-state={w.id}>
              <img src={theme === "dark" ? w.dark : w.light} alt={`${w.color} Wi-Fi status icon`} width={48} height={48} />
              <span>{w.text}</span>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
