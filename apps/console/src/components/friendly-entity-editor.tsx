import {
    FormEvent,
    ReactNode,
    useCallback,
    useEffect,
    useMemo,
    useRef,
    useState,
} from "react";
import {
    CircleAlert,
    GitCompareArrows,
    GripVertical,
    ListPlus,
    Plus,
    Save,
    X,
} from "lucide-react";
import { useRouter } from "@/lib/navigation";
import type { LintDiagnostic } from "@/lib/types";
import widgetSpec from "../../../../spec/ui-widgets.json";
import { shouldAutoFocus } from "./autofocus";
import {
    CodeEditor,
    type CodeEditorLsp,
    type CodeEditorMark,
} from "./code-editor";
import { apiFetch } from "@/lib/api";

/* The pre-registered widget vocabulary. spec/ui-widgets.json is the single
   source of truth, shared with the Rust lint rules that validate hints. */
const WIDGET_SPEC = widgetSpec as {
    version: number;
    widgets: Record<
        string,
        {
            types: string[];
            params: Record<string, string>;
            requires_bounds?: boolean;
            requires_enum?: boolean;
        }
    >;
};

/** Workspace section edited by the friendly editor. */
type EntitySection =
    | "variables"
    | "qualifiers"
    | "catalogs"
    | "schemas"
    | "context"
    | "linters";

/** Editable source file plus metadata owned by one editor instance. */
type FriendlyEntity = {
    id: string;
    kind: string;
    section: EntitySection;
    path: string;
    text: string;
    language: "json" | "lua" | "toml" | "text";
};

/** Active editor tab for a friendly editor instance. */
type EditorTab = "form" | "source";

/** Variable declaration mode understood by the friendly variable editor. */
type VariableDeclarationKind = "primitive" | "catalog" | "schema";

/**
 * Optional context harvested from sibling entities and related schemas.
 *
 * The parent screen rebuilds this from the current workspace/draft payload for
 * each editor instance. Descriptions explain fields, examples show values
 * already in use, and preview truth tables come from the Rust runtime.
 */
export type FormGuidance = {
    contextAttributeDocs?: Record<string, string>;
    attributeValueExamples?: Record<string, string[]>;
    schemaDocs?: Record<string, string>;
    catalogDocs?: Record<string, string>;
    propertyExamples?: Record<string, string[]>;
    qualifierIds?: string[];
    catalogEntryKeys?: Record<string, string[]>;
    contextPreviews?: EditContextPreview[];
};

/**
 * Saved request context with each workspace qualifier already evaluated.
 *
 * The editor walks edited rules against these runtime truths so previews update
 * live without reimplementing qualifier semantics in React.
 */
export type EditContextPreview = {
    name: string;
    qualifierTruth: Record<string, boolean>;
};

const PREDICATE_OPERATORS = [
    "eq",
    "neq",
    "in",
    "not_in",
    "gt",
    "gte",
    "lt",
    "lte",
    "bucket",
];

const OPERATOR_HINTS: Record<string, string> = {
    eq: "Matches when the attribute equals this value exactly.",
    neq: "Matches when the attribute differs from this value.",
    in: 'Matches when the attribute is in this list, e.g. ["premium", "enterprise"].',
    not_in: 'Matches when the attribute is not in this list, e.g. ["trial"].',
    gt: "Numeric: matches when the attribute is greater than this value.",
    gte: "Numeric: matches when the attribute is at least this value.",
    lt: "Numeric: matches when the attribute is less than this value.",
    lte: "Numeric: matches when the attribute is at most this value.",
    bucket: "Bucketed rollout — edit the percentage and salt in source mode.",
};

const PRIMITIVE_TYPES = ["bool", "int", "number", "string", "list"];

const DECLARATION_HINTS: Record<VariableDeclarationKind, string> = {
    primitive: "Values below must match this primitive type.",
    catalog: "Values select entries of this catalog by key.",
    schema: "Values must validate against this JSON Schema.",
};

export function FriendlyEntityEditor({
    baseText = null,
    contextAttributes = [],
    diagnostics = [],
    disabled,
    draftId,
    entity,
    guidance = {},
    catalogIds = [],
    catalogSchema = null,
    schemaPaths = [],
    sourceMarks = [],
    workspaceId,
}: {
    /* The entity's text at the draft's base ref, when known. Enables the
     changes view in both form and source modes. */
    baseText?: string | null;
    contextAttributes?: string[];
    diagnostics?: LintDiagnostic[];
    disabled?: boolean;
    draftId: string;
    entity: FriendlyEntity;
    guidance?: FormGuidance;
    catalogIds?: string[];
    catalogSchema?: string | null;
    schemaPaths?: string[];
    sourceMarks?: CodeEditorMark[];
    workspaceId: string;
}) {
    const router = useRouter();
    const [content, setContent] = useState(entity.text);
    const [pending, setPending] = useState(false);
    const [note, setNote] = useState<{
        tone: "ok" | "err";
        text: string;
    } | null>(null);
    const form = editorForm({
        content,
        contextAttributes,
        diagnostics,
        disabled: disabled || pending,
        entity,
        guidance,
        onChange: setContent,
        catalogIds,
        catalogSchema,
        schemaPaths,
    });
    const [activeTab, setActiveTab] = useState<EditorTab>(
        form ? "form" : "source",
    );
    const [showChanges, setShowChanges] = useState(false);
    const hasDelta = baseText !== null && baseText !== content;
    const skeleton = useMemo(
        () => skeletonContent({ content, entity, catalogSchema }),
        [content, entity, catalogSchema],
    );

    const formRef = useRef<HTMLFormElement>(null);

    // Source mode talks to the draft's rototo language server for as-you-type
    // diagnostics, completion, and hover.
    const lspRequest = useCallback(
        async (body: Record<string, unknown>) => {
            const response = await apiFetch(
                `/api/workspaces/${workspaceId}/drafts/${draftId}/lsp`,
                {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ ...body, path: entity.path }),
                },
            );
            const payload = (await response.json()) as { error?: string };
            if (!response.ok) {
                throw new Error(
                    payload.error ?? "language server request failed",
                );
            }
            return payload;
        },
        [workspaceId, draftId, entity.path],
    );
    const lsp = useMemo<CodeEditorLsp | undefined>(
        () => (disabled ? undefined : { request: lspRequest }),
        [disabled, lspRequest],
    );

    // Land focus on the first editable field when an entity opens.
    useEffect(() => {
        if (!shouldAutoFocus()) {
            return;
        }
        formRef.current
            ?.querySelector<HTMLElement>(
                '[role="tabpanel"] input:not([disabled]), [role="tabpanel"] select:not([disabled]), [role="tabpanel"] textarea:not([disabled])',
            )
            ?.focus({ preventScroll: true });
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [entity.path]);

    // Reset editor state only when a different entity (or fresh server text)
    // arrives — not on every re-render, which would wipe in-progress edits.
    useEffect(() => {
        setContent(entity.text);
        setNote(null);
        setActiveTab(
            editorForm({
                content: entity.text,
                contextAttributes,
                disabled,
                entity,
                onChange: setContent,
                catalogIds,
                catalogSchema,
                schemaPaths,
            })
                ? "form"
                : "source",
        );
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [entity.path, entity.text]);

    async function submit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault();
        setPending(true);
        setNote(null);
        try {
            const response = await apiFetch(
                `/api/workspaces/${workspaceId}/drafts/${draftId}/files`,
                {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ filePath: entity.path, content }),
                },
            );
            const body = (await response.json()) as { error?: string };
            if (!response.ok) {
                throw new Error(body.error ?? "failed to save file");
            }
            setNote({ tone: "ok", text: "Saved to the draft branch." });
            router.refresh();
        } catch (error) {
            setNote({
                tone: "err",
                text: error instanceof Error ? error.message : String(error),
            });
        } finally {
            setPending(false);
        }
    }

    const hasForm = form !== null;

    return (
        <form className="card" onSubmit={submit} ref={formRef}>
            <div className="card-head">
                <div className="card-head-text">
                    <h3>Definition</h3>
                    <p className="hint">
                        Form and source edit the same file:{" "}
                        <span className="mono">{entity.path}</span>
                    </p>
                </div>
                <div className="action-row">
                    {hasDelta ? (
                        <button
                            className="btn btn-sm btn-changes"
                            data-on={showChanges}
                            onClick={() => setShowChanges((value) => !value)}
                            type="button"
                        >
                            <GitCompareArrows aria-hidden size={14} />
                            {showChanges ? "Hide changes" : "Show changes"}
                        </button>
                    ) : null}
                    <div
                        className="segmented-control"
                        role="tablist"
                        aria-label="Editor mode"
                    >
                        {hasForm ? (
                            <button
                                aria-selected={activeTab === "form"}
                                className={activeTab === "form" ? "active" : ""}
                                onClick={() => setActiveTab("form")}
                                role="tab"
                                type="button"
                            >
                                Form
                            </button>
                        ) : null}
                        <button
                            aria-selected={activeTab === "source"}
                            className={activeTab === "source" ? "active" : ""}
                            onClick={() => setActiveTab("source")}
                            role="tab"
                            type="button"
                        >
                            Source
                        </button>
                    </div>
                </div>
            </div>
            {activeTab === "source" && skeleton !== null ? (
                <div className="banner banner-info">
                    <ListPlus aria-hidden size={16} />
                    <span style={{ flex: 1 }}>
                        Not sure what goes here? Insert the fields this file can
                        declare as comments — uncomment and set real values.
                    </span>
                    <button
                        className="btn btn-secondary btn-sm"
                        disabled={disabled || pending}
                        onClick={() => setContent(skeleton)}
                        type="button"
                    >
                        Help me fill
                    </button>
                </div>
            ) : null}
            {activeTab === "form" && form ? (
                <div role="tabpanel">
                    {showChanges && hasDelta ? (
                        <div className="delta-panel">
                            <span className="label">
                                changes on this branch
                            </span>
                            <CodeEditor
                                diffBase={baseText}
                                disabled
                                language={entity.language}
                                onChange={() => {}}
                                value={content}
                            />
                        </div>
                    ) : null}
                    {form}
                </div>
            ) : (
                <div role="tabpanel">
                    <CodeEditor
                        diffBase={
                            showChanges && hasDelta ? baseText : undefined
                        }
                        disabled={disabled || pending}
                        language={entity.language}
                        lsp={lsp}
                        marks={sourceMarks}
                        onChange={setContent}
                        value={content}
                    />
                </div>
            )}
            <div className="action-row">
                <button
                    className="btn btn-primary"
                    disabled={disabled || pending}
                    type="submit"
                >
                    {pending ? (
                        <span className="spin" />
                    ) : (
                        <Save aria-hidden size={15} />
                    )}
                    {pending ? "Saving" : "Save to draft"}
                </button>
                {note ? (
                    <p className="form-note" data-tone={note.tone}>
                        {note.text}
                    </p>
                ) : null}
            </div>
        </form>
    );
}

