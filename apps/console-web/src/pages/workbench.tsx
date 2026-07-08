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
    type ModelEntityRef,
    type PackageDetail,
    type PackageListing,
    type QueryModel,
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
import { entityLabel, entitySteps } from "@/components/entity-link";
import {
    CompositionPanel,
    FleetPanel,
    HistoryPanel,
    UpcomingPanel,
    ValidityPanel,
} from "@/components/insight";
import { ReferenceGraph } from "@/components/reference-graph";
import { TracePreview } from "@/components/trace-preview";
import { SearchableList } from "@/lib/ui-kit";
import {
    changeSetUrl,
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

    // The same move as an href, for everything rendered as a real link.
    const hrefView = useCallback(
        (next: PackageView): string =>
            `#${packageUrl(treeId, packagePath, next, state)}`,
        [treeId, packagePath, state],
    );
    const hrefEntity = useCallback(
        (steps: AddressStep[]): string => hrefView({ kind: "address", steps }),
        [hrefView],
    );
    const hrefFile = useCallback(
        (path: string): string => hrefView({ kind: "files", file: path }),
        [hrefView],
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

    // The package is a URL scope now, so a package switch arrives as a prop
    // change; clear the previous package's views rather than showing them
    // against the new scope while the fetches run. Pin moves (saves,
    // history) keep the current render to avoid a loading flash.
    useEffect(() => {
        setDetail(null);
        setInventory(null);
    }, [treeId, packagePath]);

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
                    tree={tree}
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
                        changeSet={active}
                        editable={editable}
                        hrefFile={hrefFile}
                        onOpenFile={(path) => go({ kind: "files", file: path })}
                        onSaved={afterSave}
                        onError={saveFailed}
                    />
                ) : (
                    <FilePanel
                        key={`${view.file}@${detail.pin}`}
                        treeId={treeId}
                        detail={detail}
                        file={view.file}
                        editable={editable}
                        changeSet={active}
                        onDeleted={() => go({ kind: "files", file: null })}
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
                    hrefView={hrefView}
                    hrefEntity={hrefEntity}
                    hrefFile={hrefFile}
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
                    hrefView={hrefView}
                    hrefPackage={(path) =>
                        `#${packageUrl(
                            treeId,
                            path,
                            { kind: "overview" },
                            { ...state, context: null },
                        )}`
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
                <a
                    className="row-link mono mode-strip-branch"
                    href={`#${changeSetUrl(treeId, active.id)}`}
                    title="Open this change set"
                >
                    {active.branch}
                </a>
            ) : null}
            {creating ? (
                <form
                    className="inline-form"
                    onSubmit={(event) => {
                        event.preventDefault();
                        const changeSetTitle = title.trim();
                        setCreating(false);
                        setTitle("");
                        createChangeSet(treeId, changeSetTitle).then(
                            onCreated,
                            onError,
                        );
                    }}
                >
                    <input
                        autoFocus
                        className="input"
                        placeholder="What is this change about?"
                        value={title}
                        onChange={(event) => setTitle(event.target.value)}
                    />
                    <button
                        className="btn btn-primary btn-sm"
                        type="submit"
                        disabled={title.trim() === ""}
                    >
                        Start
                    </button>
                    <button
                        className="btn btn-ghost btn-sm"
                        type="button"
                        onClick={() => setCreating(false)}
                    >
                        Cancel
                    </button>
                </form>
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
    hrefPackage,
}: {
    treeId: string;
    detail: PackageDetail;
    refName: string | undefined;
    outcomes: Map<string, TraceOutcome> | null;
    hrefView: (view: PackageView) => string;
    hrefPackage: (path: string) => string;
}) {
    const model = detail.model;
    const entityHref = (className: string, id: string): string =>
        hrefView({ kind: "address", steps: [{ class: className, id }] });
    const hrefEntity = (steps: AddressStep[]): string =>
        hrefView({ kind: "address", steps });
    const hrefFile = (path: string): string =>
        hrefView({ kind: "files", file: path });
    return (
        <>
            {model.variables.length > 0 || model.catalogs.length > 0 ? (
                <>
                    <div className="section-header-text">
                        <h2>
                            {outcomes === null
                                ? "Reference graph"
                                : "What this package does"}
                        </h2>
                        <p className="hint">
                            {outcomes === null
                                ? "Structure only; pick a context to light it up. Hover an entity to preview its definition."
                                : "Every variable resolved under the chosen context; bright paths fired, dim paths never ran."}
                        </p>
                    </div>
                    <div className="card graph-card">
                        <ReferenceGraph
                            key={detail.pin}
                            model={model}
                            outcomes={outcomes}
                            treeId={treeId}
                            packagePath={detail.path}
                            pin={detail.pin}
                            hrefFor={hrefEntity}
                        />
                    </div>
                </>
            ) : null}

            <UpcomingPanel
                treeId={treeId}
                packagePath={detail.path}
                pin={detail.pin}
                hrefEntity={hrefEntity}
            />

            <div className="section-header-text">
                <h2>Variables</h2>
            </div>
            <VariableRows
                variables={model.variables}
                outcomes={outcomes}
                hrefFor={(id) => entityHref("variable", id)}
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

            <ValidityPanel
                diagnostics={detail.lint.diagnostics}
                hrefFile={hrefFile}
            />

            <CompositionPanel
                treeId={treeId}
                refName={refName}
                hrefPackage={hrefPackage}
            />

            <FleetPanel
                treeId={treeId}
                packagePath={detail.path}
                pin={detail.pin}
                hrefPackage={hrefPackage}
                hrefEntity={hrefEntity}
            />

            <FileList treeId={treeId} detail={detail} hrefFile={hrefFile} />
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
    hrefFor,
}: {
    variables: VariableModel[];
    outcomes: Map<string, TraceOutcome> | null;
    hrefFor: (id: string) => string;
}) {
    return (
        <SearchableList
            label="Search variables"
            placeholder="Search variables"
            emptyLabel="No variable matches that search."
            className="row-list"
        >
            {variables.map((variable) => {
                const outcome = outcomes?.get(variable.id);
                return (
                    <a
                        className="row"
                        key={variable.id}
                        href={hrefFor(variable.id)}
                        data-search={`${variable.id} ${variable.declaration.value ?? ""} ${variable.description ?? ""}`}
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
                                <span className="pill pill-sea mono">
                                    {clipValue(outcome.trace?.resolution.value)}
                                </span>
                            )}
                        </span>
                    </a>
                );
            })}
        </SearchableList>
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
    hrefView,
    hrefEntity,
    hrefFile,
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
    hrefView: (view: PackageView) => string;
    hrefEntity: (steps: AddressStep[]) => string;
    hrefFile: (path: string) => string;
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
    const file = (path: string, onBack: () => void, deleteAs?: string) => (
        <FilePanel
            key={`${path}@${detail.pin}`}
            treeId={treeId}
            detail={detail}
            file={path}
            editable={editable}
            changeSet={changeSet}
            deleteAs={deleteAs}
            onDeleted={onBack}
            onBack={onBack}
            onSaved={onSaved}
            onError={onError}
        />
    );
    const creating = { detail, changeSet, onSaved, onError };

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
                    action={
                        editable ? (
                            <NewVariableForm
                                {...creating}
                                onCreated={(id) =>
                                    openAddress([{ class: "variable", id }])
                                }
                            />
                        ) : null
                    }
                >
                    <VariableRows
                        variables={variables}
                        outcomes={outcomes}
                        hrefFor={(id) =>
                            hrefEntity([{ class: "variable", id }])
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
                hrefEntity={hrefEntity}
                hrefFile={hrefFile}
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
                    action={
                        editable ? (
                            <NewCatalogForm
                                {...creating}
                                onCreated={(id) =>
                                    openAddress([{ class: "catalog", id }])
                                }
                            />
                        ) : null
                    }
                >
                    <SearchableList
                        label="Search catalogs"
                        placeholder="Search catalogs"
                        emptyLabel="No catalog matches that search."
                        className="row-list"
                    >
                        {catalogs.map((catalog) => {
                            const entries = model.catalogEntries.filter(
                                (entry) => entry.catalog === catalog.id,
                            ).length;
                            return (
                                <a
                                    className="row"
                                    key={catalog.id}
                                    href={hrefEntity([
                                        { class: "catalog", id: catalog.id },
                                    ])}
                                    data-search={`${catalog.id} ${catalog.path}`}
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
                                </a>
                            );
                        })}
                    </SearchableList>
                </CollectionPage>
            );
        }
        if (child !== undefined && !isCollective(child)) {
            return file(
                `data/catalogs/${head.id}/${child.id}.toml`,
                () => openAddress([{ class: "catalog", id: head.id }]),
                `catalog=${head.id}:entry=${child.id}`,
            );
        }
        return (
            <CatalogPanel
                {...creating}
                model={model}
                catalogId={head.id}
                entryPrefix={child?.id ?? ""}
                editable={editable}
                hrefEntity={hrefEntity}
                hrefFile={hrefFile}
                onOpenEntry={(key) =>
                    openAddress([
                        { class: "catalog", id: head.id },
                        { class: "entry", id: key },
                    ])
                }
                onOpenSchema={(path) => go({ kind: "files", file: path })}
                onBack={() => openAddress([{ class: "catalog", id: "" }])}
                onDeleted={() => openAddress([{ class: "catalog", id: "" }])}
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
                    action={
                        editable ? (
                            <NewListForm
                                {...creating}
                                onCreated={(id) =>
                                    openAddress([{ class: "list", id }])
                                }
                            />
                        ) : null
                    }
                >
                    <SearchableList
                        label="Search lists"
                        placeholder="Search lists"
                        emptyLabel="No list matches that search."
                        className="row-list"
                    >
                        {lists.map((list) => (
                            <a
                                className="row"
                                key={list.id}
                                href={hrefEntity([
                                    { class: "list", id: list.id },
                                ])}
                                data-search={`${list.id} ${list.memberType.value ?? "string"} ${list.description ?? ""}`}
                            >
                                <span className="row-text">
                                    <span className="row-title mono">
                                        {list.id}
                                    </span>
                                    <span className="row-sub">
                                        {list.memberType.value ?? "string"} ·{" "}
                                        {list.members.length} member
                                        {list.members.length === 1 ? "" : "s"}
                                        {list.description !== undefined
                                            ? ` — ${list.description}`
                                            : ""}
                                    </span>
                                </span>
                            </a>
                        ))}
                    </SearchableList>
                </CollectionPage>
            );
        }
        return (
            <ListPanel
                key={`${head.id}@${detail.pin}`}
                {...creating}
                model={model}
                listId={head.id}
                editable={editable}
                hrefEntity={hrefEntity}
                onOpenFile={(path) => go({ kind: "files", file: path })}
                onBack={() => openAddress([{ class: "list", id: "" }])}
                onDeleted={() => openAddress([{ class: "list", id: "" }])}
            />
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
                    action={
                        editable ? (
                            <NewContextForm
                                {...creating}
                                onCreated={(id) =>
                                    openAddress([
                                        { class: "evaluation-context", id },
                                    ])
                                }
                            />
                        ) : null
                    }
                >
                    <SearchableList
                        label="Search evaluation contexts"
                        placeholder="Search evaluation contexts"
                        emptyLabel="No evaluation context matches that search."
                        className="row-list"
                    >
                        {contexts.map((context) => (
                            <a
                                className="row"
                                key={context.id}
                                href={hrefEntity([
                                    {
                                        class: "evaluation-context",
                                        id: context.id,
                                    },
                                ])}
                                data-search={`${context.id} ${context.path}`}
                            >
                                <span className="row-text">
                                    <span className="row-title mono">
                                        {context.id}
                                    </span>
                                    <span className="row-sub">
                                        {context.path}
                                    </span>
                                </span>
                            </a>
                        ))}
                    </SearchableList>
                </CollectionPage>
            );
        }
        if (child !== undefined && !isCollective(child)) {
            return file(
                `model/context/${head.id}-samples/${child.id}.json`,
                () =>
                    openAddress([{ class: "evaluation-context", id: head.id }]),
                `evaluation-context=${head.id}:sample=${child.id}`,
            );
        }
        return (
            <ContextDetailPanel
                {...creating}
                model={model}
                contextId={head.id}
                inventory={inventory}
                editable={editable}
                hrefEntity={hrefEntity}
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
                onDeleted={() =>
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
        return file(
            `layers/${head.id}.toml`,
            () => go({ kind: "overview" }),
            `layer=${head.id}`,
        );
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
    action,
    children,
}: {
    title: string;
    prefix: string;
    count: number;
    empty: string;
    action?: ReactNode;
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
            {action !== undefined && action !== null ? (
                <div className="action-row">{action}</div>
            ) : null}
            {count === 0 ? <p className="hint">{empty}</p> : children}
        </>
    );
}

