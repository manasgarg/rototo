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
    createChangeSet,
    fetchContexts,
    listChangeSets,
    listPackages,
    readPackage,
    readPackageFile,
    runPreview,
    runVariablePreview,
    saveEdit,
    type ChangeSet,
    type ContextInventory,
    type EditOperation,
    type EditResponse,
    type LintDiagnostic,
    type MeResponse,
    type ModelEntityRef,
    type PackageDetail,
    type PackageListing,
    type QueryModel,
    type ResolutionTrace,
    type RuleModel,
    type SemanticModel,
    type SynthesizedContext,
    type TraceOutcome,
    type VariableModel,
} from "@/lib/api";
import { LspFile } from "@/lib/lsp";
import {
    ContextPicker,
    ContextPickerBody,
    contextLabel,
    syntheticLabel,
    type ChosenContext,
} from "@/components/context-picker";
import { CodeEditor, codeLanguageForPath } from "@/components/code-editor";
import { DiagnosticsPanel, LintStatusPill } from "@/components/diagnostics";
import {
    entityLabel,
    entitySteps,
    ExpressionText,
} from "@/components/entity-link";
import { HistoryPanel, UpcomingPanel } from "@/components/insight";
import { ReferenceGraph } from "@/components/reference-graph";
import { AnswerStrip } from "@/components/trace-preview";
import { resolvedValueText } from "@/lib/format";
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
    // While a buffer is open against the language server, its live
    // diagnostics replace the saved pin's findings for that file, so the
    // header's lint pill judges what the editor shows, not what was
    // committed.
    const [liveLint, setLiveLint] = useState<{
        file: string;
        diagnostics: LintDiagnostic[];
    } | null>(null);

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

    // A context-free variable can answer on its own with `{}` even when other
    // variables make the package's context schema stricter. Everything else
    // still uses the chosen context as one lenient package batch.
    const contextFreeVariableId = useMemo(() => {
        if (detail === null || detail.pin !== pin || view.kind !== "address") {
            return null;
        }
        const head = view.steps[0];
        if (
            head?.class !== "variable" ||
            head.id === "" ||
            head.id.endsWith("/")
        ) {
            return null;
        }
        const variable = detail.model.variables.find(
            (candidate) => candidate.id === head.id,
        );
        return variable !== undefined && !variable.usesContext
            ? variable.id
            : null;
    }, [detail, pin, view]);

    useEffect(() => {
        if (pin === null) {
            setOutcomes(null);
            return;
        }
        if (contextFreeVariableId === null && chosen.kind === "none") {
            setOutcomes(null);
            return;
        }
        let stale = false;
        setOutcomes(null);
        let preview: Promise<TraceOutcome[]>;
        if (contextFreeVariableId !== null) {
            preview = runVariablePreview(
                treeId,
                packagePath,
                pin,
                contextFreeVariableId,
                {},
            ).then((response) => [response.outcome]);
        } else {
            // The no-context case returned above, so this branch always owns
            // a real chosen context and can run the package batch.
            if (chosen.kind === "none") {
                return;
            }
            preview = runPreview(treeId, packagePath, pin, chosen.context).then(
                (response) => response.outcomes,
            );
        }
        preview.then(
            (response) => {
                if (!stale) {
                    setOutcomes(
                        new Map(
                            response.map((outcome) => [outcome.id, outcome]),
                        ),
                    );
                }
            },
            (error: Error) => {
                if (!stale) {
                    setOutcomes(null);
                    setBanner({
                        kind: "warn",
                        text:
                            contextFreeVariableId !== null
                                ? `Cannot preview ${contextFreeVariableId}: ${error.message}`
                                : `The ${contextLabel(chosen)} was refused: ${error.message}`,
                    });
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, packagePath, pin, chosen, contextFreeVariableId]);

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
    const heading = screenHeading(view, packagePath, treeName);

    return (
        <div className="section">
            <div className="section-header">
                <div className="section-header-text">
                    <h1 className={heading.mono ? "mono" : undefined}>
                        {heading.label}
                    </h1>
                    <BranchPill
                        listing={listing}
                        pin={state.pin}
                        active={active}
                    />
                </div>
                <div className="section-header-actions">
                    {detail !== null ? (
                        <LintStatusPill
                            diagnostics={
                                liveLint === null
                                    ? detail.lint.diagnostics
                                    : [
                                          ...detail.lint.diagnostics.filter(
                                              (diagnostic) =>
                                                  diagnostic.location?.path !==
                                                  liveLint.file,
                                          ),
                                          ...liveLint.diagnostics,
                                      ]
                            }
                            href={hrefView({ kind: "diagnostics" })}
                        />
                    ) : null}
                    <ChangeSetControl
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
                </div>
            </div>

            {/* The context parameterizes resolution; every screen that shows
                resolved values names the context they are under. The overview
                and the variables collection carry this strip; the variable
                screen mounts the picker inside its try-it card. */}
            {view.kind === "overview" ||
            (view.kind === "address" &&
                view.steps[0] !== undefined &&
                view.steps[0].class === "variable" &&
                isCollective(view.steps[0])) ? (
                <ContextPicker
                    inventory={inventory}
                    chosen={chosen}
                    onChange={chooseContext}
                />
            ) : null}

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
            ) : view.kind === "diagnostics" ? (
                <DiagnosticsPanel
                    diagnostics={detail.lint.diagnostics}
                    hrefEntity={hrefEntity}
                    hrefFile={hrefFile}
                />
            ) : view.kind === "files" ? (
                <FilePanel
                    key={`${view.file}@${detail.pin}`}
                    treeId={treeId}
                    detail={detail}
                    file={view.file}
                    editable={editable}
                    changeSet={active}
                    onDeleted={() => go({ kind: "overview" })}
                    onBack={() => go({ kind: "overview" })}
                    onSaved={afterSave}
                    onError={saveFailed}
                    onLiveLint={setLiveLint}
                />
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
                    onLiveLint={setLiveLint}
                />
            ) : (
                <Overview
                    treeId={treeId}
                    detail={detail}
                    outcomes={outcomes}
                    hrefView={hrefView}
                />
            )}
        </div>
    );
}

