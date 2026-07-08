// Workbench editing v1 (tranche C2) plus the read side (tranche C3): entity
// views from the semantic model, the form-submits-operations editor for
// variables, the raw-text escape hatch with live LSP diagnostics, and the
// execution facet — one chosen context carried across the trace previews
// and the lit-up reference graph. The URL owns what is being looked at
// (the package view is an address in the addressing grammar, lib/router.ts)
// and how (change set, pin, context ride the query); this page renders and
// navigates, it keeps no private route state. Edits land on a change set's
// branch; without an active change set the workbench is read-only and says
// why.

import {
    useCallback,
    useEffect,
    useMemo,
    useRef,
    useState,
    type ReactNode,
} from "react";

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
    type SemanticModel,
    type SynthesizedContext,
    type TraceOutcome,
    type VariableModel,
} from "@/lib/api";
import {
    ContextPicker,
    contextLabel,
    syntheticLabel,
    type ChosenContext,
} from "@/components/context-picker";
import {
    CompositionPanel,
    FleetPanel,
    HistoryPanel,
    UpcomingPanel,
    ValidityPanel,
} from "@/components/insight";
import { LitGraph } from "@/components/lit-graph";
import { TracePreview } from "@/components/trace-preview";
import {
    CLASS_LABELS,
    formatAddress,
    isCollective,
    navigate,
    packageUrl,
    type AddressStep,
    type PackageView,
    type ViewState,
} from "@/lib/router";

type Banner = { kind: "ok" | "err" | "warn"; text: string };

