import { cameraLines, useCamera } from "../lib/camera";

/** The page header's camera chip: model and mode, from the live status. */
export function CameraChip() {
  const { status } = useCamera();
  const l = cameraLines(status);
  return (
    <span className="chip" data-component="camera-chip">
      <span className={l.ok ? "dot ok" : "dot"} />
      {l.camera} · {l.detail}
    </span>
  );
}
