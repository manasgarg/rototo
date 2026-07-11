// The first-class context picker (design/console-system-view.md): a context
// is chosen once and carried across views, because the execution facet is
// always parameterized by one. The dropdown offers the package's saved
// samples and, on a variable screen, can reveal that variable's synthesized
// boundary contexts. Both views edit: chips fact by fact, JSON as a whole.
// Every edit lands on the session's ad-hoc context, never on the sample.
// Two shells share the working surface (ContextPickerBody): the overview's
// collapsible strip, and the variable screen's try-it card.

import { useEffect, useRef, useState } from "react";

import type { ContextInventory, SynthesizedContext } from "@/lib/api";
import { CodeEditor } from "@/components/code-editor";

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
    // Return true for an option that mutates this menu in place. Generation
    // uses that path so the new boundary choices replace the action without
    // making the reader reopen the dropdown.
    onPick: (value: string) => boolean;
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
        if (onPick(option.value)) {
            setOpen(true);
            setQuery(null);
            setActive(0);
        } else {
            close();
        }
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

// Whether a chip's dotted path is one the resolution reads. Prefix-aware in
// both directions: a rule reading `account.plan` lights the `account.plan`
// chip, a rule reading `account` lights every `account.*` chip, and a rule
// reading `account.plan` still lights a bare `account` chip (the read walks
// into it, whatever it finds there).
function pathIsRead(path: string, readPaths: string[]): boolean {
    return readPaths.some(
        (read) =>
            read === path ||
            read.startsWith(`${path}.`) ||
            path.startsWith(`${read}.`),
    );
}

// A context shown as flattened dotted-path facts: nested JSON braces are
// noise when the question is "which facts does this resolution see". The
// full JSON stays a hover away. With `onEdit` the chips become an editor:
// click a value to change it, × drops the key, "+ Add key" grows the
// context by one dotted path. With `readPaths` the chips explain: paths the
// resolution reads get the sea treatment, the rest dim.
export function ContextFacts({
    context,
    readPaths,
    onEdit,
}: {
    context: Record<string, unknown>;
    readPaths?: string[];
    onEdit?: (context: Record<string, unknown>) => void;
}) {
    // The freshly added path flashes for a moment so the eye lands where
    // the fact landed among the existing chips.
    const [justAdded, setJustAdded] = useState<string | null>(null);
    useEffect(() => {
        if (justAdded === null) {
            return;
        }
        const timer = setTimeout(() => setJustAdded(null), 2500);
        return () => clearTimeout(timer);
    }, [justAdded]);
    // An added JSON-object value flattens to chips below the added path,
    // so the flash covers the path's whole subtree.
    const added = (path: string): boolean =>
        justAdded !== null &&
        (path === justAdded || path.startsWith(`${justAdded}.`));
    const read = (path: string): boolean | undefined =>
        readPaths === undefined ? undefined : pathIsRead(path, readPaths);
    return (
        <span
            className="context-facts"
            data-highlights={readPaths !== undefined || undefined}
            title={JSON.stringify(context, null, 2)}
        >
            {flattenContext(context).map((fact) =>
                onEdit === undefined ? (
                    <span
                        className="context-fact"
                        data-read={read(fact.path)}
                        key={fact.path}
                    >
                        <span className="context-fact-path">{fact.path}</span>
                        {" = "}
                        <span className="context-fact-value">
                            {factText(fact.value)}
                        </span>
                    </span>
                ) : (
                    <EditableFact
                        added={added(fact.path)}
                        key={fact.path}
                        path={fact.path}
                        read={read(fact.path)}
                        value={fact.value}
                        onCommit={(text) =>
                            onEdit(
                                setAtPath(
                                    context,
                                    fact.path.split("."),
                                    factValueFromText(text),
                                ),
                            )
                        }
                        onRemove={() =>
                            onEdit(removeAtPath(context, fact.path.split(".")))
                        }
                    />
                ),
            )}
            {onEdit !== undefined ? (
                <AddFact
                    onAdd={(path, text) => {
                        onEdit(
                            setAtPath(
                                context,
                                path.split("."),
                                factValueFromText(text),
                            ),
                        );
                        setJustAdded(path);
                    }}
                />
            ) : null}
        </span>
    );
}

