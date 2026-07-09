// The first-class context picker (design/console-system-view.md): a context
// is chosen once and carried across views, because the execution facet is
// always parameterized by one. Sources: the package's saved samples,
// synthesized boundary contexts from the fixtures machinery, or ad-hoc JSON.

import { useRef, useState } from "react";

import type { ContextInventory, SynthesizedContext } from "@/lib/api";

type ComboOption = { value: string; label: string; group?: string };

// A combobox over the context sources: type to filter, arrows to move,
// Enter to pick. A package accumulates samples and synthesized cases well
// past what a plain dropdown lets anyone scan.
function ContextCombo({
    label,
    options,
    value,
    onPick,
}: {
    label: string;
    options: ComboOption[];
    value: string;
    onPick: (value: string) => void;
}) {
    const [open, setOpen] = useState(false);
    // null shows the selection's label; a string is a live filter.
    const [query, setQuery] = useState<string | null>(null);
    const [active, setActive] = useState(0);
    const wrapper = useRef<HTMLDivElement>(null);

    const selected = options.find((option) => option.value === value);
    const needle = (query ?? "").trim().toLowerCase();
    const visible =
        needle === ""
            ? options
            : options.filter((option) =>
                  option.label.toLowerCase().includes(needle),
              );
    const activeIndex = Math.min(active, Math.max(visible.length - 1, 0));

    const close = () => {
        setOpen(false);
        setQuery(null);
        setActive(0);
    };
    const pick = (option: ComboOption) => {
        onPick(option.value);
        close();
    };

    return (
        <div
            className="combo"
            ref={wrapper}
            onBlur={(event) => {
                if (!wrapper.current?.contains(event.relatedTarget as Node)) {
                    close();
                }
            }}
        >
            <input
                aria-expanded={open}
                aria-label={label}
                className="input"
                placeholder="Search contexts"
                role="combobox"
                value={query ?? selected?.label ?? ""}
                onChange={(event) => {
                    setQuery(event.target.value);
                    setOpen(true);
                    setActive(0);
                }}
                onFocus={() => setOpen(true)}
                onKeyDown={(event) => {
                    if (event.key === "ArrowDown") {
                        event.preventDefault();
                        setOpen(true);
                        setActive(
                            Math.min(activeIndex + 1, visible.length - 1),
                        );
                    } else if (event.key === "ArrowUp") {
                        event.preventDefault();
                        setActive(Math.max(activeIndex - 1, 0));
                    } else if (event.key === "Enter") {
                        event.preventDefault();
                        const option = visible[activeIndex];
                        if (open && option !== undefined) {
                            pick(option);
                        }
                    } else if (event.key === "Escape") {
                        close();
                    }
                }}
            />
            {open ? (
                <div className="combo-menu" role="listbox">
                    {visible.length === 0 ? (
                        <div className="combo-empty hint">
                            No context matches that search.
                        </div>
                    ) : null}
                    {visible.map((option, index) => (
                        <span key={`${option.value}:${option.label}`}>
                            {option.group !== undefined &&
                            option.group !== visible[index - 1]?.group ? (
                                <div className="combo-group label">
                                    {option.group}
                                </div>
                            ) : null}
                            <button
                                aria-selected={option.value === value}
                                className="combo-option"
                                data-active={
                                    index === activeIndex ? "true" : undefined
                                }
                                role="option"
                                type="button"
                                // Keep focus in the input so blur-close and
                                // typing keep working across clicks.
                                onMouseDown={(event) => event.preventDefault()}
                                onClick={() => pick(option)}
                                onMouseEnter={() => setActive(index)}
                            >
                                {option.label}
                            </button>
                        </span>
                    ))}
                </div>
            ) : null}
        </div>
    );
}

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

// A context shown as flattened dotted-path facts: nested JSON braces are
// noise when the question is "which facts does this resolution see". The
// full JSON stays a hover away.
export function ContextFacts({
    context,
}: {
    context: Record<string, unknown>;
}) {
    return (
        <span
            className="context-facts"
            title={JSON.stringify(context, null, 2)}
        >
            {flattenContext(context).map((fact) => (
                <span className="context-fact" key={fact.path}>
                    <span className="context-fact-path">{fact.path}</span>
                    {" = "}
                    <span className="context-fact-value">{fact.value}</span>
                </span>
            ))}
        </span>
    );
}

function flattenContext(
    value: Record<string, unknown>,
    prefix = "",
): { path: string; value: string }[] {
    const facts: { path: string; value: string }[] = [];
    for (const [key, entry] of Object.entries(value)) {
        const path = prefix === "" ? key : `${prefix}.${key}`;
        if (
            entry !== null &&
            typeof entry === "object" &&
            !Array.isArray(entry)
        ) {
            facts.push(
                ...flattenContext(entry as Record<string, unknown>, path),
            );
        } else {
            facts.push({ path, value: JSON.stringify(entry) ?? "null" });
        }
    }
    return facts;
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

    const apply = () => {
        try {
            const context = JSON.parse(text) as Record<string, unknown>;
            if (
                context === null ||
                typeof context !== "object" ||
                Array.isArray(context)
            ) {
                throw new Error("a context is a JSON object");
            }
            setEditing(false);
            setProblem(null);
            onChange({ kind: "adhoc", context });
        } catch (error) {
            setProblem(error instanceof Error ? error.message : String(error));
        }
    };

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

    const options: ComboOption[] = [
        { value: "", label: "None (structure only)" },
        ...samples.map((sample) => ({
            value: `sample:${sample.key}`,
            label: `${sample.evaluationContext}/${sample.key}`,
            group: "Saved samples",
        })),
        ...synthesized.map((entry) => ({
            value: `synthetic:${syntheticLabel(entry)}`,
            label: syntheticLabel(entry),
            group: "Synthesized (one per behavior case)",
        })),
        { value: "adhoc", label: "Custom JSON…" },
    ];

    return (
        <div className="context-picker">
            <span className="label mode-strip-label">given context</span>
            <ContextCombo
                label="Given context"
                options={options}
                value={editing ? "adhoc" : selectValue}
                onPick={select}
            />
            {chosen.kind !== "none" && !editing ? (
                <ContextFacts context={chosen.context} />
            ) : null}
            {editing ? (
                <span className="inline-form context-picker-editor">
                    <textarea
                        autoFocus
                        className="textarea mono"
                        rows={6}
                        value={text}
                        onChange={(event) => setText(event.target.value)}
                        onKeyDown={(event) => {
                            // Enter is a newline in JSON; Ctrl/Cmd+Enter
                            // applies the context.
                            if (
                                event.key === "Enter" &&
                                (event.metaKey || event.ctrlKey)
                            ) {
                                event.preventDefault();
                                apply();
                            }
                        }}
                    />
                    <span className="action-row">
                        <button
                            className="btn btn-primary btn-sm"
                            onClick={apply}
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
