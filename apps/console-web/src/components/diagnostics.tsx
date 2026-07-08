// Lint surfacing, two pieces working together: a status pill that is
// visible from every package screen (clean / N warnings / N errors, always
// a link here), and the diagnostics view itself — every finding lint
// reports at this pin, grouped by what it is about, searchable, each card
// carrying the rule, the location, the fix hint, and a way to jump to the
// thing being complained about.

import type { LintDiagnostic } from "@/lib/api";
import { SearchableList } from "@/lib/ui-kit";
import type { AddressStep } from "@/lib/router";

export function LintStatusPill({
    diagnostics,
    href,
}: {
    diagnostics: LintDiagnostic[];
    href: string;
}) {
    const errors = count(diagnostics, "error");
    const warnings = diagnostics.length - errors;
    const [tone, text] =
        errors > 0
            ? ["pill-err", `${errors} error${errors === 1 ? "" : "s"}`]
            : warnings > 0
              ? ["pill-warn", `${warnings} warning${warnings === 1 ? "" : "s"}`]
              : ["pill-ok", "lint clean"];
    return (
        <a className="pill-link" href={href} title="Open diagnostics">
            <span className={`pill ${tone}`}>
                <span className="d" />
                {text}
            </span>
        </a>
    );
}

export function DiagnosticsPanel({
    diagnostics,
    hrefEntity,
    hrefFile,
}: {
    diagnostics: LintDiagnostic[];
    hrefEntity: (steps: AddressStep[]) => string;
    hrefFile: (path: string) => string;
}) {
    if (diagnostics.length === 0) {
        return (
            <div className="empty-state">
                <span className="empty-puck">✓</span>
                <p>Lint is clean at this pin. Nothing to fix here.</p>
            </div>
        );
    }
    const groups = new Map<string, LintDiagnostic[]>();
    for (const diagnostic of diagnostics) {
        const key = targetLabel(diagnostic);
        groups.set(key, [...(groups.get(key) ?? []), diagnostic]);
    }
    return (
        <SearchableList
            label="Search diagnostics"
            placeholder="Search diagnostics"
            emptyLabel="No diagnostic matches that search."
            className="diagnostic-groups"
        >
            {[...groups.entries()].map(([target, items]) => (
                <section
                    className="diagnostic-group"
                    key={target}
                    data-search={`${target} ${items
                        .map(
                            (item) =>
                                `${item.rule ?? ""} ${item.message} ${item.severity}`,
                        )
                        .join(" ")}`}
                >
                    <div className="diagnostic-group-head">
                        <span className="tag">{target}</span>
                        <span className="label">
                            {items.length} diagnostic
                            {items.length === 1 ? "" : "s"}
                        </span>
                    </div>
                    {items.map((diagnostic, index) => (
                        <DiagnosticCard
                            key={`${diagnostic.rule}-${index}`}
                            diagnostic={diagnostic}
                            hrefEntity={hrefEntity}
                            hrefFile={hrefFile}
                        />
                    ))}
                </section>
            ))}
        </SearchableList>
    );
}

