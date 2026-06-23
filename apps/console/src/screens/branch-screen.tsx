import type { ReactNode } from "react";
import {
    ArrowLeft,
    Braces,
    CheckCircle2,
    Circle,
    Database,
    ExternalLink,
    FileCode2,
    GitBranch,
    GitCompare,
    GitPullRequest,
    ListChecks,
    Pencil,
    Tags,
    TriangleAlert,
    Wrench,
} from "lucide-react";

import {
    AppShell,
    NavBack,
    NavContext,
    NavGroupLabel,
    NavLink,
} from "@/components/app-shell";
import { ArchiveBranchButton } from "@/components/archive-branch-button";
import { CatalogValueList } from "@/components/catalog-value-list";
import {
    DiagnosticCard,
    DiagnosticList,
    DiagnosticSummary,
} from "@/components/diagnostic-list";
import { BranchNameEditor } from "@/components/branch-name-editor";
import {
    AddEntityForm,
    AddCatalogEntryForm,
    DeleteEntityButton,
} from "@/components/entity-actions";
import {
    FriendlyEntityEditor,
    type EditContextPreview,
    type FormGuidance,
} from "@/components/friendly-entity-editor";
import { LoadingScreen } from "@/components/loading-screen";
import { PublishBranchButton } from "@/components/publish-branch-button";
import { SearchableList } from "@/components/searchable-list";
import { BranchStatusPill } from "@/components/status-pills";
import { SyncPrButton } from "@/components/sync-pr-button";
import {
    PackageGraph,
    type PackageGraphData,
} from "@/components/package-graph";
import { useApi } from "@/lib/api";
import { Link } from "@/lib/link";
import { useShellUser } from "@/lib/me";
import { RefreshScope } from "@/lib/refresh";
import type { EditKind } from "@/lib/route-normalizers";
import type {
    BranchChangeRecord,
    BranchData,
    BranchEntityData,
    BranchRecord,
    EditableEntity as ApiEditableEntity,
    LintDiagnostic,
    PackageDefinition,
    PackageSemanticModel,
    PackageWriteCapability,
} from "@/lib/types";
import { NotFound } from "@/screens/not-found";
import { encodeEntityPath, packageGraphData } from "@/screens/package-screen";

/** Top-level branch screen tab accepted from route state. */
export type BranchScreenId =
    | "overview"
    | "edit"
    | "changes"
    | "validate"
    | "publish";

/** Branch entity loaded from the API and then edited in local component state. */
type EditableEntity = ApiEditableEntity;

/** Lint payload for the current branch, including staging failures. */
type BranchLintLoad =
    | { root: string; diagnostics: LintDiagnostic[] }
    | { root: string; diagnostics: LintDiagnostic[]; error: string };

const EDIT_KIND_TITLES: Record<EditKind, string> = {
    variables: "Variables",
    qualifiers: "Qualifiers",
    catalogs: "Catalogs",
    context: "Context",
    linters: "Linters",
};

