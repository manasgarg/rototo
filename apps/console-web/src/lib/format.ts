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
