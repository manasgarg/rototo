// Workbench editing v1 (tranche C2) plus the read side (tranche C3): entity
// views from the semantic model, the form-submits-operations editor for
// variables, the raw-text escape hatch with live LSP diagnostics, and the
// execution facet — one chosen context carried across the trace previews
// and the lit-up reference graph. Edits land on a change set's branch;
// without an active change set the workbench is read-only and says why.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
    ApiError,
    closeLspSession,
    createChangeSet,
    fetchContexts,
    listChangeSets,
    listPackageFiles,
    listPackages,
    lspNotifications,
    lspNotify,
    openLspSession,
    readPackage,
    readPackageFile,
    runPreview,
    saveEdit,
    type ChangeSet,
    type ContextInventory,
    type EditOperation,
    type EditResponse,
    type LspServerMessage,
    type MeResponse,
    type PackageDetail,
    type PackageListing,
    type RuleModel,
    type SourceTreeSummary,
    type SynthesizedContext,
    type TraceOutcome,
    type VariableModel,
} from "@/lib/api";
import {
    ContextPicker,
    contextLabel,
    type ChosenContext,
} from "@/components/context-picker";
import {
    CompositionPanel,
    HistoryPanel,
    UpcomingPanel,
    ValidityPanel,
} from "@/components/insight";
import { LitGraph } from "@/components/lit-graph";
import { TracePreview } from "@/components/trace-preview";
import { navigate } from "@/lib/router";

type Banner = { kind: "ok" | "err" | "warn"; text: string };

type View =
    | { kind: "entities" }
    | { kind: "variable"; id: string }
    | { kind: "file"; path: string }
    | { kind: "history" };