// One editable chip. Commit on Enter or blur, the ControlInput idiom; an
// unchanged draft commits nothing, so a click-through leaves a sample a
// sample.
function EditableFact({
    added,
    path,
    read,
    value,
    onCommit,
    onRemove,
}: {
    added?: boolean;
    path: string;
    read?: boolean;
    value: unknown;
    onCommit: (text: string) => void;
    onRemove: () => void;
}) {
    // null shows the value; a string is a live draft.
    const [draft, setDraft] = useState<string | null>(null);
    const commit = () => {
        if (draft !== null && draft !== factEditText(value)) {
            onCommit(draft);
        }
        setDraft(null);
    };
    return (
        <span
            className={
                added === true
                    ? "context-fact context-fact-added"
                    : "context-fact"
            }
            data-read={read}
        >
            <span className="context-fact-path">{path}</span>
            {" = "}
            {draft === null ? (
                <>
                    <button
                        className="context-fact-value"
                        title="Edit this value"
                        type="button"
                        onClick={() => setDraft(factEditText(value))}
                    >
                        {factText(value)}
                    </button>
                    <button
                        className="context-fact-remove"
                        title={`Remove ${path}`}
                        type="button"
                        onClick={onRemove}
                    >
                        ×
                    </button>
                </>
            ) : (
                <input
                    autoFocus
                    className="input mono context-fact-input"
                    size={Math.max(draft.length + 2, 6)}
                    value={draft}
                    onChange={(event) => setDraft(event.target.value)}
                    onBlur={commit}
                    onKeyDown={(event) => {
                        if (event.key === "Enter") {
                            event.preventDefault();
                            commit();
                        }
                    }}
                />
            )}
        </span>
    );
}

function AddFact({ onAdd }: { onAdd: (path: string, text: string) => void }) {
    const [open, setOpen] = useState(false);
    const [path, setPath] = useState("");
    const [text, setText] = useState("");
    if (!open) {
        return (
            <button
                className="context-fact context-fact-add"
                type="button"
                onClick={() => setOpen(true)}
            >
                + Add key
            </button>
        );
    }
    const close = () => {
        setOpen(false);
        setPath("");
        setText("");
    };
    const valid =
        path.trim() !== "" &&
        !path
            .trim()
            .split(".")
            .some((segment) => segment === "");
    return (
        <form
            className="context-fact context-fact-form"
            onSubmit={(event) => {
                event.preventDefault();
                if (!valid) {
                    return;
                }
                onAdd(path.trim(), text);
                close();
            }}
        >
            <input
                autoFocus
                className="input mono context-fact-input"
                placeholder="path.to.key"
                size={14}
                value={path}
                onChange={(event) => setPath(event.target.value)}
            />
            {" = "}
            <input
                className="input mono context-fact-input"
                placeholder="value"
                size={10}
                value={text}
                onChange={(event) => setText(event.target.value)}
            />
            <button
                className="btn btn-secondary btn-sm"
                disabled={!valid}
                type="submit"
            >
                Add
            </button>
            <button
                className="btn btn-ghost btn-sm"
                type="button"
                onClick={close}
            >
                Cancel
            </button>
        </form>
    );
}

function flattenContext(
    value: Record<string, unknown>,
    prefix = "",
): { path: string; value: unknown }[] {
    const facts: { path: string; value: unknown }[] = [];
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
            facts.push({ path, value: entry });
        }
    }
    return facts;
}

function factText(value: unknown): string {
    return JSON.stringify(value) ?? "null";
}

// A string edits without its quotes; everything else edits as JSON text.
function factEditText(value: unknown): string {
    return typeof value === "string" ? value : (JSON.stringify(value) ?? "");
}

// The CLI's `--context path=value` typing rule (src/cli/context.rs):
// JSON if it parses, otherwise the raw string.
function factValueFromText(text: string): unknown {
    try {
        return JSON.parse(text);
    } catch {
        return text;
    }
}

