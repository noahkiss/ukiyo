import { commands, type Applied, type Plan } from "../bindings";

/** Builds a plan of confirmed settings: `set`, or a library item (`preset:x`, `snapshot:x`).
 * Writes nothing. A plan with nothing to write comes back as its reason. */
export async function makePlan(set: Record<string, string>, from: string | null = null): Promise<Plan | string> {
  const r = await commands.planCreate(set, false, from, null);
  return r.status === "ok" ? r.data : r.error;
}

export async function applyPlan(planId: string): Promise<Applied | string> {
  const r = await commands.planApply(planId);
  return r.status === "ok" ? r.data : r.error;
}

/** What an applied plan did, in one line. */
export function appliedText(a: Applied): string {
  if (a.failed > 0) {
    const bad = a.steps.find((s) => !s.ok);
    return `${a.applied} written, ${a.failed} failed${bad?.error ? `: ${bad.error}` : ""}.`;
  }
  return a.applied === 1 ? "Written to the camera." : `${a.applied} settings written to the camera.`;
}

/** Plans `set` and applies it at once: one setting edited on a page. */
export async function writeNow(set: Record<string, string>): Promise<string> {
  const p = await makePlan(set);
  if (typeof p === "string") return p;
  if (p.steps.length === 0) {
    commands.planDiscard(p.plan_id);
    return p.blocked[0]?.message ?? p.skipped[0]?.message ?? "Nothing to write.";
  }
  const a = await applyPlan(p.plan_id);
  return typeof a === "string" ? a : appliedText(a);
}