function editorForm({
    content,
    contextAttributes,
    diagnostics = [],
    disabled,
    entity,
    guidance = {},
    onChange,
    catalogIds = [],
    catalogSchema,
    schemaPaths = [],
}: {
    content: string;
    contextAttributes: string[];
    diagnostics?: LintDiagnostic[];
    disabled?: boolean;
    entity: FriendlyEntity;
    guidance?: FormGuidance;
    onChange: (content: string) => void;
    catalogIds?: string[];
    catalogSchema?: string | null;
    schemaPaths?: string[];
}): ReactNode | null {
    if (entity.language !== "toml") {
        return null;
    }

    if (entity.section === "variables") {
        return (
            <VariableFields
                content={content}
                diagnostics={diagnostics}
                disabled={disabled}
                guidance={guidance}
                onChange={onChange}
                catalogIds={catalogIds}
                schemaPaths={schemaPaths}
            />
        );
    }
    if (entity.section === "qualifiers") {
        return (
            <QualifierFields
                content={content}
                contextAttributes={contextAttributes}
                diagnostics={diagnostics}
                disabled={disabled}
                entityId={entity.id}
                guidance={guidance}
                onChange={onChange}
            />
        );
    }
    if (entity.kind === "catalog") {
        return (
            <CatalogFields
                content={content}
                diagnostics={diagnostics}
                disabled={disabled}
                guidance={guidance}
                onChange={onChange}
                schemaPaths={schemaPaths}
            />
        );
    }
    if (entity.kind === "catalog entry") {
        const entryNotes = diagnostics.filter(
            (diagnostic) => targetEntityKind(diagnostic) === "catalog_entry",
        );
        const schema = parseObjectSchema(catalogSchema);
        if (schema) {
            return (
                <SchemaObjectFields
                    content={content}
                    disabled={disabled}
                    examples={guidance.propertyExamples ?? {}}
                    notes={entryNotes}
                    onChange={onChange}
                    schema={schema}
                />
            );
        }
        return (
            <>
                <FieldNotes items={entryNotes} />
                <TopLevelTomlFields
                    content={content}
                    disabled={disabled}
                    onChange={onChange}
                />
            </>
        );
    }

    return null;
}

const SKELETON_MARKER =
    "# help-me-fill: uncomment the fields you need and set real values";

/* Append the fields this file can declare — as comments, so lint keeps
   reporting what is genuinely missing until real values go in. TOML only;
   existing content is never touched. Returns null when nothing is missing. */
function skeletonContent(input: {
    content: string;
    entity: FriendlyEntity;
    catalogSchema?: string | null;
}): string | null {
    const { content, entity, catalogSchema } = input;
    if (entity.language !== "toml" || content.includes(SKELETON_MARKER)) {
        return null;
    }

    const top = topLevelLines(content);
    const has = (key: string) => top.some((field) => field.key === key);
    const hasSection = (header: string) =>
        content.split(/\r?\n/).some((line) => line.trim() === header);
    const missing: string[] = [];

    if (entity.section === "variables") {
        if (!has("schema_version")) {
            missing.push("schema_version = 1");
        }
        if (!has("description")) {
            missing.push('description = ""');
        }
        if (!has("type") && !has("schema")) {
            missing.push('type = "string"');
        }
        const fields = variableFields(content);
        if (
            fields.declarationKind !== "catalog" &&
            sectionLines(content, "[values]").length === 0
        ) {
            missing.push("[values]", 'default = ""');
        }
        if (!tomlSectionRawField(content, "[resolve]", "default")) {
            if (!hasSection("[resolve]")) {
                missing.push("[resolve]");
            }
            const firstValue =
                sectionLines(content, "[values]")[0]?.key ?? "default";
            missing.push(`default = ${JSON.stringify(firstValue)}`);
        }
    } else if (entity.section === "qualifiers") {
        if (!has("schema_version")) {
            missing.push("schema_version = 1");
        }
        if (!has("description")) {
            missing.push('description = ""');
        }
        if (!hasSection("[[predicate]]")) {
            missing.push(
                "[[predicate]]",
                'attribute = "user.tier"',
                'op = "eq"',
                'value = "premium"',
            );
        }
    } else if (entity.kind === "catalog") {
        if (!has("schema_version")) {
            missing.push("schema_version = 1");
        }
        if (!has("description")) {
            missing.push('description = ""');
        }
        if (!has("schema")) {
            missing.push(`schema = "../schemas/${entity.id}.schema.json"`);
        }
    } else if (entity.kind === "catalog entry") {
        const schema = parseObjectSchema(catalogSchema);
        if (!schema) {
            return null;
        }
        const present = new Set(top.map((field) => field.key));
        for (const property of schema.properties) {
            if (!present.has(property.key)) {
                missing.push(
                    `${property.key} = ${placeholderLiteral(property)}`,
                );
            }
        }
    } else {
        return null;
    }

    if (missing.length === 0) {
        return null;
    }
    const block = [SKELETON_MARKER, ...missing.map((line) => `# ${line}`)].join(
        "\n",
    );
    const body = content.trimEnd();
    return `${body === "" ? "" : `${body}\n\n`}${block}\n`;
}

function placeholderLiteral(property: ObjectSchemaProperty): string {
    if (property.enumValues && property.enumValues.length > 0) {
        return JSON.stringify(property.enumValues[0]);
    }
    switch (property.type) {
        case "integer":
        case "number":
            return String(property.minimum ?? 0);
        case "boolean":
            return "false";
        case "array":
            return "[]";
        case "object":
            return "{}";
        default:
            return '""';
    }
}

/* Inline lint findings attached to the form field they target. */
function FieldNotes({ items }: { items: LintDiagnostic[] }) {
    if (items.length === 0) {
        return null;
    }
    return (
        <>
            {items.map((diagnostic, index) => (
                <span
                    className="field-note"
                    data-tone={diagnostic.severity === "error" ? "err" : "warn"}
                    key={index}
                >
                    <CircleAlert aria-hidden size={13} />
                    <span>
                        {diagnostic.message ?? "Lint flagged this field."}
                    </span>
                </span>
            ))}
        </>
    );
}

function targetEntityKind(diagnostic: LintDiagnostic): string | null {
    const entity = diagnostic.target?.entity;
    return entity && typeof entity.kind === "string" ? entity.kind : null;
}

function targetEntityValue(diagnostic: LintDiagnostic, key: string): unknown {
    return diagnostic.target?.entity?.[key];
}

function targetFieldKind(diagnostic: LintDiagnostic): string | null {
    const field = diagnostic.target?.field;
    return field && typeof field.kind === "string" ? field.kind : null;
}

