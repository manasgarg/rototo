// Surfaces (design/console-surfaces.md): a surface is a catalog entry in the
// console-owned catalog `console/surfaces` — the console configures itself
// with rototo, and no new package concept exists. This module owns the
// vendored catalog schema, reads surface entries off the semantic model,
// validates them on load, infers the floor's controls, and proposes the
// cold-start suggestions. Enforcement lives below all of it: a surface
// bounds affordances, never authority.

import type { JsonObject, JsonValue } from "./native.ts";

export const SURFACES_CATALOG = "console/surfaces";

// The schema the console vendors into packages (as
// model/catalogs/console/surfaces.schema.json) through a surface-creating
// change set. Evolution is additive-only; the freshness diagnostic below
// owns staleness from then on.
export const SURFACES_SCHEMA: JsonObject = {
    description: "Console surface definitions",
    type: "object",
    required: ["title", "bind"],
    properties: {
        kind: {
            type: "string",
            description:
                "The experience that renders this surface; unknown kinds fall back to the floor",
        },
        title: { type: "string" },
        description: { type: "string" },
        audience: {
            type: "array",
            items: { type: "string", enum: ["internal", "tenant"] },
            minItems: 1,
        },
        approval: { type: "string" },
        caution: { type: "string" },
        bind: {
            type: "array",
            minItems: 1,
            items: {
                type: "object",
                required: ["target"],
                properties: {
                    target: { type: "string" },
                    editable_fields: {
                        type: "array",
                        items: { type: "string" },
                    },
                    can_add: { type: "boolean" },
                    can_delete: { type: "boolean" },
                },
                additionalProperties: false,
            },
        },
        config: { type: "object" },
    },
    additionalProperties: false,
};

// The slice of the semantic model the surface machinery reads. The Rust
// server is the source of truth for these shapes (src/lint/semantic_model.rs,
// serde camelCase).
export type ModelView = {
    variables?: {
        id: string;
        description?: string;
        location: { path: string };
        declaration: { kind: string; value?: string };
        resolve?: {
            default?: { value?: JsonValue };
            rules?: unknown[];
        };
    }[];
    catalogs?: {
        id: string;
        path: string;
        description?: string;
        json?: JsonValue;
    }[];
    catalogEntries?: {
        catalog: string;
        key: string;
        value: JsonValue;
        location: { path: string };
    }[];
    enums?: {
        id: string;
        members: { value?: JsonValue }[];
    }[];
    layers?: {
        id: string;
        location: { path: string };
    }[];
};

export type SurfaceDiagnostic = {
    severity: "error" | "warning" | "info";
    message: string;
};

export type SurfaceBinding = {
    target: string;
    editableFields: string[] | null;
    canAdd: boolean;
    canDelete: boolean;
};

export type Surface = {
    id: string;
    kind: string | null;
    title: string;
    description: string | null;
    audience: string[];
    approval: string | null;
    caution: string | null;
    config: JsonObject | null;
    bindings: SurfaceBinding[];
    diagnostics: SurfaceDiagnostic[];
};

// A floor control names exactly the operations it may emit and nothing
// else: the control is the affordance boundary, grants and governance are
// the enforcement.
export type Control =
    | { control: "toggle" }
    | { control: "select"; options: JsonValue[] }
    | { control: "number" }
    | { control: "text" }
    | { control: "json" };

export type FieldControl = Control & { field: string };

export type SurfaceItem =
    | {
          kind: "variable";
          id: string;
          variableType: string | null;
          description: string | null;
          control: Control;
          default: JsonValue | null;
          ruleCount: number;
      }
    | {
          kind: "catalog";
          id: string;
          description: string | null;
          schema: JsonValue | null;
          entries: { key: string; value: JsonValue }[];
          editableFields: string[] | null;
          canAdd: boolean;
          canDelete: boolean;
          fields: FieldControl[];
      }
    | {
          kind: "entry";
          catalog: string;
          key: string;
          value: JsonValue;
          editableFields: string[] | null;
          fields: FieldControl[];
      }
    | { kind: "missing"; target: string };

