// Instants come off the wire as RFC3339 strings with millisecond noise and
// assorted offsets; people read "when", not serialization detail.

import type { ResolutionTrace } from "@/lib/api";

// A resolved value, as people name it: a catalog-backed resolution shows
// its entry key(s) rather than the hydrated entry object. The object is
// what an app receives; the name is what a reader recognizes.
export function resolvedValueText(trace: ResolutionTrace): string {
    const source = trace.resolution.source;
    if (source?.kind === "catalog") {
        return source.value;
    }
    if (source?.kind === "catalog_array") {
        return source.values.join(", ");
    }
    return JSON.stringify(trace.resolution.value) ?? "";
}

export function formatInstant(value: string): string {
    const parsed = new Date(value);
    if (Number.isNaN(parsed.getTime())) {
        return value;
    }
    const iso = parsed.toISOString();
    return `${iso.slice(0, 10)} ${iso.slice(11, 16)} UTC`;
}

// A context rendered as TOML, for readers who think in package files. This
// is a reading view of a JSON object, not a round-trippable format: a JSON
// null has no TOML spelling and renders as the bare token `null`.
export function contextToToml(context: Record<string, unknown>): string {
    return tableLines(context, "").join("\n").trim();
}

function tableLines(table: Record<string, unknown>, path: string): string[] {
    const scalars: string[] = [];
    const subtables: string[] = [];
    for (const [key, value] of Object.entries(table)) {
        if (
            value !== null &&
            typeof value === "object" &&
            !Array.isArray(value)
        ) {
            const child =
                path === "" ? tomlKey(key) : `${path}.${tomlKey(key)}`;
            subtables.push(
                "",
                `[${child}]`,
                ...tableLines(value as Record<string, unknown>, child),
            );
        } else {
            scalars.push(`${tomlKey(key)} = ${tomlValue(value)}`);
        }
    }
    return [...scalars, ...subtables];
}

function tomlKey(key: string): string {
    return /^[A-Za-z0-9_-]+$/.test(key) ? key : JSON.stringify(key);
}

function tomlValue(value: unknown): string {
    if (typeof value === "string") {
        return JSON.stringify(value);
    }
    if (Array.isArray(value)) {
        return `[${value.map(tomlValue).join(", ")}]`;
    }
    if (value !== null && typeof value === "object") {
        const fields = Object.entries(value).map(
            ([key, entry]) => `${tomlKey(key)} = ${tomlValue(entry)}`,
        );
        return fields.length === 0 ? "{}" : `{ ${fields.join(", ")} }`;
    }
    return String(value);
}
