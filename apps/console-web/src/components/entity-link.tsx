// Turning model references into places you can go. The semantic model and
// the review speak in entity refs ({kind, id...}); the router speaks in
// address steps. This module owns that mapping plus the small renderer that
// makes identifiers inside CEL expressions clickable, so every mention of
// an entity anywhere in the console can offer the same link.

import type { ReactNode } from "react";

import type { ModelEntityRef } from "@/lib/api";
import type { AddressStep } from "@/lib/router";

// The address a model entity ref names, or null for refs that have no page
// of their own (context attributes, allocations). Two spellings arrive
// here: the semantic model's camelCase refs, and the review/diagnostics
// SemanticTarget kinds, which are snake_case (src/diagnostics.rs).
export function entitySteps(ref: ModelEntityRef): AddressStep[] | null {
    const id = typeof ref.id === "string" ? ref.id : null;
    switch (ref.kind) {
        case "variable":
            return id === null ? null : [{ class: "variable", id }];
        case "value":
        case "rule": {
            const variable = ref["variable"];
            return typeof variable === "string"
                ? [{ class: "variable", id: variable }]
                : null;
        }
        case "catalog":
            return id === null ? null : [{ class: "catalog", id }];
        case "catalogEntry":
        case "catalog_entry": {
            const catalog = ref["catalog"];
            const key = ref["key"];
            return typeof catalog === "string" && typeof key === "string"
                ? [
                      { class: "catalog", id: catalog },
                      { class: "entry", id: key },
                  ]
                : null;
        }
        case "list":
            return id === null ? null : [{ class: "list", id }];
        case "evaluationContext":
        case "evaluation_context":
            return id === null ? null : [{ class: "evaluation-context", id }];
        case "sample":
        case "evaluation_context_sample": {
            const context =
                ref["evaluationContext"] ??
                ref["evaluation_context"] ??
                ref["context"];
            const key = ref["key"];
            return typeof context === "string" && typeof key === "string"
                ? [
                      { class: "evaluation-context", id: context },
                      { class: "sample", id: key },
                  ]
                : null;
        }
        case "layer":
            return id === null ? null : [{ class: "layer", id }];
        default:
            return null;
    }
}

export function entityLabel(ref: ModelEntityRef): string {
    if (ref.kind === "catalogEntry" || ref.kind === "catalog_entry") {
        return `${String(ref["catalog"])}/${String(ref["key"])}`;
    }
    if (ref.kind === "contextAttribute") {
        return `context.${String(ref["name"])}`;
    }
    if (ref.kind === "sample" || ref.kind === "evaluation_context_sample") {
        return `${String(ref["evaluationContext"] ?? ref["evaluation_context"] ?? ref["context"])}/${String(ref["key"])}`;
    }
    if (ref.kind === "value" || ref.kind === "rule") {
        const detail = ref.kind === "value" ? ref["key"] : ref["index"];
        return `${String(ref["variable"])} ${ref.kind} ${String(detail)}`;
    }
    return typeof ref.id === "string" ? ref.id : ref.kind;
}

// The identifiers a `when` or `query` expression can name: other variables
// and lists. Namespaced ids ride in brackets, plain ids in dot access.
const EXPRESSION_ENTITY =
    /\b(variables|lists)(?:\.([a-z0-9_]+)|\["([a-z0-9_/]+)"\])/g;

// A CEL expression with its entity references rendered as links. Read-only
// surfaces use this; inputs stay plain text.
export function ExpressionText({
    text,
    hrefFor,
}: {
    text: string;
    hrefFor: (steps: AddressStep[]) => string;
}) {
    const parts: ReactNode[] = [];
    let cursor = 0;
    for (const match of text.matchAll(EXPRESSION_ENTITY)) {
        const index = match.index;
        const [whole, root, dotId, bracketId] = match;
        const id = dotId ?? bracketId;
        if (id === undefined) {
            continue;
        }
        if (index > cursor) {
            parts.push(text.slice(cursor, index));
        }
        const className = root === "variables" ? "variable" : "list";
        parts.push(
            <a
                key={index}
                className="expr-link"
                href={hrefFor([{ class: className, id }])}
                title={`open ${className} ${id}`}
            >
                {whole}
            </a>,
        );
        cursor = index + whole.length;
    }
    if (parts.length === 0) {
        return <>{text}</>;
    }
    if (cursor < text.length) {
        parts.push(text.slice(cursor));
    }
    return <>{parts}</>;
}