function VariableFields({
    content,
    diagnostics = [],
    disabled,
    guidance = {},
    onChange,
    catalogIds,
    schemaPaths,
}: {
    content: string;
    diagnostics?: LintDiagnostic[];
    disabled?: boolean;
    guidance?: FormGuidance;
    onChange: (content: string) => void;
    catalogIds: string[];
    schemaPaths: string[];
}) {
    const fields = useMemo(() => variableFields(content), [content]);
    const model = useMemo(() => variableModel(content), [content]);
    const defaultOptions =
        fields.declarationKind === "catalog"
            ? (guidance.catalogEntryKeys?.[fields.declarationValue] ?? [])
            : model.values.map((value) => value.key);
    const declarationDoc =
        fields.declarationKind === "schema"
            ? guidance.schemaDocs?.[
                  fields.declarationValue.split("/").pop() ?? ""
              ]
            : fields.declarationKind === "catalog"
              ? guidance.catalogDocs?.[fields.declarationValue]
              : undefined;
    const declarationNotes = diagnostics.filter((diagnostic) => {
        const kind = targetFieldKind(diagnostic);
        return (
            kind === "variable_type" ||
            kind === "variable_schema" ||
            kind === "variable_declaration"
        );
    });
    const defaultNotes = diagnostics.filter(
        (diagnostic) =>
            targetEntityKind(diagnostic) === "value" &&
            fields.defaultKey !== null &&
            targetEntityValue(diagnostic, "key") === fields.defaultKey,
    );

    function updateDeclaration(kind: VariableDeclarationKind, value: string) {
        let text = removeTopLevelField(content, "type");
        text = removeTopLevelField(text, "schema");
        if (kind === "schema") {
            onChange(setTopLevelStringField(text, "schema", value));
        } else if (kind === "catalog") {
            onChange(setTopLevelStringField(text, "type", `catalog:${value}`));
        } else {
            onChange(setTopLevelStringField(text, "type", value));
        }
    }

    function switchDeclarationKind(kind: VariableDeclarationKind) {
        if (kind === fields.declarationKind) {
            return;
        }
        if (kind === "primitive") {
            updateDeclaration(kind, "string");
        } else if (kind === "catalog") {
            updateDeclaration(kind, catalogIds[0] ?? "");
        } else {
            updateDeclaration(kind, schemaPaths[0] ?? "");
        }
    }

    let declarationControl: ReactNode;
    if (fields.declarationKind === "primitive") {
        declarationControl = (
            <select
                className="input mono"
                disabled={disabled}
                onChange={(event) =>
                    updateDeclaration("primitive", event.target.value)
                }
                value={
                    PRIMITIVE_TYPES.includes(fields.declarationValue)
                        ? fields.declarationValue
                        : "string"
                }
            >
                {PRIMITIVE_TYPES.map((type) => (
                    <option key={type} value={type}>
                        {type}
                    </option>
                ))}
            </select>
        );
    } else {
        const listId =
            fields.declarationKind === "catalog"
                ? "variable-catalog-ids"
                : "variable-schema-paths";
        const suggestions =
            fields.declarationKind === "catalog" ? catalogIds : schemaPaths;
        declarationControl = (
            <>
                <input
                    className="input mono"
                    disabled={disabled}
                    list={listId}
                    onChange={(event) =>
                        updateDeclaration(
                            fields.declarationKind,
                            event.target.value,
                        )
                    }
                    value={fields.declarationValue}
                />
                <datalist id={listId}>
                    {suggestions.map((suggestion) => (
                        <option key={suggestion} value={suggestion} />
                    ))}
                </datalist>
            </>
        );
    }

    return (
        <div className="form-fields">
            <label className="field-stack">
                <span className="label">description</span>
                <input
                    className="input"
                    disabled={disabled}
                    onChange={(event) =>
                        onChange(
                            setTopLevelStringField(
                                content,
                                "description",
                                event.target.value,
                            ),
                        )
                    }
                    placeholder="What this variable controls"
                    value={fields.description}
                />
            </label>
            <div className="field-grid">
                <label className="field-stack">
                    <span className="label">declaration</span>
                    <select
                        className="input"
                        disabled={disabled}
                        onChange={(event) =>
                            switchDeclarationKind(
                                event.target.value as VariableDeclarationKind,
                            )
                        }
                        value={fields.declarationKind}
                    >
                        <option value="primitive">primitive type</option>
                        <option value="catalog">catalog type</option>
                        <option value="schema">schema path</option>
                    </select>
                </label>
                <label className="field-stack">
                    <span className="label">
                        {declarationLabel(fields.declarationKind)}
                    </span>
                    {declarationControl}
                    <FieldNotes items={declarationNotes} />
                </label>
            </div>
            <span className="field-hint">
                {DECLARATION_HINTS[fields.declarationKind]}
                {declarationDoc ? ` — ${declarationDoc}` : ""}
            </span>
            {fields.declarationKind !== "catalog" ? (
                <VariableValuesEditor
                    content={content}
                    diagnostics={diagnostics}
                    disabled={disabled}
                    model={model}
                    onChange={onChange}
                />
            ) : null}
            <label className="field-stack">
                <span className="label">default value</span>
                <select
                    className="input mono"
                    disabled={disabled}
                    onChange={(event) =>
                        onChange(
                            setTomlSectionField(
                                ensureSection(content, "[resolve]"),
                                "[resolve]",
                                "default",
                                JSON.stringify(event.target.value),
                            ),
                        )
                    }
                    value={model.defaultKey ?? ""}
                >
                    {model.defaultKey === null ? (
                        <option value="">not declared</option>
                    ) : null}
                    {defaultOptions.map((option) => (
                        <option key={option} value={option}>
                            {option}
                        </option>
                    ))}
                    {model.defaultKey !== null &&
                    !defaultOptions.includes(model.defaultKey) ? (
                        <option value={model.defaultKey}>
                            {model.defaultKey} (unknown)
                        </option>
                    ) : null}
                </select>
                <FieldNotes items={defaultNotes} />
                <span className="field-hint">
                    {fields.declarationKind === "catalog"
                        ? "Selects which entry of the catalog applies when no rule matches."
                        : "Applies when no rule matches."}
                </span>
            </label>
            <VariableRulesEditor
                diagnostics={diagnostics}
                disabled={disabled}
                model={model}
                onChange={(rules) =>
                    onChange(rewriteResolveRules(content, rules))
                }
                qualifierIds={guidance.qualifierIds ?? []}
                valueOptions={defaultOptions}
            />
            {guidance.contextPreviews && guidance.contextPreviews.length > 0 ? (
                <VariableResolutionPreview
                    model={model}
                    previews={guidance.contextPreviews}
                />
            ) : null}
        </div>
    );
}

/* Shows how the variable as currently edited resolves against each request
   context saved in the workspace. */
function VariableResolutionPreview({
    model,
    previews,
}: {
    model: VariableModel;
    previews: EditContextPreview[];
}) {
    return (
        <div className="field-stack">
            <span className="label">
                how it would resolve — with saved contexts
            </span>
            <div className="spec">
                {previews.map((preview) => (
                    <div className="spec-row" key={preview.name}>
                        <span className="mono">{preview.name}</span>
                        <span className="mono">
                            {previewOutcome(model, preview)}
                        </span>
                    </div>
                ))}
            </div>
            <span className="field-hint">
                Qualifiers are evaluated by rototo on the draft branch; the
                pathway follows your edits before you save.
            </span>
        </div>
    );
}

function previewOutcome(
    model: VariableModel,
    preview: EditContextPreview,
): string {
    for (let index = 0; index < model.rules.length; index += 1) {
        const rule = model.rules[index];
        if (!rule.qualifier) {
            continue;
        }
        const matched = preview.qualifierTruth[rule.qualifier];
        if (matched === undefined) {
            return `rule[${index}] ${rule.qualifier} — qualifier not evaluable yet`;
        }
        if (matched) {
            return `rule[${index}] ${rule.qualifier} → ${rule.value || "unset"}`;
        }
    }
    return model.defaultKey !== null
        ? `default → ${model.defaultKey}`
        : "no default declared";
}

/* Stacked value rows: key + a control matching the declared type. */
function VariableValuesEditor({
    content,
    diagnostics,
    disabled,
    model,
    onChange,
}: {
    content: string;
    diagnostics: LintDiagnostic[];
    disabled?: boolean;
    model: VariableModel;
    onChange: (content: string) => void;
}) {
    const rewrite = (entries: Array<{ key: string; literal: string }>) =>
        onChange(rewriteValuesSection(content, entries));

    function addValue() {
        let key = "new-value";
        let suffix = 2;
        while (model.values.some((value) => value.key === key)) {
            key = `new-value-${suffix}`;
            suffix += 1;
        }
        rewrite([
            ...model.values,
            {
                key,
                literal: emptyLiteralFor(
                    model.declarationValue,
                    model.declarationKind,
                ),
            },
        ]);
    }

    return (
        <div className="field-stack">
            <span className="label">values</span>
            {model.values.length === 0 ? (
                <span className="field-hint">
                    No values declared yet — add the first one.
                </span>
            ) : null}
            {model.values.map((value, index) => {
                const notes = diagnostics.filter(
                    (diagnostic) =>
                        targetEntityKind(diagnostic) === "value" &&
                        targetEntityValue(diagnostic, "key") === value.key,
                );
                return (
                    <div className="field-stack" key={index}>
                        <div className="value-row">
                            <input
                                aria-label="Value key"
                                className="input mono"
                                disabled={disabled}
                                onChange={(event) => {
                                    const next = model.values.map(
                                        (candidate, at) =>
                                            at === index
                                                ? {
                                                      ...candidate,
                                                      key: event.target.value,
                                                  }
                                                : candidate,
                                    );
                                    rewrite(next);
                                }}
                                value={value.key}
                            />
                            <VariableValueControl
                                declarationKind={model.declarationKind}
                                declarationValue={model.declarationValue}
                                disabled={disabled}
                                literal={value.literal}
                                onUpdate={(literal) => {
                                    const next = model.values.map(
                                        (candidate, at) =>
                                            at === index
                                                ? { ...candidate, literal }
                                                : candidate,
                                    );
                                    rewrite(next);
                                }}
                            />
                            <button
                                aria-label={`Remove value ${value.key}`}
                                className="btn btn-ghost btn-icon btn-remove"
                                disabled={disabled}
                                onClick={() =>
                                    rewrite(
                                        model.values.filter(
                                            (_, at) => at !== index,
                                        ),
                                    )
                                }
                                type="button"
                            >
                                <X aria-hidden size={14} />
                            </button>
                        </div>
                        <FieldNotes items={notes} />
                    </div>
                );
            })}
            <button
                className="btn btn-ghost btn-sm"
                disabled={disabled}
                onClick={addValue}
                style={{ width: "fit-content" }}
                type="button"
            >
                <Plus aria-hidden size={14} />
                Add value
            </button>
        </div>
    );
}

