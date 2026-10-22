// FIXTURE: sample Hub data until the camera read on connect and a connect history exist.

export type HubCamera = {
  model: string;
  firmware: string;
  /** Last four digits only; a placeholder. */
  serialTail: string;
  lens: string;
  batteryPct: number;
  cards: Array<{ slot: number; freeGb: number; totalGb: number } | { slot: number; empty: true }>;
};

export type ShotCounts = {
  total: number;
  sinceLast: number;
  lastConnect: string;
  mechanical: number;
  electronic: number;
  firstConnected: string;
};

export type DayShots = { date: string; shots: number };
export type LensSeen = { name: string; seen: string; attached: boolean };
export type ConnectEntry = { when: string; shots: number };
export type OwnerDetails = { copyright: string; author: string; prefix: string };

export const hubReadAt = "today at 10:42";

export const hubCamera: HubCamera = {
  model: "X-T5",
  firmware: "2.10",
  serialTail: "0000",
  lens: "XF 23mm F2 R WR",
  batteryPct: 82,
  cards: [
    { slot: 1, freeGb: 64.2, totalGb: 128 },
    { slot: 2, empty: true },
  ],
};

export const shotCounts: ShotCounts = {
  total: 12486,
  sinceLast: 125,
  lastConnect: "Oct 14",
  mechanical: 9880,
  electronic: 2606,
  firstConnected: "Mar 2, 2026",
};

const DAYS = [22, 0, 41, 36, 58, 12, 0, 0, 74, 31, 45, 18, 168, 96, 27, 0, 39, 52, 33, 61, 0, 14, 48, 70, 25, 37, 9, 0, 66, 59];

/** Shots per day, oldest first, ending today (Oct 17, 2026). */
export const shotsPerDay: DayShots[] = DAYS.map((shots, i) => {
  const d = new Date(2026, 8, 18 + i);
  return { date: `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`, shots };
});

/** Shots per month, oldest first. The current month is partial. */
export const shotsPerMonth: Array<{ month: string; shots: number }> = [
  { month: "2025-11", shots: 640 },
  { month: "2025-12", shots: 1210 },
  { month: "2026-01", shots: 380 },
  { month: "2026-02", shots: 520 },
  { month: "2026-03", shots: 1430 },
  { month: "2026-04", shots: 960 },
  { month: "2026-05", shots: 1105 },
  { month: "2026-06", shots: 1890 },
  { month: "2026-07", shots: 1320 },
  { month: "2026-08", shots: 870 },
  { month: "2026-09", shots: 1015 },
  { month: "2026-10", shots: 1146 },
];

export const lensesSeen: LensSeen[] = [
  { name: "XF 23mm F2 R WR", seen: "Attached now", attached: true },
  { name: "XF 35mm F1.4 R", seen: "Oct 14", attached: false },
  { name: "XF 16-80mm F4 R OIS WR", seen: "Oct 4", attached: false },
  { name: "XF 90mm F2 R LM WR", seen: "Aug 30", attached: false },
];

export const ownerDetails: OwnerDetails = {
  copyright: "Your Name",
  author: "Your Name",
  prefix: "XT5_",
};

export const connectLog: ConnectEntry[] = [
  { when: "Today, 10:42", shots: 12486 },
  { when: "Oct 14, 19:05", shots: 12361 },
  { when: "Oct 9, 08:30", shots: 11930 },
  { when: "Oct 2, 21:12", shots: 11401 },
];
