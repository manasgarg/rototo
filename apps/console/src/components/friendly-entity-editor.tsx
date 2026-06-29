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
import widgetSpec from "../../../../src/lint/builtins/ui-widgets.json";
import { shouldAutoFocus } from "./autofocus";
import {
    CodeEditor,
    type CodeEditorLsp,
    type CodeEditorMark,
} from "./code-editor";
import { apiFetch } from "@/lib/api";

/* The pre-registered widget vocabulary. src/lint/builtins/ui-widgets.json is
   the single source of truth, shared with the Rust lint rules that validate
   hints. */
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

/** Package section edited by the friendly editor. */
type EntitySection =
    | "variables"
    | "qualifiers"
    | "catalogs"
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
type VariableDeclarationKind =
    | "primitive"
    | "catalog"
    | "primitiveList"
    | "catalogList";

/**
 * Optional context harvested from sibling entities.
 *
 * The parent screen rebuilds this from the current package/branch payload for
 * each editor instance. Descriptions explain fields, examples show values
 * already in use, and preview truth tables come from the Rust runtime.
 */
export type FormGuidance = {
    contextAttributeDocs?: Record<string, string>;
    attributeValueExamples?: Record<string, string[]>;
    catalogDocs?: Record<string, string>;
    propertyExamples?: Record<string, string[]>;
    qualifierIds?: string[];
    catalogEntryKeys?: Record<string, string[]>;
    contextPreviews?: EditContextPreview[];
};

/**
 * Saved evaluation context with each package qualifier already evaluated.
 *
 * The editor walks edited rules against these runtime truths so previews update
 * live without reimplementing qualifier semantics in React.
 */
export type EditContextPreview = {
    name: string;
    qualifierTruth: Record<string, boolean>;
};

const PRIMITIVE_TYPES = ["bool", "int", "number", "string", "list"];
const LIST_PRIMITIVE_TYPES = ["bool", "int", "number", "string"];

const DECLARATION_HINTS: Record<VariableDeclarationKind, string> = {
    primitive: "Resolve values must match this primitive type.",
    catalog: "Resolve values select values from this catalog.",
    primitiveList: "Resolve values must be a list of this primitive type.",
    catalogList: "Resolve values select a list of values from this catalog.",
};