export function WorkbenchPage({
    me,
    treeId,
    packagePath,
    view,
    state,
}: {
    me: MeResponse;
    treeId: string;
    packagePath: string;
    view: PackageView;
    state: ViewState;
}) {
    const tree = me.capabilities?.sourceTrees.find(
        (candidate) => candidate.id === treeId,
    );
    const [changeSets, setChangeSets] = useState<ChangeSet[]>([]);
    const [listing, setListing] = useState<PackageListing | null>(null);
    const [detail, setDetail] = useState<PackageDetail | null>(null);
    const [banner, setBanner] = useState<Banner | null>(null);
    // The read side's execution facet: the chosen context rides the URL as
    // `ctx` so it survives navigation and sharing; only an ad-hoc JSON
    // context stays session-local, because it has no name to link to.
    const [adhoc, setAdhoc] = useState<Record<string, unknown> | null>(null);
    const [inventory, setInventory] = useState<ContextInventory | null>(null);
    const [outcomes, setOutcomes] = useState<Map<string, TraceOutcome> | null>(
        null,
    );

    const active =
        changeSets.find((entry) => entry.id === state.changeSetId) ?? null;
    const editable =
        state.pin === null &&
        active !== null &&
        (active.state === "draft" || active.state === "proposed");

    // Stay on the current view, or move to another one, without losing the
    // query state the URL carries.
    const go = useCallback(
        (next: PackageView, patch?: Partial<ViewState>) => {
            navigate(
                packageUrl(treeId, packagePath, next, {
                    ...state,
                    ...patch,
                }),
            );
        },
        [treeId, packagePath, state],
    );

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
                if (!stale) {
                    setListing(response);
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
    }, [treeId, ref]);

    const pin = state.pin ?? listing?.pin ?? null;
    useEffect(() => {
        if (pin === null) {
            return;
        }
        let stale = false;
        readPackage(treeId, packagePath, pin).then(
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
    }, [treeId, packagePath, pin]);

    // The context inventory follows the pin: samples change with the
    // package, synthesized cases change with the rules.
    useEffect(() => {
        if (pin === null) {
            return;
        }
        let stale = false;
        fetchContexts(treeId, packagePath, pin).then(
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
    }, [treeId, packagePath, pin]);

    // The chosen context, rehydrated from the URL against the inventory.
    const chosen = useMemo<ChosenContext>(() => {
        if (adhoc !== null) {
            return { kind: "adhoc", context: adhoc };
        }
        const param = state.context;
        if (param === null || inventory === null) {
            return { kind: "none" };
        }
        if (param.startsWith("sample:")) {
            const key = param.slice("sample:".length);
            const sample = inventory.samples.find(
                (candidate) => candidate.key === key,
            );
            return sample?.context != null
                ? { kind: "sample", key, context: sample.context }
                : { kind: "none" };
        }
        if (param.startsWith("synthetic:")) {
            const label = param.slice("synthetic:".length);
            const entry = inventory.synthesized.find(
                (candidate) => syntheticLabel(candidate) === label,
            );
            return entry !== undefined
                ? { kind: "synthetic", label, context: entry.context }
                : { kind: "none" };
        }
        return { kind: "none" };
    }, [adhoc, state.context, inventory]);

    const chooseContext = useCallback(
        (next: ChosenContext) => {
            if (next.kind === "adhoc") {
                setAdhoc(next.context);
                return;
            }
            setAdhoc(null);
            go(view, {
                context:
                    next.kind === "sample"
                        ? `sample:${next.key}`
                        : next.kind === "synthetic"
                          ? `synthetic:${next.label}`
                          : null,
            });
        },
        [go, view],
    );

    // The chosen context resolves the whole package: one lenient batch
    // feeds every preview and the lit-up graph.
    useEffect(() => {
        if (chosen.kind === "none" || pin === null) {
            setOutcomes(null);
            return;
        }
        let stale = false;
        runPreview(treeId, packagePath, pin, chosen.context).then(
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
    }, [treeId, packagePath, pin, chosen]);

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
    const knownPackage =
        listing === null ||
        listing.packages.some((entry) => entry.path === packagePath);

    return (
        <div className="section">
            <div className="section-header">
                <div className="section-header-text">
                    <h1 className={packagePath === "." ? undefined : "mono"}>
                        {packagePath === "." ? treeName : packagePath}
                    </h1>
                    <p className="hint">
                        {state.pin !== null
                            ? `viewing ${state.pin.slice(0, 10)} (historical)`
                            : listing === null
                              ? "Resolving…"
                              : `${listing.ref} @ ${listing.pin.slice(0, 10)}`}
                    </p>
                </div>
            </div>

            <EditingStrip
                treeId={treeId}
                canPropose={tree.capabilities.propose}
                changeSets={changeSets}
                active={active}
                onSelect={(id) => {
                    setBanner(null);
                    go(view, { changeSetId: id });
                }}
                onCreated={(changeSet) => {
                    setChangeSets((current) => [changeSet, ...current]);
                    setBanner(null);
                    go(view, { changeSetId: changeSet.id });
                }}
                onError={saveFailed}
            />

            <ContextPicker
                inventory={inventory}
                chosen={chosen}
                onChange={chooseContext}
            />

            {state.pin !== null ? (
                <div className="banner banner-info">
                    Viewing the package as it was at{" "}
                    <span className="mono">{state.pin.slice(0, 10)}</span>;
                    editing is off.{" "}
                    <button
                        className="btn btn-ghost btn-sm"
                        onClick={() => go(view, { pin: null })}
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

            {!knownPackage ? (
                <div className="card">
                    <h2>No such package</h2>
                    <p className="hint">
                        Nothing at <span className="mono">{packagePath}</span>{" "}
                        on this ref; pick a package from the sidebar.
                    </p>
                </div>
            ) : view.kind === "history" ? (
                <HistoryPanel
                    treeId={treeId}
                    packagePath={packagePath}
                    viewingPin={state.pin}
                    onViewPin={(candidate) => {
                        setBanner(null);
                        go(view, { pin: candidate });
                    }}
                />
            ) : detail === null ? (
                <p className="muted">Loading package…</p>
            ) : view.kind === "files" ? (
                view.file === null ? (
                    <FileList
                        treeId={treeId}
                        detail={detail}
                        onOpenFile={(path) => go({ kind: "files", file: path })}
                    />
                ) : (
                    <FilePanel
                        key={`${view.file}@${detail.pin}`}
                        treeId={treeId}
                        detail={detail}
                        file={view.file}
                        editable={editable}
                        changeSet={active}
                        onBack={() => go({ kind: "files", file: null })}
                        onSaved={afterSave}
                        onError={saveFailed}
                    />
                )
            ) : view.kind === "address" ? (
                <AddressView
                    treeId={treeId}
                    detail={detail}
                    steps={view.steps}
                    go={go}
                    editable={editable}
                    changeSet={active}
                    chosen={chosen}
                    outcomes={outcomes}
                    inventory={inventory}
                    onUseContext={chooseContext}
                    onSaved={afterSave}
                    onError={saveFailed}
                />
            ) : (
                <EntityLists
                    treeId={treeId}
                    detail={detail}
                    refName={ref}
                    outcomes={outcomes}
                    hrefView={(next) =>
                        `#${packageUrl(treeId, packagePath, next, state)}`
                    }
                    onOpenVariable={(id) =>
                        go({
                            kind: "address",
                            steps: [{ class: "variable", id }],
                        })
                    }
                    onOpenFile={(path) => go({ kind: "files", file: path })}
                    onOpenPackage={(path) =>
                        navigate(
                            packageUrl(
                                treeId,
                                path,
                                { kind: "overview" },
                                { ...state, context: null },
                            ),
                        )
                    }
                />
            )}
        </div>
    );
}

// The editing context: which change set commits accumulate on. Viewing the
// default branch is read-only by design; the branch is the durable draft.
// The domain lens reuses this strip so "which change set am I on" looks the
// same everywhere.
export function EditingStrip({
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
    hrefView,
    onOpenVariable,
    onOpenFile,
    onOpenPackage,
}: {
    treeId: string;
    detail: PackageDetail;
    refName: string | undefined;
    outcomes: Map<string, TraceOutcome> | null;
    hrefView: (view: PackageView) => string;
    onOpenVariable: (id: string) => void;
    onOpenFile: (path: string) => void;
    onOpenPackage: (path: string) => void;
}) {
    const model = detail.model;
    const entityHref = (className: string, id: string): string =>
        hrefView({ kind: "address", steps: [{ class: className, id }] });
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
            <VariableRows
                variables={model.variables}
                outcomes={outcomes}
                onOpen={onOpenVariable}
            />

            <Inventory
                title="Catalogs"
                items={model.catalogs.map((catalog) => ({
                    label: `${catalog.id} (${
                        model.catalogEntries.filter(
                            (entry) => entry.catalog === catalog.id,
                        ).length
                    } entries)`,
                    href: entityHref("catalog", catalog.id),
                }))}
            />
            <Inventory
                title="Lists"
                items={model.lists.map((entry) => ({
                    label: entry.id,
                    href: entityHref("list", entry.id),
                }))}
            />
            <Inventory
                title="Evaluation contexts"
                items={model.evaluationContexts.map((entry) => ({
                    label: entry.id,
                    href: entityHref("evaluation-context", entry.id),
                }))}
            />

            <ValidityPanel diagnostics={detail.lint.diagnostics} />

            <CompositionPanel
                treeId={treeId}
                refName={refName}
                onOpenPackage={onOpenPackage}
            />

            <FleetPanel
                treeId={treeId}
                packagePath={detail.path}
                pin={detail.pin}
            />

            <FileList treeId={treeId} detail={detail} onOpenFile={onOpenFile} />
        </>
    );
}

function clipValue(value: unknown): string {
    const text = JSON.stringify(value) ?? "";
    return text.length > 20 ? `${text.slice(0, 20)}…` : text;
}

function Inventory({
    title,
    items,
}: {
    title: string;
    items: { label: string; href: string }[];
}) {
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
                    <a
                        className="pill pill-neutral mono"
                        key={item.label}
                        href={item.href}
                    >
                        {item.label}
                    </a>
                ))}
            </div>
        </>
    );
}