export type SurfaceSuggestion = {
    id: string;
    kind: string;
    title: string;
    reason: string;
    operations: JsonValue[];
};

// ---------------------------------------------------------------------------
// Reading surfaces off the model

export function readSurfaces(model: ModelView): Surface[] {
    const entries = (model.catalogEntries ?? []).filter(
        (entry) => entry.catalog === SURFACES_CATALOG,
    );
    return entries.map((entry) => parseSurface(entry.key, entry.value, model));
}

function parseSurface(
    id: string,
    value: JsonValue,
    model: ModelView,
): Surface {
    const diagnostics: SurfaceDiagnostic[] = [];
    const raw = (
        typeof value === "object" && value !== null && !Array.isArray(value)
            ? value
            : {}
    ) as JsonObject;

    const title = typeof raw.title === "string" ? raw.title : id;
    if (typeof raw.title !== "string") {
        diagnostics.push({
            severity: "error",
            message: "the surface has no title",
        });
    }
    const audience = Array.isArray(raw.audience)
        ? raw.audience.filter((a): a is string => typeof a === "string")
        : ["internal"];
    for (const a of audience) {
        if (a !== "internal" && a !== "tenant") {
            diagnostics.push({
                severity: "error",
                message: `audience "${a}" is not internal or tenant`,
            });
        }
    }
    const approval = typeof raw.approval === "string" ? raw.approval : null;
    if (
        approval !== null &&
        approval !== "none" &&
        !/^role:[a-z0-9_]+$/.test(approval)
    ) {
        diagnostics.push({
            severity: "error",
            message: `approval "${approval}" is not "none" or "role:<id>"`,
        });
    }
    const kind = typeof raw.kind === "string" ? raw.kind : null;
    if (kind !== null) {
        // Extensions land in C6; until an installed experience claims the
        // kind, the surface renders on the floor. That is degradation
        // working, not an error.
        diagnostics.push({
            severity: "info",
            message: `no installed experience renders kind "${kind}"; this surface renders on the floor`,
        });
    }

    const bindings: SurfaceBinding[] = [];
    const rawBind = Array.isArray(raw.bind) ? raw.bind : [];
    if (rawBind.length === 0) {
        diagnostics.push({
            severity: "error",
            message: "the surface binds nothing",
        });
    }
    for (const bind of rawBind) {
        const entry = (
            typeof bind === "object" && bind !== null && !Array.isArray(bind)
                ? bind
                : {}
        ) as JsonObject;
        const target = typeof entry.target === "string" ? entry.target : "";
        const binding: SurfaceBinding = {
            target,
            editableFields: Array.isArray(entry.editable_fields)
                ? entry.editable_fields.filter(
                      (f): f is string => typeof f === "string",
                  )
                : null,
            canAdd: entry.can_add === true,
            canDelete: entry.can_delete === true,
        };
        bindings.push(binding);
        diagnostics.push(...validateBinding(binding, model));
    }

    return {
        id,
        kind,
        title,
        description:
            typeof raw.description === "string" ? raw.description : null,
        audience,
        approval,
        caution: typeof raw.caution === "string" ? raw.caution : null,
        config:
            typeof raw.config === "object" &&
            raw.config !== null &&
            !Array.isArray(raw.config)
                ? (raw.config as JsonObject)
                : null,
        bindings,
        diagnostics,
    };
}

type ParsedTarget =
    | { kind: "variable"; id: string }
    | { kind: "catalog"; id: string }
    | { kind: "entry"; catalog: string; entry: string }
    | null;

// Binding targets use the addressing grammar (design/addressing.md). The
// floor supports the concrete entity forms; collectives and namespace
// subtrees can arrive when a surface needs one.
export function parseTarget(target: string): ParsedTarget {
    const entryMatch = target.match(/^catalog=([a-z0-9_/]+):entry=(.+)$/);
    if (entryMatch !== null) {
        return {
            kind: "entry",
            catalog: entryMatch[1] as string,
            entry: entryMatch[2] as string,
        };
    }
    const catalogMatch = target.match(/^catalog=([a-z0-9_/]+)$/);
    if (catalogMatch !== null) {
        return { kind: "catalog", id: catalogMatch[1] as string };
    }
    const variableMatch = target.match(/^variable=([a-z0-9_/]+)$/);
    if (variableMatch !== null) {
        return { kind: "variable", id: variableMatch[1] as string };
    }
    return null;
}

