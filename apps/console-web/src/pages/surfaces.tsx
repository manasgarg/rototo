// The Domain lens (tranche C4): surfaces at floor fidelity. A surface is a
// catalog entry in console/surfaces; the floor renders each binding with a
// control inferred from its type, and a control emits exactly its
// operations — the affordance boundary, with enforcement below. Empty
// states propose their next step, and the next step is a change set.

import {
    Component,
    useCallback,
    useEffect,
    useMemo,
    useState,
    type ReactNode,
} from "react";

import type { ExperienceRead } from "@/extension-api.ts";
import {
    ApiError,
    createChangeSet,
    fetchContexts,
    fetchSurface,
    fetchSurfaces,
    listChangeSets,
    listPackages,
    runPreview,
    saveEdit,
    type ChangeSet,
    type Control,
    type EditOperation,
    type EditResponse,
    type MeResponse,
    type PackageListing,
    type SurfaceDetail,
    type SurfaceItem,
    type SurfaceList,
    type SurfaceSuggestion,
} from "@/lib/api";
import { experienceFor } from "@/lib/experiences";
import { formatInstant } from "@/lib/format";
import { githubCommitUrl } from "@/lib/github";
import {
    ControlInput,
    SearchableList,
    SearchControl,
    UI_KIT,
} from "@/lib/ui-kit";
import {
    changeSetUrl,
    navigate,
    packageUrl,
    parseAddress,
    type AddressStep,
    type ViewState,
} from "@/lib/router";
import { DeleteButton, EditingStrip } from "@/pages/workbench";
import type { SourceTreeSummary } from "@/lib/api";

type Banner = { kind: "ok" | "err" | "warn"; text: string };

