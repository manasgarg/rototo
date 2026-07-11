// The read side of the variable screen (design/console-semantic.md
// "Previews"). AnswerStrip is the closing zone of the try-it card: one
// tinted band stating the resolved value and why. Sea-tinted when a rule
// matched, neutral when the default answered, warn when the context cannot
// evaluate. The same trace powers `rototo resolve`, so console and CLI
// cannot disagree.

import type { ReactNode } from "react";

import type { TraceOutcome } from "@/lib/api";
import type { AddressStep } from "@/lib/router";
import { CodeEditor } from "@/components/code-editor";
import { type ChosenContext, contextLabel } from "@/components/context-picker";
import { ExpressionText } from "@/components/entity-link";
import { resolvedValueText } from "@/lib/format";

export function AnswerStrip({
    chosen,
    outcome,
    method,
    requiresContext,
    stale,
    hrefEntity,
}: {
    chosen: ChosenContext;
    outcome: TraceOutcome | null;
    // The rule-walk phrasing only makes sense for first-match rules; a
    // query's entries already say how the value was selected.
    method: "rules" | "query" | "allocation";
    requiresContext: boolean;
    // The draft has unsaved edits: the answer still describes the saved
    // definition, and says so rather than silently mismatching the form.
    stale: boolean;
    hrefEntity: (steps: AddressStep[]) => string;
}) {
    if (chosen.kind === "none" && requiresContext) {
        return (
            <Strip tone="idle">
                <span className="answer-why">
                    Pick a context above and the answer appears here.
                </span>
            </Strip>
        );
    }
    if (outcome === null) {
        return (
            <Strip tone="idle">
                <span className="answer-why">Resolving…</span>
            </Strip>
        );
    }
    if (outcome.error !== undefined) {
        return (
            <Strip tone="warn">
                <span className="answer-why">
                    Can&apos;t evaluate: {outcome.error}
                </span>
            </Strip>
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
    const why: ReactNode =
        method === "rules" ? (
            matched !== undefined ? (
                <>
                    Rule {matched.index + 1} matched:{" "}
                    <span className="mono">
                        <ExpressionText
                            text={matched.condition}
                            hrefFor={hrefEntity}
                        />
                    </span>
                </>
            ) : (
                "No rule matched: the default answers."
            )
        ) : null;
    const parts: string[] = [];
    if (catalogBacked && source !== undefined) {
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
        <Strip
            tone={
                method === "rules" && matched === undefined ? "neutral" : "ok"
            }
        >
            {!composite ? (
                <span className="mono answer-value">
                    {catalogBacked
                        ? resolvedValueText(trace) || "(no entries)"
                        : JSON.stringify(value)}
                </span>
            ) : null}
            {why !== null ? <span className="answer-why">{why}</span> : null}
            <span className="hint answer-prov">{parts.join(" · ")}</span>
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
                <span className="hint answer-alloc">
                    Allocation {trace.allocation.allocation} on layer{" "}
                    {trace.allocation.layer}:{" "}
                    {trace.allocation.enrolled
                        ? `enrolled, bucket ${trace.allocation.bucket ?? "?"}${
                              trace.allocation.arm !== undefined
                                  ? `, arm ${trace.allocation.arm}`
                                  : ", unclaimed"
                          }`
                        : "not enrolled"}
                </span>
            ) : null}
        </Strip>
    );
}

// The composite value block and allocation line break below the answer row;
// scalar value, why, and provenance share one wrapping baseline row.
function Strip({
    tone,
    children,
}: {
    tone: "ok" | "neutral" | "warn" | "idle";
    children: ReactNode;
}) {
    return (
        <div className="answer-strip" data-tone={tone}>
            <span className="label answer-label">answer</span>
            {children}
        </div>
    );
}