function DiagnosticCard({
    diagnostic,
    hrefEntity,
    hrefFile,
}: {
    diagnostic: LintDiagnostic;
    hrefEntity: (steps: AddressStep[]) => string;
    hrefFile: (path: string) => string;
}) {
    const steps = targetSteps(diagnostic);
    const path = diagnostic.location?.path;
    const line = diagnostic.location?.range?.start?.line;
    return (
        <article
            className={`diagnostic ${
                diagnostic.severity === "error" ||
                diagnostic.severity === "warning"
                    ? diagnostic.severity
                    : ""
            }`}
        >
            <div className="diagnostic-title">
                <h3>{diagnostic.message}</h3>
                <span className="action-row">
                    {steps !== null ? (
                        <a
                            className="btn btn-secondary btn-sm"
                            href={hrefEntity(steps)}
                        >
                            Open entity
                        </a>
                    ) : path !== undefined ? (
                        <a
                            className="btn btn-secondary btn-sm"
                            href={hrefFile(path)}
                        >
                            Open file
                        </a>
                    ) : null}
                    <span
                        className={`pill ${
                            diagnostic.severity === "error"
                                ? "pill-err"
                                : diagnostic.severity === "warning"
                                  ? "pill-warn"
                                  : "pill-info"
                        }`}
                    >
                        {diagnostic.severity}
                    </span>
                </span>
            </div>
            <div className="kv">
                {diagnostic.rule !== undefined ? (
                    <span>
                        rule <span className="mono">{diagnostic.rule}</span>
                    </span>
                ) : null}
                {diagnostic.stage !== undefined ? (
                    <span>
                        stage <span className="mono">{diagnostic.stage}</span>
                    </span>
                ) : null}
                {path !== undefined ? (
                    <span>
                        at{" "}
                        <span className="mono">
                            {path}
                            {line !== undefined ? `:${line + 1}` : ""}
                        </span>
                    </span>
                ) : null}
            </div>
            {diagnostic.help !== undefined ? (
                <p className="diagnostic-help">
                    <span className="label">how to fix</span> {diagnostic.help}
                </p>
            ) : null}
        </article>
    );
}

// What the diagnostic is about, as people name it: the entity when the
// target names one, otherwise the file.
function targetLabel(diagnostic: LintDiagnostic): string {
    const entity = diagnostic.target?.entity;
    if (entity?.kind !== undefined) {
        const id = (field: string): string => String(entity[field] ?? "?");
        switch (entity.kind) {
            case "variable":
                return `variable ${id("id")}`;
            case "list":
                return `list ${id("id")}`;
            case "catalog":
                return `catalog ${id("id")}`;
            case "catalog_entry":
                return `catalog ${id("catalog")} / ${id("key")}`;
            case "evaluation_context":
                return `context ${id("id")}`;
            case "evaluation_context_sample":
                return `sample ${id("evaluation_context")} / ${id("key")}`;
            case "value":
                return `variable ${id("variable")} value ${id("key")}`;
            case "rule":
                return `variable ${id("variable")} rule ${id("index")}`;
            case "layer":
                return `layer ${id("id")}`;
            case "custom_lint":
                return `custom lint ${id("path")}`;
            case "manifest":
                return "package manifest";
            case "governance":
                return "governance";
            case "package":
                return "package";
        }
    }
    return diagnostic.location?.path ?? "package";
}

// The address the console can open for this diagnostic, when its entity is
// addressable. SemanticEntity kinds are snake_case (src/diagnostics.rs).
function targetSteps(diagnostic: LintDiagnostic): AddressStep[] | null {
    const entity = diagnostic.target?.entity;
    if (entity?.kind === undefined) {
        return null;
    }
    const text = (field: string): string | null =>
        typeof entity[field] === "string" ? (entity[field] as string) : null;
    switch (entity.kind) {
        case "variable": {
            const id = text("id");
            return id === null ? null : [{ class: "variable", id }];
        }
        case "value":
        case "rule": {
            const id = text("variable");
            return id === null ? null : [{ class: "variable", id }];
        }
        case "list": {
            const id = text("id");
            return id === null ? null : [{ class: "list", id }];
        }
        case "catalog": {
            const id = text("id");
            return id === null ? null : [{ class: "catalog", id }];
        }
        case "catalog_entry": {
            const catalog = text("catalog");
            const key = text("key");
            return catalog === null || key === null
                ? null
                : [
                      { class: "catalog", id: catalog },
                      { class: "entry", id: key },
                  ];
        }
        case "evaluation_context": {
            const id = text("id");
            return id === null ? null : [{ class: "evaluation-context", id }];
        }
        case "evaluation_context_sample": {
            const context = text("evaluation_context");
            const key = text("key");
            return context === null || key === null
                ? null
                : [
                      { class: "evaluation-context", id: context },
                      { class: "sample", id: key },
                  ];
        }
        default:
            return null;
    }
}

function count(diagnostics: LintDiagnostic[], severity: string): number {
    return diagnostics.filter((diagnostic) => diagnostic.severity === severity)
        .length;
}
