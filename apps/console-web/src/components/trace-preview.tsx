// The read side of the variable screen (design/console-semantic.md
// "Previews"), split into two pieces the screen composes around the rule
// ladder. OutcomeStrip states the resolved value and its provenance for the
// chosen context — the same data `rototo resolve` prints, so console and
// CLI cannot disagree; the per-rule verdicts live on the ladder itself.
//
// BoundaryContextsCard is the impact-confidence fix from the system view:
// with no satisfying context at hand, the reader can generate the
// variable's own synthesized boundary contexts — the smallest valid
// contexts that exercise each branch — and an active change set can
// promote one to a real sample with `create_sample`.

import { useState } from "react";

import type { SynthesizedContext, TraceOutcome } from "@/lib/api";
import {
    ContextFacts,
    syntheticLabel,
    type ChosenContext,
    contextLabel,
} from "@/components/context-picker";

export function OutcomeStrip({
    chosen,
    outcome,
    method,
    stale,
}: {
    chosen: ChosenContext;
    outcome: TraceOutcome | null;
    // The rule-walk phrasing only makes sense for first-match rules; a
    // query's entries already say how the value was selected.
    method: "rules" | "query" | "allocation";
    // The draft has unsaved edits: the outcome still describes the saved
    // definition, and says so rather than silently mismatching the form.
    stale: boolean;
}) {
    if (chosen.kind === "none") {
        return (
            <p className="hint">
                Pick a context above to watch this variable resolve.
            </p>
        );
    }
    if (outcome === null) {
        return <p className="muted">Resolving…</p>;
    }
    if (outcome.error !== undefined) {
        return (
            <div className="banner banner-warn">
                Cannot resolve under this context: {outcome.error}
            </div>
        );
    }
    const trace = outcome.trace;
    if (trace === undefined) {
        return null;
    }
    const matched = trace.rules.find((rule) => rule.matched);
    // A primitive reads fine inline; a catalog-backed value is a whole
    // entry, which only makes sense pretty-printed.
    const value = trace.resolution.value;
    const composite = typeof value === "object" && value !== null;
    const parts: string[] = [];
    if (method === "rules") {
        parts.push(
            matched !== undefined
                ? `rule ${matched.index + 1} matched`
                : "no rule matched; the default answers",
        );
    }
    if (trace.resolution.source?.kind === "catalog_array") {
        parts.push(`entries ${trace.resolution.source.values.join(", ")}`);
    }
    if (trace.provenance !== undefined) {
        parts.push(`from layer ${trace.provenance}`);
    }
    parts.push(`under ${contextLabel(chosen)}`);
    if (stale) {
        parts.push("before your unsaved edits");
    }
    return (
        <div className="trace-walk">
            <div className="trace-result">
                <span className="label">resolves to</span>
                {!composite ? (
                    <span className="mono trace-value">
                        {JSON.stringify(value)}
                    </span>
                ) : trace.resolution.source?.kind === "catalog" ? (
                    <span className="mono trace-value">
                        entry {trace.resolution.source.value}
                    </span>
                ) : null}
                <span className="hint">{parts.join(" · ")}</span>
            </div>
            {composite ? (
                <pre className="codewell trace-value-block">
                    {JSON.stringify(value, null, 2)}
                </pre>
            ) : null}
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
    );
}

export function BoundaryContextsCard({
    variableId,
    synthesized,
    canPromote,
    onUseContext,
    onPromote,
}: {
    variableId: string;
    synthesized: SynthesizedContext[];
    canPromote: boolean;
    onUseContext: (chosen: ChosenContext) => void;
    onPromote: (entry: SynthesizedContext) => void;
}) {
    const cases = synthesized.filter((entry) => entry.target.id === variableId);
    // The cases arrive with the context inventory; "generate" is the
    // reader's act of asking for them, so the card stays quiet until then.
    const [generated, setGenerated] = useState(false);
    if (cases.length === 0) {
        return null;
    }
    return (
        <div className="card">
            <div className="section-header-text">
                <h3>Boundary contexts</h3>
                <p className="hint">
                    The smallest valid contexts that exercise each branch of
                    this variable; labeled synthetic until promoted to a sample.
                </p>
            </div>
            {!generated ? (
                <div className="action-row">
                    <button
                        className="btn btn-secondary btn-sm"
                        onClick={() => setGenerated(true)}
                    >
                        Generate boundary contexts
                    </button>
                </div>
            ) : (
                <div className="preview-cases">
                    {cases.map((entry) => (
                        <div className="preview-case" key={entry.caseId}>
                            <span className="row-text">
                                <span className="row-title">{entry.title}</span>
                                <ContextFacts context={entry.context} />
                                <span className="row-sub">
                                    expected value{" "}
                                    <span
                                        className="mono"
                                        title={JSON.stringify(
                                            entry.expect.value,
                                            null,
                                            2,
                                        )}
                                    >
                                        {JSON.stringify(entry.expect.value) ??
                                            "none"}
                                    </span>
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
            )}
        </div>
    );
}