function CatalogPanel({
    model,
    catalogId,
    entryPrefix,
    editable,
    changeSet,
    detail,
    hrefEntity,
    hrefFile,
    onOpenEntry,
    onOpenSchema,
    onBack,
    onDeleted,
    onSaved,
    onError,
}: {
    model: SemanticModel;
    catalogId: string;
    entryPrefix: string;
    editable: boolean;
    changeSet: ChangeSet | null;
    detail: PackageDetail;
    hrefEntity: (steps: AddressStep[]) => string;
    hrefFile: (path: string) => string;
    onOpenEntry: (key: string) => void;
    onOpenSchema: (path: string) => void;
    onBack: () => void;
    onDeleted: () => void;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
}) {
    const catalog = model.catalogs.find(
        (candidate) => candidate.id === catalogId,
    );
    if (catalog === undefined) {
        return (
            <div className="card">
                <p className="hint">No such catalog at this pin.</p>
                <div className="action-row">
                    <button className="btn btn-ghost btn-sm" onClick={onBack}>
                        Back
                    </button>
                </div>
            </div>
        );
    }
    const entries = model.catalogEntries.filter(
        (entry) =>
            entry.catalog === catalogId && entry.key.startsWith(entryPrefix),
    );
    const allEntries = model.catalogEntries.filter(
        (entry) => entry.catalog === catalogId,
    ).length;
    const inbound = referenceLabels(
        model,
        (to) => to.kind === "catalog" && to.id === catalogId,
    );
    return (
        <div className="card">
            <div className="card-head">
                <div className="card-head-text">
                    <h2 className="mono">{catalogId}</h2>
                    <p className="hint">
                        {entries.length} entr
                        {entries.length === 1 ? "y" : "ies"} · schema{" "}
                        <a
                            className="row-link mono"
                            href={hrefFile(catalog.path)}
                        >
                            {catalog.path}
                        </a>
                    </p>
                </div>
                <span className="action-row">
                    <button
                        className="btn btn-secondary btn-sm"
                        onClick={() => onOpenSchema(catalog.path)}
                    >
                        Schema
                    </button>
                    {editable && changeSet !== null ? (
                        <DeleteButton
                            label="Delete catalog"
                            warning={blastWarning(
                                `the schema and ${allEntries === 1 ? "its 1 entry" : `all ${allEntries} entries`}`,
                                inbound,
                            )}
                            onConfirm={() =>
                                saveEdit(changeSet.id, {
                                    packagePath: detail.path,
                                    expectedPin: detail.pin,
                                    operations: [
                                        {
                                            op: "delete",
                                            target: `catalog=${catalogId}`,
                                        },
                                    ],
                                    summary: `Delete catalog ${catalogId}`,
                                }).then((result) => {
                                    onSaved(result);
                                    onDeleted();
                                }, onError)
                            }
                        />
                    ) : null}
                    <button className="btn btn-ghost btn-sm" onClick={onBack}>
                        Back
                    </button>
                </span>
            </div>
            {editable && changeSet !== null ? (
                <NewIdForm
                    label="New entry"
                    placeholder="entry_key"
                    onSubmit={(key) =>
                        saveEdit(changeSet.id, {
                            packagePath: detail.path,
                            expectedPin: detail.pin,
                            operations: [
                                {
                                    op: "create_entry",
                                    catalog: catalogId,
                                    key,
                                    fields: {},
                                },
                            ],
                            summary: `Create entry ${key} in ${catalogId}`,
                        }).then((result) => {
                            onSaved(result);
                            onOpenEntry(key);
                        }, onError)
                    }
                />
            ) : null}
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
                <SearchableList
                    label="Search entries"
                    placeholder="Search entries"
                    emptyLabel="No entry matches that search."
                    className="row-list"
                >
                    {entries.map((entry) => (
                        <a
                            className="row"
                            key={entry.key}
                            href={hrefEntity([
                                { class: "catalog", id: catalogId },
                                { class: "entry", id: entry.key },
                            ])}
                            data-search={entry.key}
                        >
                            <span className="row-text">
                                <span className="row-title mono">
                                    {entry.key}
                                </span>
                            </span>
                        </a>
                    ))}
                </SearchableList>
            )}
            <ReferencePills
                title="Referenced by"
                entities={model.references
                    .filter(
                        (reference) =>
                            reference.to.kind === "catalog" &&
                            reference.to.id === catalogId,
                    )
                    .map((reference) => reference.from)}
                hrefEntity={hrefEntity}
            />
        </div>
    );
}