function validateBinding(
    binding: SurfaceBinding,
    model: ModelView,
): SurfaceDiagnostic[] {
    const parsed = parseTarget(binding.target);
    if (parsed === null) {
        return [
            {
                severity: "error",
                message: `binding target "${binding.target}" is not a variable, catalog, or entry address`,
            },
        ];
    }
    const diagnostics: SurfaceDiagnostic[] = [];
    if (parsed.kind === "variable") {
        const exists = (model.variables ?? []).some(
            (variable) => variable.id === parsed.id,
        );
        if (!exists) {
            diagnostics.push({
                severity: "error",
                message: `binds variable "${parsed.id}" which does not exist`,
            });
        }
        return diagnostics;
    }
    const catalogId = parsed.kind === "catalog" ? parsed.id : parsed.catalog;
    const catalog = (model.catalogs ?? []).find((c) => c.id === catalogId);
    if (catalog === undefined) {
        diagnostics.push({
            severity: "error",
            message: `binds catalog "${catalogId}" which does not exist`,
        });
        return diagnostics;
    }
    if (parsed.kind === "entry") {
        const exists = (model.catalogEntries ?? []).some(
            (entry) =>
                entry.catalog === catalogId && entry.key === parsed.entry,
        );
        if (!exists) {
            diagnostics.push({
                severity: "error",
                message: `binds entry "${parsed.entry}" of catalog "${catalogId}" which does not exist`,
            });
        }
    }
    if (binding.editableFields !== null) {
        const properties = schemaProperties(catalog.json ?? null);
        if (properties !== null) {
            for (const field of binding.editableFields) {
                if (!properties.has(field)) {
                    diagnostics.push({
                        severity: "warning",
                        message: `editable field "${field}" is not declared by catalog "${catalogId}"`,
                    });
                }
            }
        }
    }
    return diagnostics;
}

// The freshness diagnostic: the vendored schema ages inside packages; when
// it no longer matches what this console ships, say so on the surface list
// instead of failing anything.
export function schemaFreshness(model: ModelView): SurfaceDiagnostic | null {
    const vendored = (model.catalogs ?? []).find(
        (catalog) => catalog.id === SURFACES_CATALOG,
    );
    if (vendored === undefined || vendored.json === undefined) {
        return null;
    }
    if (
        canonical(vendored.json) === canonical(SURFACES_SCHEMA as JsonValue)
    ) {
        return null;
    }
    return {
        severity: "info",
        message:
            "this package's surfaces schema differs from the one this console ships; surfaces still render",
    };
}

// Key-order-insensitive equality: the schema round-trips through parsers
// that reorder object keys, and that is not staleness.
function canonical(value: JsonValue): string {
    if (Array.isArray(value)) {
        return `[${value.map(canonical).join(",")}]`;
    }
    if (typeof value === "object" && value !== null) {
        const entries = Object.entries(value)
            .sort(([left], [right]) => left.localeCompare(right))
            .map(
                ([key, child]) =>
                    `${JSON.stringify(key)}:${canonical(child ?? null)}`,
            );
        return `{${entries.join(",")}}`;
    }
    return JSON.stringify(value);
}

export function audienceAllows(surface: Surface, audience: string): boolean {
    return surface.audience.includes(audience);
}

// ---------------------------------------------------------------------------
// The floor: every binding renders with a control inferred from its type

