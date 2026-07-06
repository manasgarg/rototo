// The ring-0 trace preview (design/console-semantic.md "Previews"): for one
// variable under the chosen context, the resolved value, its provenance,
// and the rule walk — every rule in order with its verdict. The same data
// `rototo resolve` prints, so console and CLI cannot disagree.
//
// The empty state is the impact-confidence fix from the system view: with
// no context chosen (or none that satisfies the variable), the panel offers
// the variable's own synthesized boundary contexts, and an active change
// set can promote one to a real sample with `create_sample`.

import type { SynthesizedContext, TraceOutcome } from "@/lib/api";
import {
    syntheticLabel,
    type ChosenContext,
    contextLabel,
} from "@/components/context-picker";

export function TracePreview({
    variableId,
    chosen,
    outcome,
    synthesized,
    canPromote,
    onUseContext,
    onPromote,
}: {
    variableId: string;
    chosen: ChosenContext;
    outcome: TraceOutcome | null;
    synthesized: SynthesizedContext[];
    canPromote: boolean;
    onUseContext: (chosen: ChosenContext) => void;
    onPromote: (entry: SynthesizedContext) => void;
}) {
    const cases = synthesized.filter(
        (entry) => entry.target.id === variableId,
    );

    return (
        <div className="preview-panel">
            <div className="section-header-text">
                <h3>Preview</h3>
                <p className="hint">
                    What a caller gets and why, for {contextLabel(chosen)}.
                </p>
            </div>

            {chosen.kind === "none" ? (
                <p className="hint">
                    Pick a context above to see this variable resolve.
                    {cases.length > 0
                        ? " The synthesized cases below cover its boundaries."
                        : ""}
                </p>
            ) : outcome === null ? (
                <p className="muted">Resolving…</p>
            ) : outcome.error !== undefined ? (
                <div className="banner banner-warn">
                    Cannot resolve under this context: {outcome.error}
                </div>
            ) : outcome.trace !== undefined ? (
                <TraceWalk outcome={outcome} />
            ) : null}

            {cases.length > 0 ? (
                <div className="preview-cases">
                    <div className="section-header-text">
                        <h4>Boundary contexts</h4>
                        <p className="hint">
                            Synthesized from this variable's own rules; labeled
                            synthetic until promoted to a sample.
                        </p>
                    </div>
                    {cases.map((entry) => (
                        <div className="preview-case" key={entry.caseId}>
                            <span className="row-text">
                                <span className="row-title">
                                    {entry.title}
                                </span>
                                <span className="row-sub mono">
                                    {JSON.stringify(entry.context)} →{" "}
                                    {JSON.stringify(entry.expect.value)}
                                </span>
                            </span>
                            <span className="action-row">
                                <button
                                    className="btn btn-secondary btn-sm"
                                    onClick={() =>
                                        onUseContext({
                                            kind: "synthetic",
                                            label: syntheticLabel(entry),
                                            context: entry.context,
                                        })
                                    }
                                >
                                    Preview
                                </button>
                                {canPromote ? (
                                    <button
                                        className="btn btn-ghost btn-sm"
                                        title="Adds this context as a real sample in the active change set"
                                        onClick={() => onPromote(entry)}
                                    >
                                        Promote to sample
                                    </button>
                                ) : null}
                            </span>
                        </div>
                    ))}
                </div>
            ) : null}
        </div>
    );
}

function TraceWalk({ outcome }: { outcome: TraceOutcome }) {
    const trace = outcome.trace;
    if (trace === undefined) {
        return null;
    }
    const matched = trace.rules.find((rule) => rule.matched);
    return (
        <div className="trace-walk">
            <div className="trace-result">
                <span className="label">resolves to</span>
                <span className="mono trace-value">
                    {JSON.stringify(trace.resolution.value)}
                </span>
                <span className="hint">
                    {matched !== undefined
                        ? `rule ${matched.index} matched`
                        : "no rule matched; the default answers"}
                    {trace.provenance !== undefined
                        ? ` · from layer ${trace.provenance}`
                        : ""}
                </span>
            </div>
            <div className="trace-rules">
                {trace.rules.map((rule) => (
                    <div
                        className={`trace-rule ${
                            rule.matched
                                ? "trace-rule-matched"
                                : matched !== undefined &&
                                    rule.index > matched.index
                                  ? "trace-rule-dormant"
                                  : ""
                        }`}
                        key={rule.index}
                    >
                        <span
                            className={`pill ${rule.matched ? "pill-ok" : "pill-neutral"}`}
                        >
                            {rule.matched
                                ? "matched"
                                : matched !== undefined &&
                                    rule.index > matched.index
                                  ? "not reached"
                                  : "no match"}
                        </span>
                        <span className="mono trace-rule-when">
                            {rule.condition}
                        </span>
                        <span className="mono trace-rule-value">
                            → {JSON.stringify(rule.value)}
                        </span>
                    </div>
                ))}
                <div
                    className={`trace-rule ${matched === undefined ? "trace-rule-matched" : "trace-rule-dormant"}`}
                >
                    <span
                        className={`pill ${matched === undefined ? "pill-ok" : "pill-neutral"}`}
                    >
                        default
                    </span>
                    <span className="mono trace-rule-value">
                        → {JSON.stringify(trace.default_value)}
                    </span>
                </div>
                {trace.allocation !== undefined ? (
                    <p className="hint">
                        Allocation {trace.allocation.allocation} on layer{" "}
                        {trace.allocation.layer}:{" "}
                        {trace.allocation.enrolled
                            ? `enrolled, bucket ${trace.allocation.bucket ?? "?"}${
                                  trace.allocation.arm !== undefined
                                      ? `, arm ${trace.allocation.arm}`
                                      : ", unclaimed"
                              }`
                            : "not enrolled"}
                    </p>
                ) : null}
            </div>
        </div>
    );
}