/* The value control morphs with the declaration: bool gets true/false,
   numbers get a number input, lists get a stacked item editor. */
function VariableValueControl({
    declarationKind,
    declarationValue,
    disabled,
    literal,
    onUpdate,
}: {
    declarationKind: VariableDeclarationKind;
    declarationValue: string;
    disabled?: boolean;
    literal: string;
    onUpdate: (literal: string) => void;
}) {
    if (declarationKind === "primitive" && declarationValue === "bool") {
        return (
            <select
                className="input mono"
                disabled={disabled}
                onChange={(event) => onUpdate(event.target.value)}
                value={
                    literal.trim() === "true" || literal.trim() === "false"
                        ? literal.trim()
                        : ""
                }
            >
                {literal.trim() !== "true" && literal.trim() !== "false" ? (
                    <option value="">{literal.trim() || "unset"}</option>
                ) : null}
                <option value="true">true</option>
                <option value="false">false</option>
            </select>
        );
    }
    if (
        declarationKind === "primitive" &&
        (declarationValue === "int" || declarationValue === "number")
    ) {
        return (
            <input
                className="input mono"
                disabled={disabled}
                onChange={(event) =>
                    onUpdate(
                        event.target.value.trim() === ""
                            ? "0"
                            : event.target.value.trim(),
                    )
                }
                step={declarationValue === "int" ? 1 : "any"}
                type="number"
                value={literal.trim()}
            />
        );
    }
    if (declarationKind === "primitive" && declarationValue === "string") {
        return (
            <input
                className="input"
                disabled={disabled}
                onChange={(event) =>
                    onUpdate(JSON.stringify(event.target.value))
                }
                value={inputFromLiteral(literal)}
            />
        );
    }
    if (declarationKind === "primitive" && declarationValue === "list") {
        return (
            <ListLiteralEditor
                disabled={disabled}
                literal={literal}
                onUpdate={onUpdate}
            />
        );
    }
    // schema-backed entries and anything unrecognized: raw TOML literal
    return (
        <input
            className="input mono"
            disabled={disabled}
            onChange={(event) => onUpdate(literalFromInput(event.target.value))}
            value={inputFromLiteral(literal)}
        />
    );
}

/* A list value as stacked items: add, edit, remove. */
function ListLiteralEditor({
    disabled,
    literal,
    onUpdate,
}: {
    disabled?: boolean;
    literal: string;
    onUpdate: (literal: string) => void;
}) {
    let items: unknown[] | null = null;
    try {
        const parsed = JSON.parse(literal) as unknown;
        items = Array.isArray(parsed) ? parsed : null;
    } catch {
        items = null;
    }
    if (items === null) {
        return (
            <input
                className="input mono"
                disabled={disabled}
                onChange={(event) =>
                    onUpdate(literalFromInput(event.target.value))
                }
                value={inputFromLiteral(literal)}
            />
        );
    }
    const write = (next: unknown[]) => onUpdate(JSON.stringify(next));
    return (
        <div className="list-editor">
            {items.map((item, index) => (
                <div className="list-item-row" key={index}>
                    <input
                        className="input mono"
                        disabled={disabled}
                        onChange={(event) => {
                            const next = [...items];
                            next[index] = parseListItem(event.target.value);
                            write(next);
                        }}
                        value={
                            typeof item === "string"
                                ? item
                                : JSON.stringify(item)
                        }
                    />
                    <button
                        aria-label="Remove item"
                        className="btn btn-ghost btn-icon btn-remove"
                        disabled={disabled}
                        onClick={() =>
                            write(items.filter((_, at) => at !== index))
                        }
                        type="button"
                    >
                        <X aria-hidden size={14} />
                    </button>
                </div>
            ))}
            <button
                className="btn btn-ghost btn-sm"
                disabled={disabled}
                onClick={() => write([...items, ""])}
                style={{ width: "fit-content" }}
                type="button"
            >
                <Plus aria-hidden size={14} />
                Add item
            </button>
        </div>
    );
}

function parseListItem(value: string): unknown {
    const trimmed = value.trim();
    if (trimmed === "true") return true;
    if (trimmed === "false") return false;
    if (/^-?\d+(?:\.\d+)?$/.test(trimmed)) return Number(trimmed);
    return value;
}

/* Resolve rules: if <qualifier> → <value>, in evaluation order. */
function VariableRulesEditor({
    diagnostics,
    disabled,
    model,
    onChange,
    qualifierIds,
    valueOptions,
}: {
    diagnostics: LintDiagnostic[];
    disabled?: boolean;
    model: VariableModel;
    onChange: (rules: Array<{ qualifier: string; value: string }>) => void;
    qualifierIds: string[];
    valueOptions: string[];
}) {
    // Drag state: a row is only draggable while its handle is pressed, so text
    // selection inside the inputs keeps working.
    const [armedIndex, setArmedIndex] = useState<number | null>(null);
    const [dragIndex, setDragIndex] = useState<number | null>(null);
    const [dropIndex, setDropIndex] = useState<number | null>(null);
    const reorderable = !disabled && model.rules.length > 1;

    function moveRule(from: number, to: number) {
        if (from === to || to < 0 || to >= model.rules.length) {
            return;
        }
        const next = [...model.rules];
        const [moved] = next.splice(from, 1);
        next.splice(to, 0, moved);
        onChange(next);
    }

    function resetDrag() {
        setArmedIndex(null);
        setDragIndex(null);
        setDropIndex(null);
    }

    return (
        <div className="field-stack">
            <span className="label">
                rules — checked in order, first match wins
            </span>
            {model.rules.length === 0 ? (
                <span className="field-hint">
                    No rules: every resolution gets the default value. Add a
                    rule to vary the value by a qualifier.
                </span>
            ) : null}
            {model.rules.map((rule, index) => {
                const notes = diagnostics.filter(
                    (diagnostic) =>
                        targetEntityKind(diagnostic) === "rule" &&
                        targetEntityValue(diagnostic, "index") === index,
                );
                return (
                    <div
                        className="field-stack rule-block"
                        data-dragging={dragIndex === index || undefined}
                        data-drop-target={
                            (dropIndex === index &&
                                dragIndex !== null &&
                                dragIndex !== index) ||
                            undefined
                        }
                        draggable={reorderable && armedIndex === index}
                        key={index}
                        onDragEnd={resetDrag}
                        onDragOver={(event) => {
                            if (dragIndex === null) {
                                return;
                            }
                            event.preventDefault();
                            event.dataTransfer.dropEffect = "move";
                            setDropIndex(index);
                        }}
                        onDragStart={(event) => {
                            setDragIndex(index);
                            event.dataTransfer.effectAllowed = "move";
                        }}
                        onDrop={(event) => {
                            event.preventDefault();
                            if (dragIndex !== null) {
                                moveRule(dragIndex, index);
                            }
                            resetDrag();
                        }}
                    >
                        <div className="rule-row">
                            {reorderable ? (
                                <button
                                    aria-label={`Reorder rule ${index + 1} — drag, or use arrow keys`}
                                    className="btn btn-ghost btn-icon rule-handle"
                                    onKeyDown={(event) => {
                                        if (event.key === "ArrowUp") {
                                            event.preventDefault();
                                            moveRule(index, index - 1);
                                        } else if (event.key === "ArrowDown") {
                                            event.preventDefault();
                                            moveRule(index, index + 1);
                                        }
                                    }}
                                    onMouseDown={() => setArmedIndex(index)}
                                    onMouseUp={() => setArmedIndex(null)}
                                    type="button"
                                >
                                    <GripVertical aria-hidden size={14} />
                                </button>
                            ) : null}
                            <span className="rule-word">if</span>
                            <input
                                aria-label="Rule qualifier"
                                className="input mono"
                                disabled={disabled}
                                list="variable-rule-qualifiers"
                                onChange={(event) =>
                                    onChange(
                                        model.rules.map((candidate, at) =>
                                            at === index
                                                ? {
                                                      ...candidate,
                                                      qualifier:
                                                          event.target.value,
                                                  }
                                                : candidate,
                                        ),
                                    )
                                }
                                value={rule.qualifier}
                            />
                            <span className="rule-word">→</span>
                            <select
                                aria-label="Rule value"
                                className="input mono"
                                disabled={disabled}
                                onChange={(event) =>
                                    onChange(
                                        model.rules.map((candidate, at) =>
                                            at === index
                                                ? {
                                                      ...candidate,
                                                      value: event.target.value,
                                                  }
                                                : candidate,
                                        ),
                                    )
                                }
                                value={rule.value}
                            >
                                {valueOptions.map((option) => (
                                    <option key={option} value={option}>
                                        {option}
                                    </option>
                                ))}
                                {!valueOptions.includes(rule.value) ? (
                                    <option value={rule.value}>
                                        {rule.value || "unset"}
                                    </option>
                                ) : null}
                            </select>
                            <button
                                aria-label="Remove rule"
                                className="btn btn-ghost btn-icon btn-remove"
                                disabled={disabled}
                                onClick={() =>
                                    onChange(
                                        model.rules.filter(
                                            (_, at) => at !== index,
                                        ),
                                    )
                                }
                                type="button"
                            >
                                <X aria-hidden size={14} />
                            </button>
                        </div>
                        <FieldNotes items={notes} />
                    </div>
                );
            })}
            <datalist id="variable-rule-qualifiers">
                {qualifierIds.map((id) => (
                    <option key={id} value={id} />
                ))}
            </datalist>
            <button
                className="btn btn-ghost btn-sm"
                disabled={disabled}
                onClick={() =>
                    onChange([
                        ...model.rules,
                        {
                            qualifier: qualifierIds[0] ?? "",
                            value: valueOptions[0] ?? "",
                        },
                    ])
                }
                style={{ width: "fit-content" }}
                type="button"
            >
                <Plus aria-hidden size={14} />
                Add rule
            </button>
        </div>
    );
}