export function BranchScreen({
    branchId,
    kind = null,
    path = null,
    screen = "overview",
    packageId,
}: {
    branchId: string;
    kind?: EditKind | null;
    path?: string | null;
    screen?: BranchScreenId;
    packageId: string;
}) {
    const user = useShellUser();
    const selectedScreen = screen;
    const requestedEditKind = kind ?? "variables";
    const selectedEntityPath = path;
    const base = `/api/packages/${encodeURIComponent(packageId)}/branches/${encodeURIComponent(branchId)}`;
    const load = useApi<BranchData>(`${base}/data`);
    const entityExtras = useApi<BranchEntityData>(
        selectedEntityPath
            ? `${base}/entity?path=${encodeURIComponent(selectedEntityPath)}`
            : null,
    );

    if (load.loading || (selectedEntityPath && entityExtras.loading)) {
        return <LoadingScreen />;
    }
    if (load.status === 404) {
        return <NotFound />;
    }
    if (load.error || !load.data) {
        return (
            <main className="fault-page">
                <div className="fault-panel">
                    <span className="label">branch</span>
                    <h1>This branch failed to load.</h1>
                    <div className="banner banner-err">
                        <TriangleAlert aria-hidden size={16} />
                        <span>{load.error ?? "Unknown error."}</span>
                    </div>
                </div>
            </main>
        );
    }

    const {
        package: pkg,
        branch,
        prSyncError,
        changes,
        model: branchModel,
        entities: editableEntities,
        editLoadError,
        editedPaths,
        capabilities,
    } = load.data;
    const lint = load.data.lint as BranchLintLoad;
    const localWorkingTree =
        capabilities.write.kind === "directPush" &&
        capabilities.write.backend === "localWorkingTree";

    const lintHasErrors =
        "error" in lint ||
        lint.diagnostics.some((diagnostic) => diagnostic.severity === "error");
    const selectedEntity = selectedEntityPath
        ? (editableEntities.find(
              (entity) => entity.path === selectedEntityPath,
          ) ?? null)
        : null;
    const selectedEntityDiagnostics =
        selectedEntity && !("error" in lint)
            ? lint.diagnostics.filter((diagnostic) =>
                  diagnosticMatchesEntity(
                      diagnostic,
                      selectedEntity,
                      editableEntities,
                  ),
              )
            : [];
    const selectedEditKind = selectedEntity?.section ?? requestedEditKind;
    // The base-ref version of the selected entity, for showing the branch's
    // delta in the editor. Null when the entity is new on this branch or the
    // base text is unavailable.
    const selectedEntityBaseText = entityExtras.data?.baseText ?? null;
    // Editing a variable: the server pre-evaluates every qualifier against each
    // saved request context so the form can preview resolution pathways live.
    const contextPreviews: EditContextPreview[] =
        entityExtras.data?.contextPreviews ?? [];

    // Editing-mode entity graph: same graph as the package overview, built
    // from the branch, with edited entities marked.
    let branchGraphData: PackageGraphData | null = null;
    if (selectedScreen === "overview" && branchModel !== null) {
        const pathForKey = new Map<string, string>();
        for (const entity of editableEntities) {
            const targetKey = editableEntityTargetKey(entity);
            if (targetKey) {
                pathForKey.set(targetKey, entity.path);
            }
        }
        branchGraphData = packageGraphData({
            model: branchModel,
            pathForKey,
            sourceByPath: new Map(
                editableEntities.map((entity) => [
                    entity.path,
                    {
                        language: entity.language,
                        text: entity.text,
                    },
                ]),
            ),
            hrefFor: (entityPath) =>
                editEntityHref(pkg.slug, branch.id, entityPath),
            editedPaths: new Set(editedPaths),
        });
    }
    const editableEntityCounts = editKindCounts(editableEntities);
    const contextAttributes = contextAttributeSuggestions(editableEntities);
    const catalogIdSuggestions = editableEntities
        .filter((entity) => entity.kind === "catalog")
        .map((entity) => entity.id);
    const packageName = pkg.sourceTreeLabel;
    const parentCatalogEntity =
        selectedEntity?.kind === "catalog value"
            ? (editableEntities.find(
                  (candidate) =>
                      candidate.kind === "catalog" &&
                      candidate.id === selectedEntity.catalogId,
              ) ?? null)
            : null;
    const entityCrumbLabel = selectedEntity
        ? parentCatalogEntity &&
          selectedEntity.id.startsWith(`${parentCatalogEntity.id}/`)
            ? selectedEntity.id.slice(parentCatalogEntity.id.length + 1)
            : selectedEntity.id
        : "";
    // Crumbs are ancestors only; the topbar title names the current screen.
    const crumbs = [
        { label: "console", href: "/app" },
        { label: "packages", href: "/app/packages" },
        {
            label: pkg.displayPath,
            href: `/app/packages/${pkg.slug}`,
        },
        ...(selectedScreen !== "overview"
            ? [
                  {
                      label: branch.branch,
                      href: branchScreenHref(pkg.slug, branch.id, "overview"),
                  },
              ]
            : []),
        ...(selectedScreen === "edit" && selectedEntity
            ? [
                  {
                      label: EDIT_KIND_TITLES[selectedEditKind].toLowerCase(),
                      href: editKindHref(pkg.slug, branch.id, selectedEditKind),
                  },
                  ...(parentCatalogEntity
                      ? [
                            {
                                label: parentCatalogEntity.id,
                                href: editEntityHref(
                                    pkg.slug,
                                    branch.id,
                                    parentCatalogEntity.path,
                                ),
                            },
                        ]
                      : []),
              ]
            : []),
    ];
    const title = selectedEntity
        ? entityCrumbLabel
        : selectedScreen === "edit"
          ? `Edit ${EDIT_KIND_TITLES[selectedEditKind].toLowerCase()}`
          : branchScreenTitle(selectedScreen, localWorkingTree);

    return (
        <RefreshScope
            onRefresh={() => {
                load.reload();
                entityExtras.reload();
            }}
        >
            <AppShell
                actions={
                    <>
                        <Link
                            className="pill-link"
                            href={branchScreenHref(
                                pkg.slug,
                                branch.id,
                                "validate",
                            )}
                            title="Open validation"
                        >
                            {"error" in lint ? (
                                <span className="pill pill-err">
                                    <span className="d" />
                                    lint failed
                                </span>
                            ) : (
                                <DiagnosticSummary
                                    diagnostics={lint.diagnostics}
                                />
                            )}
                        </Link>
                        <BranchStatusPill branch={branch} />
                        {branch.prUrl ? (
                            <a
                                className="btn btn-secondary btn-sm"
                                href={branch.prUrl}
                                rel="noreferrer"
                                target="_blank"
                            >
                                <GitPullRequest aria-hidden size={14} />
                                Pull request
                            </a>
                        ) : null}
                    </>
                }
                crumbs={crumbs}
                editing={{
                    label: localWorkingTree ? "working tree" : branch.branch,
                    detail:
                        branch.status === "active"
                            ? localWorkingTree
                                ? "Saves write to the local checkout. Commit and push with git when ready."
                                : "Saves commit to this branch — nothing reaches the base ref without review."
                            : "This branch is not active; editing is locked.",
                }}
                nav={
                    <>
                        <NavBack
                            href={`/app/packages/${pkg.slug}/branches`}
                            label="Package"
                        />
                        <NavContext
                            href={branchScreenHref(
                                pkg.slug,
                                branch.id,
                                "overview",
                            )}
                            label={localWorkingTree ? "working tree" : "branch"}
                            value={branch.branch}
                        />
                        <NavGroupLabel>
                            {localWorkingTree ? "Working Tree" : "Branch"}
                        </NavGroupLabel>
                        <NavLink
                            active={selectedScreen === "overview"}
                            href={branchScreenHref(
                                pkg.slug,
                                branch.id,
                                "overview",
                            )}
                            icon={<GitBranch aria-hidden size={16} />}
                            label="Overview"
                        />
                        <NavLink
                            active={selectedScreen === "changes"}
                            count={changes.length}
                            href={branchScreenHref(
                                pkg.slug,
                                branch.id,
                                "changes",
                            )}
                            icon={<GitCompare aria-hidden size={16} />}
                            label="Changes"
                        />
                        <NavLink
                            active={selectedScreen === "validate"}
                            count={
                                "error" in lint
                                    ? undefined
                                    : lint.diagnostics.length
                            }
                            href={branchScreenHref(
                                pkg.slug,
                                branch.id,
                                "validate",
                            )}
                            icon={<ListChecks aria-hidden size={16} />}
                            label="Validate"
                        />
                        <NavLink
                            active={selectedScreen === "publish"}
                            href={branchScreenHref(
                                pkg.slug,
                                branch.id,
                                "publish",
                            )}
                            icon={<GitPullRequest aria-hidden size={16} />}
                            label={localWorkingTree ? "Review" : "Publish"}
                        />
                        <NavGroupLabel>Edit</NavGroupLabel>
                        <NavLink
                            active={
                                selectedScreen === "edit" &&
                                selectedEditKind === "qualifiers"
                            }
                            count={editableEntityCounts.qualifiers}
                            href={editKindHref(
                                pkg.slug,
                                branch.id,
                                "qualifiers",
                            )}
                            icon={<Tags aria-hidden size={16} />}
                            label="Qualifiers"
                        />
                        <NavLink
                            active={
                                selectedScreen === "edit" &&
                                selectedEditKind === "variables"
                            }
                            count={editableEntityCounts.variables}
                            href={editKindHref(
                                pkg.slug,
                                branch.id,
                                "variables",
                            )}
                            icon={<FileCode2 aria-hidden size={16} />}
                            label="Variables"
                        />
                        <NavLink
                            active={
                                selectedScreen === "edit" &&
                                selectedEditKind === "catalogs"
                            }
                            count={editableEntityCounts.catalogs}
                            href={editKindHref(pkg.slug, branch.id, "catalogs")}
                            icon={<Database aria-hidden size={16} />}
                            label="Catalogs"
                        />
                        <NavLink
                            active={
                                selectedScreen === "edit" &&
                                selectedEditKind === "context"
                            }
                            count={editableEntityCounts.context}
                            href={editKindHref(pkg.slug, branch.id, "context")}
                            icon={<Braces aria-hidden size={16} />}
                            label="Context"
                        />
                        <NavLink
                            active={
                                selectedScreen === "edit" &&
                                selectedEditKind === "linters"
                            }
                            count={editableEntityCounts.linters}
                            href={editKindHref(pkg.slug, branch.id, "linters")}
                            icon={<Wrench aria-hidden size={16} />}
                            label="Linters"
                        />
                    </>
                }
                title={title}
                user={user}
            >
                {selectedScreen === "overview" ? (
                    <BranchOverview
                        changesCount={changes.length}
                        branch={branch}
                        graphData={branchGraphData}
                        localWorkingTree={localWorkingTree}
                        packageId={pkg.slug}
                    />
                ) : null}
                {selectedScreen === "edit" ? (
                    <BranchEditScreen
                        baseText={selectedEntityBaseText}
                        contextAttributes={contextAttributes}
                        contextPreviews={contextPreviews}
                        branch={branch}
                        localWorkingTree={localWorkingTree}
                        model={branchModel}
                        editableEntities={editableEntities}
                        entityDiagnostics={selectedEntityDiagnostics}
                        loadError={editLoadError}
                        catalogIds={catalogIdSuggestions}
                        selectedEntity={selectedEntity}
                        selectedKind={selectedEditKind}
                        packageId={pkg.slug}
                    />
                ) : null}
                {selectedScreen === "changes" ? (
                    <BranchChangesScreen
                        changes={changes}
                        entityHrefForFile={(filePath) => {
                            const match = entityForDiagnosticPath(
                                editableEntities,
                                filePath,
                            );
                            return match
                                ? editEntityHref(
                                      pkg.slug,
                                      branch.id,
                                      match.path,
                                  )
                                : null;
                        }}
                    />
                ) : null}
                {selectedScreen === "validate" ? (
                    <BranchValidateScreen
                        diagnosticHref={(diagnostic) =>
                            diagnosticEntityHref(
                                diagnostic,
                                editableEntities,
                                pkg.slug,
                                branch.id,
                            )
                        }
                        lint={lint}
                    />
                ) : null}
                {selectedScreen === "publish" ? (
                    <BranchPublishScreen
                        changesCount={changes.length}
                        branch={branch}
                        lintHasErrors={lintHasErrors}
                        prSyncError={prSyncError}
                        writeCapability={capabilities.write}
                        packageId={pkg.slug}
                    />
                ) : null}
            </AppShell>
        </RefreshScope>
    );
}

