// The console's design-system toolkit, handed to experiences through the
// extension contract (src/extension-api.ts). Extensions receive these as a
// capability instead of importing console internals, so they look like the
// console while staying propose-only. The floor uses the same components,
// which is what keeps the two indistinguishable to the eye.

import {
    Children,
    isValidElement,
    useMemo,
    useState,
    type ReactNode,
} from "react";

import type { Control } from "@/lib/api";
import type { UiKit } from "@/extension-api.ts";

export function Banner({
    tone,
    children,
}: {
    tone: "info" | "warn" | "err";
    children: ReactNode;
}) {
    return <div className={`banner banner-${tone}`}>{children}</div>;
}

export function Pill({
    tone,
    title,
    children,
}: {
    tone: "info" | "ok" | "warn" | "err" | "neutral";
    title?: string;
    children: ReactNode;
}) {
    return (
        <span className={`pill pill-${tone}`} title={title}>
            {children}
        </span>
    );
}

// `submit` makes this the enclosing form's default button, so Enter in any
// of the form's inputs presses it; every other Button is type="button" so
// it never submits a form it happens to sit inside.
export function Button({
    tone = "secondary",
    disabled,
    title,
    submit,
    onClick,
    children,
}: {
    tone?: "primary" | "secondary" | "ghost";
    disabled?: boolean;
    title?: string;
    submit?: boolean;
    onClick?: () => void;
    children: ReactNode;
}) {
    return (
        <button
            className={`btn btn-${tone} btn-sm`}
            type={submit === true ? "submit" : "button"}
            disabled={disabled}
            title={title}
            onClick={onClick}
        >
            {children}
        </button>
    );
}

export function Toggle({
    on,
    disabled,
    onChange,
}: {
    on: boolean;
    disabled?: boolean;
    onChange: (next: boolean) => void;
}) {
    return (
        <button
            className={`toggle ${on ? "toggle-on" : ""}`}
            role="switch"
            aria-checked={on}
            disabled={disabled}
            onClick={() => onChange(!on)}
        >
            <span className="toggle-knob" />
        </button>
    );
}

function SearchGlyph({ size }: { size: number }) {
    return (
        <svg
            aria-hidden="true"
            width={size}
            height={size}
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
        >
            <circle cx="11" cy="11" r="7" />
            <line x1="21" y1="21" x2="16.2" y2="16.2" />
        </svg>
    );
}

// The search input SearchableList renders; exported alone for lists whose
// markup the wrapper can't host (tables), so the affordance stays uniform.
export function SearchControl({
    label,
    placeholder,
    query,
    onChange,
}: {
    label: string;
    placeholder: string;
    query: string;
    onChange: (query: string) => void;
}) {
    return (
        <label className="search-control">
            <span className="search-icon">
                <SearchGlyph size={15} />
            </span>
            <input
                aria-label={label}
                className="input"
                onChange={(event) => onChange(event.target.value)}
                placeholder={placeholder}
                type="search"
                value={query}
            />
        </label>
    );
}

// A client-side searchable list: children carry a `data-search` string and
// the query filters on it. Filtering is presentation only — nothing is
// re-fetched, so it works the same on every screen that lists things.
export function SearchableList({
    label,
    placeholder,
    children,
    className,
    emptyLabel,
    action,
}: {
    label: string;
    placeholder: string;
    children: ReactNode;
    className?: string;
    emptyLabel: string;
    /** Rendered beside the search input: the collection's create action. */
    action?: ReactNode;
}) {
    const [query, setQuery] = useState("");
    const items = useMemo(() => Children.toArray(children), [children]);
    const needle = query.trim().toLowerCase();
    const visibleItems =
        needle === ""
            ? items
            : items.filter((item) => searchableText(item).includes(needle));

    return (
        <div className="searchable-list">
            <div className="searchable-toolbar">
                <SearchControl
                    label={label}
                    placeholder={placeholder}
                    query={query}
                    onChange={setQuery}
                />
                {action}
            </div>
            {visibleItems.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <SearchGlyph size={18} />
                    </span>
                    <p>{emptyLabel}</p>
                </div>
            ) : className !== undefined ? (
                <div className={className}>{visibleItems}</div>
            ) : (
                visibleItems
            )}
        </div>
    );
}

