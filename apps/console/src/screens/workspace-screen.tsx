import type { ReactNode } from "react";
import {
    ArrowLeft,
    Boxes,
    Braces,
    CheckCircle2,
    ChevronRight,
    Database,
    FileCode2,
    FileJson2,
    GitBranch,
    ListChecks,
    Pencil,
    Tags,
    TriangleAlert,
    Wrench,
} from "lucide-react";
import { Navigate } from "react-router";

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
import { LoadingScreen } from "@/components/loading-screen";
import { SearchableList } from "@/components/searchable-list";
import { BranchCandidates } from "@/components/branch-candidates";
import { ReadOnlySource } from "@/components/read-only-source";
import { BranchStatusPill } from "@/components/status-pills";
import { StartBranchButton } from "@/components/start-branch-button";
import { OpenBranchForm } from "@/components/open-branch-form";
import { WorkspaceGraph } from "@/components/workspace-graph";
import type { WorkspaceGraphData } from "@/components/workspace-graph/types";
import { useApi } from "@/lib/api";
import { schemaSummary } from "@/lib/entity-summary";
import { Link } from "@/lib/link";
import { useShellUser } from "@/lib/me";
import { RefreshScope } from "@/lib/refresh";
import type { SectionId } from "@/lib/route-normalizers";
import type {
    BranchRecord,
    LintDiagnostic,
    QualifierContextEvaluation,
    QualifierEvaluation,
    ReferenceModel,
    SavedContextResolution,
    VariableModel,
    WorkspaceData,
    WorkspaceDefinition,
    WorkspaceEntityData,
    WorkspaceCapabilities,
    WorkspaceInventory,
    WorkspaceLintView,
    WorkspaceSemanticModel,
} from "@/lib/types";
import { NotFound } from "@/screens/not-found";

/** Flattened workspace entity used for lists, links, and graph construction. */
type EntityNode = {
    section: SectionId;
    kind: string;
    id: string;
    path: string;
    description: string | null;
    badge: string | null;
    targetKey: string;
    outboundKeys: string[];
};

/** Lint payload for the staged workspace, including staging/lint failures. */
type LintLoad =
    | WorkspaceLintView
    | { root: string; diagnostics: LintDiagnostic[]; error: string };

const SECTION_TITLES: Record<SectionId, string> = {
    overview: "Overview",
    variables: "Variables",
    qualifiers: "Qualifiers",
    catalogs: "Catalogs",
    schemas: "Schemas",
    linters: "Linters",
    context: "Context",
    diagnostics: "Diagnostics",
    branches: "Branches",
};

const SECTION_HINTS: Partial<Record<SectionId, string>> = {
    variables:
        "Named values the application resolves at runtime, with defaults and rules.",
    qualifiers:
        "Named runtime conditions. Rules reference them to select values.",
    catalogs: "Typed entries that schema-backed variables can point at.",
    schemas: "JSON Schemas that validate context and selected values.",
    linters: "Custom lint rules this workspace declares beyond the built-ins.",
    context: "The context contract: schema plus example resolution contexts.",
};