function BranchOverview({
    changesCount,
    branch,
    graphData,
    localWorkingTree,
    packageId,
}: {
    changesCount: number;
    branch: BranchRecord;
    graphData: PackageGraphData | null;
    localWorkingTree: boolean;
    packageId: string;
}) {
    return (
        <section className="section">
            <div className="section-header">
                <div className="section-header-text">
                    <h1 className="mono">{branch.branch}</h1>
                    <p className="hint">
                        {localWorkingTree
                            ? "Edits save directly to the local checkout. Validate the working tree here, then commit and push with git."
                            : "Edits commit directly to this branch. When the branch is ready, publish it as a pull request from the publish screen."}
                    </p>
                </div>
                <BranchStatusPill branch={branch} />
            </div>
            <div className="meta-grid">
                <div className="meta-item">
                    <span className="label">base ref</span>
                    <span className="meta-value mono">{branch.baseRef}</span>
                </div>
                <div className="meta-item">
                    <span className="label">changed files</span>
                    <span className="meta-value">{changesCount}</span>
                </div>
                <div className="meta-item">
                    <span className="label">created</span>
                    <span className="meta-value">
                        {formatDate(branch.createdAt)}
                    </span>
                </div>
                <div className="meta-item">
                    <span className="label">updated</span>
                    <span className="meta-value">
                        {formatDate(branchUpdatedAt(branch))}
                    </span>
                </div>
            </div>
            {!localWorkingTree ? (
                <div className="card">
                    <div className="card-head-text">
                        <h3>Branch name</h3>
                        <p className="hint">
                            Renaming moves the branch on GitHub. Locked once the
                            branch has a pull request.
                        </p>
                    </div>
                    <BranchNameEditor
                        branch={branch.branch}
                        disabled={branch.status !== "active"}
                        branchId={branch.id}
                        packageId={packageId}
                    />
                </div>
            ) : null}
            {graphData ? (
                <div className="card graph-card">
                    <div className="card-head-text">
                        <h3>Entity graph</h3>
                        <p className="hint">
                            The package as this branch sees it. Entities edited
                            on this branch are marked{" "}
                            <span
                                className="mono"
                                style={{ color: "var(--warn-700)" }}
                            >
                                ~
                            </span>
                            ; hover to trace references, click to edit.
                        </p>
                    </div>
                    <PackageGraph data={graphData} />
                </div>
            ) : null}
        </section>
    );
}

