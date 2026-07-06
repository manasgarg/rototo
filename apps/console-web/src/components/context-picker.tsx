// The first-class context picker (design/console-system-view.md): a context
// is chosen once and carried across views, because the execution facet is
// always parameterized by one. Sources: the package's saved samples,
// synthesized boundary contexts from the fixtures machinery, or ad-hoc JSON.

import { useState } from "react";

import type { ContextInventory, SynthesizedContext } from "@/lib/api";

export type ChosenContext =
    | { kind: "none" }
    | {
          kind: "sample";
          key: string;
          context: Record<string, unknown>;
      }
    | {
          kind: "synthetic";
          label: string;
          context: Record<string, unknown>;
      }
    | { kind: "adhoc"; context: Record<string, unknown> };

export function contextLabel(chosen: ChosenContext): string {
    switch (chosen.kind) {
        case "none":
            return "no context";
        case "sample":
            return `sample ${chosen.key}`;
        case "synthetic":
            return `synthetic ${chosen.label}`;
        case "adhoc":
            return "custom context";
    }
}

export function syntheticLabel(entry: SynthesizedContext): string {
    return `${entry.target.id} · ${entry.caseId}`;
}

export function ContextPicker({
    inventory,
    chosen,
    onChange,
}: {
    inventory: ContextInventory | null;
    chosen: ChosenContext;
    onChange: (chosen: ChosenContext) => void;
}) {
    const [editing, setEditing] = useState(false);
    const [text, setText] = useState("{}");
    const [problem, setProblem] = useState<string | null>(null);

    const samples = inventory?.samples ?? [];
    const synthesized = inventory?.synthesized ?? [];

    const selectValue =
        chosen.kind === "none"
            ? ""
            : chosen.kind === "sample"
              ? `sample:${chosen.key}`
              : chosen.kind === "synthetic"
                ? `synthetic:${chosen.label}`
                : "adhoc";

    const select = (value: string) => {
        setEditing(false);
        if (value === "") {
            onChange({ kind: "none" });
            return;
        }
        if (value === "adhoc") {
            setEditing(true);
            setProblem(null);
            if (chosen.kind !== "adhoc") {
                setText(
                    JSON.stringify(
                        chosen.kind === "none" ? {} : chosen.context,
                        null,
                        2,
                    ),
                );
            }
            return;
        }
        if (value.startsWith("sample:")) {
            const key = value.slice("sample:".length);
            const sample = samples.find((entry) => entry.key === key);
            if (sample?.context != null) {
                onChange({ kind: "sample", key, context: sample.context });
            }
            return;
        }
        const label = value.slice("synthetic:".length);
        const entry = synthesized.find(
            (candidate) => syntheticLabel(candidate) === label,
        );
        if (entry !== undefined) {
            onChange({ kind: "synthetic", label, context: entry.context });
        }
    };

    return (
        <div className="context-picker">
            <span className="label mode-strip-label">context</span>
            <select
                className="input"
                value={editing ? "adhoc" : selectValue}
                onChange={(event) => select(event.target.value)}
            >
                <option value="">None (structure only)</option>
                {samples.length > 0 ? (
                    <optgroup label="Saved samples">
                        {samples.map((sample) => (
                            <option
                                key={sample.key}
                                value={`sample:${sample.key}`}
                            >
                                {sample.evaluationContext}/{sample.key}
                            </option>
                        ))}
                    </optgroup>
                ) : null}
                {synthesized.length > 0 ? (
                    <optgroup label="Synthesized (one per behavior case)">
                        {synthesized.map((entry) => (
                            <option
                                key={syntheticLabel(entry)}
                                value={`synthetic:${syntheticLabel(entry)}`}
                            >
                                {syntheticLabel(entry)}
                            </option>
                        ))}
                    </optgroup>
                ) : null}
                <option value="adhoc">Custom JSON…</option>
            </select>
            {chosen.kind !== "none" && !editing ? (
                <span
                    className="hint mono context-picker-peek"
                    title={JSON.stringify(chosen.context, null, 2)}
                >
                    {peek(chosen.context)}
                </span>
            ) : null}
            {editing ? (
                <span className="inline-form context-picker-editor">
                    <textarea
                        autoFocus
                        className="textarea mono"
                        rows={6}
                        value={text}
                        onChange={(event) => setText(event.target.value)}
                    />
                    <span className="action-row">
                        <button
                            className="btn btn-primary btn-sm"
                            onClick={() => {
                                try {
                                    const context = JSON.parse(text) as Record<
                                        string,
                                        unknown
                                    >;
                                    if (
                                        context === null ||
                                        typeof context !== "object" ||
                                        Array.isArray(context)
                                    ) {
                                        throw new Error(
                                            "a context is a JSON object",
                                        );
                                    }
                                    setEditing(false);
                                    setProblem(null);
                                    onChange({ kind: "adhoc", context });
                                } catch (error) {
                                    setProblem(
                                        error instanceof Error
                                            ? error.message
                                            : String(error),
                                    );
                                }
                            }}
                        >
                            Use context
                        </button>
                        <button
                            className="btn btn-ghost btn-sm"
                            onClick={() => setEditing(false)}
                        >
                            Cancel
                        </button>
                        {problem !== null ? (
                            <span className="hint">{problem}</span>
                        ) : null}
                    </span>
                </span>
            ) : null}
        </div>
    );
}

function peek(context: Record<string, unknown>): string {
    const text = JSON.stringify(context);
    return text.length > 60 ? `${text.slice(0, 60)}…` : text;
}
