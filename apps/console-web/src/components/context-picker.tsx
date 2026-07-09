// The first-class context picker (design/console-system-view.md): a context
// is chosen once and carried across views, because the execution facet is
// always parameterized by one. The strip stays collapsed to a one-line
// summary until clicked. Its dropdown offers only the package's saved
// samples; synthesized boundary contexts are generated from a variable's
// own preview panel, and any chosen context can be edited into an ad-hoc
// one in place.

import { useRef, useState } from "react";

import type { ContextInventory, SynthesizedContext } from "@/lib/api";
import { contextToToml } from "@/lib/format";

type ComboOption = { value: string; label: string; group?: string };

// A combobox over the context sources: type to filter, arrows to move,
// Enter to pick. A package accumulates samples and synthesized cases well
// past what a plain dropdown lets anyone scan.
function ContextCombo({
    label,
    options,
    value,
    fallbackLabel,
    onPick,
}: {
    label: string;
    options: ComboOption[];
    value: string;
    // Shown when the chosen context has no dropdown entry (a synthesized
    // or ad-hoc context): the input still names what is active.
    fallbackLabel?: string;
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
                value={query ?? selected?.label ?? fallbackLabel ?? ""}
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
    const [expanded, setExpanded] = useState(false);
    const [display, setDisplay] = useState<"chips" | "toml">("chips");
    const [editing, setEditing] = useState(false);
    const [text, setText] = useState("{}");
    const [problem, setProblem] = useState<string | null>(null);

    const samples = inventory?.samples ?? [];

    const selectValue = chosen.kind === "sample" ? `sample:${chosen.key}` : "";

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

    // Editing starts from whatever is chosen, so a saved sample is one
    // tweak away from a what-if; the result is the session's ad-hoc
    // context, never a write to the sample.
    const startEditing = () => {
        setText(
            JSON.stringify(
                chosen.kind === "none" ? {} : chosen.context,
                null,
                2,
            ),
        );
        setProblem(null);
        setEditing(true);
    };

    const select = (value: string) => {
        setEditing(false);
        if (value === "") {
            onChange({ kind: "none" });
            return;
        }
        const key = value.slice("sample:".length);
        const sample = samples.find((entry) => entry.key === key);
        if (sample?.context != null) {
            onChange({ kind: "sample", key, context: sample.context });
        }
    };

    const options: ComboOption[] = [
        { value: "", label: "None (structure only)" },
        ...samples.map((sample) => ({
            value: `sample:${sample.key}`,
            label: `${sample.evaluationContext}/${sample.key}`,
            group: "Saved samples",
        })),
    ];

    return (
        <div className="context-picker">
            <button
                aria-expanded={expanded}
                className="context-picker-summary"
                type="button"
                onClick={() => setExpanded(!expanded)}
            >
                <span className="label mode-strip-label">given context</span>
                <span className="context-picker-chosen">
                    {contextLabel(chosen)}
                </span>
                {!expanded && chosen.kind !== "none" ? (
                    <span
                        className="context-picker-glance mono"
                        title={JSON.stringify(chosen.context, null, 2)}
                    >
                        {flattenContext(chosen.context)
                            .map((fact) => `${fact.path} = ${fact.value}`)
                            .join("   ")}
                    </span>
                ) : null}
                <svg
                    aria-hidden="true"
                    className="context-picker-chevron"
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    style={{
                        transform: expanded ? "rotate(180deg)" : undefined,
                    }}
                >
                    <polyline points="6 9 12 15 18 9" />
                </svg>
            </button>
            {expanded ? (
                <div className="context-picker-body">
                    <div className="context-picker-controls">
                        <ContextCombo
                            label="Given context"
                            options={options}
                            value={selectValue}
                            fallbackLabel={
                                chosen.kind === "synthetic" ||
                                chosen.kind === "adhoc"
                                    ? contextLabel(chosen)
                                    : undefined
                            }
                            onPick={select}
                        />
                        <div
                            aria-label="Context display"
                            className="segmented-control"
                            role="group"
                        >
                            <button
                                className={
                                    display === "chips" ? "active" : undefined
                                }
                                type="button"
                                onClick={() => setDisplay("chips")}
                            >
                                Chips
                            </button>
                            <button
                                className={
                                    display === "toml" ? "active" : undefined
                                }
                                type="button"
                                onClick={() => setDisplay("toml")}
                            >
                                TOML
                            </button>
                        </div>
                        {!editing ? (
                            <button
                                className="btn btn-secondary btn-sm"
                                title="Edit this context as JSON; the result becomes the custom context"
                                type="button"
                                onClick={startEditing}
                            >
                                Edit
                            </button>
                        ) : null}
                    </div>
                    {chosen.kind !== "none" && !editing ? (
                        display === "chips" ? (
                            <ContextFacts context={chosen.context} />
                        ) : (
                            <pre className="codewell context-toml">
                                {contextToToml(chosen.context)}
                            </pre>
                        )
                    ) : null}
                    {editing ? (
                        <span className="inline-form context-picker-editor">
                            <textarea
                                autoFocus
                                className="textarea mono"
                                rows={6}
                                value={text}
                                onChange={(event) =>
                                    setText(event.target.value)
                                }
                                onKeyDown={(event) => {
                                    // Enter is a newline in JSON;
                                    // Ctrl/Cmd+Enter applies the context.
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
            ) : null}
        </div>
    );
}
