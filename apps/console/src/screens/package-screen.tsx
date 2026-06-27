import type { ReactNode } from "react";
import {
    ArrowLeft,
    Boxes,
    Braces,
    ChevronRight,
    Database,
    FileCode2,
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
import { CatalogValueList } from "@/components/catalog-value-list";
import { LoadingScreen } from "@/components/loading-screen";
import { SearchableList } from "@/components/searchable-list";
import { BranchCandidates } from "@/components/branch-candidates";
import { ReadOnlySource } from "@/components/read-only-source";
import { BranchStatusPill } from "@/components/status-pills";
import { StartBranchButton } from "@/components/start-branch-button";
import { OpenBranchForm } from "@/components/open-branch-form";
import { PackageGraph } from "@/components/package-graph";
import type { PackageGraphData } from "@/components/package-graph/types";
import { useApi } from "@/lib/api";
import { schemaSummary } from "@/lib/entity-summary";
import { Link } from "@/lib/link";
import { useShellUser } from "@/lib/me";
import { RefreshScope } from "@/lib/refresh";
import type { SectionId } from "@/lib/route-normalizers";
import type {
    BranchRecord,
    LintDiagnostic,
    VariableModel,
    PackageData,
    PackageDefinition,
    PackageEntityData,
    PackageCapabilities,
    PackageRecord,
    PackageInventory,
    PackageLintView,
    PackageSemanticModel,
} from "@/lib/types";
import { NotFound } from "@/screens/not-found";

/** Flattened package entity used for lists, links, and graph construction. */
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

/** Lint payload for the staged package, including staging/lint failures. */
type LintLoad =
    | PackageLintView
    | { root: string; diagnostics: LintDiagnostic[]; error: string };

const SECTION_TITLES: Record<SectionId, string> = {
    overview: "Overview",
    variables: "Variables",
    qualifiers: "Qualifiers",
    catalogs: "Catalogs",
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
    catalogs: "Typed values that catalog-backed variables can point at.",
    linters: "Custom lint rules this package declares beyond the built-ins.",
    context: "The context contract: schema plus example resolution contexts.",
};

export function PackageScreen({
    path = null,
    section = null,
    packageId,
}: {
    path?: string | null;
    section?: SectionId | null;
    packageId: string;
}) {
    const user = useShellUser();
    const data = useApi<PackageData>(
        `/api/packages/${encodeURIComponent(packageId)}/data`,
    );
    const entity = useApi<PackageEntityData>(
        path
            ? `/api/packages/${encodeURIComponent(packageId)}/entity?path=${encodeURIComponent(path)}`
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
                    <span className="label">package</span>
                    <h1>This package failed to load.</h1>
                    <div className="banner banner-err">
                        <TriangleAlert aria-hidden size={16} />
                        <span>{data.error ?? "Unknown error."}</span>
                    </div>
                </div>
            </main>
        );
    }

    const {
        package: pkg,
        branches,
        inventory,
        inventoryError,
        definitions,
        model,
        capabilities,
    } = data.data;
    const lint = data.data.lint as LintLoad;
    const writeDisabled = capabilities.write.kind === "disabled";
    const writeDisabledReason =
        capabilities.write.kind === "disabled"
            ? capabilities.write.reason
            : undefined;
    const localWorkingTree =
        capabilities.write.kind === "directPush" &&
        capabilities.write.backend === "localWorkingTree";

    // Canonical URLs use the friendly slug; id URLs redirect to it.
    if (packageId !== pkg.slug) {
        return (
            <Navigate
                replace
                to={
                    path
                        ? entityHref(pkg.slug, path)
                        : sectionHref(pkg.slug, section ?? "overview")
                }
            />
        );
    }

    const nodes = entityNodes(inventory);
    const entityCounts = {
        variables: inventory.variables.length,
        qualifiers: inventory.qualifiers.length,
        catalogs: inventory.catalogs.length,
        linters: inventory.linters.length,
        context:
            inventory.context.exampleCount +
            inventory.context.evaluationContexts.length,
    };
    const selectedPath = path;
    const selectedNode = selectedPath
        ? (nodes.find((node) => node.path === selectedPath) ?? null)
        : null;
    const selectedSection = selectedNode
        ? selectedNode.section
        : (section ?? "overview");
    if (selectedSection === "branches" && writeDisabled) {
        return <Navigate replace to={sectionHref(pkg.slug, "overview")} />;
    }
    const definition: PackageDefinition | null =
        entity.data?.definition ?? null;
    const definitionError: string | null =
        entity.data?.definitionError ?? entity.error;

    // Graph data is shared by the overview and focused entity pages. Entity
    // pages filter this to the same directly connected set lit by overview
    // hover.
    const graphData =
        model !== null
            ? packageGraphData({
                  model,
                  pathForKey: new Map(
                      nodes.map((node) => [node.targetKey, node.path]),
                  ),
                  sourceByPath: new Map(
                      definitions.map((definition) => [
                          definition.path,
                          {
                              language: definition.language,
                              text: definition.text,
                          },
                      ]),
                  ),
                  hrefFor: (entityPath) => entityHref(pkg.slug, entityPath),
              })
            : null;
    const diagnosticCount = "error" in lint ? 0 : lint.diagnostics.length;
    const parentCatalogNode =
        selectedNode?.kind === "catalog value"
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
    const packageName = pkg.sourceTreeLabel;
    const entityCrumbLabel = selectedNode
        ? parentCatalogNode &&
          selectedNode.id.startsWith(`${parentCatalogNode.id}/`)
            ? selectedNode.id.slice(parentCatalogNode.id.length + 1)
            : selectedNode.id
        : "";
    // Crumbs are ancestors only; the topbar title names the current screen.
    const crumbs = [
        { label: "console", href: "/app" },
        { label: "packages", href: "/app/packages" },
        ...(selectedNode || selectedSection !== "overview"
            ? [
                  {
                      label: pkg.displayPath,
                      href: `/app/packages/${pkg.slug}`,
                  },
              ]
            : []),
        ...(selectedNode
            ? [
                  {
                      label: SECTION_TITLES[selectedNode.section].toLowerCase(),
                      href: sectionHref(pkg.slug, selectedNode.section),
                  },
                  ...(parentCatalogNode
                      ? [
                            {
                                label: parentCatalogNode.id,
                                href: entityHref(
                                    pkg.slug,
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
                            href={sectionHref(pkg.slug, "diagnostics")}
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
                            packageId={pkg.slug}
                        />
                    </>
                }
                crumbs={crumbs}
                nav={
                    <>
                        <NavBack href="/app/packages" label="All packages" />
                        <NavContext
                            href={`/app/packages/${pkg.slug}`}
                            label="package"
                            value={`${packageName} · ${pkg.displayPath}`}
                        />
                        <NavGroupLabel>Inspect</NavGroupLabel>
                        <NavLink
                            active={
                                !selectedNode && selectedSection === "overview"
                            }
                            href={sectionHref(pkg.slug, "overview")}
                            icon={<Boxes aria-hidden size={16} />}
                            label="Overview"
                        />
                        <NavLink
                            active={selectedSection === "qualifiers"}
                            count={entityCounts.qualifiers}
                            href={sectionHref(pkg.slug, "qualifiers")}
                            icon={<Tags aria-hidden size={16} />}
                            label="Qualifiers"
                        />
                        <NavLink
                            active={selectedSection === "variables"}
                            count={entityCounts.variables}
                            href={sectionHref(pkg.slug, "variables")}
                            icon={<FileCode2 aria-hidden size={16} />}
                            label="Variables"
                        />
                        <NavLink
                            active={selectedSection === "catalogs"}
                            count={entityCounts.catalogs}
                            href={sectionHref(pkg.slug, "catalogs")}
                            icon={<Database aria-hidden size={16} />}
                            label="Catalogs"
                        />
                        <NavLink
                            active={selectedSection === "linters"}
                            count={entityCounts.linters}
                            href={sectionHref(pkg.slug, "linters")}
                            icon={<Wrench aria-hidden size={16} />}
                            label="Linters"
                        />
                        <NavLink
                            active={selectedSection === "context"}
                            count={entityCounts.context}
                            href={sectionHref(pkg.slug, "context")}
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
                            href={sectionHref(pkg.slug, "diagnostics")}
                            icon={<ListChecks aria-hidden size={16} />}
                            label="Diagnostics"
                        />
                        {!writeDisabled ? (
                            <NavLink
                                active={
                                    !selectedNode &&
                                    selectedSection === "branches"
                                }
                                count={branches.length}
                                href={sectionHref(pkg.slug, "branches")}
                                icon={<GitBranch aria-hidden size={16} />}
                                label={
                                    localWorkingTree
                                        ? "Working Tree"
                                        : "Branches"
                                }
                            />
                        ) : null}
                    </>
                }
                title={
                    selectedNode
                        ? entityCrumbLabel
                        : SECTION_TITLES[selectedSection]
                }
                user={user}
                wide={!selectedNode && selectedSection === "overview"}
            >
                {selectedNode ? (
                    <EntityDefinition
                        allNodes={nodes}
                        definition={definition}
                        diagnostics={entityDiagnostics}
                        error={definitionError}
                        model={model}
                        node={selectedNode}
                        activeBranch={
                            branches.find(
                                (branch) => branch.status === "active",
                            ) ?? null
                        }
                        parentCatalog={parentCatalogNode}
                        graphData={
                            selectedNode && graphData
                                ? focusedGraphData(
                                      graphData,
                                      selectedNode.targetKey,
                                  )
                                : null
                        }
                        packageId={pkg.slug}
                    />
                ) : (
                    <PackageSection
                        diagnosticCount={diagnosticCount}
                        branches={branches}
                        graphData={graphData}
                        capabilities={capabilities}
                        localWorkingTree={localWorkingTree}
                        inventory={inventory}
                        inventoryError={inventoryError}
                        lint={lint}
                        nodes={nodes}
                        section={selectedSection}
                        pkg={pkg}
                    />
                )}
            </AppShell>
        </RefreshScope>
    );
}

function PackageSection({
    capabilities,
    diagnosticCount,
    branches,
    graphData,
    localWorkingTree,
    inventory,
    inventoryError,
    lint,
    nodes,
    section,
    pkg,
}: {
    capabilities: PackageCapabilities;
    diagnosticCount: number;
    branches: BranchRecord[];
    graphData: PackageGraphData | null;
    localWorkingTree: boolean;
    inventory: PackageInventory;
    inventoryError: string | null;
    lint: PackageLintView | { root: string; diagnostics: []; error: string };
    nodes: EntityNode[];
    section: SectionId;
    pkg: PackageRecord;
}) {
    if (section === "overview") {
        const activeBranches = branches.filter(
            (branch) => branch.status === "active",
        );
        return (
            <section className="section package-overview">
                <div className="section-header">
                    <div className="section-header-text">
                        <h1 className="mono">{pkg.displayPath}</h1>
                        <p className="hint">
                            What this package declares, and whether it lints
                            clean right now.
                        </p>
                    </div>
                </div>
                <div className="meta-grid">
                    <div className="meta-item">
                        <span className="label">configuration source</span>
                        <span className="meta-value mono">
                            <Link
                                className="title-link"
                                href={`/app/packages?sourceTree=${pkg.sourceTreeId}`}
                            >
                                {pkg.sourceTreeLabel}
                            </Link>
                        </span>
                    </div>
                    <div className="meta-item">
                        <span className="label">package path</span>
                        <span className="meta-value mono">
                            {pkg.displayPath}
                        </span>
                    </div>
                    <div className="meta-item">
                        <span className="label">ref</span>
                        <span className="meta-value mono">{pkg.revision}</span>
                    </div>
                </div>
                {inventoryError ? (
                    <div className="banner banner-err">
                        <TriangleAlert aria-hidden size={16} />
                        <span>
                            Package inventory failed to load: {inventoryError}
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
                            packageId={pkg.slug}
                        />
                        {graphData ? (
                            <div className="card graph-card">
                                <div className="card-head-text">
                                    <h3>Entity graph</h3>
                                    <p className="hint">
                                        How qualifiers, variables, catalogs, and
                                        values connect. Hover a node to trace
                                        its references; click to open it.
                                    </p>
                                </div>
                                <PackageGraph data={graphData} />
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
                        return match ? entityHref(pkg.slug, match.path) : null;
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
                    <h1>{localWorkingTree ? "Working Tree" : "Branches"}</h1>
                    <p className="hint">
                        {localWorkingTree
                            ? "Edits write to the local checkout on "
                            : capabilities.write.kind === "pullRequest"
                              ? "Each branch is a branch created from "
                              : "Each branch edits "}
                        <span className="mono">{pkg.revision}</span>
                        {localWorkingTree
                            ? ". Validate changes in the console, then commit and push with git."
                            : capabilities.write.kind === "pullRequest"
                              ? ". Edits commit to the branch; publishing opens a pull request."
                              : ". Publishing applies the configured direct-push workflow."}
                    </p>
                </div>
                {capabilities.write.kind === "pullRequest" ? (
                    <>
                        <BranchCandidates packageId={pkg.slug} />
                        <OpenBranchForm packageId={pkg.slug} />
                    </>
                ) : null}
                {branches.length === 0 ? (
                    <div className="empty-state">
                        <span className="empty-puck">
                            <GitBranch aria-hidden size={18} />
                        </span>
                        <p>
                            {localWorkingTree
                                ? "No working tree session yet. Use “Edit package” to start one."
                                : "No branches yet. Use “Edit package” to start one, or open an existing branch above."}
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
                                href={`/app/packages/${pkg.slug}/branches/${branch.id}`}
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
                                        updated{" "}
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
                    </SearchableList>
                )}
            </section>
        );
    }

    // The catalogs section lists catalog types only; each type carries its
    // values inline.
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
                        This package declares no{" "}
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
                                          candidate.kind === "catalog value" &&
                                          candidate.badge === node.id,
                                  )
                                : [];
                        if (entries.length === 0) {
                            return (
                                <Link
                                    className="row"
                                    data-search={entitySearchText(node)}
                                    href={entityHref(pkg.slug, node.path)}
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
                                        href={entityHref(pkg.slug, node.path)}
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
                                                    pkg.slug,
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
                                            ? "value"
                                            : "values"}
                                    </span>
                                    <Link
                                        aria-label={`Open catalog ${node.id}`}
                                        className="muted"
                                        href={entityHref(pkg.slug, node.path)}
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
    definition,
    diagnostics,
    error,
    graphData,
    model,
    node,
    activeBranch,
    parentCatalog,
    packageId,
}: {
    allNodes: EntityNode[];
    definition: PackageDefinition | null;
    diagnostics: LintDiagnostic[];
    error: string | null;
    graphData: PackageGraphData | null;
    model: PackageSemanticModel | null;
    node: EntityNode;
    activeBranch: BranchRecord | null;
    parentCatalog: EntityNode | null;
    packageId: string;
}) {
    const editHref = activeBranch
        ? `/app/packages/${packageId}/branches/${activeBranch.id}/tree/${encodeEntityPath(node.path)}`
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
                    {parentCatalog ? (
                        <Link
                            className="btn btn-secondary btn-sm"
                            href={entityHref(packageId, parentCatalog.path)}
                        >
                            <ArrowLeft aria-hidden size={14} />
                            Catalog {parentCatalog.id}
                        </Link>
                    ) : (
                        <Link
                            className="btn btn-secondary btn-sm"
                            href={sectionHref(packageId, node.section)}
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
                    model={model}
                    node={node}
                    text={definition.text}
                    packageId={packageId}
                />
            ) : null}
            {graphData ? (
                <div className="card graph-card">
                    <div className="card-head-text">
                        <h3>Connected entities</h3>
                    </div>
                    <PackageGraph
                        currentEntityId={node.targetKey}
                        data={graphData}
                        showToolbar={false}
                    />
                </div>
            ) : null}
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
    model,
    node,
    text,
    packageId,
}: {
    allNodes: EntityNode[];
    model: PackageSemanticModel | null;
    node: EntityNode;
    text: string;
    packageId: string;
}) {
    if (node.kind === "variable") {
        const variable = model?.variables.find(
            (candidate) => candidate.id === node.id,
        );
        if (!variable) {
            return null;
        }
        const rules = variable.resolve?.rules ?? [];
        const defaultValue = variable.resolve?.default?.value;
        if (rules.length === 0 && defaultValue === undefined) {
            return null;
        }
        // Catalog-typed variables select by value name; link the string
        // selection to the catalog value it names.
        const catalogId =
            variable.declaration.kind === "catalog"
                ? (variable.declaration.value ?? null)
                : null;
        const valueLabel = (value: unknown): ReactNode => {
            if (value === undefined || value === null) {
                return "?";
            }
            const label =
                typeof value === "string" ? value : JSON.stringify(value);
            const entryNode =
                catalogId && typeof value === "string"
                    ? allNodes.find(
                          (candidate) =>
                              candidate.targetKey ===
                              `catalog_entries:${catalogId}:${value}`,
                      )
                    : undefined;
            return entryNode ? (
                <Link href={entityHref(packageId, entryNode.path)}>
                    {label}
                </Link>
            ) : (
                label
            );
        };
        return (
            <div className="card">
                <div className="card-head-text">
                    <h3>Values and resolution</h3>
                    <p className="hint">
                        Rules are checked in order; the first matching rule
                        wins, otherwise the default value applies.
                    </p>
                </div>
                <span className="label">how it resolves</span>
                <div className="spec">
                    {rules.map((rule) => (
                        <div className="spec-row" key={rule.index}>
                            <span className="g">rule[{rule.index}]</span>
                            <span>
                                if{" "}
                                {rule.when?.value ??
                                    rule.query?.value ??
                                    "missing condition"}{" "}
                                <span className="g">→</span>{" "}
                                {valueLabel(rule.value?.value)}
                            </span>
                        </div>
                    ))}
                    <div className="spec-row">
                        <span className="g">default</span>
                        <span>
                            <span className="g">→</span>{" "}
                            {defaultValue !== undefined
                                ? valueLabel(defaultValue)
                                : "not declared"}
                        </span>
                    </div>
                </div>
            </div>
        );
    }

    if (node.kind === "qualifier") {
        return null;
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
                    <h3>Values</h3>
                    <p className="hint">
                        Variables select these catalog values by name.
                    </p>
                </div>
                <CatalogValueList
                    items={entries.map((entry) => {
                        const entryNode = allNodes.find(
                            (candidate) =>
                                candidate.targetKey ===
                                `catalog_entries:${node.id}:${entry.key}`,
                        );
                        return {
                            key: entry.key,
                            href: entryNode
                                ? entityHref(packageId, entryNode.path)
                                : "#",
                            value: entry.value,
                        };
                    })}
                />
            </div>
        );
    }

    if (node.kind === "schema") {
        const summary = schemaSummary(text);
        if (!summary || summary.properties.length === 0) {
            return null;
        }
        return (
            <div className="card">
                <div className="card-head-text">
                    <h3>Contract</h3>
                    <p className="hint">
                        Values must validate against these properties.
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
    language: PackageDefinition["language"];
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

/* Builds the graph data contract from the semantic model. Rendering concepts
   live in components/package-graph. Callers supply entity paths per target
   key, the href builder, and optionally the set of paths edited in a branch. */
export function packageGraphData(input: {
    model: PackageSemanticModel;
    pathForKey: Map<string, string>;
    sourceByPath?: Map<
        string,
        { language: PackageDefinition["language"]; text: string }
    >;
    hrefFor: (path: string) => string;
    editedPaths?: Set<string>;
}): PackageGraphData {
    const { model, pathForKey, sourceByPath, hrefFor, editedPaths } = input;
    const graphNodes: PackageGraphData["nodes"] = [];
    const seenNodes = new Set<string>();
    const pushNode = (
        key: string,
        kind: PackageGraphData["nodes"][number]["kind"],
        label: string,
    ) => {
        const path = pathForKey.get(key);
        if (!path || seenNodes.has(key)) {
            return;
        }
        seenNodes.add(key);
        const source = sourceByPath?.get(path);
        graphNodes.push({
            id: key,
            kind,
            label,
            path,
            href: hrefFor(path),
            source: source?.text,
            language: source?.language,
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
    for (const evaluationContext of model.evaluationContexts) {
        pushNode(
            `evaluation_contexts:${evaluationContext.id}`,
            "evaluationContext",
            evaluationContext.id,
        );
    }
    for (const entry of model.catalogEntries) {
        pushNode(
            `catalog_entries:${entry.catalog}:${entry.key}`,
            "catalogEntry",
            entry.key,
        );
    }
    const edges: PackageGraphData["edges"] = [];
    const seenEdges = new Set<string>();
    const relatedByNode = new Map<string, Set<string>>();
    const ruleConditionQualifierByKey = new Map<string, string>();
    const ruleEntryByKey = new Map<
        string,
        { catalog: string; entry: string }
    >();
    const pushEdge = (
        from: string,
        to: string,
        kind: PackageGraphData["edges"][number]["kind"],
    ) => {
        const key = `${from}->${to}:${kind}`;
        if (seenEdges.has(key) || !seenNodes.has(from) || !seenNodes.has(to)) {
            return;
        }
        seenEdges.add(key);
        edges.push({ from, to, kind });
    };
    const pushRelated = (from: string, to: string) => {
        if (!seenNodes.has(from) || !seenNodes.has(to) || from === to) {
            return;
        }
        const related = relatedByNode.get(from) ?? new Set<string>();
        related.add(to);
        relatedByNode.set(from, related);
    };
    for (const compatibility of model.qualifierEvaluationContexts) {
        for (const evaluationContext of compatibility.evaluationContexts) {
            pushEdge(
                `evaluation_contexts:${evaluationContext}`,
                `qualifiers:${compatibility.qualifier}`,
                "supports",
            );
        }
    }
    for (const compatibility of model.variableEvaluationContexts) {
        for (const evaluationContext of compatibility.evaluationContexts) {
            pushRelated(
                `evaluation_contexts:${evaluationContext}`,
                `variables:${compatibility.variable}`,
            );
        }
    }
    for (const reference of model.references) {
        const { from, to, via } = reference;
        if (
            via.kind === "qualifierWhen" &&
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
            via.kind === "ruleCondition" &&
            from.kind === "variable" &&
            to.kind === "qualifier"
        ) {
            pushEdge(`qualifiers:${to.id}`, `variables:${from.id}`, "checks");
            ruleConditionQualifierByKey.set(
                `${from.id}:${via.index}`,
                `qualifiers:${to.id}`,
            );
        }
        // Variables connect to catalogs; catalogs fan out to their values.
        if (
            via.kind === "variableCatalog" &&
            from.kind === "variable" &&
            to.kind === "catalog"
        ) {
            pushEdge(`variables:${from.id}`, `catalogs:${to.id}`, "selects");
        }
        // Selected values are not drawn as edges (the path goes through the
        // catalog) but hover highlighting should reach them.
        if (
            (via.kind === "ruleValue" || via.kind === "resolveDefault") &&
            from.kind === "variable" &&
            to.kind === "catalogEntry"
        ) {
            const entryKey = `catalog_entries:${to.catalog}:${to.key}`;
            pushRelated(`variables:${from.id}`, entryKey);
            if (via.kind === "ruleValue") {
                ruleEntryByKey.set(`${from.id}:${via.index}`, {
                    catalog: to.catalog,
                    entry: to.key,
                });
            }
        }
    }
    for (const [ruleKey, entry] of ruleEntryByKey) {
        const qualifierKey = ruleConditionQualifierByKey.get(ruleKey);
        if (!qualifierKey) {
            continue;
        }
        pushRelated(qualifierKey, `catalogs:${entry.catalog}`);
        pushRelated(
            qualifierKey,
            `catalog_entries:${entry.catalog}:${entry.entry}`,
        );
    }
    for (const node of graphNodes) {
        const related = relatedByNode.get(node.id);
        if (related?.size) {
            node.related = Array.from(related);
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

function focusedGraphData(
    data: PackageGraphData,
    targetId: string,
): PackageGraphData | null {
    const nodesById = new Map(data.nodes.map((node) => [node.id, node]));
    if (!nodesById.has(targetId)) {
        return null;
    }

    const nodeIds = new Set<string>([targetId]);
    for (const edge of data.edges) {
        if (edge.from === targetId) {
            nodeIds.add(edge.to);
        }
        if (edge.to === targetId) {
            nodeIds.add(edge.from);
        }
    }
    for (const node of data.nodes) {
        for (const related of node.related ?? []) {
            if (node.id === targetId) {
                nodeIds.add(related);
            }
            if (related === targetId) {
                nodeIds.add(node.id);
            }
        }
    }

    const nodes = data.nodes
        .filter((node) => nodeIds.has(node.id))
        .map((node) => ({
            ...node,
            related: node.related?.filter((related) => nodeIds.has(related)),
        }));
    const edges = data.edges.filter(
        (edge) => nodeIds.has(edge.from) && nodeIds.has(edge.to),
    );

    return { nodes, edges };
}

function OverviewAttention({
    diagnostics,
    lintError,
    nodes,
    activeBranches,
    packageId,
}: {
    diagnostics: LintDiagnostic[];
    lintError: string | null;
    nodes: EntityNode[];
    activeBranches: BranchRecord[];
    packageId: string;
}) {
    const ranked = [
        ...diagnostics.filter((diagnostic) => diagnostic.severity === "error"),
        ...diagnostics.filter((diagnostic) => diagnostic.severity !== "error"),
    ];
    const shown = ranked.slice(0, 5);
    const allClear =
        lintError === null &&
        ranked.length === 0 &&
        activeBranches.length === 0;
    if (allClear) {
        return null;
    }

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
                        href={entityHref(packageId, match.path)}
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
                    href={sectionHref(packageId, "diagnostics")}
                >
                    … all {ranked.length} diagnostics
                </Link>
            ) : null}
            {activeBranches.map((branch) => (
                <Link
                    className="row"
                    href={`/app/packages/${packageId}/branches/${branch.id}`}
                    key={branch.id}
                >
                    <span className="row-icon">
                        <GitBranch aria-hidden size={16} />
                    </span>
                    <span className="row-text">
                        <span className="row-title mono">{branch.branch}</span>
                        <span className="row-sub">
                            active branch · updated{" "}
                            {formatDate(branchUpdatedAt(branch))}
                        </span>
                    </span>
                    <span className="row-side">
                        <BranchStatusPill branch={branch} />
                        <ChevronRight aria-hidden className="muted" size={15} />
                    </span>
                </Link>
            ))}
        </div>
    );
}

const OVERVIEW_CARD_SECTIONS: SectionId[] = [
    "variables",
    "qualifiers",
    "catalogs",
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
        case "linters":
            return <Wrench aria-hidden size={size} />;
        case "context":
            return <Braces aria-hidden size={size} />;
        default:
            return <Boxes aria-hidden size={size} />;
    }
}

function entityNodes(inventory: PackageInventory): EntityNode[] {
    const contextNodes: EntityNode[] = [];
    for (const item of inventory.context.evaluationContexts) {
        contextNodes.push({
            section: "context",
            kind: "context schema",
            id: item.id,
            path: item.path,
            description:
                item.description ?? item.title ?? "Evaluation context schema",
            badge: "schema",
            targetKey: `evaluation_contexts:${item.id}`,
            outboundKeys: [],
        });
    }
    for (const item of inventory.context.samples) {
        contextNodes.push({
            section: "context",
            kind: "context example",
            id: item.id,
            path: item.path,
            description: `Sample ${item.key} for evaluation context ${item.evaluationContextId}`,
            badge: item.evaluationContextId,
            targetKey: `evaluation_context_samples:${item.evaluationContextId}:${item.key}`,
            outboundKeys: [`evaluation_contexts:${item.evaluationContextId}`],
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
                // Catalog-typed variables select by value name, so each
                // selected catalog value is a reference too.
                ...(item.catalogReference
                    ? [
                          ...new Set([
                              ...item.ruleValues,
                              ...(item.defaultValue ? [item.defaultValue] : []),
                          ]),
                      ].map(
                          (key) =>
                              `catalog_entries:${item.catalogReference}:${key}`,
                      )
                    : []),
            ].filter((key): key is string => key !== null),
        })),
        ...inventory.qualifiers.map((item) => ({
            section: "qualifiers" as const,
            kind: "qualifier",
            id: item.id,
            path: item.path,
            description: item.description,
            badge: null,
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
            badge: `${item.entryCount} values`,
            targetKey: `catalogs:${item.id}`,
            outboundKeys: [],
        })),
        ...inventory.catalogEntries.map((item) => ({
            section: "catalogs" as const,
            kind: "catalog value",
            id: item.id,
            path: item.path,
            description: `Entry ${item.key} for catalog ${item.catalogId}`,
            badge: item.catalogId,
            targetKey: `catalog_entries:${item.catalogId}:${item.key}`,
            outboundKeys: [`catalogs:${item.catalogId}`],
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
    if (entity.kind === "evaluation_context" && typeof entity.id === "string") {
        return `evaluation_contexts:${entity.id}`;
    }
    if (
        entity.kind === "evaluation_context_sample" &&
        typeof entity.evaluation_context === "string"
    ) {
        return typeof entity.key === "string"
            ? `evaluation_context_samples:${entity.evaluation_context}:${entity.key}`
            : `evaluation_context_samples:${entity.evaluation_context}:*`;
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

function sectionHref(packageId: string, section: SectionId): string {
    return section === "overview"
        ? `/app/packages/${packageId}`
        : `/app/packages/${packageId}/${section}`;
}

function entityHref(packageId: string, path: string): string {
    return `/app/packages/${packageId}/tree/${encodeEntityPath(path)}`;
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
        kind: "package",
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
