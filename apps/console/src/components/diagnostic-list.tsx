import { CheckCircle2, SquareArrowOutUpRight } from "lucide-react";
import { Link } from "@/lib/link";
import { SearchableList } from "@/components/searchable-list";
import { Pill } from "@/components/status-pills";
import type { LintDiagnostic } from "@/lib/types";

export function DiagnosticList({
  diagnosticHref,
  diagnostics,
}: {
  diagnosticHref?: (diagnostic: LintDiagnostic) => string | null;
  diagnostics: LintDiagnostic[];
}) {
  if (diagnostics.length === 0) {
    return (
      <div className="empty-state">
        <span className="empty-puck">
          <CheckCircle2 aria-hidden size={18} />
        </span>
        <p>Lint is clean. Nothing to fix here.</p>
      </div>
    );
  }

  const groups = groupDiagnostics(diagnostics);
  return (
    <SearchableList
      className="diagnostic-groups"
      emptyLabel="No diagnostics match that search."
      label="Search diagnostics"
      placeholder="Search diagnostics"
    >
      {Array.from(groups.entries()).map(([target, items]) => (
        <section
          className="diagnostic-group"
          data-search={`${target} ${items.map(diagnosticSearchText).join(" ")}`}
          key={target}
        >
          <div className="diagnostic-group-head">
            <span className="tag">{target}</span>
            <span className="label">
              {items.length} {items.length === 1 ? "diagnostic" : "diagnostics"}
            </span>
          </div>
          {items.map((diagnostic, index) => (
            <DiagnosticCard
              diagnostic={diagnostic}
              href={diagnosticHref?.(diagnostic) ?? null}
              key={`${diagnosticRule(diagnostic)}-${index}`}
            />
          ))}
        </section>
      ))}
    </SearchableList>
  );
}

export function DiagnosticCard({
  diagnostic,
  href,
}: {
  diagnostic: LintDiagnostic;
  href?: string | null;
}) {
  const target = diagnosticEntityName(diagnostic);
  return (
    <article className={`diagnostic ${diagnostic.severity ?? ""}`}>
      <div className="diagnostic-title">
        <h3>{diagnostic.message ?? "Diagnostic"}</h3>
        <span className="action-row">
          {href ? (
            <Link className="btn btn-secondary btn-sm" href={href}>
              <SquareArrowOutUpRight aria-hidden size={13} />
              {target ? `Open ${target.kind} / ${target.name}` : "Open entity"}
            </Link>
          ) : null}
          <SeverityPill severity={diagnostic.severity} />
        </span>
      </div>
      <div className="kv">
        <span>
          rule <span className="mono">{diagnosticRule(diagnostic)}</span>
        </span>
        <span>
          stage <span className="mono">{diagnostic.stage ?? "unknown"}</span>
        </span>
        <span>
          at <span className="mono">{diagnosticLocation(diagnostic)}</span>
        </span>
        <span>
          field <span className="mono">{semanticFieldLabel(diagnostic)}</span>
        </span>
      </div>
      {diagnostic.help ? (
        <p className="diagnostic-help">
          <span className="label" style={{ marginRight: 8 }}>
            how to fix
          </span>
          {diagnostic.help}
        </p>
      ) : null}
    </article>
  );
}

function SeverityPill({ severity }: { severity: string | undefined }) {
  if (severity === "error") {
    return <Pill label="error" tone="err" />;
  }
  if (severity === "warning") {
    return <Pill label="warning" tone="warn" />;
  }
  return <Pill label={severity ?? "info"} tone="info" />;
}

function groupDiagnostics(diagnostics: LintDiagnostic[]): Map<string, LintDiagnostic[]> {
  const groups = new Map<string, LintDiagnostic[]>();
  for (const diagnostic of diagnostics) {
    const key = diagnosticTargetLabel(diagnostic);
    const items = groups.get(key) ?? [];
    items.push(diagnostic);
    groups.set(key, items);
  }
  return groups;
}

export function DiagnosticSummary({ diagnostics }: { diagnostics: LintDiagnostic[] }) {
  const errors = diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  const warnings = diagnostics.filter((diagnostic) => diagnostic.severity === "warning").length;
  if (errors > 0) {
    return <Pill label={`${errors} ${errors === 1 ? "error" : "errors"}`} tone="err" />;
  }
  if (warnings > 0) {
    return (
      <Pill label={`${warnings} ${warnings === 1 ? "warning" : "warnings"}`} tone="warn" />
    );
  }
  return <Pill label="lint clean" tone="ok" />;
}

