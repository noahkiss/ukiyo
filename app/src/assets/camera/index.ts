// The Hub's camera art: one 3/4 ink drawing per body, in light and dark.
// Built by tools/art/camera_art.py; every body faces the same way as the X-T5.
import genericDark from "./generic-dark.webp";
import genericLight from "./generic-light.webp";
import x100viDark from "./x100vi-dark.webp";
import x100viLight from "./x100vi-light.webp";
import xe5Dark from "./x-e5-dark.webp";
import xe5Light from "./x-e5-light.webp";
import xh2Dark from "./x-h2-dark.webp";
import xh2Light from "./x-h2-light.webp";
import xh2sDark from "./x-h2s-dark.webp";
import xh2sLight from "./x-h2s-light.webp";
import xpro3Dark from "./x-pro3-dark.webp";
import xpro3Light from "./x-pro3-light.webp";
import xs20Dark from "./x-s20-dark.webp";
import xs20Light from "./x-s20-light.webp";
import xt3Dark from "./x-t3-dark.webp";
import xt3Light from "./x-t3-light.webp";
import xt4Dark from "./x-t4-dark.webp";
import xt4Light from "./x-t4-light.webp";
import xt5Dark from "./x-t5-dark.webp";
import xt5Light from "./x-t5-light.webp";

export type CameraArt = { light: string; dark: string; alt: string };

/** Keyed by model name without spaces, hyphens or case: "X-H2S" is "xh2s". */
const ART: Record<string, { light: string; dark: string; name: string }> = {
  xt5: { light: xt5Light, dark: xt5Dark, name: "X-T5" },
  xt3: { light: xt3Light, dark: xt3Dark, name: "X-T3" },
  xt4: { light: xt4Light, dark: xt4Dark, name: "X-T4" },
  xh2: { light: xh2Light, dark: xh2Dark, name: "X-H2" },
  xh2s: { light: xh2sLight, dark: xh2sDark, name: "X-H2S" },
  xs20: { light: xs20Light, dark: xs20Dark, name: "X-S20" },
  x100vi: { light: x100viLight, dark: x100viDark, name: "X100VI" },
  xpro3: { light: xpro3Light, dark: xpro3Dark, name: "X-Pro3" },
  xe5: { light: xe5Light, dark: xe5Dark, name: "X-E5" },
};

const key = (model: string) => model.toLowerCase().replace(/[\s_-]/g, "");

/** The drawing for a camera model, or a generic body for a model without one. */
export function cameraArt(model: string | null | undefined): CameraArt {
  const a = model ? ART[key(model)] : undefined;
  if (a) return { light: a.light, dark: a.dark, alt: `Ink drawing of an ${a.name} camera body` };
  return { light: genericLight, dark: genericDark, alt: "Ink drawing of a camera body" };
}