function BranchEditScreen({
    baseText,
    contextAttributes,
    contextPreviews,
    branch,
    localWorkingTree,
    editableEntities,
    entityDiagnostics,
    loadError,
    model,
    catalogIds,
    selectedEntity,
    selectedKind,
    packageId,
}: {
    baseText: string | null;
    contextAttributes: string[];
    contextPreviews: EditContextPreview[];
    branch: BranchRecord;
    localWorkingTree: boolean;
    editableEntities: EditableEntity[];
    entityDiagnostics: LintDiagnostic[];
    loadError: string | null;
    model: PackageSemanticModel | null;
    catalogIds: string[];
    selectedEntity: EditableEntity | null;
    selectedKind: EditKind;
    packageId: string;
}) {
    const entities = editableEntities.filter(
        (entity) => entity.section === selectedKind,
    );

    return (
        <section className="section">
            {loadError ? (
                <div className="banner banner-err">
                    <TriangleAlert aria-hidden size={16} />
                    <span>The branch package failed to load: {loadError}</span>
                </div>
            ) : null}
            {selectedEntity ? (
                <EditableEntityDetail
                    allEntities={editableEntities}
                    baseText={baseText}
                    contextAttributes={contextAttributes}
                    contextPreviews={contextPreviews}
                    diagnostics={entityDiagnostics}
                    branch={branch}
                    entity={selectedEntity}
                    localWorkingTree={localWorkingTree}
                    model={model}
                    catalogIds={catalogIds}
                    packageId={packageId}
                />
            ) : (
                <>
                    <div className="section-header-text">
                        <h1>{EDIT_KIND_TITLES[selectedKind]}</h1>
                        <p className="hint">
                            Pick an entity to edit it with a form or as source.
                            {localWorkingTree
                                ? " Saves write to the local checkout."
                                : " Saves commit to the branch."}
                        </p>
                    </div>
                    <EditableEntityList
                        catalogIds={catalogIds}
                        disabled={branch.status !== "active"}
                        branchId={branch.id}
                        entities={entities}
                        kind={selectedKind}
                        packageId={packageId}
                    />
                </>
            )}
        </section>
    );
}

function EditableEntityList({
    catalogIds,
    disabled,
    branchId,
    entities,
    kind,
    packageId,
}: {
    catalogIds: string[];
    disabled?: boolean;
    branchId: string;
    entities: EditableEntity[];
    kind: EditKind;
    packageId: string;
}) {
    return (
        <>
            <AddEntityForm
                catalogIds={catalogIds}
                disabled={disabled}
                branchId={branchId}
                kind={kind}
                packageId={packageId}
            />
            {entities.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">{editKindIcon(kind, 18)}</span>
                    <p>
                        No {EDIT_KIND_TITLES[kind].toLowerCase()} on this branch
                        yet.
                    </p>
                </div>
            ) : (
                <SearchableList
                    className="row-list"
                    emptyLabel="Nothing matches that search."
                    label={`Search ${EDIT_KIND_TITLES[kind].toLowerCase()}`}
                    placeholder={`Search ${EDIT_KIND_TITLES[kind].toLowerCase()}`}
                >
                    {entities.map((entity) => (
                        <Link
                            className="row"
                            data-search={editableEntitySearchText(entity)}
                            href={editEntityHref(
                                packageId,
                                branchId,
                                entity.path,
                            )}
                            key={entity.path}
                        >
                            <span className="row-icon">
                                {editKindIcon(entity.section, 16)}
                            </span>
                            <span className="row-text">
                                <span className="row-title mono">
                                    {entity.id}
                                </span>
                                <span className="row-sub">
                                    {entity.description ?? entity.path}
                                </span>
                            </span>
                            <span className="row-side">
                                {entity.badge ? (
                                    <span className="tag">{entity.badge}</span>
                                ) : null}
                                <Pencil
                                    aria-hidden
                                    className="muted"
                                    size={14}
                                />
                            </span>
                        </Link>
                    ))}
                </SearchableList>
            )}
        </>
    );
}

