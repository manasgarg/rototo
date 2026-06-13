export type SectionId =
  | "overview"
  | "variables"
  | "qualifiers"
  | "catalogs"
  | "schemas"
  | "linters"
  | "context"
  | "diagnostics"
  | "drafts";

export type EditKind =
  | "variables"
  | "qualifiers"
  | "catalogs"
  | "schemas"
  | "context"
  | "linters";

export function normalizeSection(value: string | null): SectionId | null {
  if (
    value === "overview" ||
    value === "variables" ||
    value === "qualifiers" ||
    value === "catalogs" ||
    value === "schemas" ||
    value === "linters" ||
    value === "context" ||
    value === "diagnostics" ||
    value === "drafts"
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
    value === "schemas" ||
    value === "context" ||
    value === "linters"
  ) {
    return value;
  }
  return null;
}
