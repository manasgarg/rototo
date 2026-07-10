// The read side of the variable screen (design/console-semantic.md
// "Previews"). OutcomeStrip states the resolved value and its provenance
// for the chosen context. The same trace powers `rototo resolve`, so console
// and CLI cannot disagree. Boundary contexts belong to the context picker,
// where every other context is selected.

import type { TraceOutcome } from "@/lib/api";
import { CodeEditor } from "@/components/code-editor";
import { type ChosenContext, contextLabel } from "@/components/context-picker";
import { resolvedValueText } from "@/lib/format";

export function OutcomeStrip({
    chosen,
    outcome,
    method,
    requiresContext,
    stale,
}: {
    chosen: ChosenContext;
    outcome: TraceOutcome | null;
    // The rule-walk phrasing only makes sense for first-match rules; a
    // query's entries already say how the value was selected.
    method: "rules" | "query" | "allocation";
    requiresContext: boolean;
    // The draft has unsaved edits: the outcome still describes the saved
    // definition, and says so rather than silently mismatching the form.
    stale: boolean;
}) {
    if (chosen.kind === "none" && requiresContext) {
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
    // Catalog resolutions carry both the raw hydrated value an app receives
    // and the entry id a person recognizes. This screen names the entry and
    // deliberately does not dump the hydrated object.
    const value = trace.resolution.value;
    const source = trace.resolution.source;
    const catalogBacked =
        source?.kind === "catalog" || source?.kind === "catalog_array";
    const composite =
        typeof value === "object" && value !== null && !catalogBacked;
    const parts: string[] = [];
    if (method === "rules") {
        parts.push(
            matched !== undefined
                ? `rule ${matched.index + 1} matched`
                : "no rule matched; the default answers",
        );
    }
    if (source?.kind === "catalog" || source?.kind === "catalog_array") {
        parts.push(`from catalog ${source.catalog}`);
    }
    if (trace.provenance !== undefined) {
        parts.push(`from layer ${trace.provenance}`);
    }
    parts.push(
        chosen.kind === "none" || !requiresContext
            ? "without caller context"
            : `under ${contextLabel(chosen)}`,
    );
    if (stale) {
        parts.push("before your unsaved edits");
    }
    return (
        <div className="trace-walk">
            <div className="trace-result">
                <span className="label">resolves to</span>
                {!composite ? (
                    <span className="mono trace-value">
                        {catalogBacked
                            ? resolvedValueText(trace) || "(no entries)"
                            : JSON.stringify(value)}
                    </span>
                ) : null}
                <span className="hint">{parts.join(" · ")}</span>
            </div>
            {composite ? (
                <CodeEditor
                    className="trace-value-block"
                    disabled
                    language="json"
                    onChange={() => {}}
                    value={JSON.stringify(value, null, 2)}
                />
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
