import Link from "next/link";
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
import { notFound, redirect } from "next/navigation";
import { AppShell, NavBack, NavContext, NavGroupLabel, NavLink } from "@/components/app-shell";
import {
  DiagnosticCard,
  DiagnosticList,
  DiagnosticSummary,
} from "@/components/diagnostic-list";
import { SearchableList } from "@/components/searchable-list";
import { DraftCandidates } from "@/components/draft-candidates";
import { ReadOnlySource } from "@/components/read-only-source";
import { DraftStatusPill } from "@/components/status-pills";
import { StartDraftButton } from "@/components/start-draft-button";
import { StartDraftFromBranchForm } from "@/components/start-draft-from-branch-form";
import { requireUser } from "@/lib/auth";
import type { DraftSessionRecord } from "@/lib/db";
import { getWorkspaceForUser, listDraftSessionsForWorkspace } from "@/lib/db";
import type { LintDiagnostic, WorkspaceLintView } from "@/lib/rototo";
import {
  inspectWorkspace,
  lintInspectedWorkspace,
  loadWorkspaceRuntime,
  semanticModelFor,
} from "@/lib/rototo";
import type { ReferenceModel, VariableModel, WorkspaceSemanticModel } from "rototo";
import {
  inspectWorkspaceInventory,
  readWorkspaceDefinition,
  type WorkspaceDefinition,
  type WorkspaceInventory,
} from "@/lib/workspace-inventory";
import { schemaSummary } from "@/lib/entity-summary";
import { WorkspaceGraph } from "@/components/workspace-graph";
import type { WorkspaceGraphData } from "@/components/workspace-graph/types";

export type SectionId =
  | "overview"
  | "variables"
  | "qualifiers"
  | "resources"
  | "schemas"
  | "linters"
  | "context"
  | "diagnostics"
  | "drafts";

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

const SECTION_TITLES: Record<SectionId, string> = {
  overview: "Overview",
  variables: "Variables",
  qualifiers: "Qualifiers",
  resources: "Resources",
  schemas: "Schemas",
  linters: "Linters",
  context: "Context",
  diagnostics: "Diagnostics",
  drafts: "Drafts",
};

const SECTION_HINTS: Partial<Record<SectionId, string>> = {
  variables: "Named values the application resolves at runtime, with defaults and rules.",
  qualifiers: "Named runtime conditions. Rules reference them to select values.",
  resources: "Typed objects that schema-backed variables can point at.",
  schemas: "JSON Schemas that validate context and selected values.",
  linters: "Custom lint rules this workspace declares beyond the built-ins.",
  context: "The context contract: schema plus example resolution contexts.",
};