function VariableRows({
    variables,
    outcomes,
    onOpen,
}: {
    variables: VariableModel[];
    outcomes: Map<string, TraceOutcome> | null;
    onOpen: (id: string) => void;
}) {
    return (
        <div className="row-list">
            {variables.map((variable) => {
                const outcome = outcomes?.get(variable.id);
                return (
                    <button
                        className="row"
                        key={variable.id}
                        onClick={() => onOpen(variable.id)}
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
                                    {clipValue(outcome.trace?.resolution.value)}
                                </span>
                            )}
                        </span>
                    </button>
                );
            })}
        </div>
    );
}

// --- the address tail, rendered ---

// An address names an entity, a collective, or a namespace subtree
// (design/addressing.md). Entities with structured editors get them;
// entity kinds without one open as their defining file, so every address
// in the package resolves to something honest.
function AddressView({
    treeId,
    detail,
    steps,
    go,
    editable,
    changeSet,
    chosen,
    outcomes,
    inventory,
    onUseContext,
    onSaved,
    onError,
}: {
    treeId: string;
    detail: PackageDetail;
    steps: AddressStep[];
    go: (view: PackageView, patch?: Partial<ViewState>) => void;
    editable: boolean;
    changeSet: ChangeSet | null;
    chosen: ChosenContext;
    outcomes: Map<string, TraceOutcome> | null;
    inventory: ContextInventory | null;
    onUseContext: (chosen: ChosenContext) => void;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
}) {
    const model = detail.model;
    const head = steps[0] as AddressStep;
    const child = steps[1];
    const openAddress = (next: AddressStep[]) =>
        go({ kind: "address", steps: next });
    const file = (path: string, onBack: () => void) => (
        <FilePanel
            key={`${path}@${detail.pin}`}
            treeId={treeId}
            detail={detail}
            file={path}
            editable={editable}
            changeSet={changeSet}
            onBack={onBack}
            onSaved={onSaved}
            onError={onError}
        />
    );

    if (head.class === "variable") {
        if (isCollective(head)) {
            const variables = model.variables.filter((variable) =>
                variable.id.startsWith(head.id),
            );
            return (
                <CollectionPage
                    title={CLASS_LABELS["variable"] as string}
                    prefix={head.id}
                    count={variables.length}
                    empty="No variables in this package yet."
                >
                    <VariableRows
                        variables={variables}
                        outcomes={outcomes}
                        onOpen={(id) =>
                            openAddress([{ class: "variable", id }])
                        }
                    />
                </CollectionPage>
            );
        }
        return (
            <VariablePanel
                key={`${head.id}@${detail.pin}`}
                detail={detail}
                variableId={head.id}
                editable={editable}
                changeSet={changeSet}
                chosen={chosen}
                outcome={outcomes?.get(head.id) ?? null}
                synthesized={inventory?.synthesized ?? []}
                onUseContext={onUseContext}
                onBack={() => openAddress([{ class: "variable", id: "" }])}
                onSaved={onSaved}
                onError={onError}
            />
        );
    }
    if (head.class === "catalog") {
        if (isCollective(head)) {
            const catalogs = model.catalogs.filter((catalog) =>
                catalog.id.startsWith(head.id),
            );
            return (
                <CollectionPage
                    title={CLASS_LABELS["catalog"] as string}
                    prefix={head.id}
                    count={catalogs.length}
                    empty="No catalogs in this package yet."
                >
                    <div className="row-list">
                        {catalogs.map((catalog) => {
                            const entries = model.catalogEntries.filter(
                                (entry) => entry.catalog === catalog.id,
                            ).length;
                            return (
                                <button
                                    className="row"
                                    key={catalog.id}
                                    onClick={() =>
                                        openAddress([
                                            {
                                                class: "catalog",
                                                id: catalog.id,
                                            },
                                        ])
                                    }
                                >
                                    <span className="row-text">
                                        <span className="row-title mono">
                                            {catalog.id}
                                        </span>
                                        <span className="row-sub">
                                            {entries} entr
                                            {entries === 1 ? "y" : "ies"} ·{" "}
                                            {catalog.path}
                                        </span>
                                    </span>
                                </button>
                            );
                        })}
                    </div>
                </CollectionPage>
            );
        }
        if (child !== undefined && !isCollective(child)) {
            return file(`data/catalogs/${head.id}/${child.id}.toml`, () =>
                openAddress([{ class: "catalog", id: head.id }]),
            );
        }
        return (
            <CatalogPanel
                model={model}
                catalogId={head.id}
                entryPrefix={child?.id ?? ""}
                onOpenEntry={(key) =>
                    openAddress([
                        { class: "catalog", id: head.id },
                        { class: "entry", id: key },
                    ])
                }
                onOpenSchema={(path) => go({ kind: "files", file: path })}
                onBack={() => openAddress([{ class: "catalog", id: "" }])}
            />
        );
    }
    if (head.class === "list") {
        if (isCollective(head)) {
            const lists = model.lists.filter((list) =>
                list.id.startsWith(head.id),
            );
            return (
                <CollectionPage
                    title={CLASS_LABELS["list"] as string}
                    prefix={head.id}
                    count={lists.length}
                    empty="No lists in this package yet."
                >
                    <div className="row-list">
                        {lists.map((list) => (
                            <button
                                className="row"
                                key={list.id}
                                onClick={() =>
                                    openAddress([
                                        { class: "list", id: list.id },
                                    ])
                                }
                            >
                                <span className="row-text">
                                    <span className="row-title mono">
                                        {list.id}
                                    </span>
                                </span>
                            </button>
                        ))}
                    </div>
                </CollectionPage>
            );
        }
        return file(`lists/${head.id}.toml`, () =>
            openAddress([{ class: "list", id: "" }]),
        );
    }
    if (head.class === "evaluation-context") {
        if (isCollective(head)) {
            const contexts = model.evaluationContexts.filter((context) =>
                context.id.startsWith(head.id),
            );
            return (
                <CollectionPage
                    title={CLASS_LABELS["evaluation-context"] as string}
                    prefix={head.id}
                    count={contexts.length}
                    empty="No evaluation contexts in this package yet."
                >
                    <div className="row-list">
                        {contexts.map((context) => (
                            <button
                                className="row"
                                key={context.id}
                                onClick={() =>
                                    openAddress([
                                        {
                                            class: "evaluation-context",
                                            id: context.id,
                                        },
                                    ])
                                }
                            >
                                <span className="row-text">
                                    <span className="row-title mono">
                                        {context.id}
                                    </span>
                                    <span className="row-sub">
                                        {context.path}
                                    </span>
                                </span>
                            </button>
                        ))}
                    </div>
                </CollectionPage>
            );
        }
        if (child !== undefined && !isCollective(child)) {
            return file(
                `model/context/${head.id}-samples/${child.id}.json`,
                () =>
                    openAddress([{ class: "evaluation-context", id: head.id }]),
            );
        }
        return (
            <ContextDetailPanel
                model={model}
                contextId={head.id}
                inventory={inventory}
                onOpenSample={(key) =>
                    openAddress([
                        { class: "evaluation-context", id: head.id },
                        { class: "sample", id: key },
                    ])
                }
                onOpenSchema={(path) => go({ kind: "files", file: path })}
                onBack={() =>
                    openAddress([{ class: "evaluation-context", id: "" }])
                }
            />
        );
    }
    if (head.class === "manifest") {
        return file("rototo-package.toml", () => go({ kind: "overview" }));
    }
    if (head.class === "governance") {
        return file("governance.toml", () => go({ kind: "overview" }));
    }
    if (head.class === "layer" && !isCollective(head)) {
        return file(`layers/${head.id}.toml`, () => go({ kind: "overview" }));
    }
    if (head.class === "linter" && !isCollective(head)) {
        return file(`lint/${head.id}.lua`, () => go({ kind: "overview" }));
    }
    return (
        <div className="card">
            <h2>Nothing at this address</h2>
            <p className="hint">
                <span className="mono">{formatAddress(steps)}</span> names
                nothing this console can show.
            </p>
        </div>
    );
}

