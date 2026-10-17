import { useState } from "react";
import type { FakeAction } from "../bindings";

const ACTIONS: Array<[FakeAction, string]> = [
  ["nudge", "Change a setting"],
  ["openMenu", "Open menu"],
  ["closeMenu", "Close menu"],
  ["movie", "Movie"],
  ["stills", "Stills"],
  ["powerOff", "Power off"],
  ["powerOn", "Power on"],
  ["unplug", "Unplug"],
];

export function FakeBar({ onAction }: { onAction: (a: FakeAction) => void }) {
  const [auto, setAuto] = useState(true);
  return (
    <nav className="fakebar" aria-label="Fake camera" data-component="fake-bar">
      <span className="fakebar-title">Fake camera</span>
      {ACTIONS.map(([a, label]) => (
        <button key={a} type="button" className="btn small raised" onClick={() => onAction(a)} data-action={`fake-${a}`}>
          {label}
        </button>
      ))}
      <label className="check small">
        <input
          type="checkbox"
          checked={auto}
          onChange={(e) => {
            setAuto(e.target.checked);
            onAction(e.target.checked ? "autoOn" : "autoOff");
          }}
        />
        Random changes
      </label>
    </nav>
  );
}