export function surfaceItems(surface: Surface, model: ModelView): SurfaceItem[] {
    const items: SurfaceItem[] = [];
    for (const binding of surface.bindings) {
        const parsed = parseTarget(binding.target);
        if (parsed === null) {
            items.push({ kind: "missing", target: binding.target });
            continue;
        }
        if (parsed.kind === "variable") {
            const variable = (model.variables ?? []).find(
                (v) => v.id === parsed.id,
            );
            if (variable === undefined) {
                items.push({ kind: "missing", target: binding.target });
                continue;
            }
            items.push({
                kind: "variable",
                id: variable.id,
                variableType: variable.declaration.value ?? null,
                description: variable.description ?? null,
                control: variableControl(
                    variable.declaration.kind,
                    variable.declaration.value ?? null,
                    model,
                ),
                default: variable.resolve?.default?.value ?? null,
                ruleCount: variable.resolve?.rules?.length ?? 0,
            });
            continue;
        }
        const catalogId =
            parsed.kind === "catalog" ? parsed.id : parsed.catalog;
        const catalog = (model.catalogs ?? []).find(
            (c) => c.id === catalogId,
        );
        if (catalog === undefined) {
            items.push({ kind: "missing", target: binding.target });
            continue;
        }
        const fields = fieldControls(
            catalog.json ?? null,
            binding.editableFields,
            model,
        );
        if (parsed.kind === "entry") {
            const entry = (model.catalogEntries ?? []).find(
                (e) => e.catalog === catalogId && e.key === parsed.entry,
            );
            if (entry === undefined) {
                items.push({ kind: "missing", target: binding.target });
                continue;
            }
            items.push({
                kind: "entry",
                catalog: catalogId,
                key: entry.key,
                value: entry.value,
                editableFields: binding.editableFields,
                fields,
            });
            continue;
        }
        items.push({
            kind: "catalog",
            id: catalogId,
            description: catalog.description ?? null,
            schema: catalog.json ?? null,
            entries: (model.catalogEntries ?? [])
                .filter((entry) => entry.catalog === catalogId)
                .map((entry) => ({ key: entry.key, value: entry.value })),
            editableFields: binding.editableFields,
            canAdd: binding.canAdd,
            canDelete: binding.canDelete,
            fields,
        });
    }
    return items;
}

function variableControl(
    declarationKind: string,
    declared: string | null,
    model: ModelView,
): Control {
    if (declarationKind === "catalog" && declared !== null) {
        return {
            control: "select",
            options: (model.catalogEntries ?? [])
                .filter((entry) => entry.catalog === declared)
                .map((entry) => entry.key),
        };
    }
    if (declarationKind !== "primitive" || declared === null) {
        return { control: "json" };
    }
    const enumId = declared.startsWith("enum=") ? declared.slice(5) : null;
    if (enumId !== null) {
        return { control: "select", options: enumMembers(enumId, model) };
    }
    switch (declared) {
        case "bool":
            return { control: "toggle" };
        case "int":
        case "number":
            return { control: "number" };
        case "string":
            return { control: "text" };
        default:
            return { control: "json" };
    }
}

// Schema-driven cell widgets for a catalog binding: one control per
// editable field (or every declared field when the binding does not narrow
// them). Fields carrying `x-rototo-ref` render as reference pickers.
function fieldControls(
    schema: JsonValue | null,
    editableFields: string[] | null,
    model: ModelView,
): FieldControl[] {
    const properties = schemaPropertyMap(schema);
    if (properties === null) {
        return (editableFields ?? []).map((field) => ({
            field,
            control: "json",
        }));
    }
    const names = editableFields ?? [...properties.keys()];
    return names.map((field) => {
        const property = properties.get(field);
        if (property === undefined) {
            return { field, control: "json" };
        }
        return { field, ...propertyControl(property, model) };
    });
}