export async function WorkspaceScreen({
  path = null,
  section = null,
  workspaceId,
}: {
  path?: string | null;
  section?: SectionId | null;
  workspaceId: string;
}) {
  const user = await requireUser();
  const workspace = getWorkspaceForUser(workspaceId, user.githubUserId);
  if (!workspace) {
    notFound();
  }
  // Canonical URLs use the friendly slug; id URLs redirect to it.
  if (workspaceId !== workspace.slug) {
    redirect(
      path
        ? entityHref(workspace.slug, path)
        : sectionHref(workspace.slug, section ?? "overview"),
    );
  }
  const drafts = listDraftSessionsForWorkspace(workspace.id, user.githubUserId);

  let lint:
    | WorkspaceLintView
    | { root: string; diagnostics: []; error: string };
  let inventory: WorkspaceInventory = emptyInventory();
  let inventoryError: string | null = null;
  let stagedRoot: string | null = null;
  let inspectedWorkspace: Awaited<ReturnType<typeof inspectWorkspace>> | null = null;
  let model: WorkspaceSemanticModel | null = null;
  try {
    const inspected = await inspectWorkspace(workspace, user.githubToken);
    inspectedWorkspace = inspected;
    stagedRoot = inspected.root;
    const [loadedInventory, loadedLint, loadedModel] = await Promise.all([
      inspectWorkspaceInventory({
        workspace,
        inspected,
      }),
      lintInspectedWorkspace(inspected),
      semanticModelFor(inspected),
    ]);
    inventory = loadedInventory;
    lint = loadedLint;
    model = loadedModel;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    inventoryError = message;
    lint = {
      root: workspace.source,
      diagnostics: [],
      error: message,
    };
  }

  const nodes = entityNodes(inventory);
  const entityCounts = {
    variables: inventory.variables.length,
    qualifiers: inventory.qualifiers.length,
    resources: inventory.resources.length,
    schemas: inventory.schemas.length,
    linters: inventory.linters.length,
    context: inventory.context.exampleCount + (inventory.context.schemaPath ? 1 : 0),
  };
  const selectedPath = path;
  const selectedNode = selectedPath
    ? nodes.find((node) => node.path === selectedPath) ?? null
    : null;
  const selectedSection = selectedNode ? selectedNode.section : section ?? "overview";
  let definition: WorkspaceDefinition | null = null;
  let definitionError: string | null = null;
  if (selectedNode && stagedRoot) {
    try {
      definition = await readWorkspaceDefinition({
        workspace,
        root: stagedRoot,
        path: selectedNode.path,
      });
    } catch (error) {
      definitionError = error instanceof Error ? error.message : String(error);
    }
  }
  // For variables: resolve the variable with each saved request context so
  // the screen shows the actual pathway, not just the declared rules.
  let contextResolutions: SavedContextResolution[] = [];
  const selectedVariableModel =
    selectedNode?.kind === "variable"
      ? model?.variables.find((variable) => variable.id === selectedNode.id) ?? null
      : null;
  if (
    selectedNode?.kind === "variable" &&
    selectedVariableModel !== null &&
    model !== null &&
    inspectedWorkspace !== null &&
    stagedRoot !== null &&
    inventory.context.examples.length > 0
  ) {
    try {
      const runtime = await loadWorkspaceRuntime(workspace, user.githubToken);
      contextResolutions = await resolveSavedContexts({
        examples: inventory.context.examples.slice(0, 4),
        inspected: runtime,
        model,
        root: stagedRoot,
        variable: selectedVariableModel,
        workspace,
      });
    } catch {
      // no runtime (for example a structurally broken workspace): skip the preview
      contextResolutions = [];
    }
  }
  // Graph data for the overview: built server-side with entity sources for
  // hover previews.
  let graphData: WorkspaceGraphData | null = null;
  if (selectedSection === "overview" && !selectedNode && model !== null && stagedRoot !== null) {
    graphData = await workspaceGraphData(model, nodes, workspace, stagedRoot);
  }
  // For qualifiers: evaluate the qualifier (and any nested qualifiers it
  // references) against each saved request context.
  let qualifierEvaluations: QualifierContextEvaluation[] = [];
  if (
    selectedNode?.kind === "qualifier" &&
    model !== null &&
    stagedRoot !== null &&
    inventory.context.examples.length > 0
  ) {
    try {
      const runtime = await loadWorkspaceRuntime(workspace, user.githubToken);
      for (const examplePath of inventory.context.examples.slice(0, 4)) {
        const name = examplePath.split("/").pop() ?? examplePath;
        try {
          const exampleText = (
            await readWorkspaceDefinition({ workspace, root: stagedRoot, path: examplePath })
          ).text;
          const context = JSON.parse(exampleText) as Record<string, unknown>;
          const evaluation = await evaluateQualifierWithContext({
            context,
            model,
            qualifierId: selectedNode.id,
            runtime,
            seen: new Set([selectedNode.id]),
          });
          qualifierEvaluations.push({ name, path: examplePath, evaluation });
        } catch (error) {
          qualifierEvaluations.push({
            name,
            path: examplePath,
            evaluation: null,
            error: error instanceof Error ? error.message : String(error),
          });
        }
      }
    } catch {
      // no runtime: skip the preview
      qualifierEvaluations = [];
    }
  }
  const diagnosticCount = "error" in lint ? 0 : lint.diagnostics.length;
  const parentResourceNode =
    selectedNode?.kind === "resource object"
      ? nodes.find((node) => node.targetKey === `resources:${selectedNode.badge}`) ?? null
      : null;
  const entityDiagnostics =
    selectedNode && !("error" in lint)
      ? lint.diagnostics.filter((diagnostic) =>
          diagnosticMatchesNode(diagnostic, selectedNode, nodes),
        )
      : [];
  const workspaceName = `${workspace.owner}/${workspace.name}`;
  const entityCrumbLabel = selectedNode
    ? parentResourceNode && selectedNode.id.startsWith(`${parentResourceNode.id}/`)
      ? selectedNode.id.slice(parentResourceNode.id.length + 1)
      : selectedNode.id
    : "";
  // Crumbs are ancestors only; the topbar title names the current screen.
  const crumbs = [
    { label: "console", href: "/app" },
    { label: "workspaces", href: "/app/workspaces" },
    ...(selectedNode || selectedSection !== "overview"
      ? [{ label: workspace.path, href: `/app/workspaces/${workspace.slug}` }]
      : []),
    ...(selectedNode
      ? [
          {
            label: SECTION_TITLES[selectedNode.section].toLowerCase(),
            href: sectionHref(workspace.slug, selectedNode.section),
          },
          ...(parentResourceNode
            ? [
                {
                  label: parentResourceNode.id,
                  href: entityHref(workspace.slug, parentResourceNode.path),
                },
              ]
            : []),
        ]
      : []),
  ];

  return (
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
              <DiagnosticSummary diagnostics={lint.diagnostics} />
            )}
          </Link>
          <StartDraftButton workspaceId={workspace.slug} />
        </>
      }
      crumbs={crumbs}
      nav={
        <>
          <NavBack href="/app/workspaces" label="All workspaces" />
          <NavContext
            href={`/app/workspaces/${workspace.slug}`}
            label="workspace"
            value={`${workspaceName} · ${workspace.path}`}
          />
          <NavGroupLabel>Inspect</NavGroupLabel>
          <NavLink
            active={!selectedNode && selectedSection === "overview"}
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
            active={selectedSection === "resources"}
            count={entityCounts.resources}
            href={sectionHref(workspace.slug, "resources")}
            icon={<Database aria-hidden size={16} />}
            label="Resources"
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
            active={!selectedNode && selectedSection === "diagnostics"}
            count={diagnosticCount}
            href={sectionHref(workspace.slug, "diagnostics")}
            icon={<ListChecks aria-hidden size={16} />}
            label="Diagnostics"
          />
          <NavLink
            active={!selectedNode && selectedSection === "drafts"}
            count={drafts.length}
            href={sectionHref(workspace.slug, "drafts")}
            icon={<GitBranch aria-hidden size={16} />}
            label="Drafts"
          />
        </>
      }
      title={selectedNode ? entityCrumbLabel : SECTION_TITLES[selectedSection]}
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
          openDraft={drafts.find((draft) => draft.status === "open") ?? null}
          parentResource={parentResourceNode}
          qualifierEvaluations={qualifierEvaluations}
          workspaceId={workspace.slug}
        />
      ) : (
        <WorkspaceSection
          diagnosticCount={diagnosticCount}
          drafts={drafts}
          graphData={graphData}
          inventory={inventory}
          inventoryError={inventoryError}
          lint={lint}
          nodes={nodes}
          section={selectedSection}
          workspace={workspace}
        />
      )}
    </AppShell>
  );
}