export function WorkbenchPage({
    me,
    treeId,
    initialView,
}: {
    me: MeResponse;
    treeId: string;
    initialView?: "entities" | "history";
}) {
    const tree = me.capabilities?.sourceTrees.find(
        (candidate) => candidate.id === treeId,
    );
    const [changeSets, setChangeSets] = useState<ChangeSet[]>([]);
    const [activeId, setActiveId] = useState<string | null>(null);
    const [listing, setListing] = useState<PackageListing | null>(null);
    const [selectedPackage, setSelectedPackage] = useState<string | null>(null);
    const [detail, setDetail] = useState<PackageDetail | null>(null);
    const [view, setView] = useState<View>({
        kind: initialView ?? "entities",
    });
    const [banner, setBanner] = useState<Banner | null>(null);
    // The read side's execution facet: one chosen context, carried across
    // the previews and the graph; and an optional historical pin, which
    // makes the whole workbench a read-only view of that instant.
    const [chosen, setChosen] = useState<ChosenContext>({ kind: "none" });
    const [inventory, setInventory] = useState<ContextInventory | null>(null);
    const [outcomes, setOutcomes] = useState<Map<string, TraceOutcome> | null>(
        null,
    );
    const [historicalPin, setHistoricalPin] = useState<string | null>(null);

    const active = changeSets.find((entry) => entry.id === activeId) ?? null;
    const editable =
        historicalPin === null &&
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

    const pin = historicalPin ?? listing?.pin ?? null;
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

    // The context inventory follows the pin: samples change with the
    // package, synthesized cases change with the rules.
    useEffect(() => {
        if (pin === null || selectedPackage === null) {
            return;
        }
        let stale = false;
        fetchContexts(treeId, selectedPackage, pin).then(
            (response) => {
                if (!stale) {
                    setInventory(response);
                }
            },
            () => {
                if (!stale) {
                    setInventory(null);
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, selectedPackage, pin]);

    // The chosen context resolves the whole package: one lenient batch
    // feeds every preview and the lit-up graph.
    useEffect(() => {
        if (chosen.kind === "none" || pin === null || selectedPackage === null) {
            setOutcomes(null);
            return;
        }
        let stale = false;
        runPreview(treeId, selectedPackage, pin, chosen.context).then(
            (response) => {
                if (!stale) {
                    setOutcomes(
                        new Map(
                            response.outcomes.map((outcome) => [
                                outcome.id,
                                outcome,
                            ]),
                        ),
                    );
                }
            },
            (error: Error) => {
                if (!stale) {
                    setOutcomes(null);
                    setBanner({
                        kind: "warn",
                        text: `The ${contextLabel(chosen)} was refused: ${error.message}`,
                    });
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, selectedPackage, pin, chosen]);

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
                        {historicalPin !== null
                            ? `viewing ${historicalPin.slice(0, 10)} (historical)`
                            : listing === null
                              ? "Resolving…"
                              : `${listing.ref} @ ${listing.pin.slice(0, 10)}`}
                    </p>
                </div>
                <span className="action-row">
                    <button
                        className="btn btn-ghost btn-sm"
                        onClick={() =>
                            setView(
                                view.kind === "history"
                                    ? { kind: "entities" }
                                    : { kind: "history" },
                            )
                        }
                    >
                        {view.kind === "history" ? "Model" : "History"}
                    </button>
                    <button
                        className="btn btn-secondary btn-sm"
                        onClick={() => navigate(`/trees/${treeId}/changes`)}
                    >
                        Change sets
                    </button>
                </span>
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

            <ContextPicker
                inventory={inventory}
                chosen={chosen}
                onChange={setChosen}
            />

            {historicalPin !== null ? (
                <div className="banner banner-info">
                    Viewing the package as it was at{" "}
                    <span className="mono">{historicalPin.slice(0, 10)}</span>;
                    editing is off.{" "}
                    <button
                        className="btn btn-ghost btn-sm"
                        onClick={() => setHistoricalPin(null)}
                    >
                        Back to now
                    </button>
                </div>
            ) : null}

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

            {view.kind === "history" ? (
                selectedPackage !== null ? (
                    <HistoryPanel
                        treeId={treeId}
                        packagePath={selectedPackage}
                        viewingPin={historicalPin}
                        onViewPin={(candidate) => {
                            setHistoricalPin(candidate);
                            setBanner(null);
                        }}
                    />
                ) : (
                    <p className="muted">Loading…</p>
                )
            ) : detail === null ? (
                <p className="muted">Loading package…</p>
            ) : view.kind === "variable" ? (
                <VariablePanel
                    key={`${view.id}@${detail.pin}`}
                    detail={detail}
                    variableId={view.id}
                    editable={editable}
                    changeSet={active}
                    chosen={chosen}
                    outcome={outcomes?.get(view.id) ?? null}
                    synthesized={inventory?.synthesized ?? []}
                    onUseContext={setChosen}
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
                    refName={ref}
                    outcomes={outcomes}
                    onOpenVariable={(id) => setView({ kind: "variable", id })}
                    onOpenFile={(path) => setView({ kind: "file", path })}
                    onOpenPackage={(path) => {
                        setSelectedPackage(path);
                        setView({ kind: "entities" });
                    }}
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
    refName,
    outcomes,
    onOpenVariable,
    onOpenFile,
    onOpenPackage,
}: {
    treeId: string;
    detail: PackageDetail;
    refName: string | undefined;
    outcomes: Map<string, TraceOutcome> | null;
    onOpenVariable: (id: string) => void;
    onOpenFile: (path: string) => void;
    onOpenPackage: (path: string) => void;
}) {
    const model = detail.model;
    return (
        <>
            {model.variables.length > 1 ? (
                <>
                    <div className="section-header-text">
                        <h2>
                            {outcomes === null
                                ? "Reference graph"
                                : "What this package does"}
                        </h2>
                        <p className="hint">
                            {outcomes === null
                                ? "Structure only; pick a context to light it up."
                                : "Every variable resolved under the chosen context; bright paths fired, dim paths never ran."}
                        </p>
                    </div>
                    <LitGraph
                        model={model}
                        outcomes={outcomes}
                        onOpenVariable={onOpenVariable}
                    />
                </>
            ) : null}

            <UpcomingPanel
                treeId={treeId}
                packagePath={detail.path}
                pin={detail.pin}
            />

            <div className="section-header-text">
                <h2>Variables</h2>
            </div>
            <div className="row-list">
                {model.variables.map((variable) => {
                    const outcome = outcomes?.get(variable.id);
                    return (
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
                                {outcome === undefined ? (
                                    summarizeDefault(variable)
                                ) : outcome.error !== undefined ? (
                                    <span
                                        className="pill pill-warn"
                                        title={outcome.error}
                                    >
                                        cannot resolve
                                    </span>
                                ) : (
                                    <span className="pill pill-sea">
                                        {clipValue(
                                            outcome.trace?.resolution.value,
                                        )}
                                    </span>
                                )}
                            </span>
                        </button>
                    );
                })}
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

            <ValidityPanel diagnostics={detail.lint.diagnostics} />

            <CompositionPanel
                treeId={treeId}
                refName={refName}
                onOpenPackage={onOpenPackage}
            />

            <FileList treeId={treeId} detail={detail} onOpenFile={onOpenFile} />
        </>
    );
}

function clipValue(value: unknown): string {
    const text = JSON.stringify(value) ?? "";
    return text.length > 20 ? `${text.slice(0, 20)}…` : text;
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
    chosen,
    outcome,
    synthesized,
    onUseContext,
    onBack,
    onSaved,
    onError,
}: {
    detail: PackageDetail;
    variableId: string;
    editable: boolean;
    changeSet: ChangeSet | null;
    chosen: ChosenContext;
    outcome: TraceOutcome | null;
    synthesized: SynthesizedContext[];
    onUseContext: (chosen: ChosenContext) => void;
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

    // Promote a synthesized boundary context to a real sample, in the same
    // change set: the sample corpus grows as a side effect of editing.
    const promote = (entry: SynthesizedContext) => {
        if (changeSet === null) {
            return;
        }
        const contextId = detail.model.evaluationContexts[0]?.id;
        if (contextId === undefined) {
            onError(
                new Error(
                    "the package declares no evaluation context to hold the sample",
                ),
            );
            return;
        }
        saveEdit(changeSet.id, {
            packagePath: detail.path,
            expectedPin: detail.pin,
            operations: [
                {
                    op: "create_sample",
                    context: contextId,
                    key: sampleKey(entry),
                    content: entry.context,
                },
            ],
            summary: `Add sample for ${variableId}`,
        }).then(onSaved, onError);
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

            <TracePreview
                variableId={variableId}
                chosen={chosen}
                outcome={outcome}
                synthesized={synthesized}
                canPromote={editable}
                onUseContext={onUseContext}
                onPromote={promote}
            />

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
    // Live diagnostics from the LSP bridge track the unsaved buffer; until
    // the first publication arrives the staged lint report stands in.
    const [live, setLive] = useState<
        { severity: string; rule?: string; message: string }[] | null
    >(null);
    const [sessionId, setSessionId] = useState<string | null>(null);
    const opened = useRef(false);
    const version = useRef(1);

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

    // One editing session per open file; a bridge failure just means the
    // static lint diagnostics stand.
    useEffect(() => {
        let stale = false;
        let id: string | null = null;
        openLspSession(treeId, detail.path, detail.pin).then(
            (response) => {
                if (stale) {
                    void closeLspSession(response.session).catch(() => {});
                    return;
                }
                id = response.session;
                setSessionId(id);
            },
            () => {},
        );
        return () => {
            stale = true;
            if (id !== null) {
                void closeLspSession(id).catch(() => {});
            }
        };
    }, [treeId, detail.path, detail.pin]);

    // The buffer rides as an LSP overlay: didOpen once, then debounced
    // full-document didChange while typing.
    useEffect(() => {
        if (sessionId === null || content === null) {
            return;
        }
        if (!opened.current) {
            opened.current = true;
            void lspNotify(sessionId, "textDocument/didOpen", {
                textDocument: {
                    uri: file,
                    languageId: "toml",
                    version: version.current,
                    text: content,
                },
            }).catch(() => {});
            return;
        }
        const handle = setTimeout(() => {
            version.current += 1;
            void lspNotify(sessionId, "textDocument/didChange", {
                textDocument: { uri: file, version: version.current },
                contentChanges: [{ text: content }],
            }).catch(() => {});
        }, 300);
        return () => clearTimeout(handle);
    }, [sessionId, content, file]);

    // Diagnostics arrive from the server's debounced build; poll and keep
    // the latest publication for this file.
    useEffect(() => {
        if (sessionId === null) {
            return;
        }
        const interval = setInterval(() => {
            lspNotifications(sessionId).then((response) => {
                for (const message of response.notifications) {
                    if (
                        message.method ===
                            "textDocument/publishDiagnostics" &&
                        message.params?.uri === file
                    ) {
                        setLive(
                            (message.params.diagnostics ?? []).map(
                                (diagnostic) => ({
                                    severity:
                                        diagnostic.severity === 1
                                            ? "error"
                                            : "warning",
                                    rule: diagnostic.code,
                                    message: diagnostic.message,
                                }),
                            ),
                        );
                    }
                }
            }, () => {});
        }, 500);
        return () => clearInterval(interval);
    }, [sessionId, file]);

    const diagnostics =
        live ??
        detail.lint.diagnostics.filter(
            (diagnostic) => diagnostic.location?.path === file,
        );

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
                        Raw text
                        {live !== null
                            ? "; diagnostics track the buffer as you type."
                            : ": lint judges the result after the save."}
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
                            {diagnostic.rule !== undefined ? (
                                <span className="mono">
                                    {diagnostic.rule}{" "}
                                </span>
                            ) : null}
                            {diagnostic.message}
                        </p>
                    ))}
                </div>
            ) : live !== null ? (
                <p className="hint">No findings in the open buffer.</p>
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

// Sample ids are lowercase snake_case like every rototo id; the case id
// arrives kebab-flavored from the fixtures machinery.
function sampleKey(entry: SynthesizedContext): string {
    return `${entry.target.id}_${entry.caseId}`
        .toLowerCase()
        .replace(/[^a-z0-9_]+/g, "_")
        .replace(/_+/g, "_")
        .replace(/^_|_$/g, "");
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