function searchableText(item: ReactNode): string {
    if (!isValidElement<{ "data-search"?: unknown }>(item)) {
        return "";
    }
    return String(item.props["data-search"] ?? "").toLowerCase();
}

export function Table({
    head,
    children,
}: {
    head: ReactNode[];
    children: ReactNode;
}) {
    return (
        <div className="table-scroll">
            <table className="data-table">
                <thead>
                    <tr>
                        {head.map((cell, index) => (
                            <th key={index}>{cell}</th>
                        ))}
                    </tr>
                </thead>
                <tbody>{children}</tbody>
            </table>
        </div>
    );
}

export function Field({
    label,
    hint,
    children,
}: {
    label: string;
    hint?: string;
    children: ReactNode;
}) {
    return (
        <div className="field-row surface-item">
            <span className="label" title={hint}>
                {label}
            </span>
            {children}
        </div>
    );
}

// One inferred control. Commit-on-blur (or on toggle/select change): every
// commit is one operation, one save, one commit on the change set.
export function ControlInput({
    control,
    value,
    disabled,
    onCommit,
}: {
    control: Control;
    value: unknown;
    disabled: boolean;
    onCommit: (value: unknown) => void;
}) {
    const [text, setText] = useState(() => controlText(control, value));
    const commitText = () => {
        if (text === controlText(control, value)) {
            return;
        }
        try {
            onCommit(textToControlValue(control, text));
        } catch {
            setText(controlText(control, value));
        }
    };
    if (control.control === "toggle") {
        return (
            <Toggle
                on={value === true}
                disabled={disabled}
                onChange={(next) => onCommit(next)}
            />
        );
    }
    if (control.control === "select") {
        return (
            <select
                className="input"
                disabled={disabled}
                value={JSON.stringify(value)}
                onChange={(event) => onCommit(JSON.parse(event.target.value))}
            >
                {!control.options.some(
                    (option) =>
                        JSON.stringify(option) === JSON.stringify(value),
                ) ? (
                    <option value={JSON.stringify(value)}>
                        {String(value)}
                    </option>
                ) : null}
                {control.options.map((option, index) => (
                    <option key={index} value={JSON.stringify(option)}>
                        {String(option)}
                    </option>
                ))}
            </select>
        );
    }
    return (
        <input
            className="input mono"
            type={control.control === "number" ? "number" : "text"}
            disabled={disabled}
            value={text}
            onChange={(event) => setText(event.target.value)}
            onBlur={commitText}
            onKeyDown={(event) => {
                if (event.key === "Enter") {
                    // Enter commits this one cell, never an enclosing form.
                    event.preventDefault();
                    commitText();
                }
            }}
        />
    );
}

function controlText(control: Control, value: unknown): string {
    if (value === undefined || value === null) {
        return "";
    }
    if (control.control === "text" && typeof value === "string") {
        return value;
    }
    return JSON.stringify(value);
}

function textToControlValue(control: Control, text: string): unknown {
    if (control.control === "number") {
        const value = Number(text);
        if (!Number.isFinite(value)) {
            throw new Error(`${text} is not a number`);
        }
        return value;
    }
    if (control.control === "text") {
        return text;
    }
    return JSON.parse(text);
}

export function AdvancedShape({
    label,
    detail,
    onOpen,
}: {
    label: string;
    detail?: string;
    onOpen?: () => void;
}) {
    return (
        <div className="field-row surface-item">
            <span className="label mono">{label}</span>
            <span className="hint">
                {detail ?? "Advanced shape; this experience shows it plainly."}
            </span>
            {onOpen !== undefined ? (
                <button className="btn btn-ghost btn-sm" onClick={onOpen}>
                    View in workbench
                </button>
            ) : null}
        </div>
    );
}

export const UI_KIT: UiKit = {
    Banner,
    Pill,
    Button,
    Toggle,
    Table,
    Field,
    ControlInput,
    AdvancedShape,
};