// A structured list editor: members change through add_member/remove_member,
// so an inherited list can later compile to update markers instead of a
// whole-file rewrite.
function ListPanel({
    model,
    listId,
    editable,
    changeSet,
    detail,
    hrefEntity,
    onOpenFile,
    onBack,
    onDeleted,
    onSaved,
    onError,
}: {
    model: SemanticModel;
    listId: string;
    editable: boolean;
    changeSet: ChangeSet | null;
    detail: PackageDetail;
    hrefEntity: (steps: AddressStep[]) => string;
    onOpenFile: (path: string) => void;
    onBack: () => void;
    onDeleted: () => void;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
}) {
    const list = model.lists.find((candidate) => candidate.id === listId);
    const [adding, setAdding] = useState("");
    const [saving, setSaving] = useState(false);
    if (list === undefined) {
        return (
            <div className="card">
                <p className="hint">No such list at this pin.</p>
                <div className="action-row">
                    <button className="btn btn-ghost btn-sm" onClick={onBack}>
                        Back
                    </button>
                </div>
            </div>
        );
    }
    const memberType = list.memberType.value ?? "string";
    const path = `lists/${listId}.toml`;
    const canEdit = editable && changeSet !== null;
    const oneOp = (
        operation: EditOperation,
        summary: string,
        after?: () => void,
    ) => {
        if (changeSet === null) {
            return;
        }
        setSaving(true);
        saveEdit(changeSet.id, {
            packagePath: detail.path,
            expectedPin: detail.pin,
            operations: [operation],
            summary,
        })
            .then((result) => {
                onSaved(result);
                after?.();
            }, onError)
            .finally(() => setSaving(false));
    };
    return (
        <div className="card">
            <div className="card-head">
                <div className="card-head-text">
                    <h2 className="mono">{listId}</h2>
                    <p className="hint">
                        list of {memberType} · {list.members.length} member
                        {list.members.length === 1 ? "" : "s"}
                        {list.description !== undefined
                            ? ` — ${list.description}`
                            : ""}
                    </p>
                </div>
                <span className="action-row">
                    <button
                        className="btn btn-secondary btn-sm"
                        onClick={() => onOpenFile(path)}
                    >
                        Raw file
                    </button>
                    {canEdit ? (
                        <DeleteButton
                            label="Delete list"
                            warning={blastWarning(
                                `lists/${listId}.toml`,
                                listReferrers(model, listId),
                            )}
                            onConfirm={() =>
                                oneOp(
                                    { op: "delete", target: `list=${listId}` },
                                    `Delete list ${listId}`,
                                    onDeleted,
                                )
                            }
                        />
                    ) : null}
                    <button className="btn btn-ghost btn-sm" onClick={onBack}>
                        Back
                    </button>
                </span>
            </div>
            <SearchableList
                label="Search members"
                placeholder="Search members"
                emptyLabel="No member matches that search."
                className="row-list"
            >
                {list.members.map((member, index) => (
                    <div
                        className="row"
                        key={index}
                        data-search={String(member.value)}
                    >
                        <span className="row-text">
                            <span className="row-title mono">
                                {memberType === "string"
                                    ? String(member.value)
                                    : JSON.stringify(member.value)}
                            </span>
                        </span>
                        {canEdit ? (
                            <span className="row-side">
                                <button
                                    className="btn btn-icon btn-sm btn-remove"
                                    title="Remove member"
                                    disabled={saving}
                                    onClick={() =>
                                        oneOp(
                                            {
                                                op: "remove_member",
                                                list: listId,
                                                value: member.value,
                                            },
                                            `Remove ${String(member.value)} from ${listId}`,
                                        )
                                    }
                                >
                                    ×
                                </button>
                            </span>
                        ) : null}
                    </div>
                ))}
            </SearchableList>
            {canEdit ? (
                <form
                    className="action-row"
                    onSubmit={(event) => {
                        event.preventDefault();
                        let value: unknown;
                        try {
                            value = textToValue(adding.trim(), memberType);
                        } catch (error) {
                            onError(error);
                            return;
                        }
                        oneOp(
                            { op: "add_member", list: listId, value },
                            `Add ${adding.trim()} to ${listId}`,
                            () => setAdding(""),
                        );
                    }}
                >
                    <input
                        className="input mono"
                        placeholder={`new ${memberType} member`}
                        value={adding}
                        onChange={(event) => setAdding(event.target.value)}
                    />
                    <button
                        className="btn btn-secondary btn-sm"
                        type="submit"
                        disabled={saving || adding.trim() === ""}
                    >
                        Add member
                    </button>
                </form>
            ) : (
                <p className="hint">
                    Start or pick a change set above to edit.
                </p>
            )}
            {listTypedVariables(model, listId).length > 0 ? (
                <div className="reference-links">
                    <span className="label">Typed against this list</span>
                    {listTypedVariables(model, listId).map((id) => (
                        <a
                            key={id}
                            className="pill pill-neutral mono"
                            href={hrefEntity([{ class: "variable", id }])}
                        >
                            {id}
                        </a>
                    ))}
                </div>
            ) : null}
        </div>
    );
}