function propertyControl(property: JsonObject, model: ModelView): Control {
    const ref = property["x-rototo-ref"];
    const refTargets =
        typeof ref === "string"
            ? [ref]
            : Array.isArray(ref)
              ? ref.filter((r): r is string => typeof r === "string")
              : [];
    if (refTargets.length > 0) {
        // A pinned field renders as a reference picker: enum members or the
        // target catalogs' entry ids as options. (`x-rototo-ref: true`, the
        // dynamic form, stays a json control: its target is data.)
        const options: JsonValue[] = [];
        for (const target of refTargets) {
            if (target.startsWith("enum=")) {
                options.push(...enumMembers(target.slice(5), model));
                continue;
            }
            const parsed = parseTarget(target);
            if (parsed !== null && parsed.kind === "catalog") {
                options.push(
                    ...(model.catalogEntries ?? [])
                        .filter((entry) => entry.catalog === parsed.id)
                        .map((entry) => entry.key),
                );
            }
        }
        return { control: "select", options };
    }
    if (Array.isArray(property.enum)) {
        return { control: "select", options: property.enum };
    }
    switch (property.type) {
        case "boolean":
            return { control: "toggle" };
        case "integer":
        case "number":
            return { control: "number" };
        case "string":
            return { control: "text" };
        default:
            return { control: "json" };
    }
}

function enumMembers(enumId: string, model: ModelView): JsonValue[] {
    const declared = (model.enums ?? []).find((e) => e.id === enumId);
    return (declared?.members ?? [])
        .map((member) => member.value)
        .filter((value): value is JsonValue => value !== undefined);
}

function schemaProperties(schema: JsonValue | null): Set<string> | null {
    const map = schemaPropertyMap(schema);
    return map === null ? null : new Set(map.keys());
}

function schemaPropertyMap(
    schema: JsonValue | null,
): Map<string, JsonObject> | null {
    if (
        typeof schema !== "object" ||
        schema === null ||
        Array.isArray(schema)
    ) {
        return null;
    }
    const properties = (schema as JsonObject).properties;
    if (
        typeof properties !== "object" ||
        properties === null ||
        Array.isArray(properties)
    ) {
        return null;
    }
    const map = new Map<string, JsonObject>();
    for (const [name, property] of Object.entries(properties)) {
        if (
            typeof property === "object" &&
            property !== null &&
            !Array.isArray(property)
        ) {
            map.set(name, property);
        }
    }
    return map;
}

// ---------------------------------------------------------------------------
// History scoping: the package-relative paths a surface's bindings cover

export function bindingPaths(surface: Surface, model: ModelView): string[] {
    const paths = new Set<string>();
    for (const binding of surface.bindings) {
        const parsed = parseTarget(binding.target);
        if (parsed === null) {
            continue;
        }
        if (parsed.kind === "variable") {
            const variable = (model.variables ?? []).find(
                (v) => v.id === parsed.id,
            );
            if (variable !== undefined) {
                paths.add(variable.location.path);
            }
            continue;
        }
        if (parsed.kind === "entry") {
            const entry = (model.catalogEntries ?? []).find(
                (e) => e.catalog === parsed.catalog && e.key === parsed.entry,
            );
            if (entry !== undefined) {
                paths.add(entry.location.path);
            }
            continue;
        }
        const catalog = (model.catalogs ?? []).find(
            (c) => c.id === parsed.id,
        );
        if (catalog !== undefined) {
            paths.add(catalog.path);
            paths.add(`data/catalogs/${parsed.id}`);
        }
    }
    return [...paths].sort();
}

// How a set of changed files lands on a package's surfaces: which surfaces
// are touched, which approval requirements that implies, and whether any
// file escapes every surface (keeping the deployment default in force).
// Both the review panel and the approval policy read this one walk.
export type SurfaceCoverage = {
    touched: {
        id: string;
        title: string;
        approval: string | null;
        caution: string | null;
    }[];
    roles: { role: string; surfaces: string[] }[];
    autoApproved: string[];
    uncovered: boolean;
};