export function FriendlyEntityEditor({
    baseText = null,
    contextAttributes = [],
    diagnostics = [],
    disabled,
    branchId,
    entity,
    guidance = {},
    catalogIds = [],
    catalogSchema = null,
    sourceMarks = [],
    packageId,
}: {
    /* The entity's text at the branch's base ref, when known. Enables the
     changes view in both form and source modes. */
    baseText?: string | null;
    contextAttributes?: string[];
    diagnostics?: LintDiagnostic[];
    disabled?: boolean;
    branchId: string;
    entity: FriendlyEntity;
    guidance?: FormGuidance;
    catalogIds?: string[];
    catalogSchema?: string | null;
    sourceMarks?: CodeEditorMark[];
    packageId: string;
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

    // Source mode talks to the branch's rototo language server for as-you-type
    // diagnostics, completion, and hover.
    const lspRequest = useCallback(
        async (body: Record<string, unknown>) => {
            const response = await apiFetch(
                `/api/packages/${packageId}/branches/${branchId}/lsp`,
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
        [packageId, branchId, entity.path],
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
                `/api/packages/${packageId}/branches/${branchId}/files`,
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
            setNote({ tone: "ok", text: "Saved to the branch." });
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
                    {pending ? "Saving" : "Save to branch"}
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
}): ReactNode | null {
    if (entity.language !== "toml") {
        return null;
    }

    if (entity.section === "variables") {
        return (
            <VariableFields
                content={content}
                contextAttributes={contextAttributes}
                diagnostics={diagnostics}
                disabled={disabled}
                guidance={guidance}
                onChange={onChange}
                catalogIds={catalogIds}
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
    if (entity.kind === "catalog value") {
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
        if (!tomlSectionRawField(content, "[resolve]", "default")) {
            if (!hasSection("[resolve]")) {
                missing.push("[resolve]");
            }
            missing.push(`default = ${defaultLiteralFor(fields)}`);
        }
    } else if (entity.section === "qualifiers") {
        if (!has("schema_version")) {
            missing.push("schema_version = 1");
        }
        if (!has("description")) {
            missing.push('description = ""');
        }
        if (!has("when")) {
            missing.push('when = "context.user.tier == \\"premium\\""');
        }
    } else if (entity.kind === "catalog value") {
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
    contextAttributes,
    diagnostics = [],
    disabled,
    guidance = {},
    onChange,
    catalogIds,
}: {
    content: string;
    contextAttributes: string[];
    diagnostics?: LintDiagnostic[];
    disabled?: boolean;
    guidance?: FormGuidance;
    onChange: (content: string) => void;
    catalogIds: string[];
}) {
    const fields = useMemo(() => variableFields(content), [content]);
    const model = useMemo(() => variableModel(content), [content]);
    const catalogValueOptions =
        fields.declarationKind === "catalog" ||
        fields.declarationKind === "catalogList"
            ? (guidance.catalogEntryKeys?.[fields.declarationValue] ?? [])
            : [];
    const declarationDoc =
        fields.declarationKind === "catalog" ||
        fields.declarationKind === "catalogList"
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
            targetFieldKind(diagnostic) === "variable_resolve_default",
    );

    function updateDeclaration(kind: VariableDeclarationKind, value: string) {
        let text = removeSectionBlock(content, "[values]");
        text = removeTopLevelField(text, "type");
        text = removeTopLevelField(text, "schema");
        if (kind === "catalog") {
            onChange(setTopLevelStringField(text, "type", `catalog:${value}`));
        } else if (kind === "catalogList") {
            onChange(
                setTopLevelStringField(text, "type", `list<catalog:${value}>`),
            );
        } else if (kind === "primitiveList") {
            onChange(setTopLevelStringField(text, "type", `list<${value}>`));
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
        } else if (kind === "primitiveList") {
            updateDeclaration(kind, "string");
        } else {
            updateDeclaration(kind, catalogIds[0] ?? "");
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
    } else if (fields.declarationKind === "primitiveList") {
        declarationControl = (
            <select
                className="input mono"
                disabled={disabled}
                onChange={(event) =>
                    updateDeclaration("primitiveList", event.target.value)
                }
                value={
                    LIST_PRIMITIVE_TYPES.includes(fields.declarationValue)
                        ? fields.declarationValue
                        : "string"
                }
            >
                {LIST_PRIMITIVE_TYPES.map((type) => (
                    <option key={type} value={type}>
                        {type}
                    </option>
                ))}
            </select>
        );
    } else {
        const listId = "variable-catalog-ids";
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
                    {catalogIds.map((suggestion) => (
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
                        <option value="primitiveList">
                            list of primitive values
                        </option>
                        <option value="catalogList">
                            list of catalog values
                        </option>
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
            <label className="field-stack">
                <span className="label">default value</span>
                {fields.declarationKind === "catalog" ? (
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
                        value={model.defaultValue ?? ""}
                    >
                        {model.defaultValue === null ? (
                            <option value="">not declared</option>
                        ) : null}
                        {catalogValueOptions.map((option) => (
                            <option key={option} value={option}>
                                {option}
                            </option>
                        ))}
                        {model.defaultValue !== null &&
                        !catalogValueOptions.includes(model.defaultValue) ? (
                            <option value={model.defaultValue}>
                                {model.defaultValue} (unknown)
                            </option>
                        ) : null}
                    </select>
                ) : (
                    <VariableValueControl
                        catalogValueOptions={catalogValueOptions}
                        declarationKind={model.declarationKind}
                        declarationValue={model.declarationValue}
                        disabled={disabled}
                        literal={
                            model.defaultValue ??
                            emptyLiteralFor(
                                model.declarationValue,
                                model.declarationKind,
                            )
                        }
                        onUpdate={(literal) =>
                            onChange(
                                setTomlSectionField(
                                    ensureSection(content, "[resolve]"),
                                    "[resolve]",
                                    "default",
                                    literal,
                                ),
                            )
                        }
                    />
                )}
                <FieldNotes items={defaultNotes} />
                <span className="field-hint">
                    {fields.declarationKind === "catalog"
                        ? "Selects which catalog value applies when no rule matches."
                        : fields.declarationKind === "catalogList"
                          ? "Selects which catalog values apply when no rule matches."
                          : "Applies when no rule matches."}
                </span>
            </label>
            <VariableRulesEditor
                diagnostics={diagnostics}
                disabled={disabled}
                model={model}
                onChange={(rules) =>
                    onChange(
                        rewriteResolveRules(
                            content,
                            rules,
                            model.declarationKind,
                            model.declarationValue,
                        ),
                    )
                }
                contextAttributes={contextAttributes}
                qualifierIds={guidance.qualifierIds ?? []}
                valueOptions={catalogValueOptions}
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
   context saved in the package. */
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
                Conditions are evaluated by rototo on the branch; the pathway
                follows your edits before you save.
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
        if (rule.selectionKind === "query") {
            return `rule[${index}] query — preview unavailable`;
        }
        const qualifier = simpleRuleQualifierId(rule);
        if (qualifier === null) {
            return `rule[${index}] expression — preview unavailable`;
        }
        if (!qualifier) {
            continue;
        }
        const matched = preview.qualifierTruth[qualifier];
        if (matched === undefined) {
            return `rule[${index}] ${qualifier} — condition not evaluable yet`;
        }
        if (matched) {
            return `rule[${index}] ${qualifier} → ${rule.value || "unset"}`;
        }
    }
    return model.defaultValue !== null
        ? `default → ${model.defaultValue}`
        : "no default declared";
}

/* The value control morphs with the declaration: bool gets true/false,
   numbers get a number input, lists get a stacked item editor. */
function VariableValueControl({
    catalogValueOptions = [],
    declarationKind,
    declarationValue,
    disabled,
    literal,
    onUpdate,
}: {
    catalogValueOptions?: string[];
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
    if (declarationKind === "primitiveList") {
        return (
            <ListLiteralEditor
                disabled={disabled}
                itemType={declarationValue}
                literal={literal}
                onUpdate={onUpdate}
            />
        );
    }
    if (declarationKind === "catalogList") {
        return (
            <ListLiteralEditor
                disabled={disabled}
                itemType="string"
                literal={literal}
                onUpdate={onUpdate}
                suggestions={catalogValueOptions}
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
    // anything unrecognized: raw TOML literal
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
    itemType = null,
    literal,
    onUpdate,
    suggestions = [],
}: {
    disabled?: boolean;
    itemType?: string | null;
    literal: string;
    onUpdate: (literal: string) => void;
    suggestions?: string[];
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
    const appendValue = suggestions[0] ?? defaultListItem(itemType);
    return (
        <div className="list-editor">
            {items.map((item, index) => (
                <div className="list-item-row" key={index}>
                    {suggestions.length > 0 ? (
                        <select
                            className="input mono"
                            disabled={disabled}
                            onChange={(event) => {
                                const next = [...items];
                                next[index] = event.target.value;
                                write(next);
                            }}
                            value={typeof item === "string" ? item : ""}
                        >
                            {suggestions.map((suggestion) => (
                                <option key={suggestion} value={suggestion}>
                                    {suggestion}
                                </option>
                            ))}
                            {typeof item === "string" &&
                            !suggestions.includes(item) ? (
                                <option value={item}>{item}</option>
                            ) : null}
                        </select>
                    ) : (
                        <input
                            className="input mono"
                            disabled={disabled}
                            onChange={(event) => {
                                const next = [...items];
                                next[index] = parseListItem(
                                    event.target.value,
                                    itemType,
                                );
                                write(next);
                            }}
                            value={
                                typeof item === "string"
                                    ? item
                                    : JSON.stringify(item)
                            }
                        />
                    )}
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
                onClick={() => write([...items, appendValue])}
                style={{ width: "fit-content" }}
                type="button"
            >
                <Plus aria-hidden size={14} />
                Add item
            </button>
        </div>
    );
}

function parseListItem(value: string, itemType?: string | null): unknown {
    const trimmed = value.trim();
    if (itemType === "string") {
        return value;
    }
    if (itemType === "bool") {
        return trimmed === "true";
    }
    if (itemType === "int") {
        return Number.parseInt(trimmed || "0", 10);
    }
    if (itemType === "number") {
        return Number(trimmed || "0");
    }
    if (trimmed === "true") return true;
    if (trimmed === "false") return false;
    if (/^-?\d+(?:\.\d+)?$/.test(trimmed)) return Number(trimmed);
    return value;
}

function defaultListItem(itemType?: string | null): unknown {
    switch (itemType) {
        case "bool":
            return false;
        case "int":
        case "number":
            return 0;
        default:
            return "";
    }
}

/* Resolve rules: optional condition plus either an explicit value or a catalog query. */
type VariableRuleModel = {
    condition: string;
    conditionKind: "qualifier" | "expression";
    query: string;
    selectionKind: "value" | "query";
    value: string;
};

function VariableRulesEditor({
    contextAttributes,
    diagnostics,
    disabled,
    model,
    onChange,
    qualifierIds,
    valueOptions,
}: {
    contextAttributes: string[];
    diagnostics: LintDiagnostic[];
    disabled?: boolean;
    model: VariableModel;
    onChange: (rules: VariableRuleModel[]) => void;
    qualifierIds: string[];
    valueOptions: string[];
}) {
    // Drag state: a row is only draggable while its handle is pressed, so text
    // selection inside the inputs keeps working.
    const [armedIndex, setArmedIndex] = useState<number | null>(null);
    const [dragIndex, setDragIndex] = useState<number | null>(null);
    const [dropIndex, setDropIndex] = useState<number | null>(null);
    const reorderable = !disabled && model.rules.length > 1;
    const conditionSuggestions = ruleExpressionSuggestions({
        contextAttributes,
        includeEntry: false,
        qualifierIds,
    });
    const querySuggestions = ruleExpressionSuggestions({
        contextAttributes,
        includeEntry: true,
        qualifierIds,
    });

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
                    rule to vary the value by a condition.
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
                                aria-label="Rule CEL condition"
                                className="input mono"
                                disabled={disabled}
                                list="variable-rule-condition-expressions"
                                onChange={(event) =>
                                    onChange(
                                        model.rules.map((candidate, at) =>
                                            at === index
                                                ? updateRuleCondition(
                                                      candidate,
                                                      event.target.value,
                                                      qualifierIds,
                                                  )
                                                : candidate,
                                        ),
                                    )
                                }
                                placeholder={
                                    rule.selectionKind === "query"
                                        ? "optional CEL condition"
                                        : (conditionSuggestions[0] ??
                                          'context.user.tier == "premium"')
                                }
                                value={rule.condition}
                            />
                            {model.declarationKind === "catalogList" ? (
                                <select
                                    aria-label="Rule selection"
                                    className="input mono"
                                    disabled={disabled}
                                    onChange={(event) =>
                                        onChange(
                                            model.rules.map((candidate, at) =>
                                                at === index
                                                    ? {
                                                          ...candidate,
                                                          selectionKind: event
                                                              .target.value as
                                                              | "value"
                                                              | "query",
                                                      }
                                                    : candidate,
                                            ),
                                        )
                                    }
                                    value={rule.selectionKind}
                                >
                                    <option value="value">value list</option>
                                    <option value="query">query</option>
                                </select>
                            ) : null}
                            {rule.selectionKind === "query" ? (
                                <>
                                    <span className="rule-word">query</span>
                                    <input
                                        aria-label="Rule query"
                                        className="input mono"
                                        disabled={disabled}
                                        list="variable-rule-query-expressions"
                                        onChange={(event) =>
                                            onChange(
                                                model.rules.map(
                                                    (candidate, at) =>
                                                        at === index
                                                            ? {
                                                                  ...candidate,
                                                                  query: event
                                                                      .target
                                                                      .value,
                                                              }
                                                            : candidate,
                                                ),
                                            )
                                        }
                                        placeholder="entry.field == context.field"
                                        value={rule.query}
                                    />
                                </>
                            ) : (
                                <>
                                    <span className="rule-word">→</span>
                                    {model.declarationKind === "catalog" ? (
                                        <select
                                            aria-label="Rule value"
                                            className="input mono"
                                            disabled={disabled}
                                            onChange={(event) =>
                                                onChange(
                                                    model.rules.map(
                                                        (candidate, at) =>
                                                            at === index
                                                                ? {
                                                                      ...candidate,
                                                                      value: event
                                                                          .target
                                                                          .value,
                                                                  }
                                                                : candidate,
                                                    ),
                                                )
                                            }
                                            value={rule.value}
                                        >
                                            {valueOptions.map((option) => (
                                                <option
                                                    key={option}
                                                    value={option}
                                                >
                                                    {option}
                                                </option>
                                            ))}
                                            {!valueOptions.includes(
                                                rule.value,
                                            ) ? (
                                                <option value={rule.value}>
                                                    {rule.value || "unset"}
                                                </option>
                                            ) : null}
                                        </select>
                                    ) : (
                                        <VariableValueControl
                                            catalogValueOptions={valueOptions}
                                            declarationKind={
                                                model.declarationKind
                                            }
                                            declarationValue={
                                                model.declarationValue
                                            }
                                            disabled={disabled}
                                            literal={
                                                rule.value ||
                                                emptyLiteralFor(
                                                    model.declarationValue,
                                                    model.declarationKind,
                                                )
                                            }
                                            onUpdate={(literal) =>
                                                onChange(
                                                    model.rules.map(
                                                        (candidate, at) =>
                                                            at === index
                                                                ? {
                                                                      ...candidate,
                                                                      value: literal,
                                                                  }
                                                                : candidate,
                                                    ),
                                                )
                                            }
                                        />
                                    )}
                                </>
                            )}
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
            <datalist id="variable-rule-condition-expressions">
                {conditionSuggestions.map((suggestion) => (
                    <option key={suggestion} value={suggestion} />
                ))}
            </datalist>
            <datalist id="variable-rule-query-expressions">
                {querySuggestions.map((suggestion) => (
                    <option key={suggestion} value={suggestion} />
                ))}
            </datalist>
            <button
                className="btn btn-ghost btn-sm"
                disabled={disabled}
                onClick={() => {
                    const selectionKind =
                        model.declarationKind === "catalogList"
                            ? "query"
                            : "value";
                    onChange([
                        ...model.rules,
                        {
                            condition: "",
                            conditionKind: "expression",
                            query: "",
                            selectionKind,
                            value:
                                model.declarationKind === "catalog"
                                    ? (valueOptions[0] ?? "")
                                    : emptyLiteralFor(
                                          model.declarationValue,
                                          model.declarationKind,
                                      ),
                        },
                    ]);
                }}
                style={{ width: "fit-content" }}
                type="button"
            >
                <Plus aria-hidden size={14} />
                Add rule
            </button>
        </div>
    );
}

function updateRuleCondition(
    rule: VariableRuleModel,
    condition: string,
    qualifierIds: string[],
): VariableRuleModel {
    const conditionKind = inferRuleConditionKind(condition, qualifierIds);
    return {
        ...rule,
        condition,
        conditionKind,
    };
}

function inferRuleConditionKind(
    condition: string,
    qualifierIds: string[],
): VariableRuleModel["conditionKind"] {
    const trimmed = condition.trim();
    if (qualifierIds.includes(trimmed)) {
        return "qualifier";
    }
    return "expression";
}

function ruleExpressionSuggestions({
    contextAttributes,
    includeEntry,
    qualifierIds,
}: {
    contextAttributes: string[];
    includeEntry: boolean;
    qualifierIds: string[];
}): string[] {
    return [
        ...contextAttributes.map((attribute) => `context.${attribute}`),
        ...(includeEntry ? ["entry."] : []),
        ...qualifierIds.map((id) => `env.qualifier[${JSON.stringify(id)}]`),
        "has(context.)",
        'bucket(context.user.id, "salt", 0, 50)',
        'contains(context., "")',
        'startsWith(context., "")',
    ];
}

function simpleRuleQualifierId(rule: VariableRuleModel): string | null {
    if (!rule.condition) {
        return "";
    }
    if (rule.conditionKind === "qualifier") {
        return rule.condition;
    }
    return qualifierIdFromWhenExpression(rule.condition);
}

function QualifierFields({
    content,
    contextAttributes,
    diagnostics = [],
    disabled,
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
    const contextExpressions = contextAttributes.map(
        (attribute) => `context.${attribute}`,
    );
    const whenNotes = diagnostics.filter(
        (diagnostic) =>
            targetEntityKind(diagnostic) === "qualifier" &&
            targetFieldKind(diagnostic) === "qualifier_when",
    );

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
            <label className="field-stack">
                <span className="label">when</span>
                <textarea
                    className="input mono"
                    disabled={disabled}
                    onChange={(event) =>
                        onChange(
                            setTopLevelStringField(
                                content,
                                "when",
                                event.target.value,
                            ),
                        )
                    }
                    rows={3}
                    value={fields.when}
                />
                <FieldNotes items={whenNotes} />
                <span className="field-hint">
                    Expression over context and env
                    (env.qualifier[&quot;…&quot;], env.now), for example{" "}
                    {contextExpressions[0] ?? "context.user.tier"} == "premium"
                    .
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
    defaultValue: string | null;
    rules: VariableRuleModel[];
};

function variableModel(text: string): VariableModel {
    const base = variableFields(text);
    const defaultLiteral = tomlSectionRawField(text, "[resolve]", "default");
    return {
        declarationKind: base.declarationKind,
        declarationValue: base.declarationValue,
        defaultValue:
            defaultLiteral && base.declarationKind === "catalog"
                ? parseTomlStringLiteral(defaultLiteral)
                : defaultLiteral,
        rules: resolveRuleBlocks(text, base.declarationKind),
    };
}

function resolveRuleBlocks(
    text: string,
    declarationKind: VariableDeclarationKind,
): VariableRuleModel[] {
    const lines = text.split(/\r?\n/);
    const rules: VariableRuleModel[] = [];
    for (let index = 0; index < lines.length; index += 1) {
        if (lines[index].trim() !== "[[resolve.rule]]") {
            continue;
        }
        const end = nextSectionLine(lines, index + 1);
        const blockFields = lines.slice(index + 1, end).flatMap(parseFieldLine);
        const value =
            blockFields.find((field) => field.key === "value")?.value ?? "";
        const whenField = blockFields.find(
            (field) => field.key === "when",
        )?.value;
        const queryField = blockFields.find(
            (field) => field.key === "query",
        )?.value;
        const parsedWhen = qualifierConditionFromWhenExpression(
            parseTomlStringLiteral(whenField ?? '""'),
        );
        rules.push({
            condition: parsedWhen.condition,
            conditionKind: parsedWhen.conditionKind,
            query: parseTomlStringLiteral(queryField ?? '""'),
            selectionKind: queryField ? "query" : "value",
            value:
                declarationKind === "catalog"
                    ? parseTomlStringLiteral(value || '""')
                    : value,
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

function rewriteResolveRules(
    text: string,
    rules: VariableRuleModel[],
    declarationKind: VariableDeclarationKind,
    declarationValue: string,
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
        .map((rule) => {
            const value =
                declarationKind === "catalog"
                    ? JSON.stringify(rule.value)
                    : rule.value ||
                      emptyLiteralFor(declarationValue, declarationKind);
            const fields: string[] = [];
            const condition = rule.condition.trim();
            if (condition !== "") {
                fields.push(
                    `when = ${tomlLiteralString(ruleWhenExpression(rule))}`,
                );
            }
            if (rule.selectionKind === "query") {
                fields.push(`query = ${tomlLiteralString(rule.query)}`);
            } else {
                fields.push(`value = ${value}`);
            }
            return `[[resolve.rule]]\n${fields.join("\n")}`;
        })
        .join("\n\n");
    return tidyToml(`${without.trimEnd()}\n\n${blocks}`);
}

function ruleWhenExpression(rule: VariableRuleModel): string {
    return rule.conditionKind === "qualifier"
        ? `env.qualifier[${JSON.stringify(rule.condition.trim())}]`
        : rule.condition;
}

function qualifierConditionFromWhenExpression(expression: string): {
    condition: string;
    conditionKind: VariableRuleModel["conditionKind"];
} {
    return { condition: expression, conditionKind: "expression" };
}

function qualifierIdFromWhenExpression(expression: string): string | null {
    const trimmed = stripOuterParens(expression.trim());
    const match = trimmed.match(/^env\.qualifier\[(.+)\]$/);
    return match ? parseTomlStringLiteral(match[1].trim()) : null;
}

function stripOuterParens(value: string): string {
    let current = value;
    while (
        current.startsWith("(") &&
        current.endsWith(")") &&
        outerParensWrapExpression(current)
    ) {
        current = current.slice(1, -1).trim();
    }
    return current;
}

function outerParensWrapExpression(value: string): boolean {
    let depth = 0;
    for (let index = 0; index < value.length; index += 1) {
        const char = value[index];
        if (char === "(") {
            depth += 1;
        } else if (char === ")") {
            depth -= 1;
            if (depth === 0 && index < value.length - 1) {
                return false;
            }
        }
        if (depth < 0) {
            return false;
        }
    }
    return depth === 0;
}

function tomlLiteralString(value: string): string {
    return value.includes("'") ? JSON.stringify(value) : `'${value}'`;
}

function emptyLiteralFor(
    declarationValue: string,
    declarationKind: VariableDeclarationKind,
): string {
    if (
        declarationKind === "primitiveList" ||
        declarationKind === "catalogList"
    ) {
        return "[]";
    }
    if (declarationKind === "catalog") {
        return '""';
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

function defaultLiteralFor(fields: {
    declarationKind: VariableDeclarationKind;
    declarationValue: string;
}): string {
    return emptyLiteralFor(fields.declarationValue, fields.declarationKind);
}

function variableFields(text: string) {
    const type = topLevelStringField(text, "type");
    const base = {
        description: topLevelStringField(text, "description"),
        defaultValue: inputFromLiteral(
            tomlSectionRawField(text, "[resolve]", "default") ?? "",
        ),
    };
    const listInner = type
        ?.trim()
        .match(/^list<(.+)>$/)?.[1]
        ?.trim();
    if (listInner?.startsWith("catalog:")) {
        return {
            ...base,
            declarationKind: "catalogList" as VariableDeclarationKind,
            declarationValue: listInner.slice("catalog:".length),
        };
    }
    if (listInner) {
        return {
            ...base,
            declarationKind: "primitiveList" as VariableDeclarationKind,
            declarationValue: listInner,
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
        when: topLevelStringField(text, "when") ?? "",
    };
}

function declarationLabel(kind: VariableDeclarationKind): string {
    if (kind === "catalog" || kind === "catalogList") {
        return "catalog id";
    }
    if (kind === "primitiveList") {
        return "item type";
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
    if (trimmed.startsWith('"') || trimmed.startsWith("'")) {
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
    const trimmed = value.trim();
    if (trimmed.startsWith("'") && trimmed.endsWith("'")) {
        return trimmed.slice(1, -1);
    }
    try {
        return JSON.parse(trimmed) as string;
    } catch {
        return trimmed.replace(/^"|"$/g, "");
    }
}