function ContextDetailPanel({
    model,
    contextId,
    inventory,
    editable,
    changeSet,
    detail,
    hrefEntity,
    onOpenSample,
    onOpenSchema,
    onBack,
    onDeleted,
    onSaved,
    onError,
}: {
    model: SemanticModel;
    contextId: string;
    inventory: ContextInventory | null;
    editable: boolean;
    changeSet: ChangeSet | null;
    detail: PackageDetail;
    hrefEntity: (steps: AddressStep[]) => string;
    onOpenSample: (key: string) => void;
    onOpenSchema: (path: string) => void;
    onBack: () => void;
    onDeleted: () => void;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
}) {
    const context = model.evaluationContexts.find(
        (candidate) => candidate.id === contextId,
    );
    if (context === undefined) {
        return (
            <div className="card">
                <p className="hint">No such evaluation context at this pin.</p>
                <div className="action-row">
                    <button className="btn btn-ghost btn-sm" onClick={onBack}>
                        Back
                    </button>
                </div>
            </div>
        );
    }
    const samples = (inventory?.samples ?? []).filter(
        (sample) => sample.evaluationContext === contextId,
    );
    const canEdit = editable && changeSet !== null;
    return (
        <div className="card">
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
                    {canEdit && changeSet !== null ? (
                        <DeleteButton
                            label="Delete context"
                            warning={blastWarning(
                                `the schema and ${samples.length === 1 ? "its 1 sample" : `all ${samples.length} samples`}`,
                                [],
                            )}
                            onConfirm={() =>
                                saveEdit(changeSet.id, {
                                    packagePath: detail.path,
                                    expectedPin: detail.pin,
                                    operations: [
                                        {
                                            op: "delete",
                                            target: `evaluation-context=${contextId}`,
                                        },
                                    ],
                                    summary: `Delete context ${contextId}`,
                                }).then((result) => {
                                    onSaved(result);
                                    onDeleted();
                                }, onError)
                            }
                        />
                    ) : null}
                    <button className="btn btn-ghost btn-sm" onClick={onBack}>
                        Back
                    </button>
                </span>
            </div>
            {canEdit && changeSet !== null ? (
                <NewIdForm
                    label="New sample"
                    placeholder="sample_key"
                    onSubmit={(key) =>
                        saveEdit(changeSet.id, {
                            packagePath: detail.path,
                            expectedPin: detail.pin,
                            operations: [
                                {
                                    op: "create_sample",
                                    context: contextId,
                                    key,
                                    content:
                                        samples[0]?.context != null
                                            ? samples[0].context
                                            : {},
                                },
                            ],
                            summary: `Create sample ${key} for ${contextId}`,
                        }).then((result) => {
                            onSaved(result);
                            onOpenSample(key);
                        }, onError)
                    }
                />
            ) : null}
            {inventory === null ? (
                <p className="muted">Loading samples…</p>
            ) : samples.length === 0 ? (
                <p className="hint">No samples for this context yet.</p>
            ) : (
                <SearchableList
                    label="Search samples"
                    placeholder="Search samples"
                    emptyLabel="No sample matches that search."
                    className="row-list"
                >
                    {samples.map((sample) => (
                        <a
                            className="row"
                            key={sample.key}
                            href={hrefEntity([
                                {
                                    class: "evaluation-context",
                                    id: contextId,
                                },
                                { class: "sample", id: sample.key },
                            ])}
                            data-search={sample.key}
                        >
                            <span className="row-text">
                                <span className="row-title mono">
                                    {sample.key}
                                </span>
                            </span>
                        </a>
                    ))}
                </SearchableList>
            )}
        </div>
    );
}