export function surfaceCoverage(
    model: ModelView,
    files: string[],
): SurfaceCoverage {
    const touched: SurfaceCoverage["touched"] = [];
    const roleMap = new Map<string, Set<string>>();
    const autoApproved: string[] = [];
    const covered = new Set<string>();
    let undeclared = false;
    for (const surface of readSurfaces(model)) {
        const paths = bindingPaths(surface, model);
        const touches = files.filter((file) =>
            paths.some(
                (bindingPath) =>
                    file === bindingPath ||
                    file.startsWith(`${bindingPath}/`),
            ),
        );
        if (touches.length === 0) {
            continue;
        }
        for (const file of touches) {
            covered.add(file);
        }
        touched.push({
            id: surface.id,
            title: surface.title,
            approval: surface.approval,
            caution: surface.caution,
        });
        if (surface.approval === "none") {
            autoApproved.push(surface.id);
        } else if (
            surface.approval !== null &&
            surface.approval.startsWith("role:")
        ) {
            const role = surface.approval.slice(5);
            const set = roleMap.get(role) ?? new Set<string>();
            set.add(surface.id);
            roleMap.set(role, set);
        } else {
            undeclared = true;
        }
    }
    return {
        touched,
        roles: [...roleMap.entries()].map(([role, surfaces]) => ({
            role,
            surfaces: [...surfaces].sort(),
        })),
        autoApproved,
        uncovered: undeclared || files.some((file) => !covered.has(file)),
    };
}

// The variable ids a surface binds, directly or through catalog-typed
// variables is deliberately NOT computed here: upcoming changes filter on
// directly bound variables only, which is what the surface names.
export function boundVariables(surface: Surface): Set<string> {
    const ids = new Set<string>();
    for (const binding of surface.bindings) {
        const parsed = parseTarget(binding.target);
        if (parsed !== null && parsed.kind === "variable") {
            ids.add(parsed.id);
        }
    }
    return ids;
}

// ---------------------------------------------------------------------------
// Cold start: every empty state proposes its next step as a change set

export function suggestSurfaces(model: ModelView): SurfaceSuggestion[] {
    const suggestions: SurfaceSuggestion[] = [];
    const hasSchema = (model.catalogs ?? []).some(
        (catalog) => catalog.id === SURFACES_CATALOG,
    );
    // The first surface's change set carries the schema in; nobody copies
    // files by hand.
    const vendor: JsonValue[] = hasSchema
        ? []
        : [
              {
                  op: "create_catalog",
                  id: SURFACES_CATALOG,
                  schema: SURFACES_SCHEMA,
              },
          ];

    const catalogs = (model.catalogs ?? []).filter(
        (catalog) =>
            catalog.id !== SURFACES_CATALOG &&
            !catalog.id.startsWith("console/"),
    );
    for (const catalog of catalogs) {
        const key = catalog.id.replaceAll("/", "_");
        suggestions.push({
            id: key,
            kind: "table",
            title: titleize(catalog.id),
            reason: `catalog "${catalog.id}" suggests a table surface`,
            operations: [
                ...vendor,
                {
                    op: "create_entry",
                    catalog: SURFACES_CATALOG,
                    key,
                    fields: {
                        kind: "table",
                        title: titleize(catalog.id),
                        bind: [{ target: `catalog=${catalog.id}` }],
                    },
                },
            ],
        });
    }

    const flags = (model.variables ?? []).filter(
        (variable) =>
            variable.declaration.kind === "primitive" &&
            variable.declaration.value === "bool",
    );
    if (flags.length > 0) {
        suggestions.push({
            id: "flags",
            kind: "flags",
            title: "Flags",
            reason: `${flags.length} bool variable${flags.length === 1 ? "" : "s"} suggest a flags surface`,
            operations: [
                ...vendor,
                {
                    op: "create_entry",
                    catalog: SURFACES_CATALOG,
                    key: "flags",
                    fields: {
                        kind: "flags",
                        title: "Flags",
                        bind: flags.map((variable) => ({
                            target: `variable=${variable.id}`,
                        })),
                    },
                },
            ],
        });
    }
    return suggestions;
}

function titleize(id: string): string {
    const tail = id.split("/").pop() ?? id;
    const words = tail.split("_").filter((word) => word !== "");
    if (words.length === 0) {
        return id;
    }
    const first = words[0] as string;
    words[0] = first.charAt(0).toUpperCase() + first.slice(1);
    return words.join(" ");
}