export function WorkspaceScreen({
    path = null,
    section = null,
    workspaceId,
}: {
    path?: string | null;
    section?: SectionId | null;
    workspaceId: string;
}) {
    const user = useShellUser();
    const data = useApi<WorkspaceData>(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/data`,
    );
    const entity = useApi<WorkspaceEntityData>(
        path
            ? `/api/workspaces/${encodeURIComponent(workspaceId)}/entity?path=${encodeURIComponent(path)}`
            : null,
    );

    if (data.loading || (path && entity.loading)) {
        return <LoadingScreen />;
    }
    if (data.status === 404) {
        return <NotFound />;
    }
    if (data.error || !data.data) {
        return (
            <main className="fault-page">
                <div className="fault-panel">
                    <span className="label">workspace</span>
                    <h1>This workspace failed to load.</h1>
                    <div className="banner banner-err">
                        <TriangleAlert aria-hidden size={16} />
                        <span>{data.error ?? "Unknown error."}</span>
                    </div>
                </div>
            </main>
        );
    }

    const {
        workspace,
        branches,
        inventory,
        inventoryError,
        model,
        capabilities,
    } = data.data;
    const lint = data.data.lint as LintLoad;
    const writeDisabled = capabilities.write.kind === "disabled";
    const writeDisabledReason =
        capabilities.write.kind === "disabled"
            ? capabilities.write.reason
            : undefined;

    // Canonical URLs use the friendly slug; id URLs redirect to it.
    if (workspaceId !== workspace.slug) {
        return (
            <Navigate
                replace
                to={
                    path
                        ? entityHref(workspace.slug, path)
                        : sectionHref(workspace.slug, section ?? "overview")
                }
            />
        );
    }

    const nodes = entityNodes(inventory);
    const entityCounts = {
        variables: inventory.variables.length,
        qualifiers: inventory.qualifiers.length,
        catalogs: inventory.catalogs.length,
        schemas: inventory.schemas.length,
        linters: inventory.linters.length,
        context:
            inventory.context.exampleCount +
            (inventory.context.schemaPath ? 1 : 0),
    };
    const selectedPath = path;
    const selectedNode = selectedPath
        ? (nodes.find((node) => node.path === selectedPath) ?? null)
        : null;
    const selectedSection = selectedNode
        ? selectedNode.section
        : (section ?? "overview");
    const definition: WorkspaceDefinition | null =
        entity.data?.definition ?? null;
    const definitionError: string | null =
        entity.data?.definitionError ?? entity.error;
    const contextResolutions: SavedContextResolution[] =
        entity.data?.contextResolutions ?? [];
    const qualifierEvaluations: QualifierContextEvaluation[] =
        entity.data?.qualifierEvaluations ?? [];

    // Graph data for the overview, built from the semantic model with entity
    // hrefs resolved per node.
    let graphData: WorkspaceGraphData | null = null;
    if (selectedSection === "overview" && !selectedNode && model !== null) {
        graphData = workspaceGraphData({
            model,
            pathForKey: new Map(
                nodes.map((node) => [node.targetKey, node.path]),
            ),
            hrefFor: (entityPath) => entityHref(workspace.slug, entityPath),
        });
    }
    const diagnosticCount = "error" in lint ? 0 : lint.diagnostics.length;
    const parentCatalogNode =
        selectedNode?.kind === "catalog entry"
            ? (nodes.find(
                  (node) => node.targetKey === `catalogs:${selectedNode.badge}`,
              ) ?? null)
            : null;
    const entityDiagnostics =
        selectedNode && !("error" in lint)
            ? lint.diagnostics.filter((diagnostic) =>
                  diagnosticMatchesNode(diagnostic, selectedNode, nodes),
              )
            : [];
    const workspaceName = `${workspace.owner}/${workspace.name}`;
    const entityCrumbLabel = selectedNode
        ? parentCatalogNode &&
          selectedNode.id.startsWith(`${parentCatalogNode.id}/`)
            ? selectedNode.id.slice(parentCatalogNode.id.length + 1)
            : selectedNode.id
        : "";
    // Crumbs are ancestors only; the topbar title names the current screen.
    const crumbs = [
        { label: "console", href: "/app" },
        { label: "workspaces", href: "/app/workspaces" },
        ...(selectedNode || selectedSection !== "overview"
            ? [
                  {
                      label: workspace.path,
                      href: `/app/workspaces/${workspace.slug}`,
                  },
              ]
            : []),
        ...(selectedNode
            ? [
                  {
                      label: SECTION_TITLES[selectedNode.section].toLowerCase(),
                      href: sectionHref(workspace.slug, selectedNode.section),
                  },
                  ...(parentCatalogNode
                      ? [
                            {
                                label: parentCatalogNode.id,
                                href: entityHref(
                                    workspace.slug,
                                    parentCatalogNode.path,
                                ),
                            },
                        ]
                      : []),
              ]
            : []),
    ];

    return (
        <RefreshScope
            onRefresh={() => {
                data.reload();
                entity.reload();
            }}
        >
            <AppShell
                actions={
                    <>
                        <Link
                            className="pill-link"
                            href={sectionHref(workspace.slug, "diagnostics")}
                            title="Open diagnostics"
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
                        <StartBranchButton
                            disabled={writeDisabled}
                            disabledReason={writeDisabledReason}
                            workspaceId={workspace.slug}
                        />
                    </>
                }
                crumbs={crumbs}
                nav={
                    <>
                        <NavBack
                            href="/app/workspaces"
                            label="All workspaces"
                        />
                        <NavContext
                            href={`/app/workspaces/${workspace.slug}`}
                            label="workspace"
                            value={`${workspaceName} · ${workspace.path}`}
                        />
                        <NavGroupLabel>Inspect</NavGroupLabel>
                        <NavLink
                            active={
                                !selectedNode && selectedSection === "overview"
                            }
                            href={sectionHref(workspace.slug, "overview")}
                            icon={<Boxes aria-hidden size={16} />}
                            label="Overview"
                        />
                        <NavLink
                            active={selectedSection === "variables"}
                            count={entityCounts.variables}
                            href={sectionHref(workspace.slug, "variables")}
                            icon={<FileCode2 aria-hidden size={16} />}
                            label="Variables"
                        />
                        <NavLink
                            active={selectedSection === "qualifiers"}
                            count={entityCounts.qualifiers}
                            href={sectionHref(workspace.slug, "qualifiers")}
                            icon={<Tags aria-hidden size={16} />}
                            label="Qualifiers"
                        />
                        <NavLink
                            active={selectedSection === "catalogs"}
                            count={entityCounts.catalogs}
                            href={sectionHref(workspace.slug, "catalogs")}
                            icon={<Database aria-hidden size={16} />}
                            label="Catalogs"
                        />
                        <NavLink
                            active={selectedSection === "schemas"}
                            count={entityCounts.schemas}
                            href={sectionHref(workspace.slug, "schemas")}
                            icon={<FileJson2 aria-hidden size={16} />}
                            label="Schemas"
                        />
                        <NavLink
                            active={selectedSection === "linters"}
                            count={entityCounts.linters}
                            href={sectionHref(workspace.slug, "linters")}
                            icon={<Wrench aria-hidden size={16} />}
                            label="Linters"
                        />
                        <NavLink
                            active={selectedSection === "context"}
                            count={entityCounts.context}
                            href={sectionHref(workspace.slug, "context")}
                            icon={<Braces aria-hidden size={16} />}
                            label="Context"
                        />
                        <NavGroupLabel>Operate</NavGroupLabel>
                        <NavLink
                            active={
                                !selectedNode &&
                                selectedSection === "diagnostics"
                            }
                            count={diagnosticCount}
                            href={sectionHref(workspace.slug, "diagnostics")}
                            icon={<ListChecks aria-hidden size={16} />}
                            label="Diagnostics"
                        />
                        <NavLink
                            active={
                                !selectedNode && selectedSection === "branches"
                            }
                            count={branches.length}
                            href={sectionHref(workspace.slug, "branches")}
                            icon={<GitBranch aria-hidden size={16} />}
                            label="Branches"
                        />
                    </>
                }
                title={
                    selectedNode
                        ? entityCrumbLabel
                        : SECTION_TITLES[selectedSection]
                }
                user={user}
            >
                {selectedNode ? (
                    <EntityDefinition
                        allNodes={nodes}
                        contextResolutions={contextResolutions}
                        definition={definition}
                        diagnostics={entityDiagnostics}
                        error={definitionError}
                        model={model}
                        node={selectedNode}
                        activeBranch={
                            branches.find((branch) => branch.status === "active") ??
                            null
                        }
                        parentCatalog={parentCatalogNode}
                        qualifierEvaluations={qualifierEvaluations}
                        workspaceId={workspace.slug}
                    />
                ) : (
                    <WorkspaceSection
                        diagnosticCount={diagnosticCount}
                        branches={branches}
                        graphData={graphData}
                        capabilities={capabilities}
                        inventory={inventory}
                        inventoryError={inventoryError}
                        lint={lint}
                        nodes={nodes}
                        section={selectedSection}
                        workspace={workspace}
                    />
                )}
            </AppShell>
        </RefreshScope>
    );
}

function WorkspaceSection({
    capabilities,
    diagnosticCount,
    branches,
    graphData,
    inventory,
    inventoryError,
    lint,
    nodes,
    section,
    workspace,
}: {
    capabilities: WorkspaceCapabilities;
    diagnosticCount: number;
    branches: BranchRecord[];
    graphData: WorkspaceGraphData | null;
    inventory: WorkspaceInventory;
    inventoryError: string | null;
    lint: WorkspaceLintView | { root: string; diagnostics: []; error: string };
    nodes: EntityNode[];
    section: SectionId;
    workspace: {
        id: string;
        slug: string;
        repoId: string;
        owner: string;
        name: string;
        path: string;
        ref: string;
    };
}) {
    if (section === "overview") {
        const activeBranches = branches.filter(
            (branch) => branch.status === "active",
        );
        return (
            <section className="section">
                <div className="section-header">
                    <div className="section-header-text">
                        <h1 className="mono">{workspace.path}</h1>
                        <p className="hint">
                            What this workspace declares, and whether it lints
                            clean right now.
                        </p>
                    </div>
                </div>
                <div className="meta-grid">
                    <div className="meta-item">
                        <span className="label">repository</span>
                        <span className="meta-value mono">
                            <Link
                                className="title-link"
                                href={`/app/workspaces?repo=${workspace.repoId}`}
                            >
                                {workspace.owner}/{workspace.name}
                            </Link>
                        </span>
                    </div>
                    <div className="meta-item">
                        <span className="label">workspace path</span>
                        <span className="meta-value mono">
                            {workspace.path}
                        </span>
                    </div>
                    <div className="meta-item">
                        <span className="label">ref</span>
                        <span className="meta-value mono">{workspace.ref}</span>
                    </div>
                </div>
                {inventoryError ? (
                    <div className="banner banner-err">
                        <TriangleAlert aria-hidden size={16} />
                        <span>
                            Workspace inventory failed to load: {inventoryError}
                        </span>
                    </div>
                ) : (
                    <>
                        <OverviewAttention
                            diagnostics={
                                "error" in lint ? [] : lint.diagnostics
                            }
                            lintError={"error" in lint ? lint.error : null}
                            nodes={nodes}
                            activeBranches={activeBranches}
                            workspaceId={workspace.slug}
                        />
                        {graphData ? (
                            <div className="card graph-card">
                                <div className="card-head-text">
                                    <h3>Entity graph</h3>
                                    <p className="hint">
                                        How qualifiers, variables, catalogs, and
                                        entries connect. Hover a node to trace
                                        its references; click to open it.
                                    </p>
                                </div>
                                <WorkspaceGraph data={graphData} />
                            </div>
                        ) : null}
                    </>
                )}
            </section>
        );
    }

    if (section === "diagnostics") {
        return "error" in lint ? (
            <div className="banner banner-err">
                <TriangleAlert aria-hidden size={16} />
                <span>Lint failed to run: {lint.error}</span>
            </div>
        ) : (
            <section className="section">
                <div className="section-header-text">
                    <h1>Diagnostics</h1>
                    <p className="hint">
                        Semantic lint results, grouped by the entity they point
                        at. Source locations are kept so you can fix files
                        directly.
                    </p>
                </div>
                <DiagnosticList
                    diagnosticHref={(diagnostic) => {
                        const match = nodes.find((node) =>
                            diagnosticMatchesNode(diagnostic, node, nodes),
                        );
                        return match
                            ? entityHref(workspace.slug, match.path)
                            : null;
                    }}
                    diagnostics={lint.diagnostics}
                />
            </section>
        );
    }

    if (section === "branches") {
        return (
            <section className="section">
                <div className="section-header-text">
                    <h1>Branches</h1>
                    <p className="hint">
                        {capabilities.write.kind === "pullRequest"
                            ? "Each branch is a branch created from "
                            : "Each branch edits "}
                        <span className="mono">{workspace.ref}</span>
                        {capabilities.write.kind === "pullRequest"
                            ? ". Edits commit to the branch; publishing opens a pull request."
                            : ". Publishing applies the configured direct-push workflow."}
                    </p>
                </div>
                {capabilities.write.kind === "pullRequest" ? (
                    <>
                        <BranchCandidates workspaceId={workspace.slug} />
                        <OpenBranchForm
                            workspaceId={workspace.slug}
                        />
                    </>
                ) : null}
                {branches.length === 0 ? (
                    <div className="empty-state">
                        <span className="empty-puck">
                            <GitBranch aria-hidden size={18} />
                        </span>
                        <p>
                            No branches yet. Use “Edit workspace” to start
                            one, or open an existing branch above.
                        </p>
                    </div>
                ) : (
                    <SearchableList
                        className="row-list"
                        emptyLabel="No branches match that search."
                        label="Search branches"
                        placeholder="Search branches"
                    >
                        {branches.map((branch) => (
                            <Link
                                className="row"
                                data-search={`${branch.branch} ${branch.status} ${branch.prState ?? ""} ${branch.prUrl ?? ""}`}
                                href={`/app/workspaces/${workspace.slug}/branches/${branch.id}`}
                                key={branch.id}
                            >
                                <span className="row-icon">
                                    <GitBranch aria-hidden size={16} />
                                </span>
                                <span className="row-text">
                                    <span className="row-title mono">
                                        {branch.branch}
                                    </span>
                                    <span className="row-sub">
                                        updated {formatDate(branchUpdatedAt(branch))}
                                    </span>
                                </span>
                                <span className="row-side">
                                    <BranchStatusPill branch={branch} />
                                    <ChevronRight
                                        aria-hidden
                                        className="muted"
                                        size={15}
                                    />
                                </span>
                            </Link>
                        ))}
                    </SearchableList>
                )}
            </section>
        );
    }

    // The catalogs section lists catalog types only; each type carries its
    // entries inline.
    const sectionNodes = nodes.filter(
        (node) =>
            node.section === section &&
            (section !== "catalogs" || node.kind === "catalog"),
    );

    return (
        <section className="section">
            <div className="section-header-text">
                <h1>{SECTION_TITLES[section]}</h1>
                <p className="hint">
                    {SECTION_HINTS[section] ??
                        "Select an entity to read its full source definition."}
                </p>
            </div>
            {sectionNodes.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        {sectionIcon(section, 18)}
                    </span>
                    <p>
                        This workspace declares no{" "}
                        {SECTION_TITLES[section].toLowerCase()}.
                    </p>
                </div>
            ) : (
                <SearchableList
                    className="row-list"
                    emptyLabel="Nothing matches that search."
                    label={`Search ${SECTION_TITLES[section].toLowerCase()}`}
                    placeholder={`Search ${SECTION_TITLES[section].toLowerCase()}`}
                >
                    {sectionNodes.map((node) => {
                        const entries =
                            node.kind === "catalog"
                                ? nodes.filter(
                                      (candidate) =>
                                          candidate.kind === "catalog entry" &&
                                          candidate.badge === node.id,
                                  )
                                : [];
                        if (entries.length === 0) {
                            return (
                                <Link
                                    className="row"
                                    data-search={entitySearchText(node)}
                                    href={entityHref(workspace.slug, node.path)}
                                    key={node.path}
                                >
                                    <span className="row-icon">
                                        {sectionIcon(node.section, 16)}
                                    </span>
                                    <span className="row-text">
                                        <span className="row-title mono">
                                            {node.id}
                                        </span>
                                        <span className="row-sub">
                                            {node.description ?? node.path}
                                        </span>
                                    </span>
                                    <span className="row-side">
                                        {node.badge ? (
                                            <span className="tag">
                                                {node.badge}
                                            </span>
                                        ) : null}
                                        <ChevronRight
                                            aria-hidden
                                            className="muted"
                                            size={15}
                                        />
                                    </span>
                                </Link>
                            );
                        }
                        return (
                            <div
                                className="row"
                                data-search={`${entitySearchText(node)} ${entries
                                    .map((entry) => entry.id)
                                    .join(" ")}`}
                                key={node.path}
                            >
                                <span className="row-icon">
                                    {sectionIcon(node.section, 16)}
                                </span>
                                <span className="row-text">
                                    <Link
                                        className="row-title mono row-link"
                                        href={entityHref(
                                            workspace.slug,
                                            node.path,
                                        )}
                                    >
                                        {node.id}
                                    </Link>
                                    <span className="row-sub">
                                        {node.description ?? node.path}
                                    </span>
                                    <span className="row-entries">
                                        {entries.map((entry) => (
                                            <Link
                                                className="pill pill-neutral"
                                                href={entityHref(
                                                    workspace.slug,
                                                    entry.path,
                                                )}
                                                key={entry.path}
                                            >
                                                {entry.id.split("/").pop()}
                                            </Link>
                                        ))}
                                    </span>
                                </span>
                                <span className="row-side">
                                    <span className="tag">
                                        {entries.length}{" "}
                                        {entries.length === 1
                                            ? "entry"
                                            : "entries"}
                                    </span>
                                    <Link
                                        aria-label={`Open catalog ${node.id}`}
                                        className="muted"
                                        href={entityHref(
                                            workspace.slug,
                                            node.path,
                                        )}
                                    >
                                        <ChevronRight aria-hidden size={15} />
                                    </Link>
                                </span>
                            </div>
                        );
                    })}
                </SearchableList>
            )}
        </section>
    );
}

function EntityDefinition({
    allNodes,
    contextResolutions,
    definition,
    diagnostics,
    error,
    model,
    node,
    activeBranch,
    parentCatalog,
    qualifierEvaluations,
    workspaceId,
}: {
    allNodes: EntityNode[];
    contextResolutions: SavedContextResolution[];
    definition: WorkspaceDefinition | null;
    diagnostics: LintDiagnostic[];
    error: string | null;
    model: WorkspaceSemanticModel | null;
    node: EntityNode;
    activeBranch: BranchRecord | null;
    parentCatalog: EntityNode | null;
    qualifierEvaluations: QualifierContextEvaluation[];
    workspaceId: string;
}) {
    const relations = entityRelations({ allNodes, model, node, workspaceId });
    const editHref = activeBranch
        ? `/app/workspaces/${workspaceId}/branches/${activeBranch.id}/tree/${encodeEntityPath(node.path)}`
        : null;

    return (
        <section className="section">
            <div className="section-header">
                <div className="section-header-text">
                    <span className="label">{node.kind}</span>
                    <h1 className="mono">
                        {parentCatalog &&
                        node.id.startsWith(`${parentCatalog.id}/`)
                            ? node.id.slice(parentCatalog.id.length + 1)
                            : node.id}
                    </h1>
                    {node.description ? (
                        <p className="hint">{node.description}</p>
                    ) : null}
                </div>
                <div className="action-row">
                    {node.badge && !parentCatalog ? (
                        <span className="tag">{node.badge}</span>
                    ) : null}
                    {parentCatalog ? (
                        <Link
                            className="btn btn-secondary btn-sm"
                            href={entityHref(workspaceId, parentCatalog.path)}
                        >
                            <ArrowLeft aria-hidden size={14} />
                            Catalog {parentCatalog.id}
                        </Link>
                    ) : (
                        <Link
                            className="btn btn-secondary btn-sm"
                            href={sectionHref(workspaceId, node.section)}
                        >
                            <ArrowLeft aria-hidden size={14} />
                            All {SECTION_TITLES[node.section].toLowerCase()}
                        </Link>
                    )}
                    {editHref ? (
                        <Link
                            className="btn btn-primary btn-sm"
                            href={editHref}
                        >
                            <Pencil aria-hidden size={13} />
                            Edit in branch
                        </Link>
                    ) : null}
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
            {definition ? (
                <EntitySummary
                    allNodes={allNodes}
                    contextResolutions={contextResolutions}
                    model={model}
                    node={node}
                    qualifierEvaluations={qualifierEvaluations}
                    text={definition.text}
                    workspaceId={workspaceId}
                />
            ) : null}
            <div className="reference-grid">
                <RelationList
                    emptyLabel="References nothing else."
                    label="references"
                    relations={relations.outbound}
                />
                <RelationList
                    emptyLabel="Nothing references this entity."
                    label="referenced by"
                    relations={relations.inbound}
                />
            </div>
            {error ? (
                <div className="banner banner-err">
                    <TriangleAlert aria-hidden size={16} />
                    <span>The definition failed to load: {error}</span>
                </div>
            ) : (
                <SourceWell
                    diagnostics={diagnostics}
                    language={definition?.language ?? "text"}
                    node={node}
                    text={definition?.text ?? ""}
                />
            )}
        </section>
    );
}

/* CLI-inspect-style declaration summary: what the file declares, in order. */
function EntitySummary({
    allNodes,
    contextResolutions,
    model,
    node,
    qualifierEvaluations,
    text,
    workspaceId,
}: {
    allNodes: EntityNode[];
    contextResolutions: SavedContextResolution[];
    model: WorkspaceSemanticModel | null;
    node: EntityNode;
    qualifierEvaluations: QualifierContextEvaluation[];
    text: string;
    workspaceId: string;
}) {
    const qualifierHref = (id: string): string | null => {
        const match = allNodes.find(
            (candidate) =>
                candidate.section === "qualifiers" && candidate.id === id,
        );
        return match ? entityHref(workspaceId, match.path) : null;
    };
    const qualifierLabel = (id: string | null): ReactNode => {
        if (!id) {
            return "?";
        }
        const href = qualifierHref(id);
        return href ? <Link href={href}>{id}</Link> : id;
    };

    if (node.kind === "variable") {
        const variable = model?.variables.find(
            (candidate) => candidate.id === node.id,
        );
        if (!variable) {
            return null;
        }
        const rules = variable.resolve?.rules ?? [];
        const defaultKey = variable.resolve?.default?.value ?? null;
        if (rules.length === 0 && !defaultKey && variable.values.length === 0) {
            return null;
        }
        // Catalog-typed variables select entries by key; link each value key to
        // the entry it names.
        const catalogId =
            variable.declaration.kind === "catalog"
                ? (variable.declaration.value ?? null)
                : null;
        const valueKeyLabel = (key: string | null): ReactNode => {
            if (!key) {
                return "?";
            }
            const entryNode = catalogId
                ? allNodes.find(
                      (candidate) =>
                          candidate.targetKey ===
                          `catalog_entries:${catalogId}:${key}`,
                  )
                : undefined;
            return entryNode ? (
                <Link href={entityHref(workspaceId, entryNode.path)}>
                    {key}
                </Link>
            ) : (
                key
            );
        };
        return (
            <div className="card">
                <div className="card-head-text">
                    <h3>Values and resolution</h3>
                    <p className="hint">
                        Rules are checked in order; the first matching qualifier
                        wins, otherwise the default value applies.
                    </p>
                </div>
                {variable.values.length > 0 ? (
                    <>
                        <span className="label">declared values</span>
                        <div className="spec">
                            {variable.values.map((value) => (
                                <div className="spec-row" key={value.key}>
                                    <span>
                                        {value.key} <span className="g">=</span>{" "}
                                        {JSON.stringify(value.value)}
                                        {value.key === defaultKey ? (
                                            <span className="g">
                                                {" "}
                                                · default
                                            </span>
                                        ) : null}
                                    </span>
                                </div>
                            ))}
                        </div>
                    </>
                ) : null}
                <span className="label">how it resolves</span>
                <div className="spec">
                    {rules.map((rule) => (
                        <div className="spec-row" key={rule.index}>
                            <span className="g">rule[{rule.index}]</span>
                            <span>
                                if{" "}
                                {qualifierLabel(rule.qualifier?.value ?? null)}{" "}
                                <span className="g">→</span>{" "}
                                {valueKeyLabel(rule.value?.value ?? null)}
                            </span>
                        </div>
                    ))}
                    <div className="spec-row">
                        <span className="g">default</span>
                        <span>
                            <span className="g">→</span>{" "}
                            {defaultKey
                                ? valueKeyLabel(defaultKey)
                                : "not declared"}
                        </span>
                    </div>
                </div>
                {contextResolutions.length > 0 ? (
                    <>
                        <span className="label">with saved contexts</span>
                        <div className="spec">
                            {contextResolutions.map((resolution) => (
                                <div className="spec-row" key={resolution.path}>
                                    <span className="g">
                                        <Link
                                            href={entityHref(
                                                workspaceId,
                                                resolution.path,
                                            )}
                                        >
                                            {resolution.name}
                                        </Link>
                                    </span>
                                    {resolution.ok ? (
                                        <span>
                                            {(resolution.steps ?? []).map(
                                                (step, at, steps) => (
                                                    <span key={step.index}>
                                                        <span className="g">
                                                            rule[{step.index}]
                                                        </span>{" "}
                                                        {qualifierLabel(
                                                            step.qualifier,
                                                        )}{" "}
                                                        {step.matched ? (
                                                            "✓"
                                                        ) : (
                                                            <span className="g">
                                                                ✗
                                                            </span>
                                                        )}
                                                        {at <
                                                            steps.length - 1 ||
                                                        resolution.usedDefault ? (
                                                            <span className="g">
                                                                {" "}
                                                                ·{" "}
                                                            </span>
                                                        ) : (
                                                            " "
                                                        )}
                                                    </span>
                                                ),
                                            )}
                                            {resolution.usedDefault ? (
                                                <span className="g">
                                                    default{" "}
                                                </span>
                                            ) : null}
                                            <span className="g">→</span>{" "}
                                            {valueKeyLabel(
                                                resolution.valueKey ?? null,
                                            )}
                                        </span>
                                    ) : (
                                        <span className="g">
                                            did not resolve: {resolution.error}
                                        </span>
                                    )}
                                </div>
                            ))}
                        </div>
                        {contextResolutions
                            .filter(
                                (resolution) =>
                                    resolution.ok &&
                                    (resolution.steps?.length ?? 0) > 0,
                            )
                            .map((resolution) => (
                                <div key={`detail:${resolution.path}`}>
                                    <span className="label">
                                        qualifier resolution — {resolution.name}
                                    </span>
                                    <div className="spec">
                                        {(resolution.steps ?? []).map(
                                            (step) => (
                                                <QualifierEvaluationRows
                                                    depth={0}
                                                    evaluation={step.evaluation}
                                                    key={`${resolution.path}:${step.index}`}
                                                    qualifierLabel={
                                                        qualifierLabel
                                                    }
                                                />
                                            ),
                                        )}
                                    </div>
                                </div>
                            ))}
                    </>
                ) : null}
            </div>
        );
    }

    if (node.kind === "qualifier") {
        const qualifier = model?.qualifiers.find(
            (candidate) => candidate.id === node.id,
        );
        const predicates = qualifier?.predicates ?? [];
        if (predicates.length === 0) {
            return null;
        }
        return (
            <div className="card">
                <div className="card-head-text">
                    <h3>Predicates</h3>
                    <p className="hint">
                        All predicates must match for the qualifier to apply.
                    </p>
                </div>
                {qualifierEvaluations.length > 0 ? (
                    <>
                        <span className="label">with saved contexts</span>
                        <div className="spec">
                            {qualifierEvaluations.map((entry) => (
                                <div key={entry.path}>
                                    <div className="spec-row">
                                        <span className="g">
                                            <Link
                                                href={entityHref(
                                                    workspaceId,
                                                    entry.path,
                                                )}
                                            >
                                                {entry.name}
                                            </Link>
                                        </span>
                                        <span>
                                            {entry.evaluation === null ||
                                            entry.evaluation.matched ===
                                                null ? (
                                                <span className="g">
                                                    not evaluable
                                                    {entry.error
                                                        ? `: ${entry.error}`
                                                        : ""}
                                                </span>
                                            ) : entry.evaluation.matched ? (
                                                "✓ matches"
                                            ) : (
                                                <span className="g">
                                                    ✗ does not match
                                                </span>
                                            )}
                                        </span>
                                    </div>
                                    {entry.evaluation ? (
                                        <QualifierEvaluationRows
                                            depth={0}
                                            evaluation={entry.evaluation}
                                            qualifierLabel={qualifierLabel}
                                            showHeader={false}
                                        />
                                    ) : null}
                                </div>
                            ))}
                        </div>
                        <span className="label">declared predicates</span>
                    </>
                ) : null}
                <div className="spec">
                    {predicates.map((predicate) => (
                        <div className="spec-row" key={predicate.index}>
                            <span className="g">[{predicate.index}]</span>
                            <span>
                                {predicate.attribute?.value ?? "?"}{" "}
                                <strong>{predicate.op?.value ?? "?"}</strong>{" "}
                                {predicate.value !== undefined
                                    ? JSON.stringify(predicate.value)
                                    : ""}
                            </span>
                        </div>
                    ))}
                </div>
            </div>
        );
    }

    if (node.kind === "catalog") {
        const entries =
            model?.catalogEntries.filter(
                (entry) => entry.catalog === node.id,
            ) ?? [];
        if (entries.length === 0) {
            return null;
        }
        return (
            <div className="card">
                <div className="card-head-text">
                    <h3>Entries</h3>
                    <p className="hint">
                        Variable values reference these entries by key.
                    </p>
                </div>
                <div className="row-list">
                    {entries.map((entry) => {
                        const entryNode = allNodes.find(
                            (candidate) =>
                                candidate.targetKey ===
                                `catalog_entries:${node.id}:${entry.key}`,
                        );
                        const fields =
                            typeof entry.value === "object" &&
                            entry.value !== null &&
                            !Array.isArray(entry.value)
                                ? Object.entries(
                                      entry.value as Record<string, unknown>,
                                  )
                                : [];
                        return (
                            <Link
                                className="row"
                                href={
                                    entryNode
                                        ? entityHref(
                                              workspaceId,
                                              entryNode.path,
                                          )
                                        : "#"
                                }
                                key={entry.key}
                            >
                                <span className="row-icon">
                                    <Database aria-hidden size={16} />
                                </span>
                                <span className="row-text">
                                    <span className="row-title mono">
                                        {entry.key}
                                    </span>
                                    <span className="row-sub mono">
                                        {fields
                                            .slice(0, 4)
                                            .map(
                                                ([key, value]) =>
                                                    `${key} = ${JSON.stringify(value)}`,
                                            )
                                            .join("  ·  ")}
                                        {fields.length > 4
                                            ? `  ·  +${fields.length - 4} more`
                                            : ""}
                                    </span>
                                </span>
                                <span className="row-side">
                                    <span className="tag">
                                        {fields.length}{" "}
                                        {fields.length === 1
                                            ? "field"
                                            : "fields"}
                                    </span>
                                    <ChevronRight
                                        aria-hidden
                                        className="muted"
                                        size={15}
                                    />
                                </span>
                            </Link>
                        );
                    })}
                </div>
            </div>
        );
    }

    if (node.kind === "schema" || node.kind === "context schema") {
        const summary = schemaSummary(text);
        if (!summary || summary.properties.length === 0) {
            return null;
        }
        return (
            <div className="card">
                <div className="card-head-text">
                    <h3>Contract</h3>
                    <p className="hint">
                        {node.kind === "context schema"
                            ? "Context the application must supply at resolution time."
                            : "Values must validate against these properties."}
                        {summary.additionalProperties === false
                            ? " Additional properties are rejected."
                            : ""}
                    </p>
                </div>
                <div className="spec">
                    {summary.properties.map((property) => (
                        <div className="spec-row" key={property.key}>
                            <span className="g">
                                {property.required ? "●" : "○"}
                            </span>
                            <span>
                                {property.key}{" "}
                                <span className="g">{property.type ?? ""}</span>
                                {property.required ? "" : " (optional)"}
                                {property.description ? (
                                    <span className="g">
                                        {" "}
                                        — {property.description}
                                    </span>
                                ) : null}
                            </span>
                        </div>
                    ))}
                </div>
            </div>
        );
    }

    return null;
}

/* Read-only source with syntax highlighting and diagnostic lines pinpointed. */
function SourceWell({
    diagnostics,
    language,
    node,
    text,
}: {
    diagnostics: LintDiagnostic[];
    language: WorkspaceDefinition["language"];
    node: EntityNode;
    text: string;
}) {
    const markByLine = new Map<number, "error" | "warning">();
    for (const diagnostic of diagnostics) {
        const path = diagnostic.location?.path;
        const line = diagnostic.location?.range?.start?.line;
        if (
            path !== undefined &&
            line !== undefined &&
            (node.path === path ||
                node.path.endsWith(`/${path}`) ||
                path.endsWith(`/${node.path}`))
        ) {
            // lint positions are 0-based; the editor highlights 1-based lines
            const displayLine = line + 1;
            const severity =
                diagnostic.severity === "error" ? "error" : "warning";
            if (markByLine.get(displayLine) !== "error") {
                markByLine.set(displayLine, severity);
            }
        }
    }
    const marks = Array.from(markByLine.entries()).map(([line, severity]) => ({
        line,
        severity,
    }));

    return (
        <div className="codewell-frame">
            <div className="codehead">
                <span>{node.path}</span>
                <span>read-only</span>
            </div>
            <ReadOnlySource language={language} marks={marks} text={text} />
        </div>
    );
}

/* One qualifier's evaluation against a context: the qualifier with its
   verdict, then each predicate with the context value it read; nested
   qualifier references recurse with indentation. */
function QualifierEvaluationRows({
    depth,
    evaluation,
    qualifierLabel,
    showHeader = true,
}: {
    depth: number;
    evaluation: QualifierEvaluation;
    qualifierLabel: (id: string | null) => ReactNode;
    showHeader?: boolean;
}) {
    return (
        <>
            {showHeader ? (
                <div className="spec-row" style={{ paddingLeft: depth * 18 }}>
                    <span>
                        {qualifierLabel(evaluation.id)}{" "}
                        {evaluation.matched === null ? (
                            <span className="g">not evaluable</span>
                        ) : evaluation.matched ? (
                            "✓"
                        ) : (
                            <span className="g">✗</span>
                        )}
                    </span>
                </div>
            ) : null}
            {evaluation.predicates.map((predicate) =>
                predicate.nested ? (
                    <QualifierEvaluationRows
                        depth={depth + 1}
                        evaluation={predicate.nested}
                        key={`${evaluation.id}:${predicate.index}`}
                        qualifierLabel={qualifierLabel}
                    />
                ) : (
                    <div
                        className="spec-row"
                        key={`${evaluation.id}:${predicate.index}`}
                        style={{ paddingLeft: (depth + 1) * 18 }}
                    >
                        <span className="g">[{predicate.index}]</span>
                        <span>
                            {predicate.attribute ?? "?"}{" "}
                            <strong>{predicate.op ?? "?"}</strong>{" "}
                            {predicate.valueLiteral ?? ""}
                            <span className="g">
                                {" "}
                                · context: {predicate.contextValue ?? "missing"}
                            </span>
                        </span>
                    </div>
                ),
            )}
        </>
    );
}

/* Builds the graph data contract from the semantic model. Rendering concepts
   live in components/workspace-graph. Callers supply entity paths per target
   key, the href builder, and optionally the set of paths edited in a branch. */
export function workspaceGraphData(input: {
    model: WorkspaceSemanticModel;
    pathForKey: Map<string, string>;
    hrefFor: (path: string) => string;
    editedPaths?: Set<string>;
}): WorkspaceGraphData {
    const { model, pathForKey, hrefFor, editedPaths } = input;
    const graphNodes: WorkspaceGraphData["nodes"] = [];
    const seenNodes = new Set<string>();
    const pushNode = (
        key: string,
        kind: WorkspaceGraphData["nodes"][number]["kind"],
        label: string,
    ) => {
        const path = pathForKey.get(key);
        if (!path || seenNodes.has(key)) {
            return;
        }
        seenNodes.add(key);
        graphNodes.push({
            id: key,
            kind,
            label,
            href: hrefFor(path),
            edited: editedPaths?.has(path) || undefined,
        });
    };
    for (const qualifier of model.qualifiers) {
        pushNode(`qualifiers:${qualifier.id}`, "qualifier", qualifier.id);
    }
    for (const variable of model.variables) {
        pushNode(`variables:${variable.id}`, "variable", variable.id);
    }
    for (const catalog of model.catalogs) {
        pushNode(`catalogs:${catalog.id}`, "catalog", catalog.id);
    }
    for (const entry of model.catalogEntries) {
        pushNode(
            `catalog_entries:${entry.catalog}:${entry.key}`,
            "catalogEntry",
            entry.key,
        );
    }
    const edges: WorkspaceGraphData["edges"] = [];
    const seenEdges = new Set<string>();
    const pushEdge = (
        from: string,
        to: string,
        kind: WorkspaceGraphData["edges"][number]["kind"],
    ) => {
        const key = `${from}->${to}:${kind}`;
        if (seenEdges.has(key) || !seenNodes.has(from) || !seenNodes.has(to)) {
            return;
        }
        seenEdges.add(key);
        edges.push({ from, to, kind });
    };
    for (const reference of model.references) {
        const { from, to, via } = reference;
        if (
            via.kind === "predicateQualifier" &&
            from.kind === "qualifier" &&
            to.kind === "qualifier"
        ) {
            pushEdge(
                `qualifiers:${to.id}`,
                `qualifiers:${from.id}`,
                "requires",
            );
        }
        if (
            via.kind === "ruleQualifier" &&
            from.kind === "variable" &&
            to.kind === "qualifier"
        ) {
            pushEdge(`qualifiers:${to.id}`, `variables:${from.id}`, "checks");
        }
        // Variables connect to catalogs; catalogs fan out to their entries.
        if (
            via.kind === "variableCatalog" &&
            from.kind === "variable" &&
            to.kind === "catalog"
        ) {
            pushEdge(`variables:${from.id}`, `catalogs:${to.id}`, "selects");
        }
        // Selected entries are not drawn as edges (the path goes through the
        // catalog) but hover highlighting should reach them.
        if (
            (via.kind === "ruleValue" || via.kind === "resolveDefault") &&
            from.kind === "variable" &&
            to.kind === "catalogEntry"
        ) {
            const variableNode = graphNodes.find(
                (node) => node.id === `variables:${from.id}`,
            );
            const entryKey = `catalog_entries:${to.catalog}:${to.key}`;
            if (variableNode && seenNodes.has(entryKey)) {
                (variableNode.related ??= []).push(entryKey);
            }
        }
    }
    for (const entry of model.catalogEntries) {
        pushEdge(
            `catalogs:${entry.catalog}`,
            `catalog_entries:${entry.catalog}:${entry.key}`,
            "contains",
        );
    }
    return { nodes: graphNodes, edges };
}

/** Render-ready relationship row for the current entity detail panel. */
type EntityRelation = { key: string; content: ReactNode };

function RelationList({
    emptyLabel,
    label,
    relations,
}: {
    emptyLabel: string;
    label: string;
    relations: EntityRelation[];
}) {
    return (
        <div className="card">
            <span className="label">{label}</span>
            {relations.length === 0 ? (
                <p className="muted" style={{ fontSize: 13 }}>
                    {emptyLabel}
                </p>
            ) : (
                <div className="spec">
                    {relations.map((relation) => (
                        <div className="spec-row" key={relation.key}>
                            <span>{relation.content}</span>
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
}

/* Relation sentences for the reference panels, derived from the semantic
   model's edges so each reference names its site: "rule[1] checks ...",
   "default selects ...". */
function entityRelations(input: {
    allNodes: EntityNode[];
    model: WorkspaceSemanticModel | null;
    node: EntityNode;
    workspaceId: string;
}): { outbound: EntityRelation[]; inbound: EntityRelation[] } {
    const { allNodes, model, node, workspaceId } = input;
    if (!model) {
        return { outbound: [], inbound: [] };
    }
    const targetKeyForRef = (ref: ReferenceModel["from"]): string | null => {
        switch (ref.kind) {
            case "qualifier":
                return `qualifiers:${ref.id}`;
            case "variable":
                return `variables:${ref.id}`;
            case "catalog":
                return `catalogs:${ref.id}`;
            case "catalogEntry":
                return `catalog_entries:${ref.catalog}:${ref.key}`;
            case "schema":
                return `schemas:${ref.path.split("/").pop() ?? ref.path}`;
            case "value":
                return `variables:${ref.variable}`;
            default:
                return null;
        }
    };
    const entityLink = (ref: ReferenceModel["from"]): ReactNode => {
        const key = targetKeyForRef(ref);
        const match = key
            ? allNodes.find((candidate) => candidate.targetKey === key)
            : undefined;
        const text =
            ref.kind === "catalogEntry"
                ? ref.key
                : ref.kind === "schema"
                  ? (ref.path.split("/").pop() ?? ref.path)
                  : ref.kind === "value"
                    ? ref.variable
                    : ref.kind === "contextAttribute"
                      ? ref.name
                      : ref.id;
        return match ? (
            <Link href={entityHref(workspaceId, match.path)}>{text}</Link>
        ) : (
            text
        );
    };
    const g = (text: string) => <span className="g">{text}</span>;
    const matchesNode = (ref: ReferenceModel["from"]): boolean =>
        targetKeyForRef(ref) === node.targetKey;

    const outbound: EntityRelation[] = [];
    const inbound: EntityRelation[] = [];
    for (const [index, reference] of model.references.entries()) {
        const { from, to, via } = reference;
        if (to.kind === "contextAttribute") {
            continue;
        }
        // Internal references (a variable's rules naming its own values) are
        // declaration detail, not cross-entity references.
        if (targetKeyForRef(from) === targetKeyForRef(to)) {
            continue;
        }
        if (matchesNode(from)) {
            const link = entityLink(to);
            switch (via.kind) {
                case "ruleQualifier":
                    outbound.push({
                        key: `out:${index}`,
                        content: (
                            <>
                                {g(`rule[${via.index}]`)} checks {link}
                            </>
                        ),
                    });
                    break;
                case "ruleValue":
                    if (to.kind === "catalogEntry") {
                        outbound.push({
                            key: `out:${index}`,
                            content: (
                                <>
                                    {g(`rule[${via.index}]`)} selects entry{" "}
                                    {link}
                                </>
                            ),
                        });
                    }
                    break;
                case "resolveDefault":
                    if (to.kind === "catalogEntry") {
                        outbound.push({
                            key: `out:${index}`,
                            content: (
                                <>
                                    {g("default")} selects entry {link}
                                </>
                            ),
                        });
                    }
                    break;
                case "variableCatalog":
                    outbound.push({
                        key: `out:${index}`,
                        content: <>values come from catalog {link}</>,
                    });
                    break;
                case "catalogSchema":
                    outbound.push({
                        key: `out:${index}`,
                        content: <>entries validate against {link}</>,
                    });
                    break;
                case "predicateQualifier":
                    outbound.push({
                        key: `out:${index}`,
                        content: (
                            <>
                                {g(`predicate[${via.index}]`)} requires {link}
                            </>
                        ),
                    });
                    break;
                default:
                    break;
            }
        }
        if (matchesNode(to)) {
            const link = entityLink(from);
            switch (via.kind) {
                case "ruleQualifier":
                    inbound.push({
                        key: `in:${index}`,
                        content: (
                            <>
                                {link} checks this {g(`in rule[${via.index}]`)}
                            </>
                        ),
                    });
                    break;
                case "ruleValue":
                    inbound.push({
                        key: `in:${index}`,
                        content: (
                            <>
                                {link} selects this {g(`in rule[${via.index}]`)}
                            </>
                        ),
                    });
                    break;
                case "resolveDefault":
                    inbound.push({
                        key: `in:${index}`,
                        content: (
                            <>
                                {link} selects this {g("as default")}
                            </>
                        ),
                    });
                    break;
                case "variableCatalog":
                    inbound.push({
                        key: `in:${index}`,
                        content: <>{link} takes its values from this catalog</>,
                    });
                    break;
                case "catalogSchema":
                    inbound.push({
                        key: `in:${index}`,
                        content: <>{link} validates its entries against this</>,
                    });
                    break;
                case "predicateQualifier":
                    inbound.push({
                        key: `in:${index}`,
                        content: (
                            <>
                                {link} requires this{" "}
                                {g(`in predicate[${via.index}]`)}
                            </>
                        ),
                    });
                    break;
                default:
                    break;
            }
        }
    }

    // Declaration-derived relations the edge list does not carry.
    if (node.kind === "variable") {
        const variable = model.variables.find(
            (candidate) => candidate.id === node.id,
        );
        if (
            variable?.declaration.kind === "schema" &&
            variable.declaration.value
        ) {
            const schemaRef = {
                kind: "schema",
                path: variable.declaration.value,
            } as const;
            outbound.push({
                key: "out:declaration-schema",
                content: <>values validate against {entityLink(schemaRef)}</>,
            });
        }
    }
    if (node.kind === "schema") {
        for (const variable of model.variables) {
            if (
                variable.declaration.kind === "schema" &&
                (variable.declaration.value?.split("/").pop() ?? "") === node.id
            ) {
                inbound.push({
                    key: `in:declaration:${variable.id}`,
                    content: (
                        <>
                            {entityLink({ kind: "variable", id: variable.id })}{" "}
                            validates its values against this
                        </>
                    ),
                });
            }
        }
    }
    if (node.kind === "catalog entry") {
        const catalogId = node.badge ?? "";
        outbound.push({
            key: "out:membership",
            content: (
                <>
                    entry of catalog{" "}
                    {entityLink({ kind: "catalog", id: catalogId })}
                </>
            ),
        });
    }

    return { outbound, inbound };
}

function OverviewAttention({
    diagnostics,
    lintError,
    nodes,
    activeBranches,
    workspaceId,
}: {
    diagnostics: LintDiagnostic[];
    lintError: string | null;
    nodes: EntityNode[];
    activeBranches: BranchRecord[];
    workspaceId: string;
}) {
    const ranked = [
        ...diagnostics.filter((diagnostic) => diagnostic.severity === "error"),
        ...diagnostics.filter((diagnostic) => diagnostic.severity !== "error"),
    ];
    const shown = ranked.slice(0, 5);
    const allClear =
        lintError === null && ranked.length === 0 && activeBranches.length === 0;

    return (
        <div className="card">
            <div className="card-head">
                <div className="card-head-text">
                    <h3>Needs attention</h3>
                </div>
                {ranked.length > 0 ? (
                    <DiagnosticSummary diagnostics={diagnostics} />
                ) : null}
            </div>
            {lintError !== null ? (
                <div className="banner banner-err">
                    <TriangleAlert aria-hidden size={16} />
                    <span>Lint failed to run: {lintError}</span>
                </div>
            ) : null}
            {allClear ? (
                <div className="publish-check" data-ok="true">
                    <CheckCircle2 aria-hidden size={16} />
                    <span>
                        Nothing needs you right now — lint is clean and there
                        are no active branches.
                    </span>
                </div>
            ) : (
                <>
                    {shown.map((diagnostic, index) => {
                        const match = nodes.find((node) =>
                            diagnosticMatchesNode(diagnostic, node, nodes),
                        );
                        const entity = attentionEntity(match, diagnostic);
                        const content = (
                            <>
                                <span className="attn-entity">
                                    <span className="attn-entity-kind">
                                        {entity.kind}
                                    </span>
                                    <span className="attn-entity-id mono">
                                        {entity.id}
                                    </span>
                                </span>
                                <span className="attn-diagnostic">
                                    <span className="attn-severity">
                                        {diagnostic.severity ?? "warning"}
                                    </span>
                                    <span className="attn-message">
                                        {diagnostic.message ?? "Diagnostic"}
                                    </span>
                                </span>
                            </>
                        );
                        return match ? (
                            <Link
                                aria-label={`Open ${match.kind} ${match.id}: ${diagnostic.message ?? "Diagnostic"}`}
                                className={`attn-row attn-link ${diagnostic.severity ?? ""}`}
                                href={entityHref(workspaceId, match.path)}
                                key={index}
                            >
                                {content}
                                <ChevronRight
                                    aria-hidden
                                    className="attn-arrow"
                                    size={15}
                                />
                            </Link>
                        ) : (
                            <div
                                className={`attn-row ${diagnostic.severity ?? ""}`}
                                key={index}
                            >
                                {content}
                            </div>
                        );
                    })}
                    {ranked.length > shown.length ? (
                        <Link
                            className="entity-card-more"
                            href={sectionHref(workspaceId, "diagnostics")}
                        >
                            … all {ranked.length} diagnostics
                        </Link>
                    ) : null}
                    {activeBranches.map((branch) => (
                        <Link
                            className="row"
                            href={`/app/workspaces/${workspaceId}/branches/${branch.id}`}
                            key={branch.id}
                        >
                            <span className="row-icon">
                                <GitBranch aria-hidden size={16} />
                            </span>
                            <span className="row-text">
                                <span className="row-title mono">
                                    {branch.branch}
                                </span>
                                <span className="row-sub">
                                    active branch · updated{" "}
                                    {formatDate(branchUpdatedAt(branch))}
                                </span>
                            </span>
                            <span className="row-side">
                                <BranchStatusPill branch={branch} />
                                <ChevronRight
                                    aria-hidden
                                    className="muted"
                                    size={15}
                                />
                            </span>
                        </Link>
                    ))}
                </>
            )}
        </div>
    );
}

const OVERVIEW_CARD_SECTIONS: SectionId[] = [
    "variables",
    "qualifiers",
    "catalogs",
    "schemas",
    "linters",
    "context",
];

function sectionIcon(section: SectionId, size: number): ReactNode {
    switch (section) {
        case "variables":
            return <FileCode2 aria-hidden size={size} />;
        case "qualifiers":
            return <Tags aria-hidden size={size} />;
        case "catalogs":
            return <Database aria-hidden size={size} />;
        case "schemas":
            return <FileJson2 aria-hidden size={size} />;
        case "linters":
            return <Wrench aria-hidden size={size} />;
        case "context":
            return <Braces aria-hidden size={size} />;
        default:
            return <Boxes aria-hidden size={size} />;
    }
}

function entityNodes(inventory: WorkspaceInventory): EntityNode[] {
    const contextNodes: EntityNode[] = [];
    if (inventory.context.schemaPath) {
        contextNodes.push({
            section: "context",
            kind: "context schema",
            id: "context.schema.json",
            path: inventory.context.schemaPath,
            description: "Workspace context schema",
            badge: "schema",
            targetKey: "context:context.schema.json",
            outboundKeys: [],
        });
    }
    for (const path of inventory.context.examples) {
        contextNodes.push({
            section: "context",
            kind: "context example",
            id: path.split("/").pop() ?? path,
            path,
            description: "Example resolution context",
            badge: "example",
            targetKey: `context:${path}`,
            outboundKeys: [],
        });
    }

    return [
        ...inventory.variables.map((item) => ({
            section: "variables" as const,
            kind: "variable",
            id: item.id,
            path: item.path,
            description: item.description,
            badge: item.declaration,
            targetKey: `variables:${item.id}`,
            outboundKeys: [
                ...item.qualifierReferences.map((id) => `qualifiers:${id}`),
                item.catalogReference
                    ? `catalogs:${item.catalogReference}`
                    : null,
                // Catalog-typed variables select entries by value key, so each
                // selected entry is a reference too.
                ...(item.catalogReference
                    ? [
                          ...new Set([
                              ...item.ruleValueKeys,
                              ...(item.defaultValueKey
                                  ? [item.defaultValueKey]
                                  : []),
                          ]),
                      ].map(
                          (key) =>
                              `catalog_entries:${item.catalogReference}:${key}`,
                      )
                    : []),
                item.schemaReference ? `schemas:${item.schemaReference}` : null,
            ].filter((key): key is string => key !== null),
        })),
        ...inventory.qualifiers.map((item) => ({
            section: "qualifiers" as const,
            kind: "qualifier",
            id: item.id,
            path: item.path,
            description: item.description,
            badge: `${item.predicateCount} predicates`,
            targetKey: `qualifiers:${item.id}`,
            outboundKeys: item.qualifierReferences.map(
                (id) => `qualifiers:${id}`,
            ),
        })),
        ...inventory.catalogs.map((item) => ({
            section: "catalogs" as const,
            kind: "catalog",
            id: item.id,
            path: item.path,
            description: item.description,
            badge: `${item.entryCount} entries`,
            targetKey: `catalogs:${item.id}`,
            outboundKeys: item.schemaReference
                ? [`schemas:${item.schemaReference}`]
                : [],
        })),
        ...inventory.catalogEntries.map((item) => ({
            section: "catalogs" as const,
            kind: "catalog entry",
            id: item.id,
            path: item.path,
            description: `Entry ${item.key} for catalog ${item.catalogId}`,
            badge: item.catalogId,
            targetKey: `catalog_entries:${item.catalogId}:${item.key}`,
            outboundKeys: [`catalogs:${item.catalogId}`],
        })),
        ...inventory.schemas.map((item) => ({
            section: "schemas" as const,
            kind: "schema",
            id: item.id,
            path: item.path,
            description: item.title,
            badge: "json",
            targetKey: `schemas:${item.id}`,
            outboundKeys: [],
        })),
        ...inventory.linters
            .filter((item) => item.path !== null)
            .map((item) => ({
                section: "linters" as const,
                kind: "linter",
                id: item.id,
                path: item.path as string,
                description: item.title,
                badge: item.kind,
                targetKey: `linters:${item.id}`,
                outboundKeys: [],
            })),
        ...contextNodes,
    ];
}

function diagnosticMatchesNode(
    diagnostic: LintDiagnostic,
    node: EntityNode,
    nodes: EntityNode[],
): boolean {
    if (
        nodeForDiagnosticPath(nodes, diagnostic.location?.path)?.path ===
        node.path
    ) {
        return true;
    }
    const target = diagnostic.target?.entity;
    if (!isRecord(target) || typeof target.kind !== "string") {
        return false;
    }
    const key = semanticTargetKey(target);
    if (key) {
        if (key.endsWith(":*")) {
            return node.targetKey.startsWith(key.slice(0, -1));
        }
        return key === node.targetKey;
    }
    if (
        (target.kind === "schema" || target.kind === "custom_lint") &&
        typeof target.path === "string"
    ) {
        return nodeForDiagnosticPath(nodes, target.path)?.path === node.path;
    }
    return false;
}

function semanticTargetKey(entity: Record<string, unknown>): string | null {
    if (entity.kind === "variable" && typeof entity.id === "string") {
        return `variables:${entity.id}`;
    }
    if (
        (entity.kind === "value" || entity.kind === "rule") &&
        typeof entity.variable === "string"
    ) {
        return `variables:${entity.variable}`;
    }
    if (entity.kind === "qualifier" && typeof entity.id === "string") {
        return `qualifiers:${entity.id}`;
    }
    if (entity.kind === "predicate" && typeof entity.qualifier === "string") {
        return `qualifiers:${entity.qualifier}`;
    }
    if (entity.kind === "catalog" && typeof entity.id === "string") {
        return `catalogs:${entity.id}`;
    }
    if (entity.kind === "catalog_entry" && typeof entity.catalog === "string") {
        return typeof entity.key === "string"
            ? `catalog_entries:${entity.catalog}:${entity.key}`
            : `catalog_entries:${entity.catalog}:*`;
    }
    return null;
}

function nodeForDiagnosticPath(
    nodes: EntityNode[],
    diagnosticPath: string | undefined,
): EntityNode | null {
    if (!diagnosticPath) {
        return null;
    }
    return (
        nodes.find(
            (node) =>
                node.path === diagnosticPath ||
                node.path.endsWith(`/${diagnosticPath}`) ||
                diagnosticPath.endsWith(`/${node.path}`),
        ) ?? null
    );
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}

function sectionHref(workspaceId: string, section: SectionId): string {
    return section === "overview"
        ? `/app/workspaces/${workspaceId}`
        : `/app/workspaces/${workspaceId}/${section}`;
}

function entityHref(workspaceId: string, path: string): string {
    return `/app/workspaces/${workspaceId}/tree/${encodeEntityPath(path)}`;
}

export function encodeEntityPath(path: string): string {
    return path.split("/").map(encodeURIComponent).join("/");
}

function entitySearchText(node: EntityNode): string {
    return [node.id, node.kind, node.path, node.description, node.badge]
        .filter(Boolean)
        .join(" ");
}

function attentionEntity(
    node: EntityNode | undefined,
    diagnostic: LintDiagnostic,
): { kind: string; id: string } {
    if (node) {
        return { kind: node.kind, id: node.id };
    }
    return {
        kind: "workspace",
        id: diagnostic.location?.path ?? "lint diagnostic",
    };
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