// The H1 names the screen, not the package: the sidebar picker and the
// breadcrumbs already carry the package, so the heading mirrors the
// breadcrumb tail — the deepest named address step, a nav label, or the
// package itself on its overview.
function screenHeading(
    view: PackageView,
    packagePath: string,
    treeName: string,
): { label: string; mono: boolean } {
    if (view.kind === "history") {
        return { label: "History", mono: false };
    }
    if (view.kind === "diagnostics") {
        return { label: "Diagnostics", mono: false };
    }
    if (view.kind === "files") {
        return { label: view.file, mono: true };
    }
    if (view.kind === "surfaces") {
        return {
            label: view.surfaceId ?? "Surfaces",
            mono: view.surfaceId !== null,
        };
    }
    if (view.kind === "address") {
        const named = view.steps.filter((step) => step.id !== "").at(-1);
        if (named !== undefined) {
            return { label: named.id, mono: true };
        }
        const head = view.steps[0] as AddressStep;
        return { label: CLASS_LABELS[head.class] ?? head.class, mono: false };
    }
    return {
        label: packagePath === "." ? treeName : packagePath,
        mono: packagePath !== ".",
    };
}

// The editing context: which change set commits accumulate on. Viewing the
// default branch is read-only by design; the branch is the durable draft.
// One quiet header control carries the whole story: a menu of open change
// sets, the way back to read-only, and creation. The domain lens reuses it
// so "which change set am I on" looks the same everywhere.
export function ChangeSetControl({
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
    const [open, setOpen] = useState(false);
    const [creating, setCreating] = useState(false);
    const [title, setTitle] = useState("");
    const wrapper = useRef<HTMLDivElement>(null);

    const openSets = changeSets.filter(
        (entry) => entry.state === "draft" || entry.state === "proposed",
    );
    const close = () => {
        setOpen(false);
        setCreating(false);
        setTitle("");
    };
    const pick = (id: string | null) => {
        onSelect(id);
        close();
    };

    // A viewer without propose rights keeps the button as the explanation:
    // disabled, with the server's reason a hover away.
    return (
        <div
            className="menu-control"
            ref={wrapper}
            onBlur={(event) => {
                if (!wrapper.current?.contains(event.relatedTarget as Node)) {
                    close();
                }
            }}
        >
            <button
                aria-expanded={open}
                className="btn btn-secondary btn-sm"
                disabled={!canPropose.allow}
                title={
                    canPropose.allow
                        ? active === null
                            ? "Pick or start the change set your edits commit to"
                            : `Edits accumulate on ${active.branch}`
                        : `read-only: ${canPropose.reason}`
                }
                type="button"
                onClick={() => (open ? close() : setOpen(true))}
            >
                {active === null ? "Edit in a change set" : active.title}
                <span aria-hidden="true" className="menu-chevron">
                    ⌄
                </span>
            </button>
            {open ? (
                <div className="menu" role="menu">
                    {active !== null ? (
                        <>
                            <a
                                className="menu-item"
                                href={`#${changeSetUrl(treeId, active.id)}`}
                                role="menuitem"
                                onClick={close}
                            >
                                Open this change set
                            </a>
                            <button
                                className="menu-item"
                                role="menuitem"
                                type="button"
                                onClick={() => pick(null)}
                            >
                                Base branch (read-only)
                            </button>
                        </>
                    ) : null}
                    {openSets.map((entry) => (
                        <button
                            aria-current={entry.id === active?.id || undefined}
                            className="menu-item"
                            key={entry.id}
                            role="menuitem"
                            type="button"
                            onClick={() => pick(entry.id)}
                        >
                            {entry.title}
                            <span className="menu-item-sub">{entry.state}</span>
                        </button>
                    ))}
                    {creating ? (
                        <form
                            className="menu-form"
                            onSubmit={(event) => {
                                event.preventDefault();
                                const changeSetTitle = title.trim();
                                close();
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
                                onChange={(event) =>
                                    setTitle(event.target.value)
                                }
                            />
                            <button
                                className="btn btn-primary btn-sm"
                                type="submit"
                                disabled={title.trim() === ""}
                            >
                                Start
                            </button>
                        </form>
                    ) : (
                        <button
                            className="menu-item"
                            role="menuitem"
                            type="button"
                            onClick={() => setCreating(true)}
                        >
                            New change set…
                        </button>
                    )}
                </div>
            ) : null}
        </div>
    );
}

// The branch state as one quiet mono pill under the heading: the ref and
// pin when reading, the change-set branch when editing, the pinned instant
// when time-travelling. The pill states a fact; the control beside the
// heading is the action.
function BranchPill({
    listing,
    pin,
    active,
}: {
    listing: PackageListing | null;
    pin: string | null;
    active: ChangeSet | null;
}) {
    if (active !== null) {
        return (
            <span
                className="pill pill-cyan mono branch-pill"
                title={`Edits accumulate on ${active.branch}`}
            >
                ⎇ {active.branch} · editing
            </span>
        );
    }
    if (pin !== null) {
        return (
            <span
                className="pill pill-info mono branch-pill"
                title="Viewing the package as it was at this commit"
            >
                @ {pin.slice(0, 10)} · historical
            </span>
        );
    }
    if (listing === null) {
        return <p className="hint">Resolving…</p>;
    }
    return (
        <span className="pill pill-neutral mono branch-pill">
            ⎇ {listing.ref} @ {listing.pin.slice(0, 10)} · read-only
        </span>
    );
}

// The package overview: the reference graph plus the insight panels. The
// per-kind entity lists live on their collection pages (the nav's
// Contexts, Variables, Catalogs, Lists), not here.
function Overview({
    treeId,
    detail,
    outcomes,
    hrefView,
}: {
    treeId: string;
    detail: PackageDetail;
    outcomes: Map<string, TraceOutcome> | null;
    hrefView: (view: PackageView) => string;
}) {
    const model = detail.model;
    const hrefEntity = (steps: AddressStep[]): string =>
        hrefView({ kind: "address", steps });
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
                                ? "Structure only; pick a context above and every variable shows the value it yields. Hover an entity to preview its definition."
                                : "Every variable resolved under the given context; paths that never ran are dimmed."}
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
        </>
    );
}

function clipText(text: string): string {
    return text.length > 20 ? `${text.slice(0, 20)}…` : text;
}

function VariableRows({
    variables,
    outcomes,
    hrefFor,
    action,
}: {
    variables: VariableModel[];
    outcomes: Map<string, TraceOutcome> | null;
    hrefFor: (id: string) => string;
    action?: ReactNode;
}) {
    return (
        <SearchableList
            label="Search variables"
            placeholder="Search variables"
            emptyLabel="No variable matches that search."
            className="row-list"
            action={action}
        >
            {variables.map((variable) => {
                const outcome = outcomes?.get(variable.id);
                return (
                    <a
                        className="row"
                        key={variable.id}
                        href={hrefFor(variable.id)}
                        data-search={`${variable.id} ${variable.declaration.value ?? ""} ${methodText(variable)} ${variable.description ?? ""}`}
                    >
                        <span className="row-text">
                            <span className="row-title mono">
                                {variable.id}
                            </span>
                            <span className="row-sub">
                                {variable.declaration.value ?? "?"} ·{" "}
                                {methodText(variable)}
                                {variable.description !== undefined
                                    ? ` — ${variable.description}`
                                    : ""}
                            </span>
                            {outcome?.error !== undefined ? (
                                <span className="row-sub row-problem">
                                    {outcome.error}
                                </span>
                            ) : null}
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
                                <span
                                    className="pill pill-sea mono"
                                    title={
                                        JSON.stringify(
                                            outcome.trace?.resolution.value,
                                            null,
                                            2,
                                        ) ?? ""
                                    }
                                >
                                    {clipText(
                                        outcome.trace !== undefined
                                            ? resolvedValueText(outcome.trace)
                                            : "",
                                    )}
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
    onLiveLint,
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
    onLiveLint: (report: LiveLintReport) => void;
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
            onLiveLint={onLiveLint}
        />
    );
    const creating = { detail, changeSet, onSaved, onError };

    if (head.class === "variable") {
        if (isCollective(head)) {
            const variables = model.variables.filter((variable) =>
                variable.id.startsWith(head.id),
            );
            const action = editable ? (
                <NewVariableForm
                    {...creating}
                    onCreated={(id) => openAddress([{ class: "variable", id }])}
                />
            ) : null;
            return (
                <CollectionPage
                    count={variables.length}
                    empty="No variables in this package yet."
                    action={action}
                >
                    <VariableRows
                        variables={variables}
                        outcomes={outcomes}
                        action={action}
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
                treeId={treeId}
                detail={detail}
                variableId={head.id}
                editable={editable}
                changeSet={changeSet}
                chosen={chosen}
                outcome={outcomes?.get(head.id) ?? null}
                inventory={inventory}
                hrefEntity={hrefEntity}
                onUseContext={onUseContext}
                onBack={() => openAddress([{ class: "variable", id: "" }])}
                onSaved={onSaved}
                onError={onError}
                onLiveLint={onLiveLint}
            />
        );
    }
    if (head.class === "catalog") {
        if (isCollective(head)) {
            const catalogs = model.catalogs.filter((catalog) =>
                catalog.id.startsWith(head.id),
            );
            const action = editable ? (
                <NewCatalogForm
                    {...creating}
                    onCreated={(id) => openAddress([{ class: "catalog", id }])}
                />
            ) : null;
            return (
                <CollectionPage
                    count={catalogs.length}
                    empty="No catalogs in this package yet."
                    action={action}
                >
                    <SearchableList
                        label="Search catalogs"
                        placeholder="Search catalogs"
                        emptyLabel="No catalog matches that search."
                        className="row-list"
                        action={action}
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
            const action = editable ? (
                <NewListForm
                    {...creating}
                    onCreated={(id) => openAddress([{ class: "list", id }])}
                />
            ) : null;
            return (
                <CollectionPage
                    count={lists.length}
                    empty="No lists in this package yet."
                    action={action}
                >
                    <SearchableList
                        label="Search lists"
                        placeholder="Search lists"
                        emptyLabel="No list matches that search."
                        className="row-list"
                        action={action}
                    >
                        {lists.map((list) => (
                            <a
                                className="row"
                                key={list.id}
                                href={hrefEntity([
                                    { class: "list", id: list.id },
                                ])}
                                data-search={`${list.id} ${list.memberType.value ?? "string"} ${list.description ?? ""} ${list.members.map((member) => String(member.value ?? "")).join(" ")}`}
                            >
                                <span className="row-text">
                                    <span className="row-title mono">
                                        {list.id}
                                    </span>
                                    <span className="row-sub">
                                        {list.memberType.value ?? "string"} ·{" "}
                                        {list.members.length} member
                                        {list.members.length === 1
                                            ? ""
                                            : "s"}{" "}
                                        <span className="mono">
                                            {memberPreview(list.members)}
                                        </span>
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
                treeId={treeId}
                model={model}
                listId={head.id}
                editable={editable}
                hrefEntity={hrefEntity}
                onBack={() => openAddress([{ class: "list", id: "" }])}
                onDeleted={() => openAddress([{ class: "list", id: "" }])}
                onLiveLint={onLiveLint}
            />
        );
    }
    if (head.class === "evaluation-context") {
        if (isCollective(head)) {
            const contexts = model.evaluationContexts.filter((context) =>
                context.id.startsWith(head.id),
            );
            const action = editable ? (
                <NewContextForm
                    {...creating}
                    onCreated={(id) =>
                        openAddress([{ class: "evaluation-context", id }])
                    }
                />
            ) : null;
            return (
                <CollectionPage
                    count={contexts.length}
                    empty="No evaluation contexts in this package yet."
                    action={action}
                >
                    <SearchableList
                        label="Search evaluation contexts"
                        placeholder="Search evaluation contexts"
                        emptyLabel="No evaluation context matches that search."
                        className="row-list"
                        action={action}
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
                hrefFile={hrefFile}
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

// The first few list members inline: enough to recognize the list without
// opening it. Caps at four members or roughly a phrase of text, whichever
// comes first, and always shows at least one.
function memberPreview(members: { value?: unknown }[]): string {
    const texts = members.map((member) => String(member.value ?? "?"));
    const shown: string[] = [];
    let length = 0;
    for (const text of texts) {
        if (
            shown.length > 0 &&
            (shown.length >= 4 || length + text.length > 36)
        ) {
            break;
        }
        shown.push(text);
        length += text.length + 2;
    }
    const suffix = shown.length < texts.length ? ", …" : "";
    return `[${shown.join(", ")}${suffix}]`;
}

// A collection page: one entity class, optionally narrowed to a namespace
// subtree. The page heading already names the class, so the body is just
// the toolbar (search plus the create action, inside the list) and the
// rows; an empty collection keeps the create action reachable.
function CollectionPage({
    count,
    empty,
    action,
    children,
}: {
    count: number;
    empty: string;
    action?: ReactNode;
    children: ReactNode;
}) {
    if (count === 0) {
        return (
            <>
                {action !== undefined && action !== null ? (
                    <div className="action-row">{action}</div>
                ) : null}
                <p className="hint">{empty}</p>
            </>
        );
    }
    return <>{children}</>;
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
    const canEdit = editable && changeSet !== null;
    return (
        <div className="card">
            <div className="card-head">
                <div className="card-head-text">
                    <h2 className="mono">{catalogId}</h2>
                    <p className="hint">
                        {entries.length} entr
                        {entries.length === 1 ? "y" : "ies"} · schema{" "}
                        <a
                            className="expr-link mono"
                            href={hrefFile(catalog.path)}
                            title={
                                canEdit
                                    ? "Open and edit the catalog schema"
                                    : "Open the catalog schema"
                            }
                        >
                            {catalog.path}
                        </a>
                    </p>
                </div>
                <span className="action-row">
                    {canEdit && changeSet !== null ? (
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
                    <button
                        className="btn btn-secondary btn-sm"
                        title={
                            canEdit
                                ? "Open the schema file in the editor"
                                : "Open the schema file"
                        }
                        onClick={() => onOpenSchema(catalog.path)}
                    >
                        {canEdit ? "Edit schema" : "View schema"}
                    </button>
                    {canEdit && changeSet !== null ? (
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
            {!canEdit ? (
                <p className="hint">
                    Pick or start a change set (Edit in a change set, above) to
                    add entries or edit the schema.
                </p>
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
    treeId,
    model,
    listId,
    editable,
    changeSet,
    detail,
    hrefEntity,
    onBack,
    onDeleted,
    onSaved,
    onError,
    onLiveLint,
}: {
    treeId: string;
    model: SemanticModel;
    listId: string;
    editable: boolean;
    changeSet: ChangeSet | null;
    detail: PackageDetail;
    hrefEntity: (steps: AddressStep[]) => string;
    onBack: () => void;
    onDeleted: () => void;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
    onLiveLint: (report: LiveLintReport) => void;
}) {
    const list = model.lists.find((candidate) => candidate.id === listId);
    const [saving, setSaving] = useState(false);
    // One list, two editors: the member chips and the raw TOML, switched in
    // place like the variable definition.
    const [editorMode, setEditorMode] = useState<"chips" | "toml">("chips");
    const memberType = list?.memberType.value ?? "string";
    const path = `lists/${listId}.toml`;
    const canEdit = editable && changeSet !== null;
    const toml = useTomlFile(treeId, detail, path, canEdit, onLiveLint);
    // Members edit as local chips and land as one commit: a git round trip
    // per added or removed member made every click feel stuck.
    const originalTexts = useMemo(
        () =>
            (list?.members ?? []).map((member) =>
                memberType === "string"
                    ? String(member.value)
                    : JSON.stringify(member.value),
            ),
        [list, memberType],
    );
    const [memberTexts, setMemberTexts] = useState<string[]>(originalTexts);
    // Text typed into the add box but not yet committed with Enter: it
    // still counts as a change and rides along on save.
    const [memberDraft, setMemberDraft] = useState("");
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
    // The saved-on-commit member set plus whatever sits in the add box.
    const pendingTexts = memberDraft
        .split(",")
        .map((text) => text.trim())
        .filter((text) => text !== "" && !memberTexts.includes(text));
    const draftTexts = [...memberTexts, ...pendingTexts];
    const dirty =
        JSON.stringify([...draftTexts].sort()) !==
        JSON.stringify([...originalTexts].sort());
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
    const saveMembers = () => {
        if (changeSet === null) {
            return;
        }
        let operations: EditOperation[];
        try {
            const drafts = new Set(draftTexts);
            const originals = new Set(originalTexts);
            operations = [
                ...originalTexts
                    .filter((text) => !drafts.has(text))
                    .map((text) => ({
                        op: "remove_member",
                        list: listId,
                        value: textToValue(text, memberType),
                    })),
                ...draftTexts
                    .filter((text) => !originals.has(text))
                    .map((text) => ({
                        op: "add_member",
                        list: listId,
                        value: textToValue(text, memberType),
                    })),
            ];
        } catch (error) {
            onError(error);
            return;
        }
        if (operations.length === 0) {
            return;
        }
        setSaving(true);
        setMemberTexts(draftTexts);
        setMemberDraft("");
        saveEdit(changeSet.id, {
            packagePath: detail.path,
            expectedPin: detail.pin,
            operations,
            summary: `Edit members of ${listId}`,
        })
            .then(onSaved, onError)
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
                    <div
                        aria-label="List editor"
                        className="segmented-control"
                        role="group"
                    >
                        <button
                            className={
                                editorMode === "chips" ? "active" : undefined
                            }
                            type="button"
                            onClick={() => setEditorMode("chips")}
                        >
                            Chips
                        </button>
                        <button
                            className={
                                editorMode === "toml" ? "active" : undefined
                            }
                            type="button"
                            onClick={() => setEditorMode("toml")}
                        >
                            TOML
                        </button>
                    </div>
                    {canEdit ? (
                        <DeleteButton
                            label="Delete list"
                            warning={blastWarning(
                                `lists/${listId}.toml`,
                                referenceLabels(
                                    model,
                                    (to) =>
                                        to.kind === "list" && to.id === listId,
                                ),
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
            {editorMode === "toml" ? (
                <TomlEditor
                    detail={detail}
                    file={path}
                    content={toml.content}
                    problem={toml.problem}
                    draft={toml.draft}
                    onDraft={toml.setDraft}
                    lsp={toml.lsp}
                    changeSet={canEdit ? changeSet : null}
                    onSaved={onSaved}
                    onError={onError}
                />
            ) : canEdit ? (
                <>
                    <MemberChipsEditor
                        type={memberType}
                        members={memberTexts}
                        disabled={saving}
                        onChange={setMemberTexts}
                        draft={memberDraft}
                        onDraft={setMemberDraft}
                    />
                    <div className="card-actions definition-card-actions">
                        <button
                            className="btn btn-primary btn-sm"
                            disabled={saving || !dirty}
                            type="button"
                            onClick={saveMembers}
                        >
                            {saving ? "Saving…" : "Save (one commit)"}
                        </button>
                        {dirty ? (
                            <button
                                className="btn btn-ghost btn-sm"
                                disabled={saving}
                                type="button"
                                onClick={() => {
                                    setMemberTexts(originalTexts);
                                    setMemberDraft("");
                                }}
                            >
                                Discard
                            </button>
                        ) : null}
                        <span className="hint definition-save-note">
                            writes <span className="mono">{path}</span> to
                            change set {changeSet?.id}
                        </span>
                    </div>
                </>
            ) : (
                <>
                    <div className="context-facts">
                        {memberTexts.map((text, index) => (
                            <span className="context-fact" key={index}>
                                <span className="context-fact-value">
                                    {text}
                                </span>
                            </span>
                        ))}
                    </div>
                    <p className="hint">
                        Start or pick a change set above to edit.
                    </p>
                </>
            )}
            <ReferencePills
                title="Used by"
                entities={model.references
                    .filter(
                        (reference) =>
                            reference.to.kind === "list" &&
                            reference.to.id === listId,
                    )
                    .map((reference) => reference.from)}
                hrefEntity={hrefEntity}
            />
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
    hrefFile,
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
    hrefFile: (path: string) => string;
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
                    <p className="hint">
                        Evaluation context · schema{" "}
                        <a
                            className="expr-link mono"
                            href={hrefFile(context.path)}
                            title={
                                canEdit
                                    ? "Open and edit the context schema"
                                    : "Open the context schema"
                            }
                        >
                            {context.path}
                        </a>
                    </p>
                </div>
                <span className="action-row">
                    <button
                        className="btn btn-secondary btn-sm"
                        title={
                            canEdit
                                ? "Open the schema file in the editor"
                                : "Open the schema file"
                        }
                        onClick={() => onOpenSchema(context.path)}
                    >
                        {canEdit ? "Edit schema" : "View schema"}
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

// The lint findings the language server publishes for one open buffer;
// null when no buffer is open. The workbench header's pill judges these
// over the saved pin's findings for the same file.
type LiveLintReport = { file: string; diagnostics: LintDiagnostic[] } | null;

// One editable TOML buffer: the committed content, the unsaved draft, and
// (while editable) a language-server session whose publications feed both
// the editor and the header's lint pill. Every screen with a raw TOML
// surface holds one of these.
function useTomlFile(
    treeId: string,
    detail: PackageDetail,
    file: string | undefined,
    withLsp: boolean,
    onLiveLint: (report: LiveLintReport) => void,
) {
    const [content, setContent] = useState<string | null>(null);
    const [problem, setProblem] = useState<string | null>(null);
    const [draft, setDraft] = useState<string | null>(null);
    const [lsp, setLsp] = useState<LspFile | null>(null);

    useEffect(() => {
        if (file === undefined) {
            return;
        }
        let stale = false;
        setContent(null);
        setProblem(null);
        setDraft(null);
        readPackageFile(treeId, detail.path, detail.pin, file).then(
            (response) => {
                if (!stale) {
                    setContent(response.content);
                }
            },
            (error: Error) => {
                if (!stale) {
                    setProblem(error.message);
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, detail.path, detail.pin, file]);

    useEffect(() => {
        if (!withLsp || file === undefined) {
            return;
        }
        const session = new LspFile(treeId, detail.path, detail.pin, file);
        setLsp(session);
        const unsubscribe = session.onDiagnostics((published) => {
            onLiveLint({
                file,
                diagnostics: published.map((diagnostic) => ({
                    severity: diagnostic.severity === 1 ? "error" : "warning",
                    rule: diagnostic.code,
                    message: diagnostic.message,
                    location: { path: file },
                })),
            });
        });
        return () => {
            unsubscribe();
            session.dispose();
            setLsp(null);
            onLiveLint(null);
        };
    }, [withLsp, treeId, detail.path, detail.pin, file, onLiveLint]);

    return { content, problem, draft, setDraft, lsp };
}

// --- the variable form: a producer of operations, never a TOML rewriter ---

// Form-based editing is parked for now: the TOML editor is the one write
// path, and the definition card hides its Form tab. Flip this to bring the
// form back; the machinery below stays live.
const FORM_EDITING_ENABLED: boolean = false;

type RuleDraft = { when: string; valueText: string };

function VariablePanel({
    treeId,
    detail,
    variableId,
    editable,
    changeSet,
    chosen,
    outcome,
    inventory,
    hrefEntity,
    onUseContext,
    onBack,
    onSaved,
    onError,
    onLiveLint,
}: {
    treeId: string;
    detail: PackageDetail;
    variableId: string;
    editable: boolean;
    changeSet: ChangeSet | null;
    chosen: ChosenContext;
    outcome: TraceOutcome | null;
    inventory: ContextInventory | null;
    hrefEntity: (steps: AddressStep[]) => string;
    onUseContext: (chosen: ChosenContext) => void;
    onBack: () => void;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
    onLiveLint: (report: LiveLintReport) => void;
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
    // One definition, two editors: the raw TOML and the structured form,
    // switched in place. Reading defaults to the source of truth.
    const [mode, setMode] = useState<"toml" | "form">("toml");
    const canEdit = editable && changeSet !== null;
    // The query method reads a catalog's entries as a set, so the form only
    // offers the tab for list-of-catalog types; a variable already resolving
    // by query keeps it regardless, because the data wins.
    const canQuery =
        /^array<catalog=/.test(variable?.declaration.value ?? "") ||
        original.method === "query";

    // The TOML source and its unsaved draft live here, above the mode
    // switch: flipping editors must be instant, not a refetch, and must not
    // discard what was typed. The panel remounts per pin, so a save still
    // lands on fresh content.
    const file = variable?.location.path;
    const toml = useTomlFile(treeId, detail, file, canEdit, onLiveLint);

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

    const resolutionDirty =
        method !== original.method ||
        !rulesEqual(rules, original.rules) ||
        defaultText !== original.defaultText ||
        !queryEqual(query, original.query);
    // Per-row verdicts bind the server's trace to the ladder only while the
    // draft still matches the saved definition the trace was computed from.
    const trace = chosen.kind !== "none" ? (outcome?.trace ?? null) : null;
    const verdicts =
        trace !== null && method === "rules" && !resolutionDirty
            ? ladderVerdicts(trace)
            : null;

    return (
        <>
            <p className="variable-meta">
                <span>{variable.description ?? "No description yet."}</span>
                <span className="dot-sep">·</span>
                <span className="code-chip">
                    <TypeLabel
                        type={
                            variable.declaration.kind === "catalog" ||
                            variable.declaration.kind === "list"
                                ? `${variable.declaration.kind}=${type}`
                                : type
                        }
                        hrefEntity={hrefEntity}
                    />
                </span>
                <span className="dot-sep">·</span>
                <span>
                    <MethodLabel variable={variable} hrefEntity={hrefEntity} />
                </span>
            </p>

            <div className="card definition-card">
                <div className="definition-head">
                    <span className="mono definition-file">
                        {variable.location.path}
                    </span>
                    {FORM_EDITING_ENABLED ? (
                        <div
                            aria-label="Definition editor"
                            className="segmented-control"
                            role="group"
                        >
                            <button
                                className={
                                    mode === "toml" ? "active" : undefined
                                }
                                type="button"
                                onClick={() => setMode("toml")}
                            >
                                TOML
                            </button>
                            <button
                                className={
                                    mode === "form" ? "active" : undefined
                                }
                                type="button"
                                onClick={() => setMode("form")}
                            >
                                Form
                            </button>
                        </div>
                    ) : canEdit && changeSet !== null ? (
                        // With the form parked, deletion moves up here so
                        // the entity-level action stays reachable.
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
                    ) : (
                        <span className="label">definition</span>
                    )}
                </div>
                {(FORM_EDITING_ENABLED ? mode : "toml") === "toml" ? (
                    <TomlEditor
                        detail={detail}
                        file={variable.location.path}
                        content={toml.content}
                        problem={toml.problem}
                        draft={toml.draft}
                        onDraft={toml.setDraft}
                        lsp={toml.lsp}
                        changeSet={editable ? changeSet : null}
                        onSaved={onSaved}
                        onError={onError}
                    />
                ) : (
                    <form
                        className="definition-form"
                        onSubmit={(event) => {
                            event.preventDefault();
                            if (canEdit) {
                                save();
                            }
                        }}
                    >
                        {canEdit ? (
                            <label className="form-row">
                                <span className="label">Description</span>
                                <input
                                    aria-label="Description"
                                    className="input"
                                    placeholder="What this variable controls"
                                    value={description}
                                    onChange={(event) =>
                                        setDescription(event.target.value)
                                    }
                                />
                            </label>
                        ) : null}
                        <div className="form-row">
                            <span className="label">Type</span>
                            <span className="form-static">
                                <span className="code-chip">
                                    <TypeLabel
                                        type={
                                            variable.declaration.kind ===
                                                "catalog" ||
                                            variable.declaration.kind === "list"
                                                ? `${variable.declaration.kind}=${type}`
                                                : type
                                        }
                                        hrefEntity={hrefEntity}
                                    />
                                </span>
                                <span className="hint">
                                    fixed after creation
                                </span>
                            </span>
                        </div>

                        {method === "allocation" ? (
                            <p className="hint">
                                This variable resolves by allocation; adjust the
                                rollout on its layer, or edit the raw file.
                            </p>
                        ) : canQuery ? (
                            // Tabs only where there is a choice: a variable
                            // that can only resolve with rules gets the
                            // summary line, not a one-tab tab bar.
                            <div className="method-tabs">
                                <div
                                    aria-label="Resolution method"
                                    className="method-tab-list"
                                    role="tablist"
                                >
                                    <button
                                        aria-selected={method === "rules"}
                                        className={
                                            method === "rules"
                                                ? "method-tab active"
                                                : "method-tab"
                                        }
                                        disabled={!canEdit}
                                        role="tab"
                                        type="button"
                                        onClick={() => setMethod("rules")}
                                    >
                                        resolve with rules
                                    </button>
                                    <button
                                        aria-selected={method === "query"}
                                        className={
                                            method === "query"
                                                ? "method-tab active"
                                                : "method-tab"
                                        }
                                        disabled={!canEdit}
                                        role="tab"
                                        title="One query selects entries from the catalog this variable is typed over"
                                        type="button"
                                        onClick={() => setMethod("query")}
                                    >
                                        resolve with a query
                                    </button>
                                </div>
                                <span className="hint">
                                    {method === "query"
                                        ? "one query selects entries; the default answers when nothing matches"
                                        : "first match wins; the default answers when none do"}
                                </span>
                            </div>
                        ) : (
                            <p className="hint">
                                First match wins; the default answers when none
                                do.
                            </p>
                        )}

                        {method === "query" ? (
                            <QueryFields
                                editable={canEdit}
                                query={query}
                                onChange={setQuery}
                            />
                        ) : null}

                        {method !== "allocation" ? (
                            <RuleLadder
                                type={type}
                                editable={canEdit}
                                withRules={method === "rules"}
                                rules={rules}
                                defaultText={defaultText}
                                verdicts={verdicts}
                                hrefEntity={hrefEntity}
                                onRules={setRules}
                                onDefaultText={setDefaultText}
                            />
                        ) : null}

                        {canEdit && method === "rules" ? (
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
                            </div>
                        ) : null}

                        {canEdit && changeSet !== null ? (
                            <div className="card-actions definition-card-actions">
                                <button
                                    className="btn btn-primary"
                                    type="submit"
                                    disabled={saving}
                                >
                                    {saving ? "Saving…" : "Save (one commit)"}
                                </button>
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
                                                label !==
                                                `variable ${variableId}`,
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
                                <span className="hint definition-save-note">
                                    writes{" "}
                                    <span className="mono">
                                        {variable.location.path
                                            .split("/")
                                            .pop()}
                                    </span>{" "}
                                    to change set {changeSet.id}
                                </span>
                            </div>
                        ) : (
                            <p className="hint">
                                Pick or start a change set (Edit in a change
                                set, above) to edit this definition.
                            </p>
                        )}
                    </form>
                )}
            </div>

            <div className="card try-card">
                {variable.usesContext ? (
                    <>
                        <div className="try-head">
                            <h2>Try it with a context</h2>
                            <span className="hint">
                                Pick a sample context: the answer updates below.
                            </span>
                        </div>
                        <div className="try-body">
                            <ContextPickerBody
                                inventory={inventory}
                                chosen={chosen}
                                boundaryVariableId={variableId}
                                canPromoteBoundary={editable}
                                emptyOptionLabel="None (no context)"
                                readPaths={variable.contextPaths}
                                onPromoteBoundary={promote}
                                onChange={onUseContext}
                            />
                        </div>
                    </>
                ) : null}
                <AnswerStrip
                    chosen={chosen}
                    outcome={outcome}
                    method={method}
                    requiresContext={variable.usesContext}
                    stale={resolutionDirty}
                    hrefEntity={hrefEntity}
                />
            </div>
        </>
    );
}

// A raw TOML editor over one package file: read-only without a change set,
// a live editor with one-commit saves inside one. The source and the draft
// belong to the owning panel (useTomlFile), so switching editor modes
// neither refetches nor discards typing. Variables and lists both ride it.
function TomlEditor({
    detail,
    file,
    content,
    problem,
    draft,
    onDraft,
    lsp,
    changeSet,
    onSaved,
    onError,
}: {
    detail: PackageDetail;
    file: string;
    content: string | null;
    problem: string | null;
    // null mirrors the committed content; a string is an unsaved draft.
    draft: string | null;
    onDraft: (draft: string | null) => void;
    lsp: LspFile | null;
    changeSet: ChangeSet | null;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
}) {
    const [saving, setSaving] = useState(false);

    if (problem !== null) {
        return <div className="banner banner-err">{problem}</div>;
    }
    if (content === null) {
        return <p className="muted">Loading definition…</p>;
    }
    const dirty = draft !== null && draft !== content;
    const save = () => {
        if (changeSet === null || draft === null) {
            return;
        }
        setSaving(true);
        saveEdit(changeSet.id, {
            packagePath: detail.path,
            expectedPin: detail.pin,
            files: [{ path: file, content: draft }],
            summary: `Edit ${file}`,
        })
            .then(onSaved, onError)
            .finally(() => setSaving(false));
    };
    return (
        <>
            <CodeEditor
                className="variable-toml"
                disabled={changeSet === null}
                language="toml"
                lsp={lsp}
                onChange={onDraft}
                value={draft ?? content}
            />
            {dirty ? (
                <div className="action-row definition-actions">
                    <button
                        className="btn btn-primary btn-sm"
                        disabled={saving}
                        type="button"
                        onClick={save}
                    >
                        {saving ? "Saving…" : "Save (one commit)"}
                    </button>
                    <button
                        className="btn btn-ghost btn-sm"
                        disabled={saving}
                        type="button"
                        onClick={() => onDraft(null)}
                    >
                        Discard
                    </button>
                </div>
            ) : null}
        </>
    );
}

// How one resolution walked the ladder: a verdict per rule row, and one for
// the otherwise row (the default answers only when no rule matched).
type LadderVerdict = "matched" | "no-match" | "not-reached" | "answers";

const VERDICT_TEXT: Record<LadderVerdict, string> = {
    matched: "matched",
    "no-match": "no match",
    "not-reached": "not reached",
    answers: "answers",
};

function ladderVerdicts(trace: ResolutionTrace): {
    rows: LadderVerdict[];
    otherwise: LadderVerdict;
} {
    const matchedAt = trace.rules.findIndex((rule) => rule.matched);
    return {
        rows: trace.rules.map((rule, index) =>
            rule.matched
                ? "matched"
                : matchedAt !== -1 && index > matchedAt
                  ? "not-reached"
                  : "no-match",
        ),
        otherwise: matchedAt === -1 ? "answers" : "not-reached",
    };
}

function VerdictCell({ verdict }: { verdict: LadderVerdict | undefined }) {
    if (verdict === undefined) {
        return <span />;
    }
    const ok = verdict === "matched" || verdict === "answers";
    return (
        <span className={`pill ${ok ? "pill-ok" : "pill-neutral"}`}>
            {VERDICT_TEXT[verdict]}
        </span>
    );
}

// The resolution ladder: the rules in priority order and the default as the
// closing "otherwise" row, one list serving both reading and editing. With
// a context chosen it doubles as the trace, each row carrying its verdict.
function RuleLadder({
    type,
    editable,
    withRules,
    rules,
    defaultText,
    verdicts,
    hrefEntity,
    onRules,
    onDefaultText,
}: {
    type: string;
    editable: boolean;
    // A query variable keeps only the otherwise row: the query answers
    // first, the default when nothing matches.
    withRules: boolean;
    rules: RuleDraft[];
    defaultText: string;
    verdicts: { rows: LadderVerdict[]; otherwise: LadderVerdict } | null;
    hrefEntity: (steps: AddressStep[]) => string;
    onRules: (rules: RuleDraft[]) => void;
    onDefaultText: (value: string) => void;
}) {
    // A query variable with no declared default has no otherwise row to
    // read; editing still offers it so a default can be added.
    const withOtherwise = editable || withRules || defaultText !== "";
    return (
        <div
            className="ladder"
            data-verdicts={verdicts !== null || undefined}
            data-editable={editable || undefined}
        >
            {withRules
                ? rules.map((rule, index) => (
                      <div
                          className="ladder-row"
                          data-state={verdicts?.rows[index]}
                          key={index}
                      >
                          <span className="ladder-idx label">
                              rule {index + 1}
                          </span>
                          {verdicts !== null ? (
                              <VerdictCell verdict={verdicts.rows[index]} />
                          ) : null}
                          {editable ? (
                              <input
                                  aria-label={`Rule ${index + 1} condition`}
                                  className="input mono"
                                  value={rule.when}
                                  onChange={(event) =>
                                      onRules(
                                          replaceAt(rules, index, {
                                              ...rule,
                                              when: event.target.value,
                                          }),
                                      )
                                  }
                              />
                          ) : (
                              <span className="mono ladder-expr">
                                  <ExpressionText
                                      text={rule.when}
                                      hrefFor={hrefEntity}
                                  />
                              </span>
                          )}
                          <span className="ladder-arrow label">→</span>
                          {editable ? (
                              <ValueInput
                                  type={type}
                                  disabled={false}
                                  value={rule.valueText}
                                  onChange={(valueText) =>
                                      onRules(
                                          replaceAt(rules, index, {
                                              ...rule,
                                              valueText,
                                          }),
                                      )
                                  }
                              />
                          ) : (
                              <span
                                  className="mono ladder-value"
                                  title={rule.valueText}
                              >
                                  {rule.valueText}
                              </span>
                          )}
                          {editable ? (
                              <span className="action-row ladder-actions">
                                  <button
                                      className="btn btn-icon btn-sm"
                                      type="button"
                                      disabled={index === 0}
                                      title="Move up"
                                      onClick={() =>
                                          onRules(
                                              moveRule(rules, index, index - 1),
                                          )
                                      }
                                  >
                                      ↑
                                  </button>
                                  <button
                                      className="btn btn-icon btn-sm"
                                      type="button"
                                      disabled={index === rules.length - 1}
                                      title="Move down"
                                      onClick={() =>
                                          onRules(
                                              moveRule(rules, index, index + 1),
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
                                          onRules(
                                              rules.filter(
                                                  (_, i) => i !== index,
                                              ),
                                          )
                                      }
                                  >
                                      ×
                                  </button>
                              </span>
                          ) : null}
                      </div>
                  ))
                : null}
            {withOtherwise ? (
                <div className="ladder-row" data-state={verdicts?.otherwise}>
                    <span className="ladder-idx label">default</span>
                    {verdicts !== null ? (
                        <VerdictCell verdict={verdicts.otherwise} />
                    ) : null}
                    <span className="hint">
                        {withRules
                            ? "when no rule matches"
                            : "when nothing matches"}
                    </span>
                    <span className="ladder-arrow label">→</span>
                    {editable ? (
                        <ValueInput
                            type={type}
                            disabled={false}
                            value={defaultText}
                            onChange={onDefaultText}
                        />
                    ) : defaultText !== "" ? (
                        <span className="mono ladder-value" title={defaultText}>
                            {defaultText}
                        </span>
                    ) : (
                        <span className="muted">none</span>
                    )}
                </div>
            ) : null}
        </div>
    );
}

// The resolution method, named: the middle of the model sentence (given a
// context, a variable yields a value by employing a method).
function methodText(variable: VariableModel): string {
    const method = methodOf(variable);
    if (method === "query") {
        const from = variable.resolve?.query?.from?.value;
        return `query from ${from ?? "?"}`;
    }
    if (method === "allocation") {
        return "allocation";
    }
    return (variable.resolve?.rules ?? []).length > 0
        ? "first-match rules"
        : "default only";
}

// The same method, with the query's source catalog as a link.
function MethodLabel({
    variable,
    hrefEntity,
}: {
    variable: VariableModel;
    hrefEntity: (steps: AddressStep[]) => string;
}) {
    const from = variable.resolve?.query?.from?.value;
    if (methodOf(variable) === "query" && typeof from === "string") {
        return (
            <>
                query from{" "}
                <a
                    className="expr-link"
                    href={hrefEntity([{ class: "catalog", id: from }])}
                >
                    {from}
                </a>
            </>
        );
    }
    return <>{methodText(variable)}</>;
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
    // The variable form switches methods with its own tabs and omits this;
    // callers without tabs pass the way back to rules.
    onUseRules?: () => void;
}) {
    const set = (key: keyof QueryDraft) => (value: string) =>
        onChange({ ...query, [key]: value });
    return (
        <>
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
            {editable && onUseRules !== undefined ? (
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
    onLiveLint,
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
    onLiveLint: (report: LiveLintReport) => void;
}) {
    const [content, setContent] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);
    // Live diagnostics from the LSP bridge track the unsaved buffer; until
    // the first publication arrives the staged lint report stands in.
    const [live, setLive] = useState<
        { severity: string; rule?: string; message: string }[] | null
    >(null);
    const [lspFile, setLspFile] = useState<LspFile | null>(null);

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

    // One language-server session per open file: the editor streams the
    // buffer to it and renders its squiggles, completion, and hover; this
    // panel keeps the same publications as its diagnostics list. A bridge
    // failure just means the static lint diagnostics stand.
    useEffect(() => {
        const session = new LspFile(treeId, detail.path, detail.pin, file);
        setLspFile(session);
        const unsubscribe = session.onDiagnostics((published) => {
            const diagnostics = published.map((diagnostic) => ({
                severity:
                    diagnostic.severity === 1
                        ? ("error" as const)
                        : ("warning" as const),
                rule: diagnostic.code,
                message: diagnostic.message,
                location: { path: file },
            }));
            setLive(diagnostics);
            onLiveLint({ file, diagnostics });
        });
        return () => {
            unsubscribe();
            session.dispose();
            setLspFile(null);
            onLiveLint(null);
        };
    }, [treeId, detail.path, detail.pin, file, onLiveLint]);

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
                <CodeEditor
                    className="raw-file-editor"
                    disabled={!editable}
                    language={codeLanguageForPath(file)}
                    lsp={lspFile}
                    value={content}
                    onChange={setContent}
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

// The members of a list being created, as editable chips: type a value and
// Enter (or comma) adds it, click a chip's text to change it, × drops it.
// Values stay raw text here; the declared member type converts them on
// submit, so switching the type mid-form never strands a chip.
function MemberChipsEditor({
    type,
    members,
    disabled,
    onChange,
    draft: controlledDraft,
    onDraft,
}: {
    type: string;
    members: string[];
    disabled: boolean;
    onChange: (members: string[]) => void;
    // The add input's uncommitted text, lifted when the owner wants typed
    // text to count before Enter or blur commits it (a save that ignores
    // what is visibly in the box would surprise).
    draft?: string;
    onDraft?: (draft: string) => void;
}) {
    const [localDraft, setLocalDraft] = useState("");
    const draft = controlledDraft ?? localDraft;
    const setDraft = onDraft ?? setLocalDraft;
    // null shows the chips; an index is the member being edited in place.
    const [editing, setEditing] = useState<number | null>(null);
    const [editText, setEditText] = useState("");

    const addDraft = () => {
        const parts = draft
            .split(",")
            .map((text) => text.trim())
            .filter((text) => text !== "");
        if (parts.length > 0) {
            onChange([...members, ...parts]);
        }
        setDraft("");
    };
    const commitEdit = () => {
        if (editing !== null) {
            const text = editText.trim();
            onChange(
                text === ""
                    ? members.filter((_, index) => index !== editing)
                    : replaceAt(members, editing, text),
            );
        }
        setEditing(null);
        setEditText("");
    };

    return (
        <div className="context-facts">
            {members.map((member, index) =>
                editing === index ? (
                    <input
                        autoFocus
                        aria-label={`Edit member ${member}`}
                        className="input mono context-fact-input"
                        key={index}
                        size={Math.max(editText.length + 2, 6)}
                        value={editText}
                        onBlur={commitEdit}
                        onChange={(event) => setEditText(event.target.value)}
                        onKeyDown={(event) => {
                            if (event.key === "Enter") {
                                event.preventDefault();
                                commitEdit();
                            }
                        }}
                    />
                ) : (
                    <span className="context-fact" key={index}>
                        <button
                            className="context-fact-value"
                            disabled={disabled}
                            title="Edit this member"
                            type="button"
                            onClick={() => {
                                setEditing(index);
                                setEditText(member);
                            }}
                        >
                            {member}
                        </button>
                        <button
                            className="context-fact-remove"
                            disabled={disabled}
                            title={`Remove ${member}`}
                            type="button"
                            onClick={() =>
                                onChange(members.filter((_, i) => i !== index))
                            }
                        >
                            ×
                        </button>
                    </span>
                ),
            )}
            <input
                aria-label="Add member"
                className="input mono member-add-input"
                disabled={disabled}
                placeholder={`add ${type} member`}
                value={draft}
                onBlur={addDraft}
                onChange={(event) => setDraft(event.target.value)}
                onKeyDown={(event) => {
                    // Enter adds the typed member; with nothing typed it
                    // falls through and submits the surrounding form.
                    if (
                        (event.key === "Enter" || event.key === ",") &&
                        draft.trim() !== ""
                    ) {
                        event.preventDefault();
                        addDraft();
                    }
                }}
            />
        </div>
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
    const [memberTexts, setMemberTexts] = useState<string[]>([]);
    const [memberDraft, setMemberDraft] = useState("");
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
        // Text still sitting in the add box counts: Enter on the id field
        // submits the form without ever blurring the member input.
        const pendingTexts = memberDraft
            .split(",")
            .map((text) => text.trim())
            .filter((text) => text !== "" && !memberTexts.includes(text));
        let members: unknown[];
        try {
            members = [...memberTexts, ...pendingTexts].map((text) =>
                textToValue(text, type),
            );
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
            className="new-list-form"
            onSubmit={(event) => {
                event.preventDefault();
                submit();
            }}
        >
            <div className="new-list-head">
                <input
                    autoFocus
                    aria-label="List id"
                    className="input mono"
                    placeholder="list_id"
                    value={id}
                    onChange={(event) => setId(event.target.value)}
                />
                <select
                    aria-label="Member type"
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
            </div>
            <MemberChipsEditor
                type={type}
                members={memberTexts}
                disabled={busy}
                onChange={setMemberTexts}
                draft={memberDraft}
                onDraft={setMemberDraft}
            />
            <div className="action-row">
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
            </div>
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