function QualifierFields({
    content,
    contextAttributes,
    diagnostics = [],
    disabled,
    entityId,
    guidance = {},
    onChange,
}: {
    content: string;
    contextAttributes: string[];
    diagnostics?: LintDiagnostic[];
    disabled?: boolean;
    entityId: string;
    guidance?: FormGuidance;
    onChange: (content: string) => void;
}) {
    const fields = useMemo(() => qualifierFields(content), [content]);
    const datalistId = `context-attributes-${entityId}`;
    const attributeDoc = guidance.contextAttributeDocs?.[fields.attribute];
    const valueExamples = (
        guidance.attributeValueExamples?.[fields.attribute] ?? []
    ).map(inputFromLiteral);
    // The form edits the first [[predicate]] block; only attach its findings.
    const firstPredicate = diagnostics.filter(
        (diagnostic) =>
            targetEntityKind(diagnostic) === "predicate" &&
            (targetEntityValue(diagnostic, "index") === 0 ||
                targetEntityValue(diagnostic, "index") === undefined),
    );
    const noteFor = (kinds: string[]) =>
        firstPredicate.filter((diagnostic) =>
            kinds.includes(targetFieldKind(diagnostic) ?? ""),
        );
    const attributeNotes = noteFor(["predicate_attribute"]);
    const opNotes = noteFor(["predicate_op"]);
    const valueNotes = noteFor(["predicate_value", "predicate_range"]);

    return (
        <div className="form-fields">
            <label className="field-stack">
                <span className="label">description</span>
                <input
                    className="input"
                    disabled={disabled}
                    onChange={(event) =>
                        onChange(
                            setTopLevelStringField(
                                content,
                                "description",
                                event.target.value,
                            ),
                        )
                    }
                    value={fields.description}
                />
            </label>
            <div className="field-grid three">
                <label className="field-stack">
                    <span className="label">attribute</span>
                    <input
                        className="input mono"
                        disabled={disabled}
                        list={datalistId}
                        onChange={(event) =>
                            onChange(
                                setPredicateStringField(
                                    content,
                                    "attribute",
                                    event.target.value,
                                ),
                            )
                        }
                        value={fields.attribute}
                    />
                    <datalist id={datalistId}>
                        {contextAttributes.map((attribute) => (
                            <option key={attribute} value={attribute} />
                        ))}
                    </datalist>
                    <FieldNotes items={attributeNotes} />
                    {attributeDoc ? (
                        <span className="field-hint">{attributeDoc}</span>
                    ) : null}
                </label>
                <label className="field-stack">
                    <span className="label">operator</span>
                    <select
                        className="input mono"
                        disabled={disabled}
                        onChange={(event) =>
                            onChange(
                                setPredicateStringField(
                                    content,
                                    "op",
                                    event.target.value,
                                ),
                            )
                        }
                        value={fields.op}
                    >
                        {PREDICATE_OPERATORS.map((operator) => (
                            <option key={operator} value={operator}>
                                {operator}
                            </option>
                        ))}
                    </select>
                    <FieldNotes items={opNotes} />
                </label>
                <label className="field-stack">
                    <span className="label">value</span>
                    <input
                        className="input mono"
                        disabled={disabled || fields.op === "bucket"}
                        onChange={(event) =>
                            onChange(
                                setPredicateValueField(
                                    content,
                                    event.target.value,
                                ),
                            )
                        }
                        placeholder={
                            fields.op === "in" || fields.op === "not_in"
                                ? '["a", "b"]'
                                : undefined
                        }
                        value={fields.value}
                    />
                    <FieldNotes items={valueNotes} />
                    {valueExamples.length > 0 ? (
                        <span className="field-hint">
                            Used elsewhere with {fields.attribute}:{" "}
                            {valueExamples.slice(0, 3).join(" · ")}
                        </span>
                    ) : null}
                </label>
            </div>
            <span className="field-hint">
                {OPERATOR_HINTS[fields.op] ??
                    "Pick an operator to compare the context attribute against the value."}
            </span>
        </div>
    );
}

function CatalogFields({
    content,
    diagnostics = [],
    disabled,
    guidance = {},
    onChange,
    schemaPaths,
}: {
    content: string;
    diagnostics?: LintDiagnostic[];
    disabled?: boolean;
    guidance?: FormGuidance;
    onChange: (content: string) => void;
    schemaPaths: string[];
}) {
    const fields = useMemo(() => catalogFields(content), [content]);
    const schemaNotes = diagnostics.filter(
        (diagnostic) => targetFieldKind(diagnostic) === "catalog_schema",
    );
    const schemaDoc =
        guidance.schemaDocs?.[fields.schema.split("/").pop() ?? ""];

    return (
        <div className="form-fields">
            <label className="field-stack">
                <span className="label">description</span>
                <input
                    className="input"
                    disabled={disabled}
                    onChange={(event) =>
                        onChange(
                            setTopLevelStringField(
                                content,
                                "description",
                                event.target.value,
                            ),
                        )
                    }
                    placeholder="What these entries represent"
                    value={fields.description}
                />
            </label>
            <label className="field-stack">
                <span className="label">schema</span>
                <input
                    className="input mono"
                    disabled={disabled}
                    list="catalog-schema-paths"
                    onChange={(event) =>
                        onChange(
                            setTopLevelStringField(
                                content,
                                "schema",
                                event.target.value,
                            ),
                        )
                    }
                    value={fields.schema}
                />
                <datalist id="catalog-schema-paths">
                    {schemaPaths.map((path) => (
                        <option key={path} value={`../${path}`} />
                    ))}
                </datalist>
                <FieldNotes items={schemaNotes} />
                <span className="field-hint">
                    Every entry of this catalog must validate against this
                    schema. The path is relative to this file.
                    {schemaDoc ? ` — ${schemaDoc}` : ""}
                </span>
            </label>
        </div>
    );
}

/** JSON Schema property model used to render catalog-entry value controls. */
type ObjectSchemaProperty = {
    key: string;
    required: boolean;
    type: string | null;
    description: string | null;
    enumValues: string[] | null;
    itemsEnum: string[] | null;
    format: string | null;
    minimum: number | null;
    maximum: number | null;
    ui: { widget: string; params: Record<string, unknown> } | null;
};

/** Parsed object schema view model owned by one editor render. */
type ObjectSchema = {
    properties: ObjectSchemaProperty[];
};

function parseObjectSchema(
    text: string | null | undefined,
): ObjectSchema | null {
    if (!text) {
        return null;
    }
    try {
        const parsed = JSON.parse(text) as unknown;
        if (!isRecordValue(parsed) || !isRecordValue(parsed.properties)) {
            return null;
        }
        const required = Array.isArray(parsed.required)
            ? parsed.required.filter(
                  (value): value is string => typeof value === "string",
              )
            : [];
        const properties = Object.entries(parsed.properties).map(
            ([key, definition]) => {
                const record = isRecordValue(definition) ? definition : {};
                const hint = isRecordValue(record["x-rototo-ui"])
                    ? record["x-rototo-ui"]
                    : null;
                const widget =
                    hint && typeof hint.widget === "string"
                        ? hint.widget
                        : null;
                return {
                    key,
                    required: required.includes(key),
                    type: typeof record.type === "string" ? record.type : null,
                    description:
                        typeof record.description === "string"
                            ? record.description
                            : null,
                    enumValues: Array.isArray(record.enum)
                        ? record.enum.filter(
                              (value): value is string =>
                                  typeof value === "string",
                          )
                        : null,
                    itemsEnum:
                        isRecordValue(record.items) &&
                        Array.isArray(record.items.enum)
                            ? record.items.enum.filter(
                                  (value): value is string =>
                                      typeof value === "string",
                              )
                            : null,
                    format:
                        typeof record.format === "string"
                            ? record.format
                            : null,
                    minimum:
                        typeof record.minimum === "number"
                            ? record.minimum
                            : null,
                    maximum:
                        typeof record.maximum === "number"
                            ? record.maximum
                            : null,
                    ui:
                        hint && widget
                            ? {
                                  widget,
                                  params: Object.fromEntries(
                                      Object.entries(hint).filter(
                                          ([paramKey]) => paramKey !== "widget",
                                      ),
                                  ),
                              }
                            : null,
                };
            },
        );
        return properties.length > 0 ? { properties } : null;
    } catch {
        return null;
    }
}