function EditableEntityDetail({
    allEntities,
    baseText,
    contextAttributes,
    contextPreviews,
    diagnostics,
    branch,
    entity,
    localWorkingTree,
    model,
    catalogIds,
    packageId,
}: {
    allEntities: EditableEntity[];
    baseText: string | null;
    contextAttributes: string[];
    contextPreviews: EditContextPreview[];
    diagnostics: LintDiagnostic[];
    branch: BranchRecord;
    entity: EditableEntity;
    localWorkingTree: boolean;
    model: PackageSemanticModel | null;
    catalogIds: string[];
    packageId: string;
}) {
    const catalogEntries =
        entity.kind === "catalog"
            ? allEntities.filter(
                  (candidate) =>
                      candidate.kind === "catalog value" &&
                      candidate.catalogId === entity.id,
              )
            : [];
    const catalogEntryValues = new Map(
        (model?.catalogEntries ?? [])
            .filter((entry) => entry.catalog === entity.id)
            .map((entry) => [entry.key, entry.value] as const),
    );
    const parentCatalog =
        entity.kind === "catalog value"
            ? (allEntities.find(
                  (candidate) =>
                      candidate.kind === "catalog" &&
                      candidate.id === entity.catalogId,
              ) ?? null)
            : null;
    const sourceMarks = diagnostics.flatMap((diagnostic) => {
        const path = diagnostic.location?.path;
        const line = diagnostic.location?.range?.start?.line;
        if (
            path === undefined ||
            line === undefined ||
            !(
                entity.path === path ||
                entity.path.endsWith(`/${path}`) ||
                path.endsWith(`/${entity.path}`)
            )
        ) {
            return [];
        }
        return [
            {
                // lint positions are 0-based; the editor highlights 1-based lines
                line: line + 1,
                severity: (diagnostic.severity === "error"
                    ? "error"
                    : "warning") as "error" | "warning",
            },
        ];
    });

    return (
        <>
            <div className="section-header">
                <div className="section-header-text">
                    <span className="label">{entity.kind}</span>
                    <h1 className="mono">
                        {parentCatalog &&
                        entity.id.startsWith(`${parentCatalog.id}/`)
                            ? entity.id.slice(parentCatalog.id.length + 1)
                            : entity.id}
                    </h1>
                    {entity.description ? (
                        <p className="hint">{entity.description}</p>
                    ) : null}
                </div>
                <div className="action-row">
                    {entity.badge && !parentCatalog ? (
                        <span className="tag">{entity.badge}</span>
                    ) : null}
                    <Link
                        className="btn btn-ghost btn-sm"
                        href={`/app/packages/${packageId}/tree/${encodeEntityPath(entity.path)}`}
                    >
                        View on {branch.baseRef}
                    </Link>
                    {parentCatalog ? (
                        <Link
                            className="btn btn-secondary btn-sm"
                            href={editEntityHref(
                                packageId,
                                branch.id,
                                parentCatalog.path,
                            )}
                        >
                            <ArrowLeft aria-hidden size={14} />
                            Catalog {parentCatalog.id}
                        </Link>
                    ) : (
                        <Link
                            className="btn btn-secondary btn-sm"
                            href={editKindHref(
                                packageId,
                                branch.id,
                                entity.section,
                            )}
                        >
                            <ArrowLeft aria-hidden size={14} />
                            All {EDIT_KIND_TITLES[entity.section].toLowerCase()}
                        </Link>
                    )}
                </div>
            </div>
            {diagnostics.length > 0 ? (
                <div className="diagnostic-group">
                    <div className="diagnostic-group-head">
                        <span className="tag">diagnostics</span>
                        <span className="label">
                            {diagnostics.length} on this entity
                        </span>
                    </div>
                    {diagnostics.map((diagnostic, index) => (
                        <DiagnosticCard diagnostic={diagnostic} key={index} />
                    ))}
                </div>
            ) : null}
            <FriendlyEntityEditor
                baseText={baseText}
                contextAttributes={contextAttributes}
                diagnostics={diagnostics}
                disabled={branch.status !== "active"}
                branchId={branch.id}
                entity={entity}
                guidance={{
                    ...buildFormGuidance(entity, allEntities, model),
                    contextPreviews,
                }}
                catalogIds={catalogIds}
                catalogSchema={catalogEntrySchemaText(entity, allEntities)}
                sourceMarks={sourceMarks}
                packageId={packageId}
            />
            {entity.kind === "catalog" ? (
                <AddCatalogEntryForm
                    disabled={branch.status !== "active"}
                    branchId={branch.id}
                    catalogId={entity.id}
                    packageId={packageId}
                />
            ) : null}
            {catalogEntries.length > 0 ? (
                <div className="card">
                    <div className="card-head-text">
                        <h3>Values</h3>
                        <p className="hint">
                            Variables select these catalog values by name.
                        </p>
                    </div>
                    <CatalogValueList
                        items={catalogEntries.map((entry) => {
                            const key =
                                entry.entryKey ??
                                entry.id.split("/").pop() ??
                                entry.id;
                            return {
                                key,
                                href: editEntityHref(
                                    packageId,
                                    branch.id,
                                    entry.path,
                                ),
                                value: catalogEntryValues.get(key),
                            };
                        })}
                    />
                </div>
            ) : null}
            <div className="card">
                <div className="card-head">
                    <div className="card-head-text">
                        <h3>Delete from branch</h3>
                        <p className="hint">
                            Removes <span className="mono">{entity.path}</span>{" "}
                            {localWorkingTree
                                ? "from the local checkout."
                                : "from the branch. The base ref is untouched until the pull request merges."}
                        </p>
                    </div>
                    <DeleteEntityButton
                        disabled={branch.status !== "active"}
                        branchId={branch.id}
                        filePath={entity.path}
                        returnHref={editKindHref(
                            packageId,
                            branch.id,
                            entity.section,
                        )}
                        packageId={packageId}
                    />
                </div>
            </div>
        </>
    );
}

function BranchChangesScreen({
    changes,
    entityHrefForFile,
}: {
    changes: BranchChangeRecord[];
    entityHrefForFile: (filePath: string) => string | null;
}) {
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Changes</h1>
                <p className="hint">
                    Files changed on this branch compared with its base ref.
                </p>
            </div>
            {changes.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <GitCompare aria-hidden size={18} />
                    </span>
                    <p>
                        No changed files yet. Edits you save will show up here
                        as a diff.
                    </p>
                </div>
            ) : (
                <SearchableList
                    className="row-list"
                    emptyLabel="No changes match that search."
                    label="Search changes"
                    placeholder="Search changes"
                >
                    {changes.map((change) => (
                        <article
                            className="diffcard"
                            data-search={change.filePath}
                            key={change.id}
                        >
                            <div className="diffhead">
                                <span className="diffhead-path">
                                    {entityHrefForFile(change.filePath) ? (
                                        <Link
                                            href={
                                                entityHrefForFile(
                                                    change.filePath,
                                                ) as string
                                            }
                                        >
                                            {change.filePath}
                                        </Link>
                                    ) : (
                                        change.filePath
                                    )}
                                </span>
                                <span className="tag">file</span>
                            </div>
                        </article>
                    ))}
                </SearchableList>
            )}
        </section>
    );
}

