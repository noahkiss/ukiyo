import { useState } from "react";
import { commands, type Applied, type Plan } from "../bindings";
import { appliedText, applyPlan } from "../lib/plans";
import { useToast } from "../lib/toast";

const PURPOSE: Record<string, string> = { prerequisite: "for the next step", restore: "put back" };

type Props = {
  title: string;
  plan: Plan;
  onDone: (applied: Applied | null) => void;
  /** Offered when the plan blocks settings that are not confirmed in the catalog: plan again
   * with them included. */
  onAllowUnconfirmed?: () => void;
};

/** A plan before it is written: each step, what is left out, and the write button. */
export function PlanCard({ title, plan, onDone, onAllowUnconfirmed }: Props) {
  const flash = useToast();
  const [busy, setBusy] = useState(false);
  const apply = async () => {
    setBusy(true);
    const a = await applyPlan(plan.plan_id);
    setBusy(false);
    flash(typeof a === "string" ? a : appliedText(a));
    onDone(typeof a === "string" ? null : a);
  };
  const cancel = () => {
    void commands.planDiscard(plan.plan_id);
    onDone(null);
  };
  const unchanged = plan.skipped.filter((s) => s.reason === "unchanged").length;
  const other = plan.skipped.length - unchanged;
  const unconfirmed = plan.blocked.some((b) => b.reason === "unconfirmed_property");
  return (
    <section className="card pad-s" aria-labelledby="plan-title" data-component="plan-card">
      <h2 id="plan-title" className="card-title small-title">
        {title}
      </h2>
      <div className="rows">
        {plan.steps.map((s, i) => (
          <div key={`${s.code}-${i}`} className="row-line small" title={s.note ?? undefined}>
            <span>
              {s.name ?? s.code}
              {PURPOSE[s.purpose ?? "set"] && <span className="tiny muted"> ({PURPOSE[s.purpose ?? "set"]})</span>}
            </span>
            <span className="muted">
              {s.before} → {s.after}
            </span>
          </div>
        ))}
      </div>
      <p className="tiny muted">
        {[
          plan.steps.length === 1 ? "1 change" : `${plan.steps.length} changes`,
          unchanged ? `${unchanged} already set` : "",
          plan.blocked.length ? `${plan.blocked.length} not written: unconfirmed or read-only` : "",
          other ? `${other} need another mode` : "",
        ]
          .filter(Boolean)
          .join(" · ")}
      </p>
      {(plan.warnings ?? []).map((w) => (
        <p key={w} className="tiny muted">
          {w}
        </p>
      ))}
      <div className="actions-line">
        <button type="button" className="btn small primary" disabled={busy || plan.steps.length === 0} onClick={() => void apply()}>
          Write to camera
        </button>
        {unconfirmed && onAllowUnconfirmed && (
          <button type="button" className="btn small" onClick={onAllowUnconfirmed}>
            Include unconfirmed settings
          </button>
        )}
        <button type="button" className="btn text small" onClick={cancel}>
          Cancel
        </button>
      </div>
    </section>
  );
}