function WorkspaceSection({
  diagnosticCount,
  drafts,
  graphData,
  inventory,
  inventoryError,
  lint,
  nodes,
  section,
  workspace,
}: {
  diagnosticCount: number;
  drafts: DraftSessionRecord[];
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
    const openDrafts = drafts.filter((draft) => draft.status === "open");
    return (
      <section className="section">
        <div className="section-header">
          <div className="section-header-text">
            <h1 className="mono">{workspace.path}</h1>
            <p className="hint">
              What this workspace declares, and whether it lints clean right now.
            </p>
          </div>
        </div>
        <div className="meta-grid">
          <div className="meta-item">
            <span className="label">repository</span>
            <span className="meta-value mono">
              <Link className="title-link" href={`/app/workspaces?repo=${workspace.repoId}`}>
                {workspace.owner}/{workspace.name}
              </Link>
            </span>
          </div>
          <div className="meta-item">
            <span className="label">workspace path</span>
            <span className="meta-value mono">{workspace.path}</span>
          </div>
          <div className="meta-item">
            <span className="label">ref</span>
            <span className="meta-value mono">{workspace.ref}</span>
          </div>
        </div>
        {inventoryError ? (
          <div className="banner banner-err">
            <TriangleAlert aria-hidden size={16} />
            <span>Workspace inventory failed to load: {inventoryError}</span>
          </div>
        ) : (
          <>
            <OverviewAttention
              diagnostics={"error" in lint ? [] : lint.diagnostics}
              lintError={"error" in lint ? lint.error : null}
              nodes={nodes}
              openDrafts={openDrafts}
              workspaceId={workspace.slug}
            />
            {graphData ? (
              <div className="card graph-card">
                <div className="card-head-text">
                  <h3>Entity graph</h3>
                  <p className="hint">
                    How qualifiers, variables, objects, schemas, and linters connect. Hover a
                    node to trace its references and preview its source; click to open it.
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
            Semantic lint results, grouped by the entity they point at. Source locations
            are kept so you can fix files directly.
          </p>
        </div>
        <DiagnosticList
          diagnosticHref={(diagnostic) => {
            const match = nodes.find((node) => diagnosticMatchesNode(diagnostic, node, nodes));
            return match ? entityHref(workspace.slug, match.path) : null;
          }}
          diagnostics={lint.diagnostics}
        />
      </section>
    );
  }

  if (section === "drafts") {
    return (
      <section className="section">
        <div className="section-header-text">
          <h1>Drafts</h1>
          <p className="hint">
            Each draft is a branch created from <span className="mono">{workspace.ref}</span>.
            Edits commit to the branch; publishing opens a pull request.
          </p>
        </div>
        <DraftCandidates workspaceId={workspace.slug} />
        <StartDraftFromBranchForm workspaceId={workspace.slug} />
        {drafts.length === 0 ? (
          <div className="empty-state">
            <span className="empty-puck">
              <GitBranch aria-hidden size={18} />
            </span>
            <p>
              No draft branches yet. Use “Edit workspace” to start one, or open an
              existing branch above.
            </p>
          </div>
        ) : (
          <SearchableList
            className="row-list"
            emptyLabel="No drafts match that search."
            label="Search drafts"
            placeholder="Search drafts"
          >
            {drafts.map((draft) => (
              <Link
                className="row"
                data-search={`${draft.branch} ${draft.status} ${draft.prState ?? ""} ${draft.prUrl ?? ""}`}
                href={`/app/workspaces/${workspace.slug}/drafts/${draft.id}`}
                key={draft.id}
              >
                <span className="row-icon">
                  <GitBranch aria-hidden size={16} />
                </span>
                <span className="row-text">
                  <span className="row-title mono">{draft.branch}</span>
                  <span className="row-sub">updated {formatDate(draft.updatedAt)}</span>
                </span>
                <span className="row-side">
                  <DraftStatusPill draft={draft} />
                  <ChevronRight aria-hidden className="muted" size={15} />
                </span>
              </Link>
            ))}
          </SearchableList>
        )}
      </section>
    );
  }

  // The resources section lists resource types only; each type carries its
  // objects inline.
  const sectionNodes = nodes.filter(
    (node) => node.section === section && (section !== "resources" || node.kind === "resource"),
  );

  return (
    <section className="section">
      <div className="section-header-text">
        <h1>{SECTION_TITLES[section]}</h1>
        <p className="hint">
          {SECTION_HINTS[section] ?? "Select an entity to read its full source definition."}
        </p>
      </div>
      {sectionNodes.length === 0 ? (
        <div className="empty-state">
          <span className="empty-puck">{sectionIcon(section, 18)}</span>
          <p>This workspace declares no {SECTION_TITLES[section].toLowerCase()}.</p>
        </div>
      ) : (
        <SearchableList
          className="row-list"
          emptyLabel="Nothing matches that search."
          label={`Search ${SECTION_TITLES[section].toLowerCase()}`}
          placeholder={`Search ${SECTION_TITLES[section].toLowerCase()}`}
        >
          {sectionNodes.map((node) => {
            const objects =
              node.kind === "resource"
                ? nodes.filter(
                    (candidate) =>
                      candidate.kind === "resource object" && candidate.badge === node.id,
                  )
                : [];
            if (objects.length === 0) {
              return (
                <Link
                  className="row"
                  data-search={entitySearchText(node)}
                  href={entityHref(workspace.slug, node.path)}
                  key={node.path}
                >
                  <span className="row-icon">{sectionIcon(node.section, 16)}</span>
                  <span className="row-text">
                    <span className="row-title mono">{node.id}</span>
                    <span className="row-sub">{node.description ?? node.path}</span>
                  </span>
                  <span className="row-side">
                    {node.badge ? <span className="tag">{node.badge}</span> : null}
                    <ChevronRight aria-hidden className="muted" size={15} />
                  </span>
                </Link>
              );
            }
            return (
              <div
                className="row"
                data-search={`${entitySearchText(node)} ${objects
                  .map((object) => object.id)
                  .join(" ")}`}
                key={node.path}
              >
                <span className="row-icon">{sectionIcon(node.section, 16)}</span>
                <span className="row-text">
                  <Link className="row-title mono row-link" href={entityHref(workspace.slug, node.path)}>
                    {node.id}
                  </Link>
                  <span className="row-sub">{node.description ?? node.path}</span>
                  <span className="row-objects">
                    {objects.map((object) => (
                      <Link
                        className="pill pill-neutral"
                        href={entityHref(workspace.slug, object.path)}
                        key={object.path}
                      >
                        {object.id.split("/").pop()}
                      </Link>
                    ))}
                  </span>
                </span>
                <span className="row-side">
                  <span className="tag">
                    {objects.length} {objects.length === 1 ? "object" : "objects"}
                  </span>
                  <Link
                    aria-label={`Open resource ${node.id}`}
                    className="muted"
                    href={entityHref(workspace.slug, node.path)}
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
  openDraft,
  parentResource,
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
  openDraft: DraftSessionRecord | null;
  parentResource: EntityNode | null;
  qualifierEvaluations: QualifierContextEvaluation[];
  workspaceId: string;
}) {
  const relations = entityRelations({ allNodes, model, node, workspaceId });
  const editHref = openDraft
    ? `/app/workspaces/${workspaceId}/drafts/${openDraft.id}/tree/${encodeEntityPath(node.path)}`
    : null;

  return (
    <section className="section">
      <div className="section-header">
        <div className="section-header-text">
          <span className="label">{node.kind}</span>
          <h1 className="mono">
            {parentResource && node.id.startsWith(`${parentResource.id}/`)
              ? node.id.slice(parentResource.id.length + 1)
              : node.id}
          </h1>
          {node.description ? <p className="hint">{node.description}</p> : null}
        </div>
        <div className="action-row">
          {node.badge && !parentResource ? <span className="tag">{node.badge}</span> : null}
          {parentResource ? (
            <Link
              className="btn btn-secondary btn-sm"
              href={entityHref(workspaceId, parentResource.path)}
            >
              <ArrowLeft aria-hidden size={14} />
              Resource {parentResource.id}
            </Link>
          ) : (
            <Link className="btn btn-secondary btn-sm" href={sectionHref(workspaceId, node.section)}>
              <ArrowLeft aria-hidden size={14} />
              All {SECTION_TITLES[node.section].toLowerCase()}
            </Link>
          )}
          {editHref ? (
            <Link className="btn btn-primary btn-sm" href={editHref}>
              <Pencil aria-hidden size={13} />
              Edit in draft
            </Link>
          ) : null}
        </div>
      </div>
      {diagnostics.length > 0 ? (
        <div className="diagnostic-group">
          <div className="diagnostic-group-head">
            <span className="tag">diagnostics</span>
            <span className="label">{diagnostics.length} on this entity</span>
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
      (candidate) => candidate.section === "qualifiers" && candidate.id === id,
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
    const variable = model?.variables.find((candidate) => candidate.id === node.id);
    if (!variable) {
      return null;
    }
    const rules = variable.resolve?.rules ?? [];
    const defaultKey = variable.resolve?.default?.value ?? null;
    if (rules.length === 0 && !defaultKey && variable.values.length === 0) {
      return null;
    }
    // Resource-typed variables select objects by key; link each value key to
    // the object it names.
    const resourceId =
      variable.declaration.kind === "resource" ? variable.declaration.value ?? null : null;
    const valueKeyLabel = (key: string | null): ReactNode => {
      if (!key) {
        return "?";
      }
      const objectNode = resourceId
        ? allNodes.find(
            (candidate) => candidate.targetKey === `resource_objects:${resourceId}:${key}`,
          )
        : undefined;
      return objectNode ? <Link href={entityHref(workspaceId, objectNode.path)}>{key}</Link> : key;
    };
    return (
      <div className="card">
        <div className="card-head-text">
          <h3>Values and resolution</h3>
          <p className="hint">
            Rules are checked in order; the first matching qualifier wins, otherwise the
            default value applies.
          </p>
        </div>
        {variable.values.length > 0 ? (
          <>
            <span className="label">declared values</span>
            <div className="spec">
              {variable.values.map((value) => (
                <div className="spec-row" key={value.key}>
                  <span>
                    {value.key} <span className="g">=</span> {JSON.stringify(value.value)}
                    {value.key === defaultKey ? <span className="g"> · default</span> : null}
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
                if {qualifierLabel(rule.qualifier?.value ?? null)}{" "}
                <span className="g">→</span> {valueKeyLabel(rule.value?.value ?? null)}
              </span>
            </div>
          ))}
          <div className="spec-row">
            <span className="g">default</span>
            <span>
              <span className="g">→</span>{" "}
              {defaultKey ? valueKeyLabel(defaultKey) : "not declared"}
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
                    <Link href={entityHref(workspaceId, resolution.path)}>
                      {resolution.name}
                    </Link>
                  </span>
                  {resolution.ok ? (
                    <span>
                      {(resolution.steps ?? []).map((step, at, steps) => (
                        <span key={step.index}>
                          <span className="g">rule[{step.index}]</span>{" "}
                          {qualifierLabel(step.qualifier)}{" "}
                          {step.matched ? "✓" : <span className="g">✗</span>}
                          {at < steps.length - 1 || resolution.usedDefault ? (
                            <span className="g"> · </span>
                          ) : (
                            " "
                          )}
                        </span>
                      ))}
                      {resolution.usedDefault ? <span className="g">default </span> : null}
                      <span className="g">→</span> {valueKeyLabel(resolution.valueKey ?? null)}
                    </span>
                  ) : (
                    <span className="g">did not resolve: {resolution.error}</span>
                  )}
                </div>
              ))}
            </div>
            {contextResolutions
              .filter((resolution) => resolution.ok && (resolution.steps?.length ?? 0) > 0)
              .map((resolution) => (
                <div key={`detail:${resolution.path}`}>
                  <span className="label">
                    qualifier resolution — {resolution.name}
                  </span>
                  <div className="spec">
                    {(resolution.steps ?? []).map((step) => (
                      <QualifierEvaluationRows
                        depth={0}
                        evaluation={step.evaluation}
                        key={`${resolution.path}:${step.index}`}
                        qualifierLabel={qualifierLabel}
                      />
                    ))}
                  </div>
                </div>
              ))}
          </>
        ) : null}
      </div>
    );
  }

  if (node.kind === "qualifier") {
    const qualifier = model?.qualifiers.find((candidate) => candidate.id === node.id);
    const predicates = qualifier?.predicates ?? [];
    if (predicates.length === 0) {
      return null;
    }
    return (
      <div className="card">
        <div className="card-head-text">
          <h3>Predicates</h3>
          <p className="hint">All predicates must match for the qualifier to apply.</p>
        </div>
        {qualifierEvaluations.length > 0 ? (
          <>
            <span className="label">with saved contexts</span>
            <div className="spec">
              {qualifierEvaluations.map((entry) => (
                <div key={entry.path}>
                  <div className="spec-row">
                    <span className="g">
                      <Link href={entityHref(workspaceId, entry.path)}>{entry.name}</Link>
                    </span>
                    <span>
                      {entry.evaluation === null || entry.evaluation.matched === null ? (
                        <span className="g">
                          not evaluable{entry.error ? `: ${entry.error}` : ""}
                        </span>
                      ) : entry.evaluation.matched ? (
                        "✓ matches"
                      ) : (
                        <span className="g">✗ does not match</span>
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
                {predicate.attribute?.value ?? "?"} <strong>{predicate.op?.value ?? "?"}</strong>{" "}
                {predicate.value !== undefined ? JSON.stringify(predicate.value) : ""}
              </span>
            </div>
          ))}
        </div>
      </div>
    );
  }

  if (node.kind === "resource") {
    const objects = model?.resourceObjects.filter((object) => object.resource === node.id) ?? [];
    if (objects.length === 0) {
      return null;
    }
    return (
      <div className="card">
        <div className="card-head-text">
          <h3>Objects</h3>
          <p className="hint">Variable values reference these objects by key.</p>
        </div>
        <div className="row-list">
          {objects.map((object) => {
            const objectNode = allNodes.find(
              (candidate) => candidate.targetKey === `resource_objects:${node.id}:${object.key}`,
            );
            const fields =
              typeof object.value === "object" &&
              object.value !== null &&
              !Array.isArray(object.value)
                ? Object.entries(object.value as Record<string, unknown>)
                : [];
            return (
              <Link
                className="row"
                href={objectNode ? entityHref(workspaceId, objectNode.path) : "#"}
                key={object.key}
              >
                <span className="row-icon">
                  <Database aria-hidden size={16} />
                </span>
                <span className="row-text">
                  <span className="row-title mono">{object.key}</span>
                  <span className="row-sub mono">
                    {fields
                      .slice(0, 4)
                      .map(([key, value]) => `${key} = ${JSON.stringify(value)}`)
                      .join("  ·  ")}
                    {fields.length > 4 ? `  ·  +${fields.length - 4} more` : ""}
                  </span>
                </span>
                <span className="row-side">
                  <span className="tag">
                    {fields.length} {fields.length === 1 ? "field" : "fields"}
                  </span>
                  <ChevronRight aria-hidden className="muted" size={15} />
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
              <span className="g">{property.required ? "●" : "○"}</span>
              <span>
                {property.key} <span className="g">{property.type ?? ""}</span>
                {property.required ? "" : " (optional)"}
                {property.description ? <span className="g"> — {property.description}</span> : null}
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
      (node.path === path || node.path.endsWith(`/${path}`) || path.endsWith(`/${node.path}`))
    ) {
      // lint positions are 0-based; the editor highlights 1-based lines
      const displayLine = line + 1;
      const severity = diagnostic.severity === "error" ? "error" : "warning";
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
              {predicate.attribute ?? "?"} <strong>{predicate.op ?? "?"}</strong>{" "}
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
   live in components/workspace-graph. */
async function workspaceGraphData(
  model: WorkspaceSemanticModel,
  nodes: EntityNode[],
  workspace: Parameters<typeof readWorkspaceDefinition>[0]["workspace"] & { slug: string },
  stagedRoot: string,
): Promise<WorkspaceGraphData> {
  const workspaceId = workspace.slug;
  const nodeByKey = new Map(nodes.map((node) => [node.targetKey, node]));
  const graphNodes: WorkspaceGraphData["nodes"] = [];
  const seenNodes = new Set<string>();
  const pushNode = (
    key: string,
    kind: WorkspaceGraphData["nodes"][number]["kind"],
    label: string,
  ) => {
    const node = nodeByKey.get(key);
    if (!node || seenNodes.has(key)) {
      return;
    }
    seenNodes.add(key);
    graphNodes.push({ id: key, kind, label, href: entityHref(workspaceId, node.path) });
  };
  for (const qualifier of model.qualifiers) {
    pushNode(`qualifiers:${qualifier.id}`, "qualifier", qualifier.id);
  }
  for (const variable of model.variables) {
    pushNode(`variables:${variable.id}`, "variable", variable.id);
  }
  for (const object of model.resourceObjects) {
    pushNode(
      `resource_objects:${object.resource}:${object.key}`,
      "resourceObject",
      object.key,
    );
  }
  for (const schema of model.schemas) {
    const file = schema.path.split("/").pop() ?? schema.path;
    if (file === "context.schema.json") {
      continue;
    }
    pushNode(`schemas:${file}`, "schema", file.replace(/\.schema\.json$/, ""));
  }
  for (const node of nodes) {
    if (node.kind === "linter") {
      pushNode(node.targetKey, "linter", node.id);
    }
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
  const objectsByResource = new Map<string, string[]>();
  for (const object of model.resourceObjects) {
    const keys = objectsByResource.get(object.resource) ?? [];
    keys.push(object.key);
    objectsByResource.set(object.resource, keys);
  }
  for (const reference of model.references) {
    const { from, to, via } = reference;
    if (via.kind === "predicateQualifier" && from.kind === "qualifier" && to.kind === "qualifier") {
      pushEdge(`qualifiers:${to.id}`, `qualifiers:${from.id}`, "requires");
    }
    if (via.kind === "ruleQualifier" && from.kind === "variable" && to.kind === "qualifier") {
      pushEdge(`qualifiers:${to.id}`, `variables:${from.id}`, "checks");
    }
    if (
      (via.kind === "ruleValue" || via.kind === "resolveDefault") &&
      from.kind === "variable" &&
      to.kind === "resourceObject"
    ) {
      pushEdge(
        `variables:${from.id}`,
        `resource_objects:${to.resource}:${to.key}`,
        "selects",
      );
    }
    if (via.kind === "resourceSchema" && from.kind === "resource" && to.kind === "schema") {
      const file = to.path.split("/").pop() ?? to.path;
      for (const key of objectsByResource.get(from.id) ?? []) {
        pushEdge(`resource_objects:${from.id}:${key}`, `schemas:${file}`, "validates");
      }
    }
  }
  for (const variable of model.variables) {
    if (variable.declaration.kind === "schema" && variable.declaration.value) {
      const file = variable.declaration.value.split("/").pop() ?? variable.declaration.value;
      pushEdge(`variables:${variable.id}`, `schemas:${file}`, "validates");
    }
  }
  // Source previews for hover tooltips, truncated to keep the page light.
  await Promise.all(
    graphNodes.map(async (graphNode) => {
      const node = nodeByKey.get(graphNode.id);
      if (!node) {
        return;
      }
      try {
        const definition = await readWorkspaceDefinition({
          workspace,
          root: stagedRoot,
          path: node.path,
        });
        const lines = definition.text.split("\n");
        const preview = lines.slice(0, 160).join("\n");
        graphNode.source = lines.length > 160 ? `${preview}\n…` : preview;
        graphNode.language = definition.language;
      } catch {
        // no preview for unreadable files
      }
    }),
  );
  return { nodes: graphNodes, edges };
}

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
      case "resource":
        return `resources:${ref.id}`;
      case "resourceObject":
        return `resource_objects:${ref.resource}:${ref.key}`;
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
    const match = key ? allNodes.find((candidate) => candidate.targetKey === key) : undefined;
    const text =
      ref.kind === "resourceObject"
        ? ref.key
        : ref.kind === "schema"
          ? (ref.path.split("/").pop() ?? ref.path)
          : ref.kind === "value"
            ? ref.variable
            : ref.kind === "contextAttribute"
              ? ref.name
              : ref.id;
    return match ? <Link href={entityHref(workspaceId, match.path)}>{text}</Link> : text;
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
          if (to.kind === "resourceObject") {
            outbound.push({
              key: `out:${index}`,
              content: (
                <>
                  {g(`rule[${via.index}]`)} selects object {link}
                </>
              ),
            });
          }
          break;
        case "resolveDefault":
          if (to.kind === "resourceObject") {
            outbound.push({
              key: `out:${index}`,
              content: <>{g("default")} selects object {link}</>,
            });
          }
          break;
        case "variableResource":
          outbound.push({
            key: `out:${index}`,
            content: <>values come from resource {link}</>,
          });
          break;
        case "resourceSchema":
          outbound.push({
            key: `out:${index}`,
            content: <>objects validate against {link}</>,
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
            content: <>{link} selects this {g("as default")}</>,
          });
          break;
        case "variableResource":
          inbound.push({
            key: `in:${index}`,
            content: <>{link} takes its values from this resource</>,
          });
          break;
        case "resourceSchema":
          inbound.push({
            key: `in:${index}`,
            content: <>{link} validates its objects against this</>,
          });
          break;
        case "predicateQualifier":
          inbound.push({
            key: `in:${index}`,
            content: (
              <>
                {link} requires this {g(`in predicate[${via.index}]`)}
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
    const variable = model.variables.find((candidate) => candidate.id === node.id);
    if (variable?.declaration.kind === "schema" && variable.declaration.value) {
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
              {entityLink({ kind: "variable", id: variable.id })} validates its values against
              this
            </>
          ),
        });
      }
    }
  }
  if (node.kind === "resource object") {
    const resourceId = node.badge ?? "";
    outbound.push({
      key: "out:membership",
      content: <>object of resource {entityLink({ kind: "resource", id: resourceId })}</>,
    });
  }

  return { outbound, inbound };
}

function OverviewAttention({
  diagnostics,
  lintError,
  nodes,
  openDrafts,
  workspaceId,
}: {
  diagnostics: LintDiagnostic[];
  lintError: string | null;
  nodes: EntityNode[];
  openDrafts: DraftSessionRecord[];
  workspaceId: string;
}) {
  const ranked = [
    ...diagnostics.filter((diagnostic) => diagnostic.severity === "error"),
    ...diagnostics.filter((diagnostic) => diagnostic.severity !== "error"),
  ];
  const shown = ranked.slice(0, 5);
  const allClear = lintError === null && ranked.length === 0 && openDrafts.length === 0;

  return (
    <div className="card">
      <div className="card-head">
        <div className="card-head-text">
          <h3>Needs attention</h3>
        </div>
        {ranked.length > 0 ? <DiagnosticSummary diagnostics={diagnostics} /> : null}
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
          <span>Nothing needs you right now — lint is clean and there are no open drafts.</span>
        </div>
      ) : (
        <>
          {shown.map((diagnostic, index) => {
            const match = nodes.find((node) => diagnosticMatchesNode(diagnostic, node, nodes));
            return (
              <div className={`attn-row ${diagnostic.severity ?? ""}`} key={index}>
                <span className="attn-text">{diagnostic.message ?? "Diagnostic"}</span>
                {match ? (
                  <Link
                    className="btn btn-ghost btn-sm"
                    href={entityHref(workspaceId, match.path)}
                  >
                    Open {match.kind} / {match.id}
                  </Link>
                ) : null}
              </div>
            );
          })}
          {ranked.length > shown.length ? (
            <Link className="entity-card-more" href={sectionHref(workspaceId, "diagnostics")}>
              … all {ranked.length} diagnostics
            </Link>
          ) : null}
          {openDrafts.map((draft) => (
            <Link
              className="row"
              href={`/app/workspaces/${workspaceId}/drafts/${draft.id}`}
              key={draft.id}
            >
              <span className="row-icon">
                <GitBranch aria-hidden size={16} />
              </span>
              <span className="row-text">
                <span className="row-title mono">{draft.branch}</span>
                <span className="row-sub">
                  open draft · updated {formatDate(draft.updatedAt)}
                </span>
              </span>
              <span className="row-side">
                <DraftStatusPill draft={draft} />
                <ChevronRight aria-hidden className="muted" size={15} />
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
  "resources",
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
    case "resources":
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
        item.resourceReference ? `resources:${item.resourceReference}` : null,
        // Resource-typed variables select objects by value key, so each
        // selected object is a reference too.
        ...(item.resourceReference
          ? [...new Set([...item.ruleValueKeys, ...(item.defaultValueKey ? [item.defaultValueKey] : [])])].map(
              (key) => `resource_objects:${item.resourceReference}:${key}`,
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
      outboundKeys: item.qualifierReferences.map((id) => `qualifiers:${id}`),
    })),
    ...inventory.resources.map((item) => ({
      section: "resources" as const,
      kind: "resource",
      id: item.id,
      path: item.path,
      description: item.description,
      badge: `${item.objectCount} objects`,
      targetKey: `resources:${item.id}`,
      outboundKeys: item.schemaReference ? [`schemas:${item.schemaReference}`] : [],
    })),
    ...inventory.resourceObjects.map((item) => ({
      section: "resources" as const,
      kind: "resource object",
      id: item.id,
      path: item.path,
      description: `Object ${item.key} for resource ${item.resourceId}`,
      badge: item.resourceId,
      targetKey: `resource_objects:${item.resourceId}:${item.key}`,
      outboundKeys: [`resources:${item.resourceId}`],
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

type SavedContextResolution = {
  name: string;
  path: string;
  ok: boolean;
  valueKey?: string;
  /* The walk through the rules: each step is a qualifier evaluation, in
     order, ending at the first match. */
  steps?: Array<{
    index: number;
    qualifier: string;
    matched: boolean;
    evaluation: QualifierEvaluation;
  }>;
  usedDefault?: boolean;
  error?: string;
};

type QualifierContextEvaluation = {
  name: string;
  path: string;
  evaluation: QualifierEvaluation | null;
  error?: string;
};

/* A qualifier's resolution against one context: its verdict plus every
   predicate, with the context value the predicate read and — for
   qualifier.<id> predicates — the nested qualifier's own resolution. */
type QualifierEvaluation = {
  id: string;
  matched: boolean | null;
  predicates: Array<{
    index: number;
    attribute: string | null;
    op: string | null;
    valueLiteral: string | null;
    contextValue: string | null;
    nested: QualifierEvaluation | null;
  }>;
};

async function evaluateQualifierWithContext(input: {
  context: Record<string, unknown>;
  model: WorkspaceSemanticModel;
  qualifierId: string;
  runtime: Awaited<ReturnType<typeof inspectWorkspace>>;
  seen: Set<string>;
}): Promise<QualifierEvaluation> {
  let matched: boolean | null = null;
  try {
    matched = (
      await input.runtime.resolveQualifier(input.qualifierId, input.context as never)
    ).value;
  } catch {
    matched = null;
  }
  const qualifier = input.model.qualifiers.find(
    (candidate) => candidate.id === input.qualifierId,
  );
  const predicates: QualifierEvaluation["predicates"] = [];
  for (const predicate of qualifier?.predicates ?? []) {
    const attribute = predicate.attribute?.value ?? null;
    let nested: QualifierEvaluation | null = null;
    let contextValue: string | null = null;
    if (attribute?.startsWith("qualifier.")) {
      const nestedId = attribute.slice("qualifier.".length);
      if (!input.seen.has(nestedId)) {
        input.seen.add(nestedId);
        nested = await evaluateQualifierWithContext({ ...input, qualifierId: nestedId });
      }
    } else if (attribute) {
      const value = contextPathValue(input.context, attribute);
      contextValue = value === undefined ? null : JSON.stringify(value);
    }
    predicates.push({
      index: predicate.index,
      attribute,
      op: predicate.op?.value ?? null,
      valueLiteral: predicate.value !== undefined ? JSON.stringify(predicate.value) : null,
      contextValue,
      nested,
    });
  }
  return { id: input.qualifierId, matched, predicates };
}

/* Display-only lookup of the context value a predicate reads. */
function contextPathValue(context: Record<string, unknown>, path: string): unknown {
  let current: unknown = context;
  for (const segment of path.split(".")) {
    if (typeof current !== "object" || current === null || Array.isArray(current)) {
      return undefined;
    }
    current = (current as Record<string, unknown>)[segment];
  }
  return current;
}

async function resolveSavedContexts(input: {
  examples: string[];
  inspected: Awaited<ReturnType<typeof inspectWorkspace>>;
  model: WorkspaceSemanticModel;
  root: string;
  variable: VariableModel;
  workspace: Parameters<typeof readWorkspaceDefinition>[0]["workspace"];
}): Promise<SavedContextResolution[]> {
  const rules = input.variable.resolve?.rules ?? [];
  const resolutions: SavedContextResolution[] = [];
  for (const examplePath of input.examples) {
    const name = examplePath.split("/").pop() ?? examplePath;
    try {
      const exampleText = (
        await readWorkspaceDefinition({
          workspace: input.workspace,
          root: input.root,
          path: examplePath,
        })
      ).text;
      const context = JSON.parse(exampleText) as Record<string, unknown>;
      const resolution = await input.inspected.resolveVariable(
        input.variable.id,
        context as Parameters<typeof input.inspected.resolveVariable>[1],
      );
      const steps: NonNullable<SavedContextResolution["steps"]> = [];
      let matchedRule = false;
      for (const rule of rules) {
        const qualifier = rule.qualifier?.value;
        if (!qualifier) {
          continue;
        }
        const evaluation = await evaluateQualifierWithContext({
          context,
          model: input.model,
          qualifierId: qualifier,
          runtime: input.inspected,
          seen: new Set([qualifier]),
        });
        if (evaluation.matched === null) {
          throw new Error(`qualifier ${qualifier} could not be evaluated`);
        }
        steps.push({ index: rule.index, qualifier, matched: evaluation.matched, evaluation });
        if (evaluation.matched) {
          matchedRule = true;
          break;
        }
      }
      resolutions.push({
        name,
        path: examplePath,
        ok: true,
        valueKey: resolution.valueKey,
        steps,
        usedDefault: !matchedRule,
      });
    } catch (error) {
      resolutions.push({
        name,
        path: examplePath,
        ok: false,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }
  return resolutions;
}

function diagnosticMatchesNode(
  diagnostic: LintDiagnostic,
  node: EntityNode,
  nodes: EntityNode[],
): boolean {
  if (nodeForDiagnosticPath(nodes, diagnostic.location?.path)?.path === node.path) {
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
  if ((entity.kind === "value" || entity.kind === "rule") && typeof entity.variable === "string") {
    return `variables:${entity.variable}`;
  }
  if (entity.kind === "qualifier" && typeof entity.id === "string") {
    return `qualifiers:${entity.id}`;
  }
  if (entity.kind === "predicate" && typeof entity.qualifier === "string") {
    return `qualifiers:${entity.qualifier}`;
  }
  if (entity.kind === "resource" && typeof entity.id === "string") {
    return `resources:${entity.id}`;
  }
  if (entity.kind === "resource_object" && typeof entity.resource === "string") {
    return typeof entity.key === "string"
      ? `resource_objects:${entity.resource}:${entity.key}`
      : `resource_objects:${entity.resource}:*`;
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
  return [node.id, node.kind, node.path, node.description, node.badge].filter(Boolean).join(" ");
}

export function normalizeSection(value: string | null): SectionId | null {
  if (
    value === "overview" ||
    value === "variables" ||
    value === "qualifiers" ||
    value === "resources" ||
    value === "schemas" ||
    value === "linters" ||
    value === "context" ||
    value === "diagnostics" ||
    value === "drafts"
  ) {
    return value;
  }
  return null;
}

function formatDate(value: string): string {
  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function emptyInventory(): WorkspaceInventory {
  return {
    variables: [],
    qualifiers: [],
    resources: [],
    resourceObjects: [],
    schemas: [],
    linters: [],
    context: {
      schemaPath: null,
      exampleCount: 0,
      examples: [],
    },
  };
}