export function SurfacesPage({
    me,
    treeId,
    packagePath,
    surfaceId,
    state,
}: {
    me: MeResponse;
    treeId: string;
    packagePath: string;
    surfaceId: string | null;
    state: ViewState;
}) {
    const tree = me.capabilities?.sourceTrees.find(
        (candidate) => candidate.id === treeId,
    );
    const [changeSets, setChangeSets] = useState<ChangeSet[]>([]);
    const [listing, setListing] = useState<PackageListing | null>(null);
    const [surfaces, setSurfaces] = useState<SurfaceList | null>(null);
    const [banner, setBanner] = useState<Banner | null>(null);

    const active =
        changeSets.find((entry) => entry.id === state.changeSetId) ?? null;
    const editable =
        active !== null &&
        (active.state === "draft" || active.state === "proposed");

    // Move within the surfaces lens without losing the URL's view state.
    const go = useCallback(
        (nextSurface: string | null, patch?: Partial<ViewState>) => {
            navigate(
                packageUrl(
                    treeId,
                    packagePath,
                    { kind: "surfaces", surfaceId: nextSurface },
                    { ...state, ...patch },
                ),
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

    const ref = active?.branch;
    useEffect(() => {
        let stale = false;
        setListing(null);
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

    const pin = listing?.pin ?? null;
    useEffect(() => {
        if (pin === null) {
            return;
        }
        let stale = false;
        setSurfaces(null);
        fetchSurfaces(treeId, packagePath, pin).then(
            (response) => {
                if (!stale) {
                    setSurfaces(response);
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

    // Accepting a suggestion is the user's first change set: create one,
    // apply the ready-made operations (the first vendors the schema), and
    // land on the editing strip with it active.
    const acceptSuggestion = (suggestion: SurfaceSuggestion) => {
        if (listing === null) {
            return;
        }
        createChangeSet(treeId, `Add the ${suggestion.title} surface`).then(
            (changeSet) => {
                setChangeSets((current) => [changeSet, ...current]);
                go(surfaceId, { changeSetId: changeSet.id });
                saveEdit(changeSet.id, {
                    packagePath,
                    expectedPin: changeSet.baseShaAtCreation ?? listing.pin,
                    operations: suggestion.operations,
                    summary: `Add the ${suggestion.title} surface`,
                }).then(afterSave, saveFailed);
            },
            saveFailed,
        );
    };

    // Vendoring the lint script is one raw-file change set: the same
    // dangling-binding failures the console shows land in the package's CI.
    const acceptLintScript = () => {
        if (
            listing === null ||
            surfaces === null ||
            surfaces.lintScript.content === undefined
        ) {
            return;
        }
        const { path, content } = surfaces.lintScript;
        createChangeSet(treeId, "Vendor the console surfaces lint").then(
            (changeSet) => {
                setChangeSets((current) => [changeSet, ...current]);
                go(surfaceId, { changeSetId: changeSet.id });
                saveEdit(changeSet.id, {
                    packagePath,
                    expectedPin: changeSet.baseShaAtCreation ?? listing.pin,
                    files: [{ path, content }],
                    summary: "Vendor the console surfaces lint",
                }).then(afterSave, saveFailed);
            },
            saveFailed,
        );
    };

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
                    <h1>Surfaces</h1>
                    <p className="hint">
                        {treeName}
                        {listing !== null
                            ? ` · ${listing.ref} @ ${listing.pin.slice(0, 10)}`
                            : ""}
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
                    go(surfaceId, { changeSetId: id });
                }}
                onCreated={(changeSet) => {
                    setChangeSets((current) => [changeSet, ...current]);
                    setBanner(null);
                    go(surfaceId, { changeSetId: changeSet.id });
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

            {surfaces === null || pin === null ? (
                <p className="muted">Loading…</p>
            ) : surfaceId !== null ? (
                <SurfacePanel
                    key={`${surfaceId}@${pin}`}
                    treeId={treeId}
                    tree={tree}
                    packagePath={packagePath}
                    pin={pin}
                    surfaceId={surfaceId}
                    editable={editable}
                    changeSet={active}
                    state={state}
                    onBack={() => go(null)}
                    onSaved={afterSave}
                    onError={saveFailed}
                />
            ) : (
                <SurfaceCatalog
                    surfaces={surfaces}
                    hrefFor={(id) =>
                        `#${packageUrl(
                            treeId,
                            packagePath,
                            { kind: "surfaces", surfaceId: id },
                            state,
                        )}`
                    }
                    onAccept={acceptSuggestion}
                    onVendorLint={acceptLintScript}
                    canPropose={tree.capabilities.propose.allow}
                />
            )}
        </div>
    );
}

function SurfaceCatalog({
    surfaces,
    hrefFor,
    onAccept,
    onVendorLint,
    canPropose,
}: {
    surfaces: SurfaceList;
    hrefFor: (id: string) => string;
    onAccept: (suggestion: SurfaceSuggestion) => void;
    onVendorLint: () => void;
    canPropose: boolean;
}) {
    if (surfaces.surfaces.length === 0) {
        return (
            <div className="card">
                <h2>No surfaces yet</h2>
                <p className="hint">
                    This package works fully without them; a surface is a
                    curated view for the people who should never need rototo's
                    vocabulary. The package's shape suggests these:
                </p>
                <div className="row-list">
                    {surfaces.suggestions.map((suggestion) => (
                        <div className="row row-static" key={suggestion.id}>
                            <span className="row-text">
                                <span className="row-title">
                                    {suggestion.title}
                                </span>
                                <span className="row-sub">
                                    {suggestion.reason}
                                </span>
                            </span>
                            <span className="row-side">
                                {canPropose ? (
                                    <button
                                        className="btn btn-secondary btn-sm"
                                        onClick={() => onAccept(suggestion)}
                                    >
                                        Draft change set
                                    </button>
                                ) : (
                                    <span className="pill pill-neutral">
                                        {suggestion.kind}
                                    </span>
                                )}
                            </span>
                        </div>
                    ))}
                </div>
                {surfaces.suggestions.length === 0 ? (
                    <p className="hint">
                        Nothing to suggest: the package declares no catalogs or
                        bool variables yet.
                    </p>
                ) : null}
            </div>
        );
    }
    return (
        <>
            {surfaces.diagnostics.map((diagnostic, index) => (
                <div className="banner banner-info" key={index}>
                    {diagnostic.message}
                </div>
            ))}
            {!surfaces.lintScript.vendored && canPropose ? (
                <div className="card">
                    <h3>Surface checks in CI</h3>
                    <p className="hint">
                        This package's surfaces are validated only when the
                        console looks at them. Vendor{" "}
                        <span className="mono">{surfaces.lintScript.path}</span>{" "}
                        and dangling bindings fail CI too.
                    </p>
                    <div className="action-row">
                        <button
                            className="btn btn-secondary btn-sm"
                            onClick={onVendorLint}
                        >
                            Draft change set
                        </button>
                    </div>
                </div>
            ) : null}
            <SearchableList
                label="Search surfaces"
                placeholder="Search surfaces"
                emptyLabel="No surface matches that search."
                className="row-list"
            >
                {surfaces.surfaces.map((surface) => {
                    const errors = surface.diagnostics.filter(
                        (diagnostic) => diagnostic.severity === "error",
                    );
                    return (
                        <a
                            className="row"
                            key={surface.id}
                            href={hrefFor(surface.id)}
                            data-search={`${surface.title} ${surface.description ?? ""} ${surface.bindings.map((binding) => binding.target).join(" ")} ${surface.kind ?? ""}`}
                        >
                            <span className="row-text">
                                <span className="row-title">
                                    {surface.title}
                                </span>
                                <span className="row-sub">
                                    {surface.description ??
                                        surface.bindings
                                            .map((binding) => binding.target)
                                            .join(", ")}
                                </span>
                            </span>
                            <span className="row-side">
                                {surface.kind !== null ? (
                                    <span className="pill pill-neutral">
                                        {experienceFor(surface.kind) !== null
                                            ? surface.kind
                                            : `${surface.kind} · floor`}
                                    </span>
                                ) : null}
                                {surface.approval !== null ? (
                                    <span
                                        className="pill pill-info"
                                        title="Approval requirement; GitHub enforces in this phase"
                                    >
                                        {surface.approval}
                                    </span>
                                ) : null}
                                {errors.length > 0 ? (
                                    <span
                                        className="pill pill-err"
                                        title={errors[0]?.message}
                                    >
                                        {errors.length} problem
                                        {errors.length === 1 ? "" : "s"}
                                    </span>
                                ) : null}
                            </span>
                        </a>
                    );
                })}
            </SearchableList>
        </>
    );
}

// --- one surface: the floor renderer plus the four read affordances ---

function SurfacePanel({
    treeId,
    tree,
    packagePath,
    pin,
    surfaceId,
    editable,
    changeSet,
    state,
    onBack,
    onSaved,
    onError,
}: {
    treeId: string;
    tree: SourceTreeSummary;
    packagePath: string;
    pin: string;
    surfaceId: string;
    editable: boolean;
    changeSet: ChangeSet | null;
    state: ViewState;
    onBack: () => void;
    onSaved: (result: EditResponse) => void;
    onError: (error: unknown) => void;
}) {
    const [detail, setDetail] = useState<SurfaceDetail | null>(null);
    const [saving, setSaving] = useState(false);

    useEffect(() => {
        let stale = false;
        fetchSurface(treeId, packagePath, pin, surfaceId).then(
            (response) => {
                if (!stale) {
                    setDetail(response);
                }
            },
            (error: Error) => onError(error),
        );
        return () => {
            stale = true;
        };
    }, [treeId, packagePath, pin, surfaceId, onError]);

    // The read capability an experience receives: the read side the host
    // already fetched, plus contexts and previews scoped by the server.
    const read = useMemo<ExperienceRead | null>(
        () =>
            detail === null
                ? null
                : {
                      pin,
                      packagePath,
                      upcoming: detail.upcoming,
                      history: detail.history,
                      pending: detail.pending,
                      contexts: () => fetchContexts(treeId, packagePath, pin),
                      preview: (context) =>
                          runPreview(treeId, packagePath, pin, context).then(
                              (response) => response.outcomes,
                          ),
                  },
        [treeId, packagePath, pin, detail],
    );

    if (detail === null || read === null) {
        return <p className="muted">Loading surface…</p>;
    }
    const surface = detail.surface;

    const propose = (operations: EditOperation[], summary: string) => {
        if (changeSet === null) {
            return;
        }
        setSaving(true);
        saveEdit(changeSet.id, {
            packagePath,
            expectedPin: pin,
            operations,
            summary,
        })
            .then(onSaved, onError)
            .finally(() => setSaving(false));
    };

    // Every identifier a binding shows can open in the workbench, with the
    // URL's view state (change set, context) carried along.
    const hrefEntity = (steps: AddressStep[]): string =>
        `#${packageUrl(treeId, packagePath, { kind: "address", steps }, state)}`;

    const experience = experienceFor(surface.kind);
    const floor = (
        <>
            {detail.items.map((item, index) => (
                <SurfaceItemView
                    key={index}
                    item={item}
                    editable={editable && !saving}
                    hrefEntity={hrefEntity}
                    onPropose={propose}
                />
            ))}
        </>
    );

    return (
        <div className="section">
            <div className="card">
                <div className="card-head">
                    <div className="card-head-text">
                        <h2>{surface.title}</h2>
                        <p className="hint">{surface.description ?? ""}</p>
                    </div>
                    <span className="action-row">
                        {editable && changeSet !== null ? (
                            <DeleteButton
                                label="Delete surface"
                                warning={`This removes the "${surface.title}" surface entry; the configuration it binds stays untouched.`}
                                onConfirm={() =>
                                    saveEdit(changeSet.id, {
                                        packagePath,
                                        expectedPin: pin,
                                        operations: [
                                            {
                                                op: "delete",
                                                target: `catalog=console/surfaces:entry=${surfaceId}`,
                                            },
                                        ],
                                        summary: `Delete surface ${surfaceId}`,
                                    }).then((result) => {
                                        onSaved(result);
                                        onBack();
                                    }, onError)
                                }
                            />
                        ) : null}
                        <button
                            className="btn btn-ghost btn-sm"
                            onClick={onBack}
                        >
                            Back
                        </button>
                    </span>
                </div>
                {surface.caution !== null ? (
                    <div className="banner banner-warn">{surface.caution}</div>
                ) : null}
                {surface.diagnostics
                    .filter((diagnostic) => diagnostic.severity === "error")
                    .map((diagnostic, index) => (
                        <div className="banner banner-err" key={index}>
                            {diagnostic.message}
                        </div>
                    ))}
                {!editable ? (
                    <p className="hint">
                        Start or pick a change set above to edit through this
                        surface.
                    </p>
                ) : null}
                {experience !== null ? (
                    <ExperienceBoundary
                        note={`The "${surface.kind}" experience failed to render; showing the floor.`}
                        fallback={floor}
                    >
                        <experience.Render
                            surface={surface}
                            items={detail.items}
                            editable={editable && !saving}
                            now={detail.now}
                            read={read}
                            propose={propose}
                            ui={UI_KIT}
                            openWorkbench={() =>
                                navigate(
                                    packageUrl(
                                        treeId,
                                        packagePath,
                                        { kind: "overview" },
                                        state,
                                    ),
                                )
                            }
                        />
                    </ExperienceBoundary>
                ) : (
                    <>
                        {surface.kind !== null ? (
                            <p className="hint">
                                No installed experience renders kind "
                                {surface.kind}"; showing the floor.
                            </p>
                        ) : null}
                        {floor}
                    </>
                )}
            </div>

            {detail.upcoming.length > 0 ? (
                <div className="card">
                    <h3>Scheduled to change on its own</h3>
                    {detail.upcoming.map((change, index) => (
                        <p className="diagnostic" key={index}>
                            <a
                                className="row-link mono"
                                href={hrefEntity([
                                    { class: "variable", id: change.variable },
                                ])}
                            >
                                {change.variable}
                            </a>{" "}
                            crosses{" "}
                            <span className="mono">{change.boundary}</span> (
                            {change.expression})
                        </p>
                    ))}
                </div>
            ) : null}

            {detail.pending.length > 0 ? (
                <div className="card">
                    <h3>Pending change sets touching this surface</h3>
                    <div className="row-list">
                        {detail.pending.map((row) => (
                            <a
                                className="row"
                                key={row.id}
                                href={`#${changeSetUrl(treeId, row.id)}`}
                            >
                                <span className="row-text">
                                    <span className="row-title">
                                        {row.title}
                                    </span>
                                    {row.prNumber !== null ? (
                                        <span className="row-sub mono">
                                            PR #{row.prNumber}
                                        </span>
                                    ) : null}
                                </span>
                                <span className="row-side">
                                    <span className="pill pill-info">
                                        {row.state}
                                    </span>
                                </span>
                            </a>
                        ))}
                    </div>
                </div>
            ) : null}

            <div className="card">
                <h3>History of what this surface binds</h3>
                <div className="timeline">
                    {detail.history.map((commit) => {
                        const commitUrl = githubCommitUrl(tree, commit.sha);
                        return (
                            <div className="tl-row" key={commit.sha}>
                                <span className="tl-icon" aria-hidden>
                                    •
                                </span>
                                <span className="tl-body">
                                    <span className="tl-detail">
                                        {commit.message}
                                        {commit.authorName !== null
                                            ? ` — ${commit.authorName}`
                                            : ""}
                                    </span>
                                    <span className="tl-when">
                                        {commit.date.slice(0, 10)} ·{" "}
                                        {commitUrl !== null ? (
                                            <a
                                                className="row-link mono"
                                                href={commitUrl}
                                                rel="noreferrer"
                                                target="_blank"
                                                title="Open this commit on GitHub"
                                            >
                                                {commit.sha.slice(0, 10)}
                                            </a>
                                        ) : (
                                            <span className="mono">
                                                {commit.sha.slice(0, 10)}
                                            </span>
                                        )}
                                    </span>
                                </span>
                            </div>
                        );
                    })}
                </div>
            </div>
        </div>
    );
}

// Degradation (design/console-surfaces.md): an experience that throws
// renders as the floor plus a plain note. A missing or broken extension
// never breaks a deployment and never hides configuration; it only makes
// it plainer.
class ExperienceBoundary extends Component<
    { note: string; fallback: ReactNode; children: ReactNode },
    { failed: boolean }
> {
    override state = { failed: false };

    static getDerivedStateFromError(): { failed: boolean } {
        return { failed: true };
    }

    override render(): ReactNode {
        if (this.state.failed) {
            return (
                <>
                    <div className="banner banner-warn">{this.props.note}</div>
                    {this.props.fallback}
                </>
            );
        }
        return this.props.children;
    }
}

function SurfaceItemView({
    item,
    editable,
    hrefEntity,
    onPropose,
}: {
    item: SurfaceItem;
    editable: boolean;
    hrefEntity: (steps: AddressStep[]) => string;
    onPropose: (operations: EditOperation[], summary: string) => void;
}) {
    if (item.kind === "missing") {
        const steps = parseAddress(item.target);
        return (
            <div className="banner banner-err">
                Binding{" "}
                {steps === null ? (
                    <span className="mono">{item.target}</span>
                ) : (
                    <a className="row-link mono" href={hrefEntity(steps)}>
                        {item.target}
                    </a>
                )}{" "}
                resolves to nothing at this pin.
            </div>
        );
    }
    if (item.kind === "variable") {
        return (
            <div className="field-row surface-item">
                <a
                    className="row-link label"
                    title={item.description ?? undefined}
                    href={hrefEntity([{ class: "variable", id: item.id }])}
                >
                    {item.id}
                </a>
                <ControlInput
                    control={item.control}
                    value={item.default}
                    disabled={!editable}
                    onCommit={(value) =>
                        onPropose(
                            [
                                {
                                    op: "set_default",
                                    variable: item.id,
                                    value,
                                },
                            ],
                            `Set ${item.id} default`,
                        )
                    }
                />
                {item.ruleCount > 0 ? (
                    <a
                        className="hint row-link"
                        title="Rules may answer before this default; the workbench shows them"
                        href={hrefEntity([{ class: "variable", id: item.id }])}
                    >
                        +{item.ruleCount} rule{item.ruleCount === 1 ? "" : "s"}
                    </a>
                ) : null}
            </div>
        );
    }
    if (item.kind === "entry") {
        return (
            <EntryTable
                catalog={item.catalog}
                fields={item.fields}
                entries={[{ key: item.key, value: item.value }]}
                canDelete={false}
                editable={editable}
                onPropose={onPropose}
            />
        );
    }
    if (item.kind === "layer") {
        // The floor's layer: the allocation list with a range dial at text
        // fidelity. Each arm's bucket range commits one set_arm_buckets;
        // the status select commits one set_allocation_status.
        return (
            <div className="surface-item">
                <div className="section-header-text">
                    <h3 className="mono">
                        layer{" "}
                        <a
                            className="row-link"
                            href={hrefEntity([{ class: "layer", id: item.id }])}
                        >
                            {item.id}
                        </a>
                    </h3>
                    {item.description !== null ? (
                        <p className="hint">{item.description}</p>
                    ) : null}
                </div>
                {item.allocations.map((allocation, index) => (
                    <div className="field-row surface-item" key={index}>
                        <span className="label mono">
                            {allocation.id ?? `#${index}`}
                        </span>
                        <select
                            className="input"
                            disabled={!editable || allocation.id === null}
                            value={allocation.status ?? "draft"}
                            onChange={(event) =>
                                onPropose(
                                    [
                                        {
                                            op: "set_allocation_status",
                                            layer: item.id,
                                            id: allocation.id,
                                            status: event.target.value,
                                        },
                                    ],
                                    `Set ${item.id}/${allocation.id} ${event.target.value}`,
                                )
                            }
                        >
                            {["draft", "running", "concluded"].map((status) => (
                                <option key={status} value={status}>
                                    {status}
                                </option>
                            ))}
                        </select>
                        {allocation.arms.map((arm, armIndex) => (
                            <span
                                className="field-row"
                                key={armIndex}
                                title={`arm ${arm.name ?? armIndex}`}
                            >
                                <span className="hint mono">
                                    {arm.name ?? `arm ${armIndex}`}
                                </span>
                                <ControlInput
                                    control={{ control: "text" }}
                                    value={arm.buckets}
                                    disabled={
                                        !editable ||
                                        allocation.id === null ||
                                        arm.name === null
                                    }
                                    onCommit={(value) =>
                                        onPropose(
                                            [
                                                {
                                                    op: "set_arm_buckets",
                                                    layer: item.id,
                                                    allocation: allocation.id,
                                                    arm: arm.name,
                                                    buckets: value,
                                                },
                                            ],
                                            `Set ${item.id}/${allocation.id} ${arm.name} buckets`,
                                        )
                                    }
                                />
                            </span>
                        ))}
                        <span
                            className="field-row"
                            title="CEL eligibility; empty means everyone"
                        >
                            <span className="hint">eligible</span>
                            <ControlInput
                                control={{ control: "text" }}
                                value={allocation.eligibility}
                                disabled={!editable || allocation.id === null}
                                onCommit={(value) => {
                                    const when = String(value ?? "").trim();
                                    onPropose(
                                        [
                                            {
                                                op: "set_allocation_eligibility",
                                                layer: item.id,
                                                id: allocation.id,
                                                ...(when === ""
                                                    ? {}
                                                    : { when }),
                                            },
                                        ],
                                        `Set ${item.id}/${allocation.id} eligibility`,
                                    );
                                }}
                            />
                        </span>
                        {allocation.variables.length > 0 ? (
                            <span className="hint">
                                drives {allocation.variables.join(", ")}
                            </span>
                        ) : null}
                        <button
                            className="btn btn-icon btn-sm btn-remove"
                            disabled={!editable || allocation.id === null}
                            title="Remove allocation: ends the experiment or rollout"
                            onClick={() =>
                                onPropose(
                                    [
                                        {
                                            op: "remove_allocation",
                                            layer: item.id,
                                            id: allocation.id,
                                        },
                                    ],
                                    `Remove ${item.id}/${allocation.id}`,
                                )
                            }
                        >
                            ×
                        </button>
                    </div>
                ))}
                {editable ? (
                    <AddAllocationForm layer={item.id} onPropose={onPropose} />
                ) : null}
            </div>
        );
    }
    return (
        <div className="surface-item">
            <div className="section-header-text">
                <h3 className="mono">
                    <a
                        className="row-link"
                        href={hrefEntity([{ class: "catalog", id: item.id }])}
                    >
                        {item.id}
                    </a>
                </h3>
                {item.description !== null ? (
                    <p className="hint">{item.description}</p>
                ) : null}
            </div>
            <EntryTable
                catalog={item.id}
                fields={item.fields}
                entries={item.entries}
                canDelete={item.canDelete}
                editable={editable}
                onPropose={onPropose}
            />
            {item.canAdd && editable ? (
                <AddEntryForm catalog={item.id} onPropose={onPropose} />
            ) : null}
        </div>
    );
}

// The floor's table: one row per entry, one schema-driven cell per editable
// field. A cell edit emits exactly one set_field against the entry address.
function EntryTable({
    catalog,
    fields,
    entries,
    canDelete,
    editable,
    onPropose,
}: {
    catalog: string;
    fields: { field: string; control: string; options?: unknown[] }[];
    entries: { key: string; value: unknown }[];
    canDelete: boolean;
    editable: boolean;
    onPropose: (operations: EditOperation[], summary: string) => void;
}) {
    // A table can't host the SearchableList wrapper, so the same search
    // affordance filters the rows directly, matching keys and field values.
    const [query, setQuery] = useState("");
    const needle = query.trim().toLowerCase();
    const visible =
        needle === ""
            ? entries
            : entries.filter((entry) =>
                  `${entry.key} ${fields
                      .map((field) =>
                          String(fieldValue(entry.value, field.field) ?? ""),
                      )
                      .join(" ")}`
                      .toLowerCase()
                      .includes(needle),
              );
    return (
        <div className="searchable-list">
            <SearchControl
                label="Search entries"
                placeholder="Search entries"
                query={query}
                onChange={setQuery}
            />
            {visible.length === 0 ? (
                <p className="hint">No entry matches that search.</p>
            ) : (
                <div className="table-scroll">
                    <table className="data-table">
                        <thead>
                            <tr>
                                <th>entry</th>
                                {fields.map((field) => (
                                    <th key={field.field} className="mono">
                                        {field.field}
                                    </th>
                                ))}
                                {canDelete ? <th /> : null}
                            </tr>
                        </thead>
                        <tbody>
                            {visible.map((entry) => (
                                <tr key={entry.key}>
                                    <td className="mono">{entry.key}</td>
                                    {fields.map((field) => (
                                        <td key={field.field}>
                                            <ControlInput
                                                control={field as Control}
                                                value={fieldValue(
                                                    entry.value,
                                                    field.field,
                                                )}
                                                disabled={!editable}
                                                onCommit={(value) =>
                                                    onPropose(
                                                        [
                                                            {
                                                                op: "set_field",
                                                                target: `catalog=${catalog}:entry=${entry.key}#/${field.field}`,
                                                                value,
                                                            },
                                                        ],
                                                        `Set ${catalog}/${entry.key} ${field.field}`,
                                                    )
                                                }
                                            />
                                        </td>
                                    ))}
                                    {canDelete ? (
                                        <td>
                                            <button
                                                className="btn btn-icon btn-sm btn-remove"
                                                disabled={!editable}
                                                title="Delete entry"
                                                onClick={() =>
                                                    onPropose(
                                                        [
                                                            {
                                                                op: "delete",
                                                                target: `catalog=${catalog}:entry=${entry.key}`,
                                                            },
                                                        ],
                                                        `Delete ${catalog}/${entry.key}`,
                                                    )
                                                }
                                            >
                                                ×
                                            </button>
                                        </td>
                                    ) : null}
                                </tr>
                            ))}
                        </tbody>
                    </table>
                </div>
            )}
        </div>
    );
}

// A new allocation on the floor: an id and its arms, defined together the
// way the operation wants them. Arms parse from "control=0-499,
// treatment=500-999"; the allocation starts as a draft.
function AddAllocationForm({
    layer,
    onPropose,
}: {
    layer: string;
    onPropose: (operations: EditOperation[], summary: string) => void;
}) {
    const [open, setOpen] = useState(false);
    const [id, setId] = useState("");
    const [armsText, setArmsText] = useState("");
    if (!open) {
        return (
            <div className="action-row">
                <button
                    className="btn btn-secondary btn-sm"
                    onClick={() => setOpen(true)}
                >
                    Add allocation
                </button>
            </div>
        );
    }
    const arms = armsText
        .split(",")
        .map((part) => part.trim())
        .filter((part) => part !== "")
        .map((part) => {
            const [name, buckets] = part.split("=").map((half) => half.trim());
            return name !== undefined &&
                name !== "" &&
                buckets !== undefined &&
                buckets !== ""
                ? { name, buckets }
                : null;
        });
    const parsed = arms.every((arm) => arm !== null) ? arms : null;
    return (
        <form
            className="inline-form"
            onSubmit={(event) => {
                event.preventDefault();
                if (parsed === null) {
                    return;
                }
                setOpen(false);
                onPropose(
                    [
                        {
                            op: "add_allocation",
                            layer,
                            id: id.trim(),
                            arms: parsed,
                        },
                    ],
                    `Add ${layer}/${id.trim()}`,
                );
            }}
        >
            <input
                autoFocus
                className="input mono"
                placeholder="allocation_id"
                value={id}
                onChange={(event) => setId(event.target.value)}
            />
            <input
                className="input mono"
                placeholder="control=0-499, treatment=500-999"
                value={armsText}
                onChange={(event) => setArmsText(event.target.value)}
            />
            <button
                className="btn btn-primary btn-sm"
                type="submit"
                disabled={
                    id.trim() === "" || parsed === null || parsed.length === 0
                }
            >
                Create
            </button>
            <button
                className="btn btn-ghost btn-sm"
                type="button"
                onClick={() => setOpen(false)}
            >
                Cancel
            </button>
        </form>
    );
}

function AddEntryForm({
    catalog,
    onPropose,
}: {
    catalog: string;
    onPropose: (operations: EditOperation[], summary: string) => void;
}) {
    const [open, setOpen] = useState(false);
    const [key, setKey] = useState("");
    const [fieldsText, setFieldsText] = useState("{}");
    if (!open) {
        return (
            <div className="action-row">
                <button
                    className="btn btn-secondary btn-sm"
                    onClick={() => setOpen(true)}
                >
                    Add entry
                </button>
            </div>
        );
    }
    return (
        <form
            className="inline-form"
            onSubmit={(event) => {
                event.preventDefault();
                let parsed: unknown;
                try {
                    parsed = JSON.parse(fieldsText);
                } catch {
                    return;
                }
                setOpen(false);
                onPropose(
                    [
                        {
                            op: "create_entry",
                            catalog,
                            key: key.trim(),
                            fields: parsed,
                        },
                    ],
                    `Add ${catalog}/${key.trim()}`,
                );
            }}
        >
            <input
                className="input mono"
                placeholder="entry_id"
                value={key}
                onChange={(event) => setKey(event.target.value)}
            />
            <input
                className="input mono"
                placeholder="fields as JSON"
                value={fieldsText}
                onChange={(event) => setFieldsText(event.target.value)}
            />
            <button
                className="btn btn-primary btn-sm"
                type="submit"
                disabled={key.trim() === ""}
            >
                Create
            </button>
            <button
                className="btn btn-ghost btn-sm"
                type="button"
                onClick={() => setOpen(false)}
            >
                Cancel
            </button>
        </form>
    );
}

function fieldValue(entry: unknown, field: string): unknown {
    if (typeof entry !== "object" || entry === null || Array.isArray(entry)) {
        return undefined;
    }
    return (entry as Record<string, unknown>)[field];
}