function BranchValidateScreen({
    diagnosticHref,
    lint,
}: {
    diagnosticHref: (diagnostic: LintDiagnostic) => string | null;
    lint: BranchLintLoad;
}) {
    return "error" in lint ? (
        <div className="banner banner-err">
            <TriangleAlert aria-hidden size={16} />
            <span>Lint failed to run: {lint.error}</span>
        </div>
    ) : (
        <section className="section">
            <div className="section-header-text">
                <h1>Validate</h1>
                <p className="hint">
                    Lint runs against the branch. Publishing is blocked while
                    errors are present — warnings ship, errors don’t.
                </p>
            </div>
            <DiagnosticList
                diagnosticHref={diagnosticHref}
                diagnostics={lint.diagnostics}
            />
        </section>
    );
}

function BranchPublishScreen({
    changesCount,
    branch,
    lintHasErrors,
    prSyncError,
    writeCapability,
    packageId,
}: {
    changesCount: number;
    branch: BranchRecord;
    lintHasErrors: boolean;
    prSyncError: string | null;
    writeCapability: PackageWriteCapability;
    packageId: string;
}) {
    const published = Boolean(branch.prUrl);
    const directPush = writeCapability.kind === "directPush";
    const localWorkingTree =
        writeCapability.kind === "directPush" &&
        writeCapability.backend === "localWorkingTree";
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>{localWorkingTree ? "Review" : "Publish"}</h1>
                <p className="hint">
                    {localWorkingTree
                        ? "Validate the local working tree for "
                        : directPush
                          ? "Publishing applies the configured direct-push workflow for "
                          : "Publishing opens a pull request from "}
                    <span className="mono">{branch.branch}</span>
                    {localWorkingTree ? (
                        ". Commit and push it with git when ready."
                    ) : directPush ? (
                        "."
                    ) : (
                        <>
                            {" "}
                            to <span className="mono">{branch.baseRef}</span>.
                            Nothing reaches the base ref without review.
                        </>
                    )}
                </p>
            </div>
            {published ? (
                <div className="card">
                    <div className="applied-row">
                        <span className="checkdot">
                            <GitPullRequest aria-hidden size={15} />
                        </span>
                        <div className="card-head-text">
                            <h3>Pull request {branch.prState ?? "open"}</h3>
                            <p className="hint">
                                Review and merge on GitHub. State syncs back
                                here.
                            </p>
                        </div>
                        <div
                            className="action-row"
                            style={{ marginLeft: "auto" }}
                        >
                            <a
                                className="btn btn-primary"
                                href={branch.prUrl ?? "#"}
                                rel="noreferrer"
                                target="_blank"
                            >
                                <ExternalLink aria-hidden size={15} />
                                Open pull request
                            </a>
                            <SyncPrButton
                                branchId={branch.id}
                                packageId={packageId}
                            />
                        </div>
                    </div>
                    {prSyncError ? (
                        <p className="form-note" data-tone="err">
                            Pull request sync failed: {prSyncError}
                        </p>
                    ) : null}
                </div>
            ) : (
                <>
                    <div className="card">
                        <div className="card-head-text">
                            <h3>
                                {localWorkingTree
                                    ? "Ready to review?"
                                    : "Ready to publish?"}
                            </h3>
                        </div>
                        <PublishCheck ok={changesCount > 0}>
                            {changesCount > 0
                                ? localWorkingTree
                                    ? `${changesCount} changed ${changesCount === 1 ? "file" : "files"} in the working tree`
                                    : `${changesCount} changed ${changesCount === 1 ? "change" : "changes"} to publish`
                                : "No changed files yet — save an edit first"}
                        </PublishCheck>
                        <PublishCheck ok={!lintHasErrors}>
                            {lintHasErrors
                                ? "Lint reports errors — fix them on the validate screen"
                                : "Lint is clean"}
                        </PublishCheck>
                        <PublishCheck ok={branch.status === "active"}>
                            {branch.status === "active"
                                ? localWorkingTree
                                    ? "Working tree session is active"
                                    : "Branch is active"
                                : `Branch is ${branch.status}`}
                        </PublishCheck>
                        <div className="action-row">
                            <PublishBranchButton
                                disabled={
                                    branch.status !== "active" ||
                                    changesCount === 0 ||
                                    lintHasErrors
                                }
                                branchId={branch.id}
                                writeBackend={
                                    writeCapability.kind === "directPush"
                                        ? writeCapability.backend
                                        : undefined
                                }
                                writeKind={writeCapability.kind}
                                packageId={packageId}
                            />
                        </div>
                    </div>
                    <div className="card">
                        <div className="card-head-text">
                            <h3>
                                {localWorkingTree
                                    ? "Archive this working tree session"
                                    : "Archive this branch"}
                            </h3>
                            <p className="hint">
                                {localWorkingTree
                                    ? "Hide this working tree session from the console without touching local files. It can be opened again later."
                                    : "Hide this branch from the console without deleting it from the repository. It can be opened again later."}
                            </p>
                        </div>
                        <ArchiveBranchButton
                            branch={branch.branch}
                            disabled={branch.status !== "active"}
                            branchId={branch.id}
                            packageId={packageId}
                        />
                    </div>
                </>
            )}
        </section>
    );
}