// The CLI's `insert_context_path` shape: a dotted path descends objects,
// and a non-object in the way is replaced by one.
function setAtPath(
    context: Record<string, unknown>,
    path: string[],
    value: unknown,
): Record<string, unknown> {
    const [head, ...rest] = path;
    if (head === undefined) {
        return context;
    }
    if (rest.length === 0) {
        return { ...context, [head]: value };
    }
    const current = context[head];
    const child =
        current !== null &&
        typeof current === "object" &&
        !Array.isArray(current)
            ? (current as Record<string, unknown>)
            : {};
    return { ...context, [head]: setAtPath(child, rest, value) };
}

function removeAtPath(
    context: Record<string, unknown>,
    path: string[],
): Record<string, unknown> {
    const [head, ...rest] = path;
    if (head === undefined || !(head in context)) {
        return context;
    }
    if (rest.length === 0) {
        const remaining = { ...context };
        delete remaining[head];
        return remaining;
    }
    const current = context[head];
    if (
        current === null ||
        typeof current !== "object" ||
        Array.isArray(current)
    ) {
        return context;
    }
    return {
        ...context,
        [head]: removeAtPath(current as Record<string, unknown>, rest),
    };
}

// The picker's working surface: the source combo, the chips/JSON editor,
// and the boundary-context flows. The overview's collapsible strip and the
// variable screen's try-it card both render this body; only the shell
// around it differs.
export function ContextPickerBody({
    inventory,
    chosen,
    boundaryVariableId,
    canPromoteBoundary = false,
    // The overview reads an empty pick as "show structure only"; the
    // variable screen's try-it card just has no context yet.
    emptyOptionLabel = "None (structure only)",
    // The context paths the surrounding screen's resolution reads; chips on
    // those paths highlight, the rest dim, and a caption explains why.
    readPaths,
    onPromoteBoundary,
    onChange,
}: {
    inventory: ContextInventory | null;
    chosen: ChosenContext;
    boundaryVariableId?: string;
    canPromoteBoundary?: boolean;
    emptyOptionLabel?: string;
    readPaths?: string[];
    onPromoteBoundary?: (entry: SynthesizedContext) => void;
    onChange: (chosen: ChosenContext) => void;
}) {
    const [display, setDisplay] = useState<"chips" | "json">("chips");
    const [text, setText] = useState("{}");
    const [problem, setProblem] = useState<string | null>(null);
    const [boundariesGenerated, setBoundariesGenerated] = useState(false);

    const samples = inventory?.samples ?? [];
    const boundaryCases =
        inventory?.synthesized.filter(
            (entry) => entry.target.id === boundaryVariableId,
        ) ?? [];
    const showBoundaries = boundariesGenerated || chosen.kind === "synthetic";

    const selectValue =
        chosen.kind === "sample"
            ? `sample:${chosen.key}`
            : chosen.kind === "synthetic"
              ? `synthetic:${chosen.label}`
              : "";

    // The JSON tab is the whole-context editor; entering it, or switching
    // the chosen context while on it, re-prefills the draft.
    useEffect(() => {
        if (display === "json") {
            setText(
                JSON.stringify(
                    chosen.kind === "none" ? {} : chosen.context,
                    null,
                    2,
                ),
            );
            setProblem(null);
        }
    }, [display, chosen]);

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
            setProblem(null);
            onChange({ kind: "adhoc", context });
        } catch (error) {
            setProblem(error instanceof Error ? error.message : String(error));
        }
    };

    const select = (value: string): boolean => {
        if (value === "") {
            onChange({ kind: "none" });
            return false;
        }
        if (value === "generate:boundaries") {
            setBoundariesGenerated(true);
            return true;
        }
        if (value.startsWith("sample:")) {
            const key = value.slice("sample:".length);
            const sample = samples.find((entry) => entry.key === key);
            if (sample?.context != null) {
                onChange({ kind: "sample", key, context: sample.context });
            }
            return false;
        }
        if (value.startsWith("synthetic:")) {
            const label = value.slice("synthetic:".length);
            const entry = boundaryCases.find(
                (candidate) => syntheticLabel(candidate) === label,
            );
            if (entry !== undefined) {
                onChange({
                    kind: "synthetic",
                    label,
                    context: entry.context,
                });
            }
        }
        return false;
    };

    const options: ComboOption[] = [
        { value: "", label: emptyOptionLabel },
        ...samples.map((sample) => ({
            value: `sample:${sample.key}`,
            label: `${sample.evaluationContext}/${sample.key}`,
            group: "Saved samples",
        })),
        ...(boundaryCases.length === 0
            ? []
            : showBoundaries
              ? boundaryCases.map((entry) => ({
                    value: `synthetic:${syntheticLabel(entry)}`,
                    label: entry.title,
                    group: "Boundary contexts",
                }))
              : [
                    {
                        value: "generate:boundaries",
                        label: "Generate boundary contexts…",
                        group: "Boundary contexts",
                    },
                ]),
    ];
    const selectedBoundary =
        chosen.kind === "synthetic"
            ? boundaryCases.find(
                  (entry) => syntheticLabel(entry) === chosen.label,
              )
            : undefined;

    return (
        <div className="context-picker-body">
            <div className="context-picker-controls">
                <ContextCombo
                    label="Given context"
                    options={options}
                    value={selectValue}
                    fallbackLabel={
                        chosen.kind === "synthetic" || chosen.kind === "adhoc"
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
                        className={display === "chips" ? "active" : undefined}
                        type="button"
                        onClick={() => setDisplay("chips")}
                    >
                        Chips
                    </button>
                    <button
                        className={display === "json" ? "active" : undefined}
                        type="button"
                        onClick={() => setDisplay("json")}
                    >
                        JSON
                    </button>
                </div>
            </div>
            {display === "chips" ? (
                <>
                    <ContextFacts
                        context={chosen.kind === "none" ? {} : chosen.context}
                        readPaths={readPaths}
                        onEdit={(context) =>
                            onChange({ kind: "adhoc", context })
                        }
                    />
                    {readPaths !== undefined && chosen.kind !== "none" ? (
                        <div className="hint context-read-caption">
                            Highlighted keys are the ones this variable&apos;s
                            resolution actually reads.
                        </div>
                    ) : null}
                </>
            ) : (
                <span className="inline-form context-picker-editor">
                    <CodeEditor
                        className="context-json-editor"
                        language="json"
                        value={text}
                        onChange={setText}
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
                        {problem !== null ? (
                            <span className="hint">{problem}</span>
                        ) : null}
                    </span>
                </span>
            )}
            {canPromoteBoundary &&
            selectedBoundary !== undefined &&
            onPromoteBoundary !== undefined ? (
                <div className="action-row">
                    <button
                        className="btn btn-ghost btn-sm"
                        type="button"
                        title="Adds this boundary context as a saved sample in the active change set"
                        onClick={() => onPromoteBoundary(selectedBoundary)}
                    >
                        Save as sample
                    </button>
                </div>
            ) : null}
        </div>
    );
}

// The collapsible given-context strip (design/console-system-view.md): a
// context is chosen once and carried across views, so the overview keeps
// this one-line summary until clicked. The variable screen skips the strip
// and mounts the body inside its try-it card instead.
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
    return (
        <div className="context-picker">
            <button
                aria-expanded={expanded}
                className="context-picker-summary"
                type="button"
                onClick={() => setExpanded(!expanded)}
            >
                <span className="label">given context</span>
                <span className="context-picker-chosen">
                    {contextLabel(chosen)}
                </span>
                {!expanded && chosen.kind !== "none" ? (
                    <span
                        className="context-picker-glance mono"
                        title={JSON.stringify(chosen.context, null, 2)}
                    >
                        {flattenContext(chosen.context)
                            .map(
                                (fact) =>
                                    `${fact.path} = ${factText(fact.value)}`,
                            )
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
                <ContextPickerBody
                    inventory={inventory}
                    chosen={chosen}
                    onChange={onChange}
                />
            ) : null}
        </div>
    );
}