// A collection page: one entity class, optionally narrowed to a namespace
// subtree by the address's trailing-slash prefix.
function CollectionPage({
    title,
    prefix,
    count,
    empty,
    children,
}: {
    title: string;
    prefix: string;
    count: number;
    empty: string;
    children: ReactNode;
}) {
    return (
        <>
            <div className="section-header-text">
                <h2>
                    {title}
                    {prefix !== "" ? (
                        <span className="mono"> · {prefix}</span>
                    ) : null}
                </h2>
            </div>
            {count === 0 ? <p className="hint">{empty}</p> : children}
        </>
    );
}

function CatalogPanel({
    model,
    catalogId,
    entryPrefix,
    onOpenEntry,
    onOpenSchema,
    onBack,
}: {
    model: SemanticModel;
    catalogId: string;
    entryPrefix: string;
    onOpenEntry: (key: string) => void;
    onOpenSchema: (path: string) => void;
    onBack: () => void;
}) {
    const catalog = model.catalogs.find(
        (candidate) => candidate.id === catalogId,
    );
    if (catalog === undefined) {
        return (
            <div className="card">
                <p className="hint">No such catalog at this pin.</p>
                <button className="btn btn-ghost btn-sm" onClick={onBack}>
                    Back
                </button>
            </div>
        );
    }
    const entries = model.catalogEntries.filter(
        (entry) =>
            entry.catalog === catalogId && entry.key.startsWith(entryPrefix),
    );
    return (
        <div className="card card-stretch">
            <div className="card-head">
                <div className="card-head-text">
                    <h2 className="mono">{catalogId}</h2>
                    <p className="hint">
                        {entries.length} entr
                        {entries.length === 1 ? "y" : "ies"} · schema{" "}
                        {catalog.path}
                    </p>
                </div>
                <span className="action-row">
                    <button
                        className="btn btn-secondary btn-sm"
                        onClick={() => onOpenSchema(catalog.path)}
                    >
                        Schema
                    </button>
                    <button className="btn btn-ghost btn-sm" onClick={onBack}>
                        Back
                    </button>
                </span>
            </div>
            {entries.length === 0 ? (
                <p className="hint">
                    No entries
                    {entryPrefix !== "" ? (
                        <>
                            {" "}
                            under <span className="mono">{entryPrefix}</span>
                        </>
                    ) : null}{" "}
                    yet.
                </p>
            ) : (
                <div className="row-list">
                    {entries.map((entry) => (
                        <button
                            className="row"
                            key={entry.key}
                            onClick={() => onOpenEntry(entry.key)}
                        >
                            <span className="row-text">
                                <span className="row-title mono">
                                    {entry.key}
                                </span>
                            </span>
                        </button>
                    ))}
                </div>
            )}
        </div>
    );
}