function FileList({
    treeId,
    detail,
    changeSet,
    editable,
    hrefFile,
    onOpenFile,
    onSaved,
    onError,
}: {
    treeId: string;
    detail: PackageDetail;
    changeSet?: ChangeSet | null;
    editable?: boolean;
    hrefFile: (path: string) => string;
    onOpenFile?: (path: string) => void;
    onSaved?: (result: EditResponse) => void;
    onError?: (error: unknown) => void;
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
            {editable === true &&
            changeSet != null &&
            onSaved !== undefined &&
            onError !== undefined ? (
                <NewIdForm
                    label="New file"
                    placeholder="path/in/package.toml"
                    onSubmit={(path) =>
                        saveEdit(changeSet.id, {
                            packagePath: detail.path,
                            expectedPin: detail.pin,
                            files: [{ path, content: "" }],
                            summary: `Create ${path}`,
                        }).then((result) => {
                            onSaved(result);
                            onOpenFile?.(path);
                        }, onError)
                    }
                />
            ) : null}
            <SearchableList
                label="Search files"
                placeholder="Search files"
                emptyLabel="No file matches that search."
                className="row-list"
            >
                {(files ?? []).map((file) => (
                    <a
                        className="row"
                        key={file}
                        href={hrefFile(file)}
                        data-search={file}
                    >
                        <span className="row-text">
                            <span className="row-title mono">{file}</span>
                        </span>
                    </a>
                ))}
            </SearchableList>
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
    hrefEntity,
    hrefFile,
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
    hrefEntity: (steps: AddressStep[]) => string;
    hrefFile: (path: string) => string;
    onUseContext: (chosen: ChosenContext) => void;
    onBack: () => void;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
}) {
    const variable = detail.model.variables.find(
        (candidate) => candidate.id === variableId,
    );
    const type = variable?.declaration.value ?? "string";
    const original = useMemo<VariableDraft>(
        () => ({
            description: variable?.description ?? "",
            defaultText: valueToText(variable?.resolve?.default?.value, type),
            rules: (variable?.resolve?.rules ?? []).map(ruleToDraft(type)),
            method: methodOf(variable),
            query: queryToDraft(variable?.resolve?.query),
        }),
        [variable, type],
    );
    const [description, setDescription] = useState(original.description);
    const [defaultText, setDefaultText] = useState(original.defaultText);
    const [rules, setRules] = useState<RuleDraft[]>(original.rules);
    const [method, setMethod] = useState(original.method);
    const [query, setQuery] = useState<QueryDraft>(original.query);
    const [saving, setSaving] = useState(false);

    if (variable === undefined) {
        return (
            <div className="card">
                <p className="hint">No such variable at this pin.</p>
                <div className="action-row">
                    <button className="btn btn-ghost btn-sm" onClick={onBack}>
                        Back
                    </button>
                </div>
            </div>
        );
    }

    const reads = detail.model.references
        .filter(
            (reference) =>
                reference.from.kind === "variable" &&
                reference.from.id === variableId,
        )
        .map((reference) => reference.to);
    const readBy = detail.model.references
        .filter(
            (reference) =>
                reference.to.kind === "variable" &&
                reference.to.id === variableId,
        )
        .map((reference) => reference.from);

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
                method,
                query,
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
        <div className="card">
            <div className="card-head">
                <div className="card-head-text">
                    <h2 className="mono">{variable.id}</h2>
                    <p className="hint">
                        <TypeLabel type={type} hrefEntity={hrefEntity} /> ·{" "}
                        <a
                            className="row-link mono"
                            href={hrefFile(variable.location.path)}
                        >
                            {variable.location.path}
                        </a>
                    </p>
                </div>
                <span className="action-row">
                    {editable && changeSet !== null ? (
                        <DeleteButton
                            label="Delete variable"
                            warning={blastWarning(
                                `variables/${variableId}.toml`,
                                referenceLabels(
                                    detail.model,
                                    (to) =>
                                        to.kind === "variable" &&
                                        to.id === variableId,
                                ).filter(
                                    (label) =>
                                        label !== `variable ${variableId}`,
                                ),
                            )}
                            onConfirm={() =>
                                saveEdit(changeSet.id, {
                                    packagePath: detail.path,
                                    expectedPin: detail.pin,
                                    operations: [
                                        {
                                            op: "delete",
                                            target: `variable=${variableId}`,
                                        },
                                    ],
                                    summary: `Delete ${variableId}`,
                                }).then((result) => {
                                    onSaved(result);
                                    onBack();
                                }, onError)
                            }
                        />
                    ) : null}
                    <button className="btn btn-ghost btn-sm" onClick={onBack}>
                        Back
                    </button>
                </span>
            </div>

            {reads.length > 0 || readBy.length > 0 ? (
                <div className="field-stack">
                    <ReferencePills
                        title="Reads"
                        entities={reads}
                        hrefEntity={hrefEntity}
                    />
                    <ReferencePills
                        title="Read by"
                        entities={readBy}
                        hrefEntity={hrefEntity}
                    />
                </div>
            ) : null}

            <TracePreview
                variableId={variableId}
                chosen={chosen}
                outcome={outcome}
                synthesized={synthesized}
                canPromote={editable}
                hrefEntity={hrefEntity}
                onUseContext={onUseContext}
                onPromote={promote}
            />

            <form
                className="form-contents"
                onSubmit={(event) => {
                    event.preventDefault();
                    save();
                }}
            >
                <div className="form-fields">
                    <div className="form-row">
                        <span className="label">Description</span>
                        <input
                            className="input"
                            disabled={!editable}
                            value={description}
                            onChange={(event) =>
                                setDescription(event.target.value)
                            }
                        />
                    </div>
                    <div className="form-row">
                        <span className="label">Default</span>
                        <ValueInput
                            type={type}
                            disabled={!editable}
                            value={defaultText}
                            onChange={setDefaultText}
                        />
                    </div>

                    {method === "allocation" ? (
                        <div className="section-header-text">
                            <h3>Allocation</h3>
                            <p className="hint">
                                This variable resolves by allocation; adjust the
                                rollout on its layer, or edit the raw file.
                            </p>
                        </div>
                    ) : method === "query" ? (
                        <QueryFields
                            editable={editable}
                            query={query}
                            onChange={setQuery}
                            onUseRules={() => setMethod("rules")}
                        />
                    ) : (
                        <>
                            <div className="section-header-text">
                                <h3>Rules</h3>
                                <p className="hint">
                                    First match wins; the default answers when
                                    none do.
                                </p>
                            </div>
                            {rules.map((rule, index) => (
                                <div className="rule-row" key={index}>
                                    <span className="rule-word label">
                                        when
                                    </span>
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
                                    <span className="rule-word label">
                                        value
                                    </span>
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
                                                type="button"
                                                disabled={index === 0}
                                                title="Move up"
                                                onClick={() =>
                                                    setRules(
                                                        moveRule(
                                                            rules,
                                                            index,
                                                            index - 1,
                                                        ),
                                                    )
                                                }
                                            >
                                                ↑
                                            </button>
                                            <button
                                                className="btn btn-icon btn-sm"
                                                type="button"
                                                disabled={
                                                    index === rules.length - 1
                                                }
                                                title="Move down"
                                                onClick={() =>
                                                    setRules(
                                                        moveRule(
                                                            rules,
                                                            index,
                                                            index + 1,
                                                        ),
                                                    )
                                                }
                                            >
                                                ↓
                                            </button>
                                            <button
                                                className="btn btn-icon btn-sm btn-remove"
                                                type="button"
                                                title="Remove rule"
                                                onClick={() =>
                                                    setRules(
                                                        rules.filter(
                                                            (_, i) =>
                                                                i !== index,
                                                        ),
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
                                        type="button"
                                        onClick={() =>
                                            setRules([
                                                ...rules,
                                                {
                                                    when: "",
                                                    valueText:
                                                        defaultRuleValue(type),
                                                },
                                            ])
                                        }
                                    >
                                        Add rule
                                    </button>
                                    <button
                                        className="btn btn-ghost btn-sm"
                                        type="button"
                                        title="Select the value with one catalog query instead of rules"
                                        onClick={() => setMethod("query")}
                                    >
                                        Use a query instead
                                    </button>
                                </div>
                            ) : null}
                        </>
                    )}
                </div>

                <div className="card-actions">
                    {editable ? (
                        <button
                            className="btn btn-primary"
                            type="submit"
                            disabled={saving}
                        >
                            {saving ? "Saving…" : "Save (one commit)"}
                        </button>
                    ) : (
                        <span className="hint">
                            Start or pick a change set above to edit.
                        </span>
                    )}
                </div>
            </form>
        </div>
    );
}

// A variable's declared type; entity-backed types (catalog=, list=, and
// their array item forms) link to the entity that defines the value shape.
function TypeLabel({
    type,
    hrefEntity,
}: {
    type: string;
    hrefEntity: (steps: AddressStep[]) => string;
}) {
    const match = /^(array<)?(catalog|list)=([a-z0-9_/]+)>?$/.exec(type);
    if (match === null) {
        return <>{type}</>;
    }
    const [, arrayOpen, className, id] = match as unknown as [
        string,
        string | undefined,
        string,
        string,
    ];
    const link = (
        <a className="expr-link" href={hrefEntity([{ class: className, id }])}>
            {className}={id}
        </a>
    );
    return arrayOpen !== undefined ? <>array&lt;{link}&gt;</> : link;
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

function QueryFields({
    editable,
    query,
    onChange,
    onUseRules,
}: {
    editable: boolean;
    query: QueryDraft;
    onChange: (query: QueryDraft) => void;
    onUseRules: () => void;
}) {
    const set = (key: keyof QueryDraft) => (value: string) =>
        onChange({ ...query, [key]: value });
    return (
        <>
            <div className="section-header-text">
                <h3>Query</h3>
                <p className="hint">
                    One query selects the value from a catalog; the default
                    answers when nothing matches.
                </p>
            </div>
            <div className="form-row">
                <span className="label">From</span>
                <input
                    className="input mono"
                    disabled={!editable}
                    placeholder="catalog_id"
                    value={query.from}
                    onChange={(event) => set("from")(event.target.value)}
                />
            </div>
            <div className="form-row">
                <span className="label">Filter</span>
                {/* A query filter is routinely several clauses long; a
                    wrapping textarea keeps the whole expression readable.
                    It stays one expression: Enter submits, never a newline. */}
                <textarea
                    className="input mono expression-input"
                    disabled={!editable}
                    placeholder="entry.tier == context.account.tier"
                    rows={1}
                    value={query.filter}
                    onChange={(event) => set("filter")(event.target.value)}
                    onKeyDown={(event) => {
                        if (event.key === "Enter") {
                            event.preventDefault();
                            event.currentTarget.form?.requestSubmit();
                        }
                    }}
                />
            </div>
            <div className="form-row">
                <span className="label">Sort</span>
                <input
                    className="input mono"
                    disabled={!editable}
                    placeholder="entry.priority (optional)"
                    value={query.sort}
                    onChange={(event) => set("sort")(event.target.value)}
                />
            </div>
            <div className="form-row">
                <span className="label">Order</span>
                <select
                    className="input"
                    disabled={!editable}
                    value={query.order}
                    onChange={(event) => set("order")(event.target.value)}
                >
                    <option value="">default (asc)</option>
                    <option value="asc">asc</option>
                    <option value="desc">desc</option>
                </select>
            </div>
            <div className="form-row">
                <span className="label">Limit</span>
                <input
                    className="input mono"
                    type="number"
                    disabled={!editable}
                    placeholder="all matches"
                    value={query.limitText}
                    onChange={(event) => set("limitText")(event.target.value)}
                />
            </div>
            {editable ? (
                <div className="action-row">
                    <button
                        className="btn btn-ghost btn-sm"
                        type="button"
                        title="Switch back to first-match rules"
                        onClick={onUseRules}
                    >
                        Use rules instead
                    </button>
                </div>
            ) : null}
        </>
    );
}

// --- the raw-text path ---

function FilePanel({
    treeId,
    detail,
    file,
    editable,
    changeSet,
    deleteAs,
    onDeleted,
    onBack,
    onSaved,
    onError,
}: {
    treeId: string;
    detail: PackageDetail;
    file: string;
    editable: boolean;
    changeSet: ChangeSet | null;
    // The entity address this file defines, when the file was reached
    // through one: deletion then goes through the semantic operation so
    // the change record names the entity, not just the path.
    deleteAs?: string;
    onDeleted?: () => void;
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
        <div className="card">
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
                <span className="action-row">
                    {editable &&
                    changeSet !== null &&
                    file !== "rototo-package.toml" ? (
                        <DeleteButton
                            label="Delete"
                            warning={`This removes ${deleteAs ?? file}; anything still referencing it shows up in lint.`}
                            onConfirm={() =>
                                saveEdit(changeSet.id, {
                                    packagePath: detail.path,
                                    expectedPin: detail.pin,
                                    ...(deleteAs !== undefined
                                        ? {
                                              operations: [
                                                  {
                                                      op: "delete",
                                                      target: deleteAs,
                                                  },
                                              ],
                                          }
                                        : { deletes: [file] }),
                                    summary: `Delete ${deleteAs ?? file}`,
                                }).then((result) => {
                                    onSaved(result);
                                    onDeleted?.();
                                }, onError)
                            }
                        />
                    ) : null}
                    <button className="btn btn-ghost btn-sm" onClick={onBack}>
                        Back
                    </button>
                </span>
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
                    onKeyDown={(event) => {
                        // Enter is a newline here; the submit accelerator
                        // for multi-line editors is Ctrl/Cmd+Enter.
                        if (
                            event.key === "Enter" &&
                            (event.metaKey || event.ctrlKey) &&
                            !saving
                        ) {
                            event.preventDefault();
                            save();
                        }
                    }}
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

// --- creating and deleting entities ---

// The starter schema a console-created catalog or context begins with:
// open, so the first entry or sample validates while the real contract is
// drafted in the raw schema file.
const STARTER_SCHEMA = { type: "object", additionalProperties: true };

type CreateDeps = {
    detail: PackageDetail;
    changeSet: ChangeSet | null;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
    onCreated: (id: string) => void;
};

function NewVariableForm({
    detail,
    changeSet,
    onSaved,
    onError,
    onCreated,
}: CreateDeps) {
    const [open, setOpen] = useState(false);
    const [id, setId] = useState("");
    const [type, setType] = useState("bool");
    const [defaultText, setDefaultText] = useState("false");
    const [busy, setBusy] = useState(false);
    if (changeSet === null) {
        return null;
    }
    if (!open) {
        return (
            <button
                className="btn btn-secondary btn-sm"
                onClick={() => setOpen(true)}
            >
                New variable
            </button>
        );
    }
    const submit = () => {
        let value: unknown;
        try {
            value = textToValue(defaultText, type);
        } catch (error) {
            onError(error);
            return;
        }
        const variableId = id.trim();
        setBusy(true);
        saveEdit(changeSet.id, {
            packagePath: detail.path,
            expectedPin: detail.pin,
            operations: [
                { op: "create_variable", id: variableId, type, default: value },
            ],
            summary: `Create ${variableId}`,
        })
            .then((result) => {
                onSaved(result);
                onCreated(variableId);
            }, onError)
            .finally(() => setBusy(false));
    };
    return (
        <form
            className="inline-form"
            onSubmit={(event) => {
                event.preventDefault();
                submit();
            }}
        >
            <input
                autoFocus
                className="input mono"
                placeholder="variable_id"
                value={id}
                onChange={(event) => setId(event.target.value)}
            />
            <select
                className="input"
                value={type}
                onChange={(event) => {
                    setType(event.target.value);
                    setDefaultText(
                        event.target.value === "bool"
                            ? "false"
                            : event.target.value === "string"
                              ? ""
                              : "0",
                    );
                }}
            >
                {["bool", "int", "number", "string"].map((option) => (
                    <option key={option} value={option}>
                        {option}
                    </option>
                ))}
            </select>
            <ValueInput
                type={type}
                disabled={false}
                value={defaultText}
                onChange={setDefaultText}
            />
            <button
                className="btn btn-primary btn-sm"
                type="submit"
                disabled={busy || id.trim() === ""}
            >
                Create
            </button>
            <button
                className="btn btn-ghost btn-sm"
                type="button"
                disabled={busy}
                onClick={() => setOpen(false)}
            >
                Cancel
            </button>
        </form>
    );
}

function NewCatalogForm({
    detail,
    changeSet,
    onSaved,
    onError,
    onCreated,
}: CreateDeps) {
    if (changeSet === null) {
        return null;
    }
    return (
        <NewIdForm
            label="New catalog"
            placeholder="catalog_id"
            onSubmit={(id) =>
                saveEdit(changeSet.id, {
                    packagePath: detail.path,
                    expectedPin: detail.pin,
                    operations: [
                        { op: "create_catalog", id, schema: STARTER_SCHEMA },
                    ],
                    summary: `Create catalog ${id}`,
                }).then((result) => {
                    onSaved(result);
                    onCreated(id);
                }, onError)
            }
        />
    );
}

function NewContextForm({
    detail,
    changeSet,
    onSaved,
    onError,
    onCreated,
}: CreateDeps) {
    if (changeSet === null) {
        return null;
    }
    return (
        <NewIdForm
            label="New context"
            placeholder="context_id"
            onSubmit={(id) =>
                saveEdit(changeSet.id, {
                    packagePath: detail.path,
                    expectedPin: detail.pin,
                    operations: [
                        { op: "create_context", id, schema: STARTER_SCHEMA },
                    ],
                    summary: `Create context ${id}`,
                }).then((result) => {
                    onSaved(result);
                    onCreated(id);
                }, onError)
            }
        />
    );
}

function NewListForm({
    detail,
    changeSet,
    onSaved,
    onError,
    onCreated,
}: CreateDeps) {
    const [open, setOpen] = useState(false);
    const [id, setId] = useState("");
    const [type, setType] = useState("string");
    const [membersText, setMembersText] = useState("");
    const [busy, setBusy] = useState(false);
    if (changeSet === null) {
        return null;
    }
    if (!open) {
        return (
            <button
                className="btn btn-secondary btn-sm"
                onClick={() => setOpen(true)}
            >
                New list
            </button>
        );
    }
    const submit = () => {
        let members: unknown[];
        try {
            members = membersText
                .split(",")
                .map((text) => text.trim())
                .filter((text) => text !== "")
                .map((text) => textToValue(text, type));
        } catch (error) {
            onError(error);
            return;
        }
        if (members.length === 0) {
            onError(new Error("a list needs at least one member"));
            return;
        }
        const listId = id.trim();
        setBusy(true);
        saveEdit(changeSet.id, {
            packagePath: detail.path,
            expectedPin: detail.pin,
            operations: [{ op: "create_list", id: listId, type, members }],
            summary: `Create list ${listId}`,
        })
            .then((result) => {
                onSaved(result);
                onCreated(listId);
            }, onError)
            .finally(() => setBusy(false));
    };
    return (
        <form
            className="inline-form"
            onSubmit={(event) => {
                event.preventDefault();
                submit();
            }}
        >
            <input
                autoFocus
                className="input mono"
                placeholder="list_id"
                value={id}
                onChange={(event) => setId(event.target.value)}
            />
            <select
                className="input"
                value={type}
                onChange={(event) => setType(event.target.value)}
            >
                {["string", "int", "number", "bool"].map((option) => (
                    <option key={option} value={option}>
                        {option}
                    </option>
                ))}
            </select>
            <input
                className="input mono"
                placeholder="member, member, member"
                value={membersText}
                onChange={(event) => setMembersText(event.target.value)}
            />
            <button
                className="btn btn-primary btn-sm"
                type="submit"
                disabled={busy || id.trim() === ""}
            >
                Create
            </button>
            <button
                className="btn btn-ghost btn-sm"
                type="button"
                disabled={busy}
                onClick={() => setOpen(false)}
            >
                Cancel
            </button>
        </form>
    );
}

// One id in, one create operation out: entries, samples, and files share
// the shape.
function NewIdForm({
    label,
    placeholder,
    onSubmit,
}: {
    label: string;
    placeholder: string;
    onSubmit: (id: string) => Promise<unknown> | void;
}) {
    const [open, setOpen] = useState(false);
    const [id, setId] = useState("");
    const [busy, setBusy] = useState(false);
    if (!open) {
        return (
            <div className="action-row">
                <button
                    className="btn btn-secondary btn-sm"
                    onClick={() => setOpen(true)}
                >
                    {label}
                </button>
            </div>
        );
    }
    return (
        <form
            className="action-row"
            onSubmit={(event) => {
                event.preventDefault();
                setBusy(true);
                void Promise.resolve(onSubmit(id.trim())).finally(() => {
                    setBusy(false);
                    setOpen(false);
                    setId("");
                });
            }}
        >
            <input
                autoFocus
                className="input mono"
                placeholder={placeholder}
                value={id}
                onChange={(event) => setId(event.target.value)}
            />
            <button
                className="btn btn-primary btn-sm"
                type="submit"
                disabled={busy || id.trim() === ""}
            >
                Create
            </button>
            <button
                className="btn btn-ghost btn-sm"
                type="button"
                disabled={busy}
                onClick={() => setOpen(false)}
            >
                Cancel
            </button>
        </form>
    );
}

// A two-step delete: the first click shows the blast radius, the second
// commits it. Deletes land like any edit, so lint reports what dangles.
// The surfaces lens reuses it, so "delete" reads the same everywhere.
export function DeleteButton({
    label,
    warning,
    onConfirm,
}: {
    label: string;
    warning: string;
    onConfirm: () => Promise<unknown> | void;
}) {
    const [confirming, setConfirming] = useState(false);
    const [busy, setBusy] = useState(false);
    if (!confirming) {
        return (
            <button
                className="btn btn-ghost btn-sm btn-remove"
                onClick={() => setConfirming(true)}
            >
                {label}
            </button>
        );
    }
    return (
        <span className="inline-form">
            <span className="hint">{warning}</span>
            <button
                className="btn btn-primary btn-sm"
                disabled={busy}
                onClick={() => {
                    setBusy(true);
                    void Promise.resolve(onConfirm()).finally(() => {
                        setBusy(false);
                        setConfirming(false);
                    });
                }}
            >
                {busy ? "Deleting…" : "Confirm delete"}
            </button>
            <button
                className="btn btn-ghost btn-sm"
                disabled={busy}
                onClick={() => setConfirming(false)}
            >
                Keep
            </button>
        </span>
    );
}

// Who points at this entity, from the model's reference index: the blast
// radius a delete shows before it lands.
function referenceLabels(
    model: SemanticModel,
    matches: (to: ModelEntityRef) => boolean,
): string[] {
    const labels = new Set<string>();
    for (const reference of model.references) {
        if (!matches(reference.to)) {
            continue;
        }
        const from = reference.from;
        const id =
            typeof from.id === "string"
                ? from.id
                : typeof from["variable"] === "string"
                  ? (from["variable"] as string)
                  : "?";
        labels.add(`${from.kind} ${id}`);
    }
    return [...labels].sort();
}

// The reference index does not track list usage, so the visible blast
// radius for a list is the variables typed against it; expression uses
// (`lists.<id>`) surface through lint after the delete.
function listTypedVariables(model: SemanticModel, listId: string): string[] {
    return model.variables
        .filter((variable) => {
            const value = variable.declaration.value ?? "";
            return (
                value === `list=${listId}` || value === `array<list=${listId}>`
            );
        })
        .map((variable) => variable.id);
}

function listReferrers(model: SemanticModel, listId: string): string[] {
    return listTypedVariables(model, listId).map((id) => `variable ${id}`);
}

// One side of the reference index as links: the entities a panel's subject
// reads, or the entities that read it, rendered the same everywhere.
function ReferencePills({
    title,
    entities,
    hrefEntity,
}: {
    title: string;
    entities: ModelEntityRef[];
    hrefEntity: (steps: AddressStep[]) => string;
}) {
    const seen = new Map<string, AddressStep[] | null>();
    for (const entity of entities) {
        seen.set(entityLabel(entity), entitySteps(entity));
    }
    if (seen.size === 0) {
        return null;
    }
    return (
        <div className="reference-links">
            <span className="label">{title}</span>
            {[...seen.entries()]
                .sort(([a], [b]) => a.localeCompare(b))
                .map(([label, steps]) =>
                    steps === null ? (
                        <span key={label} className="pill pill-neutral mono">
                            {label}
                        </span>
                    ) : (
                        <a
                            key={label}
                            className="pill pill-neutral mono"
                            href={hrefEntity(steps)}
                        >
                            {label}
                        </a>
                    ),
                )}
        </div>
    );
}

function blastWarning(what: string, referrers: string[]): string {
    const base = `This removes ${what}.`;
    if (referrers.length === 0) {
        return `${base} Anything still referencing it shows up in lint.`;
    }
    const shown = referrers.slice(0, 4).join(", ");
    const more =
        referrers.length > 4 ? ` and ${referrers.length - 4} more` : "";
    return `${base} Still referenced by ${shown}${more}; those references will fail lint.`;
}

// --- form state to operations ---

type QueryDraft = {
    from: string;
    filter: string;
    sort: string;
    order: string;
    limitText: string;
};

type VariableDraft = {
    description: string;
    defaultText: string;
    rules: RuleDraft[];
    method: "rules" | "query" | "allocation";
    query: QueryDraft;
};

function methodOf(
    variable: VariableModel | undefined,
): "rules" | "query" | "allocation" {
    const value = variable?.resolve?.method?.value;
    return value === "query" || value === "allocation" ? value : "rules";
}

function queryToDraft(query: QueryModel | undefined): QueryDraft {
    return {
        from: query?.from?.value ?? "",
        filter: query?.filter?.value ?? "",
        sort: query?.sort?.value ?? "",
        order: query?.order?.value ?? "",
        limitText: query?.limit?.value ?? "",
    };
}

function queryEqual(a: QueryDraft, b: QueryDraft): boolean {
    return (
        a.from === b.from &&
        a.filter === b.filter &&
        a.sort === b.sort &&
        a.order === b.order &&
        a.limitText === b.limitText
    );
}

function queryOperation(variableId: string, query: QueryDraft): EditOperation {
    if (query.from.trim() === "" || query.filter.trim() === "") {
        throw new Error("a query needs a catalog to read and a filter");
    }
    const operation: EditOperation = {
        op: "set_query",
        variable: variableId,
        from: query.from.trim(),
        filter: query.filter.trim(),
    };
    if (query.sort.trim() !== "") {
        operation["sort"] = query.sort.trim();
    }
    if (query.order !== "") {
        operation["order"] = query.order;
    }
    if (query.limitText.trim() !== "") {
        const limit = Number(query.limitText);
        if (!Number.isInteger(limit) || limit < 1) {
            throw new Error(
                `${query.limitText} is not a positive integer limit`,
            );
        }
        operation["limit"] = limit;
    }
    return operation;
}

function buildOperations(
    variableId: string,
    type: string,
    original: VariableDraft,
    draft: VariableDraft,
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
    if (draft.method === "query") {
        // The whole query saves as one operation; the engine drops any
        // rules, since a query resolve has no rules to run.
        if (
            original.method !== "query" ||
            !queryEqual(draft.query, original.query)
        ) {
            operations.push(queryOperation(variableId, draft.query));
        }
        return operations;
    }
    if (original.method === "query") {
        operations.push({ op: "clear_query", variable: variableId });
        for (const rule of draft.rules) {
            operations.push({
                op: "add_rule",
                variable: variableId,
                when: rule.when,
                value: textToValue(rule.valueText, type),
            });
        }
        return operations;
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