function SchemaObjectFields({
    content,
    disabled,
    examples = {},
    notes = [],
    onChange,
    schema,
}: {
    content: string;
    disabled?: boolean;
    examples?: Record<string, string[]>;
    notes?: LintDiagnostic[];
    onChange: (content: string) => void;
    schema: ObjectSchema;
}) {
    const lines = useMemo(() => topLevelLines(content), [content]);
    const schemaKeys = new Set(
        schema.properties.map((property) => property.key),
    );
    const extras = lines.filter(
        (line) => !schemaKeys.has(line.key) && line.key !== "schema_version",
    );

    function update(key: string, raw: string | null) {
        onChange(
            raw === null
                ? removeTopLevelField(content, key)
                : setTopLevelRawField(content, key, raw),
        );
    }

    return (
        <div className="form-fields">
            <FieldNotes items={notes} />
            {schema.properties.map((property) => (
                <SchemaPropertyField
                    disabled={disabled}
                    examples={examples[property.key] ?? []}
                    key={property.key}
                    onUpdate={(raw) => update(property.key, raw)}
                    property={property}
                    raw={
                        lines.find((line) => line.key === property.key)
                            ?.value ?? null
                    }
                />
            ))}
            {extras.map((field) => (
                <label className="field-stack" key={field.key}>
                    <span className="label">{field.key} · not in schema</span>
                    <input
                        className="input mono"
                        disabled={disabled}
                        onChange={(event) =>
                            update(
                                field.key,
                                event.target.value.trim() === ""
                                    ? null
                                    : literalFromInput(event.target.value),
                            )
                        }
                        value={inputFromLiteral(field.value)}
                    />
                </label>
            ))}
        </div>
    );
}

function SchemaPropertyField({
    disabled,
    examples = [],
    onUpdate,
    property,
    raw,
}: {
    disabled?: boolean;
    examples?: string[];
    onUpdate: (raw: string | null) => void;
    property: ObjectSchemaProperty;
    raw: string | null;
}) {
    const value = raw === null ? "" : inputFromLiteral(raw);
    const displayExamples = examples.map(inputFromLiteral);
    const widget = resolveWidget(property);
    const constrained =
        (property.enumValues && property.enumValues.length > 0) ||
        property.type === "boolean" ||
        (widget !== null && !WIDGETS_WITH_EXAMPLES.has(widget));
    let control: ReactNode =
        widget !== null ? (
            <WidgetControl
                disabled={disabled}
                onUpdate={onUpdate}
                property={property}
                raw={raw}
                value={value}
                widget={widget}
            />
        ) : null;
    if (
        control === null &&
        property.enumValues &&
        property.enumValues.length > 0
    ) {
        control = (
            <select
                className="input mono"
                disabled={disabled}
                onChange={(event) =>
                    onUpdate(
                        event.target.value === ""
                            ? null
                            : JSON.stringify(event.target.value),
                    )
                }
                value={property.enumValues.includes(value) ? value : ""}
            >
                <option value="">unset</option>
                {property.enumValues.map((option) => (
                    <option key={option} value={option}>
                        {option}
                    </option>
                ))}
            </select>
        );
    } else if (control === null && property.type === "boolean") {
        control = (
            <select
                className="input mono"
                disabled={disabled}
                onChange={(event) =>
                    onUpdate(
                        event.target.value === "" ? null : event.target.value,
                    )
                }
                value={raw === "true" || raw === "false" ? raw : ""}
            >
                <option value="">unset</option>
                <option value="true">true</option>
                <option value="false">false</option>
            </select>
        );
    } else if (control === null && property.type === "string") {
        control = (
            <input
                className="input"
                disabled={disabled}
                onChange={(event) =>
                    onUpdate(
                        event.target.value === ""
                            ? null
                            : JSON.stringify(event.target.value),
                    )
                }
                value={value}
            />
        );
    } else if (control === null) {
        control = (
            <input
                className="input mono"
                disabled={disabled}
                onChange={(event) =>
                    onUpdate(
                        event.target.value.trim() === ""
                            ? null
                            : literalFromInput(event.target.value),
                    )
                }
                value={value}
            />
        );
    }

    const hintParts = [
        property.description,
        !constrained && displayExamples.length > 0
            ? `e.g. ${displayExamples.slice(0, 3).join(" · ")}`
            : null,
    ].filter(Boolean);

    return (
        <label className="field-stack">
            <span className="label">
                {property.key}
                {property.type ? ` · ${property.type}` : ""}
                {property.required ? "" : " · optional"}
            </span>
            {control}
            {hintParts.length > 0 ? (
                <span className="field-hint">{hintParts.join(" — ")}</span>
            ) : null}
        </label>
    );
}

/* Widgets where suggesting sibling values still helps. */
const WIDGETS_WITH_EXAMPLES = new Set([
    "textarea",
    "markdown",
    "code",
    "url",
    "email",
    "tags",
]);

/* Standard JSON Schema formats the admin opts into rendering specially. */
const FORMAT_WIDGETS: Record<string, string> = {
    color: "color",
    "date-time": "datetime",
    date: "date",
    email: "email",
    time: "time",
    uri: "url",
};

/* A hint only renders when the spec knows the widget, the property type fits,
   and required enums or bounds are present; anything else falls back to the
   default control and lint flags it. */
function resolveWidget(property: ObjectSchemaProperty): string | null {
    if (property.ui) {
        const spec = WIDGET_SPEC.widgets[property.ui.widget];
        if (
            !spec ||
            (property.type !== null && !spec.types.includes(property.type))
        ) {
            return null;
        }
        if (
            spec.requires_enum &&
            !(property.enumValues?.length || property.itemsEnum?.length)
        ) {
            return null;
        }
        if (
            spec.requires_bounds &&
            sliderBounds(property, property.ui.widget) === null
        ) {
            return null;
        }
        return property.ui.widget;
    }
    if (
        property.type === "string" &&
        property.format &&
        FORMAT_WIDGETS[property.format]
    ) {
        return FORMAT_WIDGETS[property.format];
    }
    return null;
}

function sliderBounds(
    property: ObjectSchemaProperty,
    widget: string,
): { min: number; max: number; step: number } | null {
    const params = property.ui?.params ?? {};
    let min = typeof params.min === "number" ? params.min : property.minimum;
    let max = typeof params.max === "number" ? params.max : property.maximum;
    if (widget === "percent") {
        min ??= 0;
        max ??= 100;
    }
    if (
        min === null ||
        min === undefined ||
        max === null ||
        max === undefined ||
        max <= min
    ) {
        return null;
    }
    const step =
        typeof params.step === "number"
            ? params.step
            : property.type === "integer"
              ? 1
              : (max - min) / 100;
    return { min, max, step };
}