function diagnosticEntityName(
  diagnostic: LintDiagnostic,
): { kind: string; name: string } | null {
  const entity = diagnostic.target?.entity;
  if (!entity || typeof entity.kind !== "string") {
    return null;
  }
  const text = (value: unknown): string | null => (typeof value === "string" ? value : null);
  switch (entity.kind) {
    case "variable":
      return text(entity.id) ? { kind: "variable", name: entity.id as string } : null;
    case "value":
    case "rule":
      return text(entity.variable)
        ? { kind: "variable", name: entity.variable as string }
        : null;
    case "qualifier":
      return text(entity.id) ? { kind: "qualifier", name: entity.id as string } : null;
    case "predicate":
      return text(entity.qualifier)
        ? { kind: "qualifier", name: entity.qualifier as string }
        : null;
    case "resource":
      return text(entity.id) ? { kind: "resource", name: entity.id as string } : null;
    case "resource_object":
      return text(entity.resource)
        ? {
            kind: "resource object",
            name: text(entity.key)
              ? `${entity.resource as string}/${entity.key as string}`
              : (entity.resource as string),
          }
        : null;
    case "schema":
      return text(entity.path)
        ? { kind: "schema", name: (entity.path as string).split("/").pop() ?? (entity.path as string) }
        : null;
    case "custom_lint":
      return text(entity.path)
        ? { kind: "linter", name: (entity.path as string).split("/").pop() ?? (entity.path as string) }
        : null;
    default:
      return null;
  }
}

function diagnosticRule(diagnostic: LintDiagnostic): string {
  if (typeof diagnostic.rule === "string") {
    return diagnostic.rule;
  }
  return diagnostic.rule?.id ?? "unknown";
}

function diagnosticLocation(diagnostic: LintDiagnostic): string {
  const path = diagnostic.location?.path ?? "unknown";
  const start = diagnostic.location?.range?.start;
  if (start?.line === undefined) {
    return path;
  }
  const character = start.character ?? start.column;
  const position =
    character === undefined ? `${start.line + 1}` : `${start.line + 1}:${character + 1}`;
  return `${path}:${position}`;
}

function diagnosticTargetLabel(diagnostic: LintDiagnostic): string {
  const entity = diagnostic.target?.entity;
  if (!entity || typeof entity.kind !== "string") {
    return "workspace";
  }
  const kind = entity.kind;
  if (kind === "variable" && typeof entity.id === "string") {
    return `variable:${entity.id}`;
  }
  if (kind === "value" && typeof entity.variable === "string" && typeof entity.key === "string") {
    return `variable:${entity.variable}.value:${entity.key}`;
  }
  if (kind === "rule" && typeof entity.variable === "string") {
    return `variable:${entity.variable}.rule`;
  }
  if (kind === "qualifier" && typeof entity.id === "string") {
    return `qualifier:${entity.id}`;
  }
  if (kind === "predicate" && typeof entity.qualifier === "string") {
    return `qualifier:${entity.qualifier}.predicate`;
  }
  if (kind === "resource" && typeof entity.id === "string") {
    return `resource:${entity.id}`;
  }
  if (kind === "resource_object" && typeof entity.resource === "string") {
    return `resource:${entity.resource}.object`;
  }
  if (kind === "schema" && typeof entity.path === "string") {
    return `schema:${entity.path}`;
  }
  if (kind === "custom_lint" && typeof entity.path === "string") {
    return `lint:${entity.path}`;
  }
  return kind;
}

function semanticFieldLabel(diagnostic: LintDiagnostic): string {
  const field = diagnostic.target?.field;
  if (!field || typeof field.kind !== "string") {
    return "entity";
  }
  if (Array.isArray(field.path)) {
    return `${field.kind}:${field.path.join(".")}`;
  }
  return field.kind;
}

function diagnosticSearchText(diagnostic: LintDiagnostic): string {
  return [
    diagnostic.message,
    diagnostic.help,
    diagnostic.severity,
    diagnostic.stage,
    diagnosticRule(diagnostic),
    diagnosticLocation(diagnostic),
    semanticFieldLabel(diagnostic),
  ]
    .filter(Boolean)
    .join(" ");
}
