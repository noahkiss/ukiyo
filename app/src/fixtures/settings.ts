// FIXTURE: sample camera settings until the catalog drives the Settings page.

export type SettingRow =
  | { id: string; name: string; cam: string; kind: "select"; value: string; options: string[]; fav?: boolean }
  | { id: string; name: string; cam: string; kind: "toggle"; value: boolean; fav?: boolean };

export type Menu = { id: string; label: string; cam: string; children?: Array<{ id: string; label: string }>; rows: SettingRow[] };

export const menus: Menu[] = [
  {
    id: "image-quality",
    label: "Image quality",
    cam: "IMAGE QUALITY SETTING",
    rows: [
      { id: "file-format", name: "File format", cam: "IMAGE QUALITY", kind: "select", value: "Fine + RAW", options: ["Fine", "Normal", "Fine + RAW", "RAW"] },
      { id: "raw-recording", name: "RAW recording", cam: "RAW RECORDING", kind: "select", value: "Lossless compressed", options: ["Uncompressed", "Lossless compressed", "Compressed"] },
      { id: "colour-space", name: "Colour space", cam: "COLOR SPACE", kind: "select", value: "sRGB", options: ["sRGB", "Adobe RGB"] },
    ],
  },
  {
    id: "af-mf",
    label: "AF/MF",
    cam: "AF/MF SETTING",
    children: [
      { id: "focus-area", label: "Focus area" },
      { id: "face-eye", label: "Face and eye" },
      { id: "manual-focus", label: "Manual focus" },
    ],
    rows: [
      { id: "focus-area", name: "Focus area", cam: "FOCUS AREA", kind: "select", value: "Zone, 3 × 3", options: ["Single point", "Zone, 3 × 3", "Zone, 5 × 5", "Wide/Tracking"] },
      { id: "af-mode", name: "AF mode", cam: "AF MODE", kind: "select", value: "Single point", options: ["Single point", "Zone", "Wide/Tracking", "All"] },
      { id: "face-eye", name: "Face and eye detection", cam: "FACE/EYE DETECTION SETTING", kind: "select", value: "Face on, eye auto", options: ["Off", "Face on, eye off", "Face on, eye auto", "Face on, right eye", "Face on, left eye"], fav: true },
      { id: "subject", name: "Subject detection", cam: "SUBJECT DETECTION SETTING", kind: "select", value: "Off", options: ["Off", "Animal", "Bird", "Car", "Motorcycle", "Airplane", "Train"] },
      { id: "pre-af", name: "Pre-AF", cam: "PRE-AF", kind: "toggle", value: false },
      { id: "af-illuminator", name: "AF illuminator", cam: "AF ILLUMINATOR", kind: "toggle", value: true },
      { id: "afc-custom", name: "AF-C custom settings", cam: "AF-C CUSTOM SETTINGS", kind: "select", value: "Set 1, multi purpose", options: ["Set 1, multi purpose", "Set 2, ignore obstacles", "Set 3, accelerating", "Set 4, sudden appearance", "Set 5, erratic motion", "Set 6, custom"] },
      { id: "mf-assist", name: "Manual focus assist", cam: "MF ASSIST", kind: "select", value: "Focus peak, red, high", options: ["Standard", "Digital split image", "Digital microprism", "Focus peak, red, high", "Focus peak, red, low"] },
      { id: "focus-check", name: "Focus check", cam: "FOCUS CHECK", kind: "toggle", value: true },
      { id: "priority", name: "Release or focus priority", cam: "RELEASE/FOCUS PRIORITY", kind: "select", value: "Release first", options: ["Release first", "Focus first"] },
    ],
  },
  {
    id: "shooting",
    label: "Shooting",
    cam: "SHOOTING SETTING",
    rows: [
      { id: "shutter-type", name: "Shutter type", cam: "SHUTTER TYPE", kind: "select", value: "Mechanical", options: ["Mechanical", "Electronic", "Electronic front curtain", "Mechanical + electronic"], fav: true },
      { id: "stabilisation", name: "Image stabilisation", cam: "IS MODE", kind: "select", value: "Continuous", options: ["Continuous", "Shooting only", "Off"], fav: true },
      { id: "self-timer", name: "Self-timer", cam: "SELF-TIMER", kind: "select", value: "Off", options: ["Off", "2 s", "10 s"] },
    ],
  },
  {
    id: "flash",
    label: "Flash",
    cam: "FLASH SETTING",
    rows: [{ id: "red-eye", name: "Red-eye removal", cam: "RED EYE REMOVAL", kind: "toggle", value: false }],
  },
  {
    id: "movie",
    label: "Movie",
    cam: "MOVIE SETTING",
    rows: [{ id: "movie-mode", name: "Movie mode", cam: "MOVIE MODE", kind: "select", value: "4K 29.97P", options: ["6.2K 29.97P", "4K 29.97P", "FHD 59.94P"] }],
  },
  {
    id: "set-up",
    label: "Set up",
    cam: "SET UP",
    rows: [
      { id: "auto-off", name: "Auto power off", cam: "AUTO POWER OFF", kind: "select", value: "2 min", options: ["Off", "5 min", "2 min", "1 min", "30 s", "15 s"], fav: true },
      { id: "sound", name: "Operation volume", cam: "OPERATION VOL.", kind: "select", value: "Off", options: ["Off", "Low", "Medium", "High"] },
    ],
  },
];

export const presets = [
  { name: "Street", meta: "12 settings · applied today 09:15" },
  { name: "Studio", meta: "18 settings" },
  { name: "Night walk", meta: "9 settings" },
];

export const snapshots = [
  { name: "Before trip", when: "Today 10:42" },
  { name: "Studio day", when: "Oct 2" },
  { name: "Factory reset", when: "Mar 2" },
];

/** Words a search also matches, so a setting is found by what it does. */
export const searchWords: Record<string, string> = {
  "face-eye": "eye focus face detect portrait",
  subject: "eye focus animal bird detect",
  "focus-check": "focus magnify zoom check",
  "mf-assist": "focus peaking manual",
  "shutter-type": "silent quiet electronic",
  "auto-off": "sleep battery power",
};