function WidgetControl({
    disabled,
    onUpdate,
    property,
    raw,
    value,
    widget,
}: {
    disabled?: boolean;
    onUpdate: (raw: string | null) => void;
    property: ObjectSchemaProperty;
    raw: string | null;
    value: string;
    widget: string;
}) {
    const params = property.ui?.params ?? {};

    switch (widget) {
        case "slider":
        case "percent": {
            const bounds = sliderBounds(property, widget);
            if (!bounds) {
                return null;
            }
            const numeric = Number(value);
            return (
                <div className="widget-row">
                    <input
                        disabled={disabled}
                        max={bounds.max}
                        min={bounds.min}
                        onChange={(event) =>
                            onUpdate(literalFromInput(event.target.value))
                        }
                        step={bounds.step}
                        type="range"
                        value={
                            Number.isFinite(numeric) && value !== ""
                                ? numeric
                                : bounds.min
                        }
                    />
                    <input
                        className="input mono widget-num"
                        disabled={disabled}
                        onChange={(event) =>
                            onUpdate(
                                event.target.value.trim() === ""
                                    ? null
                                    : literalFromInput(event.target.value),
                            )
                        }
                        value={value}
                    />
                    {widget === "percent" ? (
                        <span className="widget-suffix">%</span>
                    ) : null}
                </div>
            );
        }
        case "number": {
            const bounds = sliderBounds(property, widget);
            return (
                <input
                    className="input mono widget-num"
                    disabled={disabled}
                    max={bounds?.max}
                    min={bounds?.min}
                    onChange={(event) =>
                        onUpdate(
                            event.target.value.trim() === ""
                                ? null
                                : literalFromInput(event.target.value),
                        )
                    }
                    step={bounds?.step}
                    type="number"
                    value={value}
                />
            );
        }
        case "color": {
            const hex = /^#[0-9a-fA-F]{6}$/.test(value) ? value : "#1fa37c";
            return (
                <div className="widget-row">
                    <input
                        className="widget-color"
                        disabled={disabled}
                        onChange={(event) =>
                            onUpdate(JSON.stringify(event.target.value))
                        }
                        type="color"
                        value={hex}
                    />
                    <input
                        className="input mono"
                        disabled={disabled}
                        onChange={(event) =>
                            onUpdate(
                                event.target.value === ""
                                    ? null
                                    : JSON.stringify(event.target.value),
                            )
                        }
                        placeholder="#1fa37c"
                        value={value}
                    />
                </div>
            );
        }
        case "textarea":
        case "markdown":
        case "code": {
            const rows =
                typeof params.rows === "number"
                    ? params.rows
                    : widget === "textarea"
                      ? 4
                      : 6;
            return (
                <textarea
                    className={
                        widget === "textarea"
                            ? "input textarea"
                            : "input textarea mono"
                    }
                    disabled={disabled}
                    onChange={(event) =>
                        onUpdate(
                            event.target.value === ""
                                ? null
                                : JSON.stringify(event.target.value),
                        )
                    }
                    placeholder={
                        widget === "code" && typeof params.language === "string"
                            ? params.language
                            : undefined
                    }
                    rows={rows}
                    value={value}
                />
            );
        }
        case "url":
        case "email":
        case "date":
        case "time":
        case "datetime": {
            const inputType =
                widget === "datetime"
                    ? "datetime-local"
                    : widget === "url"
                      ? "url"
                      : widget;
            return (
                <input
                    className="input"
                    disabled={disabled}
                    onChange={(event) =>
                        onUpdate(
                            event.target.value === ""
                                ? null
                                : JSON.stringify(event.target.value),
                        )
                    }
                    type={inputType}
                    value={value}
                />
            );
        }
        case "radio": {
            const options = property.enumValues ?? [];
            return (
                <div className="choice-group" role="radiogroup">
                    {options.map((option) => (
                        <label className="choice-row" key={option}>
                            <input
                                checked={value === option}
                                disabled={disabled}
                                name={`radio-${property.key}`}
                                onChange={() =>
                                    onUpdate(JSON.stringify(option))
                                }
                                type="radio"
                            />
                            <span className="mono">{option}</span>
                        </label>
                    ))}
                </div>
            );
        }
        case "toggle": {
            const on = raw === "true";
            return (
                <button
                    aria-checked={on}
                    className="switch"
                    data-on={on}
                    disabled={disabled}
                    onClick={() => onUpdate(on ? "false" : "true")}
                    role="switch"
                    type="button"
                >
                    <span className="switch-knob" />
                    <span className="mono switch-text">
                        {on ? "true" : "false"}
                    </span>
                </button>
            );
        }
        case "checkbox": {
            return (
                <label className="choice-row">
                    <input
                        checked={raw === "true"}
                        disabled={disabled}
                        onChange={(event) =>
                            onUpdate(event.target.checked ? "true" : "false")
                        }
                        type="checkbox"
                    />
                    <span className="mono">
                        {raw === "true" ? "true" : "false"}
                    </span>
                </label>
            );
        }
        case "tags": {
            const items = parseStringArray(raw);
            if (items === null) {
                return null;
            }
            return (
                <TagsControl
                    disabled={disabled}
                    items={items}
                    onUpdate={onUpdate}
                />
            );
        }
        case "multiselect": {
            const options = property.itemsEnum ?? [];
            const selected = parseStringArray(raw);
            if (selected === null) {
                return null;
            }
            return (
                <div className="choice-group">
                    {options.map((option) => (
                        <label className="choice-row" key={option}>
                            <input
                                checked={selected.includes(option)}
                                disabled={disabled}
                                onChange={(event) => {
                                    const next = event.target.checked
                                        ? [...selected, option]
                                        : selected.filter(
                                              (item) => item !== option,
                                          );
                                    const ordered = options.filter(
                                        (candidate) => next.includes(candidate),
                                    );
                                    onUpdate(JSON.stringify(ordered));
                                }}
                                type="checkbox"
                            />
                            <span className="mono">{option}</span>
                        </label>
                    ))}
                </div>
            );
        }
        default:
            return null;
    }
}

function TagsControl({
    disabled,
    items,
    onUpdate,
}: {
    disabled?: boolean;
    items: string[];
    onUpdate: (raw: string | null) => void;
}) {
    const [pending, setPending] = useState("");

    function add() {
        const tag = pending.trim();
        setPending("");
        if (tag === "" || items.includes(tag)) {
            return;
        }
        onUpdate(JSON.stringify([...items, tag]));
    }

    return (
        <div className="chips">
            {items.map((item, index) => (
                <span className="chip" key={`${item}-${index}`}>
                    <span className="mono">{item}</span>
                    <button
                        aria-label={`Remove ${item}`}
                        className="chip-remove"
                        disabled={disabled}
                        onClick={() =>
                            onUpdate(
                                JSON.stringify(
                                    items.filter((_, at) => at !== index),
                                ),
                            )
                        }
                        type="button"
                    >
                        ×
                    </button>
                </span>
            ))}
            <input
                className="chip-input"
                disabled={disabled}
                onBlur={add}
                onChange={(event) => setPending(event.target.value)}
                onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === ",") {
                        event.preventDefault();
                        add();
                    }
                }}
                placeholder={
                    items.length === 0 ? "add a value and press enter" : "add"
                }
                value={pending}
            />
        </div>
    );
}

function parseStringArray(raw: string | null): string[] | null {
    if (raw === null) {
        return [];
    }
    try {
        const parsed = JSON.parse(raw) as unknown;
        return Array.isArray(parsed) &&
            parsed.every((item) => typeof item === "string")
            ? parsed
            : null;
    } catch {
        return null;
    }
}

function isRecordValue(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}

function TopLevelTomlFields({
    content,
    disabled,
    onChange,
}: {
    content: string;
    disabled?: boolean;
    onChange: (content: string) => void;
}) {
    const fields = useMemo(() => topLevelLines(content), [content]);

    if (fields.length === 0) {
        return (
            <p className="muted">
                No top-level TOML fields are available for form editing.
            </p>
        );
    }

    return (
        <div className="form-fields">
            {fields.map((field) => (
                <label className="field-stack" key={field.key}>
                    <span className="label">{field.key}</span>
                    <input
                        className="input mono"
                        disabled={disabled}
                        onChange={(event) =>
                            onChange(
                                setTopLevelRawField(
                                    content,
                                    field.key,
                                    literalFromInput(event.target.value),
                                ),
                            )
                        }
                        value={inputFromLiteral(field.value)}
                    />
                </label>
            ))}
        </div>
    );
}

/** Minimal variable TOML model used for form editing and source regeneration. */
type VariableModel = {
    declarationKind: VariableDeclarationKind;
    declarationValue: string;
    values: Array<{ key: string; literal: string }>;
    defaultKey: string | null;
    rules: Array<{ qualifier: string; value: string }>;
};

function variableModel(text: string): VariableModel {
    const base = variableFields(text);
    return {
        declarationKind: base.declarationKind,
        declarationValue: base.declarationValue,
        values: sectionLines(text, "[values]").map((field) => ({
            key: field.key,
            literal: field.value,
        })),
        defaultKey: tomlSectionStringField(text, "[resolve]", "default"),
        rules: resolveRuleBlocks(text),
    };
}

function resolveRuleBlocks(
    text: string,
): Array<{ qualifier: string; value: string }> {
    const lines = text.split(/\r?\n/);
    const rules: Array<{ qualifier: string; value: string }> = [];
    for (let index = 0; index < lines.length; index += 1) {
        if (lines[index].trim() !== "[[resolve.rule]]") {
            continue;
        }
        const end = nextSectionLine(lines, index + 1);
        const blockFields = lines.slice(index + 1, end).flatMap(parseFieldLine);
        rules.push({
            qualifier: parseTomlStringLiteral(
                blockFields.find((field) => field.key === "qualifier")?.value ??
                    '""',
            ),
            value: parseTomlStringLiteral(
                blockFields.find((field) => field.key === "value")?.value ??
                    '""',
            ),
        });
        index = end - 1;
    }
    return rules;
}

function removeSectionBlock(text: string, header: string): string {
    const lines = text.split(/\r?\n/);
    const start = lines.findIndex((line) => line.trim() === header);
    if (start === -1) {
        return text;
    }
    const end = nextSectionLine(lines, start + 1);
    lines.splice(start, end - start);
    return lines.join("\n");
}

function tidyToml(text: string): string {
    return `${text.replace(/\n{3,}/g, "\n\n").replace(/\n+$/, "")}\n`;
}

function rewriteValuesSection(
    text: string,
    entries: Array<{ key: string; literal: string }>,
): string {
    const without = removeSectionBlock(text, "[values]");
    if (entries.length === 0) {
        return tidyToml(without);
    }
    const section = `[values]\n${entries
        .map((entry) => `${entry.key} = ${entry.literal}`)
        .join("\n")}`;
    const lines = without.split(/\r?\n/);
    const resolveAt = lines.findIndex((line) => line.trim() === "[resolve]");
    if (resolveAt === -1) {
        return tidyToml(`${without.trimEnd()}\n\n${section}`);
    }
    lines.splice(resolveAt, 0, ...section.split("\n"), "");
    return tidyToml(lines.join("\n"));
}

