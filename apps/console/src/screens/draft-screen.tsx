import type { ReactNode } from "react";
import {
    ArrowLeft,
    Braces,
    CheckCircle2,
    ChevronRight,
    Circle,
    Database,
    ExternalLink,
    FileCode2,
    FileJson2,
    GitBranch,
    GitCompare,
    GitPullRequest,
    ListChecks,
    Pencil,
    Tags,
    Trash2,
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
import {
    DiagnosticCard,
    DiagnosticList,
    DiagnosticSummary,
} from "@/components/diagnostic-list";
import { DraftBranchEditor } from "@/components/draft-branch-editor";
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
import { PublishDraftButton } from "@/components/publish-draft-button";
import { SearchableList } from "@/components/searchable-list";
import { DraftStatusPill } from "@/components/status-pills";
import { SyncPrButton } from "@/components/sync-pr-button";
import {
    WorkspaceGraph,
    type WorkspaceGraphData,
} from "@/components/workspace-graph";
import { useApi } from "@/lib/api";
import { Link } from "@/lib/link";
import { useShellUser } from "@/lib/me";
import { RefreshScope } from "@/lib/refresh";
import type { EditKind } from "@/lib/route-normalizers";
import type {
    DraftChangeRecord,
    DraftData,
    DraftEntityData,
    DraftEventRecord,
    DraftSessionRecord,
    EditableEntity as ApiEditableEntity,
    LintDiagnostic,
    WorkspaceDefinition,
    WorkspaceSemanticModel,
} from "@/lib/types";
import { NotFound } from "@/screens/not-found";
import {
    encodeEntityPath,
    workspaceGraphData,
} from "@/screens/workspace-screen";

export type DraftScreenId =
    | "overview"
    | "edit"
    | "changes"
    | "validate"
    | "publish";

type EditableEntity = ApiEditableEntity;

type DraftLintLoad =
    | { root: string; diagnostics: LintDiagnostic[] }
    | { root: string; diagnostics: LintDiagnostic[]; error: string };

const EDIT_KIND_TITLES: Record<EditKind, string> = {
    variables: "Variables",
    qualifiers: "Qualifiers",
    catalogs: "Catalogs",
    schemas: "Schemas",
    context: "Context",
    linters: "Linters",
};

export function DraftScreen({
    draftId,
    kind = null,
    path = null,
    screen = "overview",
    workspaceId,
}: {
    draftId: string;
    kind?: EditKind | null;
    path?: string | null;
    screen?: DraftScreenId;
    workspaceId: string;
}) {
    const user = useShellUser();
    const selectedScreen = screen;
    const requestedEditKind = kind ?? "variables";
    const selectedEntityPath = path;
    const base = `/api/workspaces/${encodeURIComponent(workspaceId)}/drafts/${encodeURIComponent(draftId)}`;
    const load = useApi<DraftData>(`${base}/data`);
    const entityExtras = useApi<DraftEntityData>(
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
                    <span className="label">draft</span>
                    <h1>This draft failed to load.</h1>
                    <div className="banner banner-err">
                        <TriangleAlert aria-hidden size={16} />
                        <span>{load.error ?? "Unknown error."}</span>
                    </div>
                </div>
            </main>
        );
    }

    const {
        workspace,
        draft,
        prSyncError,
        changes,
        events,
        model: draftModel,
        entities: editableEntities,
        editLoadError,
        editedPaths,
    } = load.data;
    const lint = load.data.lint as DraftLintLoad;

    const activity = draftActivity(draft, events);
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
    // The base-ref version of the selected entity, for showing the draft's
    // delta in the editor. Null when the entity is new on this branch or the
    // base text is unavailable.
    const selectedEntityBaseText = entityExtras.data?.baseText ?? null;
    // Editing a variable: the server pre-evaluates every qualifier against each
    // saved request context so the form can preview resolution pathways live.
    const contextPreviews: EditContextPreview[] =
        entityExtras.data?.contextPreviews ?? [];

    // Editing-mode entity graph: same graph as the workspace overview, built
    // from the draft branch, with edited entities marked.
    let draftGraphData: WorkspaceGraphData | null = null;
    if (selectedScreen === "overview" && draftModel !== null) {
        const pathForKey = new Map<string, string>();
        for (const entity of editableEntities) {
            if (entity.section === "variables") {
                pathForKey.set(`variables:${entity.id}`, entity.path);
            } else if (entity.section === "qualifiers") {
                pathForKey.set(`qualifiers:${entity.id}`, entity.path);
            } else if (entity.kind === "catalog") {
                pathForKey.set(`catalogs:${entity.id}`, entity.path);
            } else if (
                entity.kind === "catalog entry" &&
                entity.catalogId &&
                entity.entryKey
            ) {
                pathForKey.set(
                    `catalog_entries:${entity.catalogId}:${entity.entryKey}`,
                    entity.path,
                );
            }
        }
        draftGraphData = workspaceGraphData({
            model: draftModel,
            pathForKey,
            hrefFor: (entityPath) =>
                editEntityHref(workspace.slug, draft.id, entityPath),
            editedPaths: new Set(editedPaths),
        });
    }
    const editableEntityCounts = editKindCounts(editableEntities);
    const contextAttributes = contextAttributeSuggestions(editableEntities);
    const schemaPathSuggestions = editableEntities
        .filter((entity) => entity.section === "schemas")
        .map((entity) => workspaceRelativePath(workspace.path, entity.path));
    const catalogIdSuggestions = editableEntities
        .filter((entity) => entity.kind === "catalog")
        .map((entity) => entity.id);
    const workspaceName = `${workspace.owner}/${workspace.name}`;
    const parentCatalogEntity =
        selectedEntity?.kind === "catalog entry"
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
        { label: "workspaces", href: "/app/workspaces" },
        { label: workspace.path, href: `/app/workspaces/${workspace.slug}` },
        ...(selectedScreen !== "overview"
            ? [
                  {
                      label: draft.branch,
                      href: draftScreenHref(
                          workspace.slug,
                          draft.id,
                          "overview",
                      ),
                  },
              ]
            : []),
        ...(selectedScreen === "edit" && selectedEntity
            ? [
                  {
                      label: EDIT_KIND_TITLES[selectedEditKind].toLowerCase(),
                      href: editKindHref(
                          workspace.slug,
                          draft.id,
                          selectedEditKind,
                      ),
                  },
                  ...(parentCatalogEntity
                      ? [
                            {
                                label: parentCatalogEntity.id,
                                href: editEntityHref(
                                    workspace.slug,
                                    draft.id,
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
          : draftScreenTitle(selectedScreen);

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
                            href={draftScreenHref(
                                workspace.slug,
                                draft.id,
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
                        <DraftStatusPill draft={draft} />
                        {draft.prUrl ? (
                            <a
                                className="btn btn-secondary btn-sm"
                                href={draft.prUrl}
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
                    label: draft.branch,
                    detail:
                        draft.status === "open"
                            ? "Saves commit to this branch — nothing reaches the base ref without review."
                            : "This draft is published; editing is locked.",
                }}
                nav={
                    <>
                        <NavBack
                            href={`/app/workspaces/${workspace.slug}/drafts`}
                            label="Workspace"
                        />
                        <NavContext
                            href={draftScreenHref(
                                workspace.slug,
                                draft.id,
                                "overview",
                            )}
                            label="draft branch"
                            value={draft.branch}
                        />
                        <NavGroupLabel>Draft</NavGroupLabel>
                        <NavLink
                            active={selectedScreen === "overview"}
                            href={draftScreenHref(
                                workspace.slug,
                                draft.id,
                                "overview",
                            )}
                            icon={<GitBranch aria-hidden size={16} />}
                            label="Overview"
                        />
                        <NavLink
                            active={selectedScreen === "changes"}
                            count={changes.length}
                            href={draftScreenHref(
                                workspace.slug,
                                draft.id,
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
                            href={draftScreenHref(
                                workspace.slug,
                                draft.id,
                                "validate",
                            )}
                            icon={<ListChecks aria-hidden size={16} />}
                            label="Validate"
                        />
                        <NavLink
                            active={selectedScreen === "publish"}
                            href={draftScreenHref(
                                workspace.slug,
                                draft.id,
                                "publish",
                            )}
                            icon={<GitPullRequest aria-hidden size={16} />}
                            label="Publish"
                        />
                        <NavGroupLabel>Edit</NavGroupLabel>
                        <NavLink
                            active={
                                selectedScreen === "edit" &&
                                selectedEditKind === "variables"
                            }
                            count={editableEntityCounts.variables}
                            href={editKindHref(
                                workspace.slug,
                                draft.id,
                                "variables",
                            )}
                            icon={<FileCode2 aria-hidden size={16} />}
                            label="Variables"
                        />
                        <NavLink
                            active={
                                selectedScreen === "edit" &&
                                selectedEditKind === "qualifiers"
                            }
                            count={editableEntityCounts.qualifiers}
                            href={editKindHref(
                                workspace.slug,
                                draft.id,
                                "qualifiers",
                            )}
                            icon={<Tags aria-hidden size={16} />}
                            label="Qualifiers"
                        />
                        <NavLink
                            active={
                                selectedScreen === "edit" &&
                                selectedEditKind === "catalogs"
                            }
                            count={editableEntityCounts.catalogs}
                            href={editKindHref(
                                workspace.slug,
                                draft.id,
                                "catalogs",
                            )}
                            icon={<Database aria-hidden size={16} />}
                            label="Catalogs"
                        />
                        <NavLink
                            active={
                                selectedScreen === "edit" &&
                                selectedEditKind === "schemas"
                            }
                            count={editableEntityCounts.schemas}
                            href={editKindHref(
                                workspace.slug,
                                draft.id,
                                "schemas",
                            )}
                            icon={<FileJson2 aria-hidden size={16} />}
                            label="Schemas"
                        />
                        <NavLink
                            active={
                                selectedScreen === "edit" &&
                                selectedEditKind === "context"
                            }
                            count={editableEntityCounts.context}
                            href={editKindHref(
                                workspace.slug,
                                draft.id,
                                "context",
                            )}
                            icon={<Braces aria-hidden size={16} />}
                            label="Context"
                        />
                        <NavLink
                            active={
                                selectedScreen === "edit" &&
                                selectedEditKind === "linters"
                            }
                            count={editableEntityCounts.linters}
                            href={editKindHref(
                                workspace.slug,
                                draft.id,
                                "linters",
                            )}
                            icon={<Wrench aria-hidden size={16} />}
                            label="Linters"
                        />
                    </>
                }
                title={title}
                user={user}
            >
                {selectedScreen === "overview" ? (
                    <DraftOverview
                        activity={activity}
                        changesCount={changes.length}
                        draft={draft}
                        graphData={draftGraphData}
                        workspaceId={workspace.slug}
                    />
                ) : null}
                {selectedScreen === "edit" ? (
                    <DraftEditScreen
                        baseText={selectedEntityBaseText}
                        contextAttributes={contextAttributes}
                        contextPreviews={contextPreviews}
                        draft={draft}
                        model={draftModel}
                        editableEntities={editableEntities}
                        entityDiagnostics={selectedEntityDiagnostics}
                        loadError={editLoadError}
                        catalogIds={catalogIdSuggestions}
                        schemaPaths={schemaPathSuggestions}
                        selectedEntity={selectedEntity}
                        selectedKind={selectedEditKind}
                        workspaceId={workspace.slug}
                    />
                ) : null}
                {selectedScreen === "changes" ? (
                    <DraftChangesScreen
                        changes={changes}
                        entityHrefForFile={(filePath) => {
                            const match = entityForDiagnosticPath(
                                editableEntities,
                                filePath,
                            );
                            return match
                                ? editEntityHref(
                                      workspace.slug,
                                      draft.id,
                                      match.path,
                                  )
                                : null;
                        }}
                    />
                ) : null}
                {selectedScreen === "validate" ? (
                    <DraftValidateScreen
                        diagnosticHref={(diagnostic) =>
                            diagnosticEntityHref(
                                diagnostic,
                                editableEntities,
                                workspace.slug,
                                draft.id,
                            )
                        }
                        lint={lint}
                    />
                ) : null}
                {selectedScreen === "publish" ? (
                    <DraftPublishScreen
                        changesCount={changes.length}
                        draft={draft}
                        lintHasErrors={lintHasErrors}
                        prSyncError={prSyncError}
                        workspaceId={workspace.slug}
                    />
                ) : null}
            </AppShell>
        </RefreshScope>
    );
}

function DraftOverview({
    activity,
    changesCount,
    draft,
    graphData,
    workspaceId,
}: {
    activity: DraftEventRecord[];
    changesCount: number;
    draft: DraftSessionRecord;
    graphData: WorkspaceGraphData | null;
    workspaceId: string;
}) {
    const newestFirst = [...activity].sort(
        (left, right) =>
            Date.parse(right.createdAt) - Date.parse(left.createdAt),
    );
    return (
        <section className="section">
            <div className="section-header">
                <div className="section-header-text">
                    <h1 className="mono">{draft.branch}</h1>
                    <p className="hint">
                        Edits commit directly to this branch. When the draft is
                        ready, publish it as a pull request from the publish
                        screen.
                    </p>
                </div>
                <DraftStatusPill draft={draft} />
            </div>
            <div className="meta-grid">
                <div className="meta-item">
                    <span className="label">base ref</span>
                    <span className="meta-value mono">{draft.baseRef}</span>
                </div>
                <div className="meta-item">
                    <span className="label">tracked changes</span>
                    <span className="meta-value">{changesCount}</span>
                </div>
                <div className="meta-item">
                    <span className="label">created</span>
                    <span className="meta-value">
                        {formatDate(draft.createdAt)}
                    </span>
                </div>
                <div className="meta-item">
                    <span className="label">updated</span>
                    <span className="meta-value">
                        {formatDate(draft.updatedAt)}
                    </span>
                </div>
            </div>
            <div className="card">
                <div className="card-head-text">
                    <h3>Branch name</h3>
                    <p className="hint">
                        Renaming moves the branch on GitHub. Locked once the
                        draft is published.
                    </p>
                </div>
                <DraftBranchEditor
                    branch={draft.branch}
                    disabled={draft.status !== "open"}
                    draftId={draft.id}
                    workspaceId={workspaceId}
                />
            </div>
            {graphData ? (
                <div className="card graph-card">
                    <div className="card-head-text">
                        <h3>Entity graph</h3>
                        <p className="hint">
                            The workspace as this draft sees it. Entities edited
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
                    <WorkspaceGraph data={graphData} />
                </div>
            ) : null}
            <div className="section-header" style={{ marginTop: 8 }}>
                <div className="section-header-text">
                    <h2>Activity</h2>
                    <p className="hint">
                        Everything that happened on this draft, newest first.
                    </p>
                </div>
            </div>
            <div className="timeline">
                {newestFirst.map((event) => (
                    <div className="tl-row" key={event.id}>
                        <span
                            className="tl-icon"
                            data-tone={eventTone(event.kind)}
                        >
                            {eventIcon(event.kind)}
                        </span>
                        <span className="tl-body">
                            <span>{event.summary}</span>
                            {event.detailJson ? (
                                <span
                                    className="tl-detail"
                                    title={event.detailJson}
                                >
                                    {event.detailJson}
                                </span>
                            ) : null}
                        </span>
                        <span className="tl-when">
                            {formatDate(event.createdAt)}
                        </span>
                    </div>
                ))}
            </div>
        </section>
    );
}

function DraftEditScreen({
    baseText,
    contextAttributes,
    contextPreviews,
    draft,
    editableEntities,
    entityDiagnostics,
    loadError,
    model,
    catalogIds,
    schemaPaths,
    selectedEntity,
    selectedKind,
    workspaceId,
}: {
    baseText: string | null;
    contextAttributes: string[];
    contextPreviews: EditContextPreview[];
    draft: DraftSessionRecord;
    editableEntities: EditableEntity[];
    entityDiagnostics: LintDiagnostic[];
    loadError: string | null;
    model: WorkspaceSemanticModel | null;
    catalogIds: string[];
    schemaPaths: string[];
    selectedEntity: EditableEntity | null;
    selectedKind: EditKind;
    workspaceId: string;
}) {
    const entities = editableEntities.filter(
        (entity) => entity.section === selectedKind,
    );

    return (
        <section className="section">
            {loadError ? (
                <div className="banner banner-err">
                    <TriangleAlert aria-hidden size={16} />
                    <span>The draft workspace failed to load: {loadError}</span>
                </div>
            ) : null}
            {selectedEntity ? (
                <EditableEntityDetail
                    allEntities={editableEntities}
                    baseText={baseText}
                    contextAttributes={contextAttributes}
                    contextPreviews={contextPreviews}
                    diagnostics={entityDiagnostics}
                    draft={draft}
                    entity={selectedEntity}
                    model={model}
                    catalogIds={catalogIds}
                    schemaPaths={schemaPaths}
                    workspaceId={workspaceId}
                />
            ) : (
                <>
                    <div className="section-header-text">
                        <h1>{EDIT_KIND_TITLES[selectedKind]}</h1>
                        <p className="hint">
                            Pick an entity to edit it with a form or as source.
                            Saves commit to the draft branch.
                        </p>
                    </div>
                    <EditableEntityList
                        disabled={draft.status !== "open"}
                        draftId={draft.id}
                        entities={entities}
                        kind={selectedKind}
                        workspaceId={workspaceId}
                    />
                </>
            )}
        </section>
    );
}

function EditableEntityList({
    disabled,
    draftId,
    entities,
    kind,
    workspaceId,
}: {
    disabled?: boolean;
    draftId: string;
    entities: EditableEntity[];
    kind: EditKind;
    workspaceId: string;
}) {
    return (
        <>
            <AddEntityForm
                disabled={disabled}
                draftId={draftId}
                kind={kind}
                workspaceId={workspaceId}
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
                                workspaceId,
                                draftId,
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
    draft,
    entity,
    model,
    catalogIds,
    schemaPaths,
    workspaceId,
}: {
    allEntities: EditableEntity[];
    baseText: string | null;
    contextAttributes: string[];
    contextPreviews: EditContextPreview[];
    diagnostics: LintDiagnostic[];
    draft: DraftSessionRecord;
    entity: EditableEntity;
    model: WorkspaceSemanticModel | null;
    catalogIds: string[];
    schemaPaths: string[];
    workspaceId: string;
}) {
    const catalogEntries =
        entity.kind === "catalog"
            ? allEntities.filter(
                  (candidate) =>
                      candidate.kind === "catalog entry" &&
                      candidate.catalogId === entity.id,
              )
            : [];
    const parentCatalog =
        entity.kind === "catalog entry"
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
                        href={`/app/workspaces/${workspaceId}/tree/${encodeEntityPath(entity.path)}`}
                    >
                        View on {draft.baseRef}
                    </Link>
                    {parentCatalog ? (
                        <Link
                            className="btn btn-secondary btn-sm"
                            href={editEntityHref(
                                workspaceId,
                                draft.id,
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
                                workspaceId,
                                draft.id,
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
                disabled={draft.status !== "open"}
                draftId={draft.id}
                entity={entity}
                guidance={{
                    ...buildFormGuidance(entity, allEntities, model),
                    contextPreviews,
                }}
                catalogIds={catalogIds}
                catalogSchema={catalogEntrySchemaText(entity, allEntities)}
                schemaPaths={schemaPaths}
                sourceMarks={sourceMarks}
                workspaceId={workspaceId}
            />
            {entity.kind === "catalog" ? (
                <AddCatalogEntryForm
                    disabled={draft.status !== "open"}
                    draftId={draft.id}
                    catalogId={entity.id}
                    workspaceId={workspaceId}
                />
            ) : null}
            {catalogEntries.length > 0 ? (
                <div className="card">
                    <div className="card-head-text">
                        <h3>Catalog entries</h3>
                        <p className="hint">
                            Entries available for this catalog.
                        </p>
                    </div>
                    <div className="reference-links">
                        {catalogEntries.map((entry) => (
                            <Link
                                className="pill pill-neutral"
                                href={editEntityHref(
                                    workspaceId,
                                    draft.id,
                                    entry.path,
                                )}
                                key={entry.path}
                            >
                                {entry.entryKey ?? entry.id}
                            </Link>
                        ))}
                    </div>
                </div>
            ) : null}
            <div className="card">
                <div className="card-head">
                    <div className="card-head-text">
                        <h3>Delete from draft</h3>
                        <p className="hint">
                            Removes <span className="mono">{entity.path}</span>{" "}
                            from the draft branch. The base ref is untouched
                            until the pull request merges.
                        </p>
                    </div>
                    <DeleteEntityButton
                        disabled={draft.status !== "open"}
                        draftId={draft.id}
                        filePath={entity.path}
                        returnHref={editKindHref(
                            workspaceId,
                            draft.id,
                            entity.section,
                        )}
                        workspaceId={workspaceId}
                    />
                </div>
            </div>
        </>
    );
}

function DraftChangesScreen({
    changes,
    entityHrefForFile,
}: {
    changes: DraftChangeRecord[];
    entityHrefForFile: (filePath: string) => string | null;
}) {
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Changes</h1>
                <p className="hint">
                    Semantic changes tracked on this draft. They become the pull
                    request body, so reviewers see what changed in rototo terms
                    — not just file diffs.
                </p>
            </div>
            {changes.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <GitCompare aria-hidden size={18} />
                    </span>
                    <p>
                        No tracked changes yet. Edits you save will show up here
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
                            data-search={`${change.variableId} ${change.valueKey} ${change.filePath} ${change.beforeJson} ${change.afterJson}`}
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
                                            {change.variableId}
                                        </Link>
                                    ) : (
                                        change.variableId
                                    )}{" "}
                                    · {change.filePath}
                                </span>
                                <span className="tag">{change.valueKey}</span>
                            </div>
                            <div className="diffbody">
                                <div className="dl dl-del">
                                    <span className="g">−</span>
                                    <span className="t">
                                        {jsonSummary(change.beforeJson)}
                                    </span>
                                </div>
                                <div className="dl dl-add">
                                    <span className="g">+</span>
                                    <span className="t">
                                        {jsonSummary(change.afterJson)}
                                    </span>
                                </div>
                            </div>
                        </article>
                    ))}
                </SearchableList>
            )}
        </section>
    );
}

function DraftValidateScreen({
    diagnosticHref,
    lint,
}: {
    diagnosticHref: (diagnostic: LintDiagnostic) => string | null;
    lint: DraftLintLoad;
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
                    Lint runs against the draft branch. Publishing is blocked
                    while errors are present — warnings ship, errors don’t.
                </p>
            </div>
            <DiagnosticList
                diagnosticHref={diagnosticHref}
                diagnostics={lint.diagnostics}
            />
        </section>
    );
}

function DraftPublishScreen({
    changesCount,
    draft,
    lintHasErrors,
    prSyncError,
    workspaceId,
}: {
    changesCount: number;
    draft: DraftSessionRecord;
    lintHasErrors: boolean;
    prSyncError: string | null;
    workspaceId: string;
}) {
    const published = Boolean(draft.prUrl);
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Publish</h1>
                <p className="hint">
                    Publishing opens a pull request from{" "}
                    <span className="mono">{draft.branch}</span> to{" "}
                    <span className="mono">{draft.baseRef}</span>. Nothing
                    reaches the base ref without review.
                </p>
            </div>
            {published ? (
                <div className="card">
                    <div className="applied-row">
                        <span className="checkdot">
                            <GitPullRequest aria-hidden size={15} />
                        </span>
                        <div className="card-head-text">
                            <h3>Pull request {draft.prState ?? "open"}</h3>
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
                                href={draft.prUrl ?? "#"}
                                rel="noreferrer"
                                target="_blank"
                            >
                                <ExternalLink aria-hidden size={15} />
                                Open pull request
                            </a>
                            <SyncPrButton
                                draftId={draft.id}
                                workspaceId={workspaceId}
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
                <div className="card">
                    <div className="card-head-text">
                        <h3>Ready to publish?</h3>
                    </div>
                    <PublishCheck ok={changesCount > 0}>
                        {changesCount > 0
                            ? `${changesCount} tracked ${changesCount === 1 ? "change" : "changes"} to publish`
                            : "No tracked changes yet — save an edit first"}
                    </PublishCheck>
                    <PublishCheck ok={!lintHasErrors}>
                        {lintHasErrors
                            ? "Lint reports errors — fix them on the validate screen"
                            : "Lint is clean"}
                    </PublishCheck>
                    <PublishCheck ok={draft.status === "open"}>
                        {draft.status === "open"
                            ? "Draft is open"
                            : `Draft is ${draft.status}`}
                    </PublishCheck>
                    <div className="action-row">
                        <PublishDraftButton
                            disabled={
                                draft.status !== "open" ||
                                changesCount === 0 ||
                                lintHasErrors
                            }
                            draftId={draft.id}
                            workspaceId={workspaceId}
                        />
                    </div>
                </div>
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

function eventTone(kind: string): "sea" | "ok" | "err" | "neutral" {
    if (kind.includes("publish") || kind.includes("pr")) {
        return "ok";
    }
    if (kind.includes("delete")) {
        return "err";
    }
    if (kind.includes("created")) {
        return "sea";
    }
    return "neutral";
}

function eventIcon(kind: string): ReactNode {
    if (kind.includes("publish") || kind.includes("pr")) {
        return <GitPullRequest aria-hidden size={14} />;
    }
    if (kind.includes("delete")) {
        return <Trash2 aria-hidden size={14} />;
    }
    if (kind.includes("created")) {
        return <GitBranch aria-hidden size={14} />;
    }
    return <Pencil aria-hidden size={14} />;
}

function editKindIcon(kind: EditKind, size: number): ReactNode {
    switch (kind) {
        case "variables":
            return <FileCode2 aria-hidden size={size} />;
        case "qualifiers":
            return <Tags aria-hidden size={size} />;
        case "catalogs":
            return <Database aria-hidden size={size} />;
        case "schemas":
            return <FileJson2 aria-hidden size={size} />;
        case "context":
            return <Braces aria-hidden size={size} />;
        case "linters":
            return <Wrench aria-hidden size={size} />;
    }
}

function draftScreenTitle(screen: DraftScreenId): string {
    switch (screen) {
        case "overview":
            return "Draft overview";
        case "edit":
            return "Edit";
        case "changes":
            return "Changes";
        case "validate":
            return "Validate";
        case "publish":
            return "Publish";
    }
}

function jsonSummary(value: string): string {
    try {
        return JSON.stringify(JSON.parse(value));
    } catch {
        return value;
    }
}

function draftActivity(
    draft: DraftSessionRecord,
    events: DraftEventRecord[],
): DraftEventRecord[] {
    const hasCreatedEvent = events.some(
        (event) => event.kind === "draft.created",
    );
    if (hasCreatedEvent) {
        return events;
    }
    return [
        {
            id: `${draft.id}:created`,
            draftId: draft.id,
            kind: "draft.created",
            summary: `Created draft branch ${draft.branch}`,
            detailJson: JSON.stringify({
                branch: draft.branch,
                baseRef: draft.baseRef,
            }),
            createdAt: draft.createdAt,
        },
        ...events,
    ];
}

function formatDate(value: string): string {
    return new Intl.DateTimeFormat("en", {
        dateStyle: "medium",
        timeStyle: "short",
    }).format(new Date(value));
}

function draftScreenHref(
    workspaceId: string,
    draftId: string,
    screen: DraftScreenId,
): string {
    const base = `/app/workspaces/${workspaceId}/drafts/${draftId}`;
    return screen === "overview" ? base : `${base}/${screen}`;
}

function editKindHref(
    workspaceId: string,
    draftId: string,
    kind: EditKind,
): string {
    return `/app/workspaces/${workspaceId}/drafts/${draftId}/edit/${kind}`;
}

function editEntityHref(
    workspaceId: string,
    draftId: string,
    path: string,
): string {
    return `/app/workspaces/${workspaceId}/drafts/${draftId}/tree/${encodeEntityPath(path)}`;
}

export function normalizeDraftScreen(
    value: string | null,
): DraftScreenId | null {
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
        schemas: 0,
        context: 0,
        linters: 0,
    };
    for (const entity of entities) {
        counts[entity.section] += 1;
    }
    return counts;
}

function contextAttributeSuggestions(entities: EditableEntity[]): string[] {
    const contextSchema = entities.find((entity) =>
        entity.path.endsWith("schemas/context.schema.json"),
    );
    if (!contextSchema) {
        return [];
    }
    try {
        const parsed = JSON.parse(contextSchema.text) as unknown;
        return collectSchemaAttributes(parsed)
            .filter((attribute) => !attribute.startsWith("qualifier."))
            .sort((left, right) => left.localeCompare(right));
    } catch {
        return [];
    }
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

/* Descriptions from related schemas and example values harvested from
   sibling entities, so forms can show what good input looks like. */
function buildFormGuidance(
    entity: EditableEntity,
    entities: EditableEntity[],
    model: WorkspaceSemanticModel | null,
): FormGuidance {
    const guidance: FormGuidance = {};
    if (entity.section === "qualifiers") {
        guidance.contextAttributeDocs = contextAttributeDocs(entities);
        const examples: Record<string, string[]> = {};
        for (const qualifier of model?.qualifiers ?? []) {
            if (qualifier.id === entity.id) {
                continue;
            }
            for (const predicate of qualifier.predicates) {
                const subject = predicate.attribute?.value;
                if (
                    !subject ||
                    subject.startsWith("qualifier.") ||
                    predicate.value === undefined
                ) {
                    continue;
                }
                const literal = JSON.stringify(predicate.value);
                const list = (examples[subject] ??= []);
                if (!list.includes(literal) && list.length < 6) {
                    list.push(literal);
                }
            }
        }
        guidance.attributeValueExamples = examples;
    }
    if (entity.section === "variables") {
        guidance.qualifierIds = entities
            .filter((candidate) => candidate.section === "qualifiers")
            .map((candidate) => candidate.id);
        const entryKeys: Record<string, string[]> = {};
        for (const candidate of entities) {
            if (
                candidate.kind === "catalog entry" &&
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
        guidance.schemaDocs = Object.fromEntries(
            entities
                .filter(
                    (candidate) =>
                        candidate.section === "schemas" &&
                        candidate.description,
                )
                .map((candidate) => [
                    candidate.path.split("/").pop() ?? candidate.path,
                    candidate.description as string,
                ]),
        );
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
    if (entity.kind === "catalog entry") {
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
    const schema = entities.find((candidate) =>
        candidate.path.endsWith("schemas/context.schema.json"),
    );
    if (!schema) {
        return {};
    }
    try {
        const docs: Record<string, string> = {};
        collectAttributeDocs(JSON.parse(schema.text), "", docs);
        return docs;
    } catch {
        return {};
    }
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

function workspaceRelativePath(workspacePath: string, path: string): string {
    const prefix = workspacePath === "." ? "" : `${workspacePath}/`;
    return prefix && path.startsWith(prefix) ? path.slice(prefix.length) : path;
}

function catalogEntrySchemaText(
    entity: EditableEntity,
    entities: EditableEntity[],
): string | null {
    if (entity.kind !== "catalog entry" || !entity.catalogId) {
        return null;
    }
    const catalog = entities.find(
        (candidate) =>
            candidate.kind === "catalog" && candidate.id === entity.catalogId,
    );
    if (!catalog) {
        return null;
    }
    const schemaRef = /^\s*schema\s*=\s*"([^"]+)"\s*$/m.exec(catalog.text)?.[1];
    const basename = schemaRef?.split("/").pop();
    if (!basename) {
        return null;
    }
    const schema = entities.find(
        (candidate) =>
            candidate.section === "schemas" &&
            candidate.path.split("/").pop() === basename,
    );
    return schema?.text ?? null;
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
    workspaceId: string,
    draftId: string,
): string | null {
    const pathMatch = entityForDiagnosticPath(
        entities,
        diagnostic.location?.path,
    );
    if (pathMatch) {
        return editEntityHref(workspaceId, draftId, pathMatch.path);
    }

    const entity = diagnostic.target?.entity;
    if (!isRecord(entity) || typeof entity.kind !== "string") {
        return null;
    }

    const match = entityFromSemanticTarget(entities, entity);
    return match ? editEntityHref(workspaceId, draftId, match.path) : null;
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
    if (entity.kind === "predicate" && typeof entity.qualifier === "string") {
        return (
            entities.find(
                (candidate) =>
                    candidate.section === "qualifiers" &&
                    candidate.id === entity.qualifier,
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
                    candidate.kind === "catalog entry" &&
                    candidate.catalogId === entity.catalog &&
                    (typeof entity.key !== "string" ||
                        candidate.entryKey === entity.key),
            ) ?? null
        );
    }
    if (entity.kind === "schema" && typeof entity.path === "string") {
        return entityForDiagnosticPath(entities, entity.path);
    }
    if (entity.kind === "custom_lint" && typeof entity.path === "string") {
        return entityForDiagnosticPath(entities, entity.path);
    }
    return null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}