function ContextDetailPanel({
    model,
    contextId,
    inventory,
    onOpenSample,
    onOpenSchema,
    onBack,
}: {
    model: SemanticModel;
    contextId: string;
    inventory: ContextInventory | null;
    onOpenSample: (key: string) => void;
    onOpenSchema: (path: string) => void;
    onBack: () => void;
}) {
    const context = model.evaluationContexts.find(
        (candidate) => candidate.id === contextId,
    );
    if (context === undefined) {
        return (
            <div className="card">
                <p className="hint">No such evaluation context at this pin.</p>
                <button className="btn btn-ghost btn-sm" onClick={onBack}>
                    Back
                </button>
            </div>
        );
    }
    const samples = (inventory?.samples ?? []).filter(
        (sample) => sample.evaluationContext === contextId,
    );
    return (
        <div className="card card-stretch">
            <div className="card-head">
                <div className="card-head-text">
                    <h2 className="mono">{contextId}</h2>
                    <p className="hint">Evaluation context · {context.path}</p>
                </div>
                <span className="action-row">
                    <button
                        className="btn btn-secondary btn-sm"
                        onClick={() => onOpenSchema(context.path)}
                    >
                        Schema
                    </button>
                    <button className="btn btn-ghost btn-sm" onClick={onBack}>
                        Back
                    </button>
                </span>
            </div>
            {inventory === null ? (
                <p className="muted">Loading samples…</p>
            ) : samples.length === 0 ? (
                <p className="hint">No samples for this context yet.</p>
            ) : (
                <div className="row-list">
                    {samples.map((sample) => (
                        <button
                            className="row"
                            key={sample.key}
                            onClick={() => onOpenSample(sample.key)}
                        >
                            <span className="row-text">
                                <span className="row-title mono">
                                    {sample.key}
                                </span>
                            </span>
                        </button>
                    ))}
                </div>
            )}
        </div>
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
            lspNotifications(sessionId).then(
                (response) => {
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
                },
                () => {},
            );
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
                                <span className="mono">{diagnostic.rule} </span>
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