function rewriteResolveRules(
    text: string,
    rules: Array<{ qualifier: string; value: string }>,
): string {
    let without = text;
    for (;;) {
        const next = removeSectionBlock(without, "[[resolve.rule]]");
        if (next === without) {
            break;
        }
        without = next;
    }
    if (rules.length === 0) {
        return tidyToml(without);
    }
    without = ensureSection(without, "[resolve]");
    const blocks = rules
        .map(
            (rule) =>
                `[[resolve.rule]]\nqualifier = ${JSON.stringify(rule.qualifier)}\nvalue = ${JSON.stringify(rule.value)}`,
        )
        .join("\n\n");
    return tidyToml(`${without.trimEnd()}\n\n${blocks}`);
}

function emptyLiteralFor(
    declarationValue: string,
    declarationKind: VariableDeclarationKind,
): string {
    if (declarationKind !== "primitive") {
        return "{}";
    }
    switch (declarationValue) {
        case "bool":
            return "false";
        case "int":
        case "number":
            return "0";
        case "list":
            return "[]";
        default:
            return '""';
    }
}

function variableFields(text: string) {
    const type = topLevelStringField(text, "type");
    const schema = topLevelStringField(text, "schema");
    const defaultKey = tomlSectionStringField(text, "[resolve]", "default");
    const defaultRaw = defaultKey
        ? tomlSectionRawField(text, "[values]", defaultKey)
        : null;
    const base = {
        description: topLevelStringField(text, "description"),
        defaultKey,
        defaultValue: defaultRaw ? inputFromLiteral(defaultRaw) : "",
    };
    if (schema) {
        return {
            ...base,
            declarationKind: "schema" as VariableDeclarationKind,
            declarationValue: schema,
        };
    }
    if (type?.startsWith("catalog:")) {
        return {
            ...base,
            declarationKind: "catalog" as VariableDeclarationKind,
            declarationValue: type.slice("catalog:".length),
        };
    }
    return {
        ...base,
        declarationKind: "primitive" as VariableDeclarationKind,
        declarationValue: type || "string",
    };
}

function qualifierFields(text: string) {
    return {
        description: topLevelStringField(text, "description"),
        attribute: predicateStringField(text, "attribute") ?? "user.tier",
        op: predicateStringField(text, "op") ?? "eq",
        value: inputFromLiteral(
            predicateRawField(text, "value") ?? '"premium"',
        ),
    };
}

function catalogFields(text: string) {
    return {
        description: topLevelStringField(text, "description"),
        schema: topLevelStringField(text, "schema"),
    };
}

function declarationLabel(kind: VariableDeclarationKind): string {
    if (kind === "catalog") {
        return "catalog id";
    }
    if (kind === "schema") {
        return "schema path";
    }
    return "primitive type";
}

function topLevelStringField(text: string, key: string): string {
    const raw = topLevelLines(text).find(
        (candidate) => candidate.key === key,
    )?.value;
    return raw ? parseTomlStringLiteral(raw) : "";
}

function setTopLevelStringField(
    text: string,
    key: string,
    value: string,
): string {
    return setTopLevelRawField(text, key, JSON.stringify(value));
}

function setTopLevelRawField(
    text: string,
    key: string,
    rawValue: string,
): string {
    const lines = text.split(/\r?\n/);
    const encoded = `${key} = ${rawValue}`;
    const end = firstSectionLine(lines);
    for (let index = 0; index < end; index += 1) {
        if (fieldKey(lines[index]) === key) {
            lines[index] = encoded;
            return lines.join("\n");
        }
    }
    const insertAt = Math.min(end, schemaVersionInsertLine(lines, end));
    lines.splice(insertAt, 0, encoded);
    return lines.join("\n");
}

function removeTopLevelField(text: string, key: string): string {
    const lines = text.split(/\r?\n/);
    const end = firstSectionLine(lines);
    return lines
        .filter((line, index) => index >= end || fieldKey(line) !== key)
        .join("\n");
}

function tomlSectionStringField(
    text: string,
    section: string,
    key: string,
): string | null {
    const raw = tomlSectionRawField(text, section, key);
    return raw ? parseTomlStringLiteral(raw) : null;
}

function tomlSectionRawField(
    text: string,
    section: string,
    key: string,
): string | null {
    return (
        sectionLines(text, section).find((candidate) => candidate.key === key)
            ?.value ?? null
    );
}

function setTomlSectionField(
    text: string,
    section: string,
    key: string,
    rawValue: string,
): string {
    const lines = ensureSection(text, section).split(/\r?\n/);
    const start = lines.findIndex((line) => line.trim() === section);
    const end = nextSectionLine(lines, start + 1);
    const encoded = `${key} = ${rawValue}`;
    for (let index = start + 1; index < end; index += 1) {
        if (fieldKey(lines[index]) === key) {
            lines[index] = encoded;
            return lines.join("\n");
        }
    }
    lines.splice(end, 0, encoded);
    return lines.join("\n");
}

function predicateStringField(text: string, key: string): string | null {
    const raw = predicateRawField(text, key);
    return raw ? parseTomlStringLiteral(raw) : null;
}

function predicateRawField(text: string, key: string): string | null {
    return (
        predicateBlockLines(text).find((candidate) => candidate.key === key)
            ?.value ?? null
    );
}

function setPredicateStringField(
    text: string,
    key: string,
    value: string,
): string {
    return setPredicateRawField(text, key, JSON.stringify(value));
}

function setPredicateValueField(text: string, value: string): string {
    return setPredicateRawField(text, "value", literalFromInput(value));
}

function setPredicateRawField(
    text: string,
    key: string,
    value: string,
): string {
    const lines = ensurePredicateBlock(text).split(/\r?\n/);
    const start = lines.findIndex((line) => line.trim() === "[[predicate]]");
    const end = nextSectionLine(lines, start + 1);
    const encoded = `${key} = ${value}`;
    for (let index = start + 1; index < end; index += 1) {
        if (fieldKey(lines[index]) === key) {
            lines[index] = encoded;
            return lines.join("\n");
        }
    }
    lines.splice(end, 0, encoded);
    return lines.join("\n");
}

function ensurePredicateBlock(text: string): string {
    if (text.split(/\r?\n/).some((line) => line.trim() === "[[predicate]]")) {
        return text;
    }
    return `${text.trimEnd()}\n\n[[predicate]]\nattribute = "user.tier"\nop = "eq"\nvalue = "premium"\n`;
}

function ensureSection(text: string, section: string): string {
    if (text.split(/\r?\n/).some((line) => line.trim() === section)) {
        return text;
    }
    return `${text.trimEnd()}\n\n${section}\n`;
}

function topLevelLines(text: string) {
    const lines = text.split(/\r?\n/);
    return lines.slice(0, firstSectionLine(lines)).flatMap(parseFieldLine);
}

function sectionLines(text: string, section: string) {
    const lines = text.split(/\r?\n/);
    const start = lines.findIndex((line) => line.trim() === section);
    if (start === -1) {
        return [];
    }
    return lines
        .slice(start + 1, nextSectionLine(lines, start + 1))
        .flatMap(parseFieldLine);
}

function predicateBlockLines(text: string) {
    const lines = text.split(/\r?\n/);
    const start = lines.findIndex((line) => line.trim() === "[[predicate]]");
    if (start === -1) {
        return [];
    }
    return lines
        .slice(start + 1, nextSectionLine(lines, start + 1))
        .flatMap(parseFieldLine);
}

function parseFieldLine(line: string): Array<{ key: string; value: string }> {
    const match = /^(\s*)([A-Za-z0-9_-]+)\s*=\s*(.+?)\s*$/.exec(line);
    return match ? [{ key: match[2], value: match[3] }] : [];
}

function fieldKey(line: string): string | null {
    return parseFieldLine(line)[0]?.key ?? null;
}

function firstSectionLine(lines: string[]): number {
    const index = lines.findIndex((line) => /^\s*\[/.test(line));
    return index === -1 ? lines.length : index;
}

function nextSectionLine(lines: string[], start: number): number {
    const index = lines.findIndex(
        (line, candidate) => candidate >= start && /^\s*\[/.test(line),
    );
    return index === -1 ? lines.length : index;
}

function schemaVersionInsertLine(lines: string[], end: number): number {
    for (let index = 0; index < end; index += 1) {
        if (fieldKey(lines[index]) === "schema_version") {
            return index + 1;
        }
    }
    let insertAt = end;
    while (insertAt > 0 && lines[insertAt - 1].trim() === "") {
        insertAt -= 1;
    }
    return insertAt;
}

function inputFromLiteral(literal: string): string {
    const trimmed = literal.trim();
    if (trimmed.startsWith('"')) {
        return parseTomlStringLiteral(trimmed);
    }
    return trimmed;
}

function literalFromInput(value: string): string {
    const trimmed = value.trim();
    if (
        trimmed === "true" ||
        trimmed === "false" ||
        trimmed.startsWith("[") ||
        trimmed.startsWith("{") ||
        /^-?\d+(?:\.\d+)?$/.test(trimmed)
    ) {
        return trimmed;
    }
    return JSON.stringify(value);
}

function parseTomlStringLiteral(value: string): string {
    try {
        return JSON.parse(value) as string;
    } catch {
        return value.replace(/^"|"$/g, "");
    }
}