function PublishCheck({ children, ok }: { children: ReactNode; ok: boolean }) {
    return (
        <div className="publish-check" data-ok={ok}>
            {ok ? (
                <CheckCircle2 aria-hidden size={16} />
            ) : (
                <Circle aria-hidden size={16} />
            )}
            <span>{children}</span>
        </div>
    );
}

function editKindIcon(kind: EditKind, size: number): ReactNode {
    switch (kind) {
        case "variables":
            return <FileCode2 aria-hidden size={size} />;
        case "qualifiers":
            return <Tags aria-hidden size={size} />;
        case "catalogs":
            return <Database aria-hidden size={size} />;
        case "context":
            return <Braces aria-hidden size={size} />;
        case "linters":
            return <Wrench aria-hidden size={size} />;
    }
}

function branchScreenTitle(
    screen: BranchScreenId,
    localWorkingTree = false,
): string {
    switch (screen) {
        case "overview":
            return localWorkingTree ? "Working tree" : "Branch overview";
        case "edit":
            return "Edit";
        case "changes":
            return "Changes";
        case "validate":
            return "Validate";
        case "publish":
            return localWorkingTree ? "Review" : "Publish";
    }
}

function formatDate(value: string): string {
    return new Intl.DateTimeFormat("en", {
        dateStyle: "medium",
        timeStyle: "short",
    }).format(new Date(value));
}

function branchUpdatedAt(branch: BranchRecord): string {
    return branch.lastEditedAt ?? branch.lastOpenedAt ?? branch.createdAt;
}

function branchScreenHref(
    packageId: string,
    branchId: string,
    screen: BranchScreenId,
): string {
    const base = `/app/packages/${packageId}/branches/${branchId}`;
    return screen === "overview" ? base : `${base}/${screen}`;
}

function editKindHref(
    packageId: string,
    branchId: string,
    kind: EditKind,
): string {
    return `/app/packages/${packageId}/branches/${branchId}/edit/${kind}`;
}

function editEntityHref(
    packageId: string,
    branchId: string,
    path: string,
): string {
    return `/app/packages/${packageId}/branches/${branchId}/tree/${encodeEntityPath(path)}`;
}

export function normalizeBranchScreen(
    value: string | null,
): BranchScreenId | null {
    if (
        value === "overview" ||
        value === "edit" ||
        value === "changes" ||
        value === "validate" ||
        value === "publish"
    ) {
        return value;
    }
    return null;
}

function editableEntitySearchText(entity: EditableEntity): string {
    return [
        entity.id,
        entity.kind,
        entity.path,
        entity.description,
        entity.badge,
    ]
        .filter(Boolean)
        .join(" ");
}

function editKindCounts(entities: EditableEntity[]): Record<EditKind, number> {
    const counts: Record<EditKind, number> = {
        variables: 0,
        qualifiers: 0,
        catalogs: 0,
        context: 0,
        linters: 0,
    };
    for (const entity of entities) {
        counts[entity.section] += 1;
    }
    return counts;
}

function editableEntityTargetKey(entity: EditableEntity): string | null {
    if (entity.section === "variables") {
        return `variables:${entity.id}`;
    }
    if (entity.section === "qualifiers") {
        return `qualifiers:${entity.id}`;
    }
    if (entity.kind === "catalog") {
        return `catalogs:${entity.id}`;
    }
    if (entity.kind === "catalog value" && entity.catalogId) {
        const entryKey = entity.entryKey ?? entity.id.split("/").pop();
        return entryKey
            ? `catalog_entries:${entity.catalogId}:${entryKey}`
            : null;
    }
    return null;
}

function contextAttributeSuggestions(entities: EditableEntity[]): string[] {
    const suggestions = new Set<string>();
    for (const contextSchema of requestContextSchemaEntities(entities)) {
        try {
            const parsed = JSON.parse(contextSchema.text) as unknown;
            for (const attribute of collectSchemaAttributes(parsed)) {
                if (!attribute.startsWith("qualifier.")) {
                    suggestions.add(attribute);
                }
            }
        } catch {
            // Draft text can be malformed while editing; lint owns diagnostics.
        }
    }
    return [...suggestions].sort((left, right) => left.localeCompare(right));
}

function requestContextSchemaEntities(
    entities: EditableEntity[],
): EditableEntity[] {
    return entities.filter(
        (entity) =>
            entity.kind === "context schema" ||
            (entity.path.includes("request-contexts/") &&
                entity.path.endsWith(".schema.json")),
    );
}

function collectSchemaAttributes(schema: unknown, prefix = ""): string[] {
    if (!isRecord(schema)) {
        return [];
    }
    const properties = schema.properties;
    if (!isRecord(properties)) {
        return [];
    }
    return Object.entries(properties).flatMap(([key, value]) => {
        const path = prefix ? `${prefix}.${key}` : key;
        if (isRecord(value) && isRecord(value.properties)) {
            return collectSchemaAttributes(value, path);
        }
        return [path];
    });
}

/* Descriptions and example values harvested from sibling entities, so forms
   can show what good input looks like. */
