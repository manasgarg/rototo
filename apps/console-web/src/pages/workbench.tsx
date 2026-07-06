// Workbench editing v1 (tranche C2): entity views from the semantic model,
// the form-submits-operations editor for variables, and the raw-text escape
// hatch. Edits land on a change set's branch; without an active change set
// the workbench is read-only and says why.

import { useCallback, useEffect, useMemo, useState } from "react";

import {
    ApiError,
    createChangeSet,
    listChangeSets,
    listPackageFiles,
    listPackages,
    readPackage,
    readPackageFile,
    saveEdit,
    type ChangeSet,
    type EditOperation,
    type EditResponse,
    type MeResponse,
    type PackageDetail,
    type PackageListing,
    type RuleModel,
    type SourceTreeSummary,
    type VariableModel,
} from "@/lib/api";
import { navigate } from "@/lib/router";

type Banner = { kind: "ok" | "err" | "warn"; text: string };

type View =
    | { kind: "entities" }
    | { kind: "variable"; id: string }
    | { kind: "file"; path: string };

export function WorkbenchPage({
    me,
    treeId,
}: {
    me: MeResponse;
    treeId: string;
}) {
    const tree = me.capabilities?.sourceTrees.find(
        (candidate) => candidate.id === treeId,
    );
    const [changeSets, setChangeSets] = useState<ChangeSet[]>([]);
    const [activeId, setActiveId] = useState<string | null>(null);
    const [listing, setListing] = useState<PackageListing | null>(null);
    const [selectedPackage, setSelectedPackage] = useState<string | null>(null);
    const [detail, setDetail] = useState<PackageDetail | null>(null);
    const [view, setView] = useState<View>({ kind: "entities" });
    const [banner, setBanner] = useState<Banner | null>(null);

    const active = changeSets.find((entry) => entry.id === activeId) ?? null;
    const editable =
        active !== null &&
        (active.state === "draft" || active.state === "proposed");

    const refreshChangeSets = useCallback(() => {
        listChangeSets(treeId).then(
            (response) => setChangeSets(response.changeSets),
            (error: Error) => setBanner({ kind: "err", text: error.message }),
        );
    }, [treeId]);

    useEffect(() => {
        refreshChangeSets();
    }, [refreshChangeSets]);

    // The reading ref: the change set's branch while one is active,
    // otherwise the default branch. Resolved to a pin by the server.
    const ref = active?.branch;
    useEffect(() => {
        let stale = false;
        setListing(null);
        setDetail(null);
        listPackages(treeId, ref).then(
            (response) => {
                if (stale) {
                    return;
                }
                setListing(response);
                setSelectedPackage((current) =>
                    current !== null &&
                    response.packages.some((entry) => entry.path === current)
                        ? current
                        : (response.packages[0]?.path ?? null),
                );
            },
            (error: Error) => {
                if (!stale) {
                    setBanner({ kind: "err", text: error.message });
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, ref]);

    const pin = listing?.pin ?? null;
    useEffect(() => {
        if (pin === null || selectedPackage === null) {
            return;
        }
        let stale = false;
        readPackage(treeId, selectedPackage, pin).then(
            (response) => {
                if (!stale) {
                    setDetail(response);
                }
            },
            (error: Error) => {
                if (!stale) {
                    setBanner({ kind: "err", text: error.message });
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, selectedPackage, pin]);

    const afterSave = useCallback(
        (result: EditResponse) => {
            setListing((current) =>
                current === null ? current : { ...current, pin: result.pin },
            );
            const errors = result.lint.diagnostics.filter(
                (diagnostic) => diagnostic.severity === "error",
            );
            setBanner(
                errors.length === 0
                    ? { kind: "ok", text: "Saved: one commit, lint clean." }
                    : {
                          kind: "warn",
                          text: `Saved, but lint reports ${errors.length} error${errors.length === 1 ? "" : "s"}: ${errors[0]?.message ?? ""}`,
                      },
            );
            refreshChangeSets();
        },
        [refreshChangeSets],
    );

    const saveFailed = useCallback((error: unknown) => {
        if (error instanceof ApiError && error.paths !== undefined) {
            setBanner({
                kind: "err",
                text: `${error.message} (${error.paths.join(", ")})`,
            });
            return;
        }
        setBanner({
            kind: "err",
            text: error instanceof Error ? error.message : String(error),
        });
    }, []);

    if (tree === undefined) {
        return (
            <div className="card">
                <h1>Not visible</h1>
                <p className="hint">
                    This source tree does not exist or is not visible to you.
                </p>
            </div>
        );
    }

    const treeName =
        tree.kind === "github" ? `${tree.owner}/${tree.name}` : tree.id;

    return (
        <div className="section">
            <div className="section-header">
                <div className="section-header-text">
                    <h1>{treeName}</h1>
                    <p className="hint">
                        {listing === null
                            ? "Resolving…"
                            : `${listing.ref} @ ${listing.pin.slice(0, 10)}`}
                    </p>
                </div>
                <button
                    className="btn btn-secondary btn-sm"
                    onClick={() => navigate(`/trees/${treeId}/changes`)}
                >
                    Change sets
                </button>
            </div>

            <EditingStrip
                treeId={treeId}
                canPropose={tree.capabilities.propose}
                changeSets={changeSets}
                active={active}
                onSelect={(id) => {
                    setActiveId(id);
                    setView({ kind: "entities" });
                    setBanner(null);
                }}
                onCreated={(changeSet) => {
                    setChangeSets((current) => [changeSet, ...current]);
                    setActiveId(changeSet.id);
                    setBanner(null);
                }}
                onError={saveFailed}
            />

            {banner !== null ? (
                <div
                    className={`banner ${banner.kind === "ok" ? "banner-info" : banner.kind === "warn" ? "banner-warn" : "banner-err"}`}
                >
                    {banner.text}
                </div>
            ) : null}

            {listing !== null && listing.packages.length > 1 ? (
                <div className="field-row">
                    <span className="label">Package</span>
                    <select
                        className="input"
                        value={selectedPackage ?? ""}
                        onChange={(event) => {
                            setSelectedPackage(event.target.value);
                            setView({ kind: "entities" });
                        }}
                    >
                        {listing.packages.map((entry) => (
                            <option key={entry.path} value={entry.path}>
                                {entry.path}
                            </option>
                        ))}
                    </select>
                </div>
            ) : null}

            {detail === null ? (
                <p className="muted">Loading package…</p>
            ) : view.kind === "variable" ? (
                <VariablePanel
                    key={`${view.id}@${detail.pin}`}
                    detail={detail}
                    variableId={view.id}
                    editable={editable}
                    changeSet={active}
                    onBack={() => setView({ kind: "entities" })}
                    onSaved={afterSave}
                    onError={saveFailed}
                />
            ) : view.kind === "file" ? (
                <FilePanel
                    key={`${view.path}@${detail.pin}`}
                    treeId={treeId}
                    detail={detail}
                    file={view.path}
                    editable={editable}
                    changeSet={active}
                    onBack={() => setView({ kind: "entities" })}
                    onSaved={afterSave}
                    onError={saveFailed}
                />
            ) : (
                <EntityLists
                    treeId={treeId}
                    detail={detail}
                    onOpenVariable={(id) => setView({ kind: "variable", id })}
                    onOpenFile={(path) => setView({ kind: "file", path })}
                />
            )}
        </div>
    );
}

// The editing context: which change set commits accumulate on. Viewing the
// default branch is read-only by design; the branch is the durable draft.
function EditingStrip({
    treeId,
    canPropose,
    changeSets,
    active,
    onSelect,
    onCreated,
    onError,
}: {
    treeId: string;
    canPropose: { allow: boolean; reason: string };
    changeSets: ChangeSet[];
    active: ChangeSet | null;
    onSelect: (id: string | null) => void;
    onCreated: (changeSet: ChangeSet) => void;
    onError: (error: unknown) => void;
}) {
    const [creating, setCreating] = useState(false);
    const [title, setTitle] = useState("");
    const open = changeSets.filter(
        (entry) => entry.state === "draft" || entry.state === "proposed",
    );

    return (
        <div className="mode-strip">
            <span className="label mode-strip-label">
                {active === null ? "viewing" : "editing on"}
            </span>
            <select
                className="input"
                value={active?.id ?? ""}
                onChange={(event) =>
                    onSelect(
                        event.target.value === "" ? null : event.target.value,
                    )
                }
            >
                <option value="">Base branch (read-only)</option>
                {open.map((entry) => (
                    <option key={entry.id} value={entry.id}>
                        {entry.title} ({entry.state})
                    </option>
                ))}
            </select>
            {active !== null ? (
                <span className="mono mode-strip-branch">{active.branch}</span>
            ) : null}
            {creating ? (
                <span className="inline-form">
                    <input
                        autoFocus
                        className="input"
                        placeholder="What is this change about?"
                        value={title}
                        onChange={(event) => setTitle(event.target.value)}
                    />
                    <button
                        className="btn btn-primary btn-sm"
                        disabled={title.trim() === ""}
                        onClick={() => {
                            const changeSetTitle = title.trim();
                            setCreating(false);
                            setTitle("");
                            createChangeSet(treeId, changeSetTitle).then(
                                onCreated,
                                onError,
                            );
                        }}
                    >
                        Start
                    </button>
                    <button
                        className="btn btn-ghost btn-sm"
                        onClick={() => setCreating(false)}
                    >
                        Cancel
                    </button>
                </span>
            ) : canPropose.allow ? (
                <button
                    className="btn btn-secondary btn-sm"
                    onClick={() => setCreating(true)}
                >
                    New change set
                </button>
            ) : (
                <span className="hint" title={canPropose.reason}>
                    read-only: {canPropose.reason}
                </span>
            )}
        </div>
    );
}

function EntityLists({
    treeId,
    detail,
    onOpenVariable,
    onOpenFile,
}: {
    treeId: string;
    detail: PackageDetail;
    onOpenVariable: (id: string) => void;
    onOpenFile: (path: string) => void;
}) {
    const model = detail.model;
    const errors = detail.lint.diagnostics.filter(
        (diagnostic) => diagnostic.severity === "error",
    );
    return (
        <>
            {errors.length > 0 ? (
                <div className="banner banner-warn">
                    Lint reports {errors.length} error
                    {errors.length === 1 ? "" : "s"} on this package.
                </div>
            ) : null}

            <div className="section-header-text">
                <h2>Variables</h2>
            </div>
            <div className="row-list">
                {model.variables.map((variable) => (
                    <button
                        className="row"
                        key={variable.id}
                        onClick={() => onOpenVariable(variable.id)}
                    >
                        <span className="row-text">
                            <span className="row-title mono">
                                {variable.id}
                            </span>
                            <span className="row-sub">
                                {variable.declaration.value ?? "?"}
                                {variable.description !== undefined
                                    ? ` — ${variable.description}`
                                    : ""}
                            </span>
                        </span>
                        <span className="row-side mono">
                            {summarizeDefault(variable)}
                        </span>
                    </button>
                ))}
            </div>

            <Inventory
                title="Catalogs"
                items={model.catalogs.map(
                    (catalog) =>
                        `${catalog.id} (${
                            model.catalogEntries.filter(
                                (entry) => entry.catalog === catalog.id,
                            ).length
                        } entries)`,
                )}
            />
            <Inventory
                title="Enums"
                items={model.enums.map((entry) => entry.id)}
            />
            <Inventory
                title="Evaluation contexts"
                items={model.evaluationContexts.map((entry) => entry.id)}
            />

            <FileList treeId={treeId} detail={detail} onOpenFile={onOpenFile} />
        </>
    );
}

function Inventory({ title, items }: { title: string; items: string[] }) {
    if (items.length === 0) {
        return null;
    }
    return (
        <>
            <div className="section-header-text">
                <h2>{title}</h2>
            </div>
            <div>
                {items.map((item) => (
                    <span className="pill pill-neutral mono" key={item}>
                        {item}
                    </span>
                ))}
            </div>
        </>
    );
}

function FileList({
    treeId,
    detail,
    onOpenFile,
}: {
    treeId: string;
    detail: PackageDetail;
    onOpenFile: (path: string) => void;
}) {
    const [files, setFiles] = useState<string[] | null>(null);
    useEffect(() => {
        let stale = false;
        listPackageFiles(treeId, detail.path, detail.pin).then(
            (response) => {
                if (!stale) {
                    setFiles(response.files);
                }
            },
            () => {
                if (!stale) {
                    setFiles([]);
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, detail.path, detail.pin]);
    return (
        <>
            <div className="section-header-text">
                <h2>Files</h2>
                <p className="hint">
                    The raw-text escape hatch: every file, structured or not.
                </p>
            </div>
            <div className="row-list">
                {(files ?? []).map((file) => (
                    <button
                        className="row"
                        key={file}
                        onClick={() => onOpenFile(file)}
                    >
                        <span className="row-text">
                            <span className="row-title mono">{file}</span>
                        </span>
                    </button>
                ))}
            </div>
        </>
    );
}

// --- the variable form: a producer of operations, never a TOML rewriter ---

type RuleDraft = { when: string; valueText: string };

function VariablePanel({
    detail,
    variableId,
    editable,
    changeSet,
    onBack,
    onSaved,
    onError,
}: {
    detail: PackageDetail;
    variableId: string;
    editable: boolean;
    changeSet: ChangeSet | null;
    onBack: () => void;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
}) {
    const variable = detail.model.variables.find(
        (candidate) => candidate.id === variableId,
    );
    const type = variable?.declaration.value ?? "string";
    const original = useMemo(
        () => ({
            description: variable?.description ?? "",
            defaultText: valueToText(variable?.resolve?.default?.value, type),
            rules: (variable?.resolve?.rules ?? []).map(ruleToDraft(type)),
        }),
        [variable, type],
    );
    const [description, setDescription] = useState(original.description);
    const [defaultText, setDefaultText] = useState(original.defaultText);
    const [rules, setRules] = useState<RuleDraft[]>(original.rules);
    const [saving, setSaving] = useState(false);

    if (variable === undefined) {
        return (
            <div className="card">
                <p className="hint">No such variable at this pin.</p>
                <button className="btn btn-ghost btn-sm" onClick={onBack}>
                    Back
                </button>
            </div>
        );
    }

    const save = () => {
        if (changeSet === null) {
            return;
        }
        let operations: EditOperation[];
        try {
            operations = buildOperations(variableId, type, original, {
                description,
                defaultText,
                rules,
            });
        } catch (error) {
            onError(error);
            return;
        }
        if (operations.length === 0) {
            onError(new Error("nothing changed"));
            return;
        }
        setSaving(true);
        saveEdit(changeSet.id, {
            packagePath: detail.path,
            expectedPin: detail.pin,
            operations,
            summary: `Edit ${variableId}`,
        })
            .then(onSaved, onError)
            .finally(() => setSaving(false));
    };

    return (
        <div className="card card-stretch">
            <div className="card-head">
                <div className="card-head-text">
                    <h2 className="mono">{variable.id}</h2>
                    <p className="hint">
                        {type} · {variable.location.path}
                    </p>
                </div>
                <button className="btn btn-ghost btn-sm" onClick={onBack}>
                    Back
                </button>
            </div>

            <div className="form-fields">
                <div className="field-row">
                    <span className="label">Description</span>
                    <input
                        className="input"
                        disabled={!editable}
                        value={description}
                        onChange={(event) => setDescription(event.target.value)}
                    />
                </div>
                <div className="field-row">
                    <span className="label">Default</span>
                    <ValueInput
                        type={type}
                        disabled={!editable}
                        value={defaultText}
                        onChange={setDefaultText}
                    />
                </div>

                <div className="section-header-text">
                    <h3>Rules</h3>
                    <p className="hint">
                        First match wins; the default answers when none do.
                    </p>
                </div>
                {rules.map((rule, index) => (
                    <div className="rule-row" key={index}>
                        <span className="rule-word label">when</span>
                        <input
                            className="input mono"
                            disabled={!editable}
                            value={rule.when}
                            onChange={(event) =>
                                setRules(
                                    replaceAt(rules, index, {
                                        ...rule,
                                        when: event.target.value,
                                    }),
                                )
                            }
                        />
                        <span className="rule-word label">value</span>
                        <ValueInput
                            type={type}
                            disabled={!editable}
                            value={rule.valueText}
                            onChange={(valueText) =>
                                setRules(
                                    replaceAt(rules, index, {
                                        ...rule,
                                        valueText,
                                    }),
                                )
                            }
                        />
                        {editable ? (
                            <span className="action-row">
                                <button
                                    className="btn btn-icon btn-sm"
                                    disabled={index === 0}
                                    title="Move up"
                                    onClick={() =>
                                        setRules(
                                            moveRule(rules, index, index - 1),
                                        )
                                    }
                                >
                                    ↑
                                </button>
                                <button
                                    className="btn btn-icon btn-sm"
                                    disabled={index === rules.length - 1}
                                    title="Move down"
                                    onClick={() =>
                                        setRules(
                                            moveRule(rules, index, index + 1),
                                        )
                                    }
                                >
                                    ↓
                                </button>
                                <button
                                    className="btn btn-icon btn-sm btn-remove"
                                    title="Remove rule"
                                    onClick={() =>
                                        setRules(
                                            rules.filter((_, i) => i !== index),
                                        )
                                    }
                                >
                                    ×
                                </button>
                            </span>
                        ) : null}
                    </div>
                ))}
                {editable ? (
                    <div className="action-row">
                        <button
                            className="btn btn-secondary btn-sm"
                            onClick={() =>
                                setRules([
                                    ...rules,
                                    {
                                        when: "",
                                        valueText: defaultRuleValue(type),
                                    },
                                ])
                            }
                        >
                            Add rule
                        </button>
                    </div>
                ) : null}
            </div>

            <div className="card-actions">
                {editable ? (
                    <button
                        className="btn btn-primary"
                        disabled={saving}
                        onClick={save}
                    >
                        {saving ? "Saving…" : "Save (one commit)"}
                    </button>
                ) : (
                    <span className="hint">
                        Start or pick a change set above to edit.
                    </span>
                )}
            </div>
        </div>
    );
}

function ValueInput({
    type,
    value,
    disabled,
    onChange,
}: {
    type: string;
    value: string;
    disabled: boolean;
    onChange: (value: string) => void;
}) {
    if (type === "bool") {
        return (
            <select
                className="input"
                disabled={disabled}
                value={value}
                onChange={(event) => onChange(event.target.value)}
            >
                <option value="true">true</option>
                <option value="false">false</option>
            </select>
        );
    }
    if (type === "int" || type === "number") {
        return (
            <input
                className="input mono"
                type="number"
                disabled={disabled}
                value={value}
                onChange={(event) => onChange(event.target.value)}
            />
        );
    }
    return (
        <input
            className="input mono"
            disabled={disabled}
            value={value}
            onChange={(event) => onChange(event.target.value)}
            placeholder={type === "string" ? "text" : `${type} value as JSON`}
        />
    );
}

// --- the raw-text path ---

function FilePanel({
    treeId,
    detail,
    file,
    editable,
    changeSet,
    onBack,
    onSaved,
    onError,
}: {
    treeId: string;
    detail: PackageDetail;
    file: string;
    editable: boolean;
    changeSet: ChangeSet | null;
    onBack: () => void;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
}) {
    const [content, setContent] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);
    const diagnostics = detail.lint.diagnostics.filter(
        (diagnostic) => diagnostic.location?.path === file,
    );

    useEffect(() => {
        let stale = false;
        readPackageFile(treeId, detail.path, detail.pin, file).then(
            (response) => {
                if (!stale) {
                    setContent(response.content);
                }
            },
            (error: Error) => onError(error),
        );
        return () => {
            stale = true;
        };
    }, [treeId, detail.path, detail.pin, file, onError]);

    const save = () => {
        if (changeSet === null || content === null) {
            return;
        }
        setSaving(true);
        saveEdit(changeSet.id, {
            packagePath: detail.path,
            expectedPin: detail.pin,
            files: [{ path: file, content }],
            summary: `Edit ${file}`,
        })
            .then(onSaved, onError)
            .finally(() => setSaving(false));
    };

    return (
        <div className="card card-stretch">
            <div className="card-head">
                <div className="card-head-text">
                    <h2 className="mono">{file}</h2>
                    <p className="hint">
                        Raw text: lint judges the result after the save.
                    </p>
                </div>
                <button className="btn btn-ghost btn-sm" onClick={onBack}>
                    Back
                </button>
            </div>
            {content === null ? (
                <p className="muted">Loading…</p>
            ) : (
                <textarea
                    className="textarea mono"
                    disabled={!editable}
                    rows={Math.min(30, content.split("\n").length + 2)}
                    value={content}
                    onChange={(event) => setContent(event.target.value)}
                />
            )}
            {diagnostics.length > 0 ? (
                <div className="diagnostic-group">
                    {diagnostics.map((diagnostic, index) => (
                        <p className="diagnostic" key={index}>
                            <span
                                className={`pill ${diagnostic.severity === "error" ? "pill-err" : "pill-warn"}`}
                            >
                                {diagnostic.severity}
                            </span>{" "}
                            {diagnostic.message}
                        </p>
                    ))}
                </div>
            ) : null}
            <div className="card-actions">
                {editable ? (
                    <button
                        className="btn btn-primary"
                        disabled={saving || content === null}
                        onClick={save}
                    >
                        {saving ? "Saving…" : "Save (one commit)"}
                    </button>
                ) : (
                    <span className="hint">
                        Start or pick a change set above to edit.
                    </span>
                )}
            </div>
        </div>
    );
}

// --- form state to operations ---

function buildOperations(
    variableId: string,
    type: string,
    original: { description: string; defaultText: string; rules: RuleDraft[] },
    draft: { description: string; defaultText: string; rules: RuleDraft[] },
): EditOperation[] {
    const operations: EditOperation[] = [];
    if (draft.description !== original.description) {
        operations.push({
            op: "set_description",
            target: `variable=${variableId}`,
            ...(draft.description === "" ? {} : { text: draft.description }),
        });
    }
    if (draft.defaultText !== original.defaultText) {
        operations.push({
            op: "set_default",
            variable: variableId,
            value: textToValue(draft.defaultText, type),
        });
    }
    if (!rulesEqual(draft.rules, original.rules)) {
        if (draft.rules.length === original.rules.length) {
            // In-place edits: one update per changed rule, index preserved.
            draft.rules.forEach((rule, index) => {
                const before = original.rules[index] as RuleDraft;
                if (
                    rule.when === before.when &&
                    rule.valueText === before.valueText
                ) {
                    return;
                }
                operations.push({
                    op: "update_rule",
                    variable: variableId,
                    index,
                    when: rule.when,
                    value: textToValue(rule.valueText, type),
                });
            });
        } else {
            // Structure changed: replace the list wholesale. Removes run
            // last-to-first so indexes stay valid as the engine applies
            // them in order.
            for (let i = original.rules.length - 1; i >= 0; i--) {
                operations.push({
                    op: "remove_rule",
                    variable: variableId,
                    index: i,
                });
            }
            for (const rule of draft.rules) {
                operations.push({
                    op: "add_rule",
                    variable: variableId,
                    when: rule.when,
                    value: textToValue(rule.valueText, type),
                });
            }
        }
    }
    return operations;
}

function rulesEqual(a: RuleDraft[], b: RuleDraft[]): boolean {
    return (
        a.length === b.length &&
        a.every(
            (rule, index) =>
                rule.when === b[index]?.when &&
                rule.valueText === b[index]?.valueText,
        )
    );
}

function ruleToDraft(type: string): (rule: RuleModel) => RuleDraft {
    return (rule) => ({
        when: rule.when?.value ?? "",
        valueText: valueToText(rule.value?.value, type),
    });
}

function valueToText(value: unknown, type: string): string {
    if (value === undefined) {
        return type === "bool" ? "false" : "";
    }
    if (type === "string" && typeof value === "string") {
        return value;
    }
    return JSON.stringify(value);
}

function textToValue(text: string, type: string): unknown {
    if (type === "bool") {
        return text === "true";
    }
    if (type === "int") {
        const value = Number(text);
        if (!Number.isInteger(value)) {
            throw new Error(`${text || "(empty)"} is not an integer`);
        }
        return value;
    }
    if (type === "number") {
        const value = Number(text);
        if (!Number.isFinite(value)) {
            throw new Error(`${text || "(empty)"} is not a number`);
        }
        return value;
    }
    if (type === "string") {
        return text;
    }
    try {
        return JSON.parse(text);
    } catch {
        throw new Error(`${text || "(empty)"} is not valid JSON for ${type}`);
    }
}

function defaultRuleValue(type: string): string {
    if (type === "bool") {
        return "true";
    }
    if (type === "int" || type === "number") {
        return "0";
    }
    return "";
}

function summarizeDefault(variable: VariableModel): string {
    const value = variable.resolve?.default?.value;
    if (value === undefined) {
        return "";
    }
    const text = JSON.stringify(value);
    return text.length > 24 ? `${text.slice(0, 24)}…` : text;
}

function replaceAt<T>(items: T[], index: number, item: T): T[] {
    const next = [...items];
    next[index] = item;
    return next;
}

function moveRule(rules: RuleDraft[], from: number, to: number): RuleDraft[] {
    const next = [...rules];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved as RuleDraft);
    return next;
}
