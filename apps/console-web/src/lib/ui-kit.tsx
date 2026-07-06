// The console's design-system toolkit, handed to experiences through the
// extension contract (src/extension-api.ts). Extensions receive these as a
// capability instead of importing console internals, so they look like the
// console while staying propose-only. The floor uses the same components,
// which is what keeps the two indistinguishable to the eye.

import { useState, type ReactNode } from "react";

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

export function Button({
    tone = "secondary",
    disabled,
    title,
    onClick,
    children,
}: {
    tone?: "primary" | "secondary" | "ghost";
    disabled?: boolean;
    title?: string;
    onClick: () => void;
    children: ReactNode;
}) {
    return (
        <button
            className={`btn btn-${tone} btn-sm`}
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