function buildFormGuidance(
    entity: EditableEntity,
    entities: EditableEntity[],
    model: PackageSemanticModel | null,
): FormGuidance {
    const guidance: FormGuidance = {};
    if (entity.section === "qualifiers") {
        guidance.contextAttributeDocs = contextAttributeDocs(entities);
    }
    if (entity.section === "variables") {
        guidance.qualifierIds = entities
            .filter((candidate) => candidate.section === "qualifiers")
            .map((candidate) => candidate.id);
        const entryKeys: Record<string, string[]> = {};
        for (const candidate of entities) {
            if (
                candidate.kind === "catalog value" &&
                candidate.catalogId &&
                candidate.entryKey
            ) {
                (entryKeys[candidate.catalogId] ??= []).push(
                    candidate.entryKey,
                );
            }
        }
        guidance.catalogEntryKeys = entryKeys;
    }
    if (entity.section === "variables" || entity.kind === "catalog") {
        guidance.catalogDocs = Object.fromEntries(
            entities
                .filter(
                    (candidate) =>
                        candidate.kind === "catalog" && candidate.description,
                )
                .map((candidate) => [
                    candidate.id,
                    candidate.description as string,
                ]),
        );
    }
    if (entity.kind === "catalog value") {
        const examples: Record<string, string[]> = {};
        for (const sibling of model?.catalogEntries ?? []) {
            if (
                sibling.catalog !== entity.catalogId ||
                sibling.key === entity.entryKey
            ) {
                continue;
            }
            const fields =
                typeof sibling.value === "object" &&
                sibling.value !== null &&
                !Array.isArray(sibling.value)
                    ? Object.entries(sibling.value as Record<string, unknown>)
                    : [];
            for (const [key, value] of fields) {
                const literal = JSON.stringify(value);
                const list = (examples[key] ??= []);
                if (!list.includes(literal) && list.length < 4) {
                    list.push(literal);
                }
            }
        }
        guidance.propertyExamples = examples;
    }
    return guidance;
}

function contextAttributeDocs(
    entities: EditableEntity[],
): Record<string, string> {
    const docs: Record<string, string> = {};
    for (const schema of requestContextSchemaEntities(entities)) {
        try {
            collectAttributeDocs(JSON.parse(schema.text), "", docs);
        } catch {
            // Draft text can be malformed while editing; lint owns diagnostics.
        }
    }
    return docs;
}

function collectAttributeDocs(
    schema: unknown,
    prefix: string,
    out: Record<string, string>,
): void {
    if (!isRecord(schema) || !isRecord(schema.properties)) {
        return;
    }
    for (const [key, value] of Object.entries(schema.properties)) {
        if (!isRecord(value)) {
            continue;
        }
        const path = prefix ? `${prefix}.${key}` : key;
        if (typeof value.description === "string") {
            out[path] = value.description;
        }
        collectAttributeDocs(value, path, out);
    }
}

function packageRelativePath(packagePath: string, path: string): string {
    const prefix = packagePath === "." ? "" : `${packagePath}/`;
    return prefix && path.startsWith(prefix) ? path.slice(prefix.length) : path;
}

function catalogEntrySchemaText(
    entity: EditableEntity,
    entities: EditableEntity[],
): string | null {
    if (entity.kind !== "catalog value" || !entity.catalogId) {
        return null;
    }
    const catalog = entities.find(
        (candidate) =>
            candidate.kind === "catalog" && candidate.id === entity.catalogId,
    );
    if (!catalog) {
        return null;
    }
    return catalog.text;
}

function diagnosticMatchesEntity(
    diagnostic: LintDiagnostic,
    entity: EditableEntity,
    entities: EditableEntity[],
): boolean {
    if (
        entityForDiagnosticPath(entities, diagnostic.location?.path)?.path ===
        entity.path
    ) {
        return true;
    }
    const target = diagnostic.target?.entity;
    if (!isRecord(target) || typeof target.kind !== "string") {
        return false;
    }
    return entityFromSemanticTarget(entities, target)?.path === entity.path;
}

function diagnosticEntityHref(
    diagnostic: LintDiagnostic,
    entities: EditableEntity[],
    packageId: string,
    branchId: string,
): string | null {
    const pathMatch = entityForDiagnosticPath(
        entities,
        diagnostic.location?.path,
    );
    if (pathMatch) {
        return editEntityHref(packageId, branchId, pathMatch.path);
    }

    const entity = diagnostic.target?.entity;
    if (!isRecord(entity) || typeof entity.kind !== "string") {
        return null;
    }

    const match = entityFromSemanticTarget(entities, entity);
    return match ? editEntityHref(packageId, branchId, match.path) : null;
}

function entityForDiagnosticPath(
    entities: EditableEntity[],
    diagnosticPath: string | undefined,
): EditableEntity | null {
    if (!diagnosticPath) {
        return null;
    }
    return (
        entities.find(
            (entity) =>
                entity.path === diagnosticPath ||
                entity.path.endsWith(`/${diagnosticPath}`) ||
                diagnosticPath.endsWith(`/${entity.path}`),
        ) ?? null
    );
}

function entityFromSemanticTarget(
    entities: EditableEntity[],
    entity: Record<string, unknown>,
): EditableEntity | null {
    if (entity.kind === "variable" && typeof entity.id === "string") {
        return (
            entities.find(
                (candidate) =>
                    candidate.section === "variables" &&
                    candidate.id === entity.id,
            ) ?? null
        );
    }
    if (
        (entity.kind === "value" || entity.kind === "rule") &&
        typeof entity.variable === "string"
    ) {
        return (
            entities.find(
                (candidate) =>
                    candidate.section === "variables" &&
                    candidate.id === entity.variable,
            ) ?? null
        );
    }
    if (entity.kind === "qualifier" && typeof entity.id === "string") {
        return (
            entities.find(
                (candidate) =>
                    candidate.section === "qualifiers" &&
                    candidate.id === entity.id,
            ) ?? null
        );
    }
    if (entity.kind === "catalog" && typeof entity.id === "string") {
        return (
            entities.find(
                (candidate) =>
                    candidate.kind === "catalog" && candidate.id === entity.id,
            ) ?? null
        );
    }
    if (entity.kind === "catalog_entry" && typeof entity.catalog === "string") {
        return (
            entities.find(
                (candidate) =>
                    candidate.kind === "catalog value" &&
                    candidate.catalogId === entity.catalog &&
                    (typeof entity.key !== "string" ||
                        candidate.entryKey === entity.key),
            ) ?? null
        );
    }
    if (entity.kind === "custom_lint" && typeof entity.path === "string") {
        return entityForDiagnosticPath(entities, entity.path);
    }
    return null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}
