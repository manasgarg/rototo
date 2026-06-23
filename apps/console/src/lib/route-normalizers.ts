/** Workspace screen section id accepted from route/query state. */
export type SectionId =
    | "overview"
    | "variables"
    | "qualifiers"
    | "catalogs"
    | "linters"
    | "context"
    | "diagnostics"
    | "branches";

/** Branch edit section id accepted from route/query state. */
export type EditKind =
    | "variables"
    | "qualifiers"
    | "catalogs"
    | "context"
    | "linters";

export function normalizeSection(value: string | null): SectionId | null {
    if (
        value === "overview" ||
        value === "variables" ||
        value === "qualifiers" ||
        value === "catalogs" ||
        value === "linters" ||
        value === "context" ||
        value === "diagnostics" ||
        value === "branches"
    ) {
        return value;
    }
    return null;
}

export function normalizeEditKind(value: string | null): EditKind | null {
    if (
        value === "variables" ||
        value === "qualifiers" ||
        value === "catalogs" ||
        value === "context" ||
        value === "linters"
    ) {
        return value;
    }
    return null;
}
