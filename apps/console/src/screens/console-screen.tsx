import { type MouseEvent, useEffect, useState } from "react";
import {
    Boxes,
    ChevronRight,
    FolderGit2,
    GitBranch,
    History,
    Layers,
    TriangleAlert,
} from "lucide-react";
import { useNavigate, useSearchParams } from "react-router";

import { AppShell, NavGroupLabel, NavLink } from "@/components/app-shell";
import { LoadingScreen } from "@/components/loading-screen";
import { RefreshSourceTreeButton } from "@/components/refresh-source-tree-button";
import { RemoveSourceTreeButton } from "@/components/remove-source-tree-button";
import { SourceTreeRegistrationForm } from "@/components/source-tree-registration-form";
import { SearchableList } from "@/components/searchable-list";
import { BranchStatusPill } from "@/components/status-pills";
import { api, useApi } from "@/lib/api";
import { Link } from "@/lib/link";
import { useShellUser } from "@/lib/me";
import { RefreshScope } from "@/lib/refresh";
import type {
    ConsoleData,
    BranchRecord,
    SourceTreeWithWorkspaces,
    WorkspaceRecord,
    WorkspaceSummary,
    WorkspaceSummariesData,
} from "@/lib/types";

/** App shell tab id accepted from route state. */
export type AppScreen =
    | "configuration-sources"
    | "workspaces"
    | "branches"
    | "activity";

/** Active branch row paired with workspace metadata for dashboard lists. */
type BranchEntry = { branch: BranchRecord; workspace: WorkspaceRecord };

const SCREEN_TITLES: Record<AppScreen, string> = {
    "configuration-sources": "Configuration Sources",
    workspaces: "Workspaces",
    branches: "Branches",
    activity: "Activity",
};

export function ConsoleScreen({ screen }: { screen: AppScreen }) {
    const [query] = useSearchParams();
    const sourceTreeFilterId = query.get("sourceTree");
    const user = useShellUser();
    const { data, error, loading, reload } =
        useApi<ConsoleData>("/api/console");
    const [workspaceSummaries, setWorkspaceSummaries] = useState<
        Map<string, WorkspaceSummary>
    >(new Map());

    const selectedScreen = screen;

    // The workspaces screen decorates each row with inventory counts; those
    // stage remote sources, so one batched API request keeps browser fan-out
    // and layout shifts under control.
    useEffect(() => {
        if (selectedScreen !== "workspaces" || !data) {
            return;
        }
        let cancelled = false;
        const activeSourceTreeId =
            sourceTreeFilterId &&
            data.sourceTrees.some(
                (sourceTree) => sourceTree.id === sourceTreeFilterId,
            )
                ? sourceTreeFilterId
                : null;
        const visibleWorkspaces = activeSourceTreeId
            ? data.workspaces.filter(
                  (workspace) => workspace.sourceTreeId === activeSourceTreeId,
              )
            : data.workspaces;
        const path = activeSourceTreeId
            ? `/api/workspaces/summaries?sourceTreeId=${encodeURIComponent(activeSourceTreeId)}`
            : "/api/workspaces/summaries";
        setWorkspaceSummaries(new Map());
        void api<WorkspaceSummariesData>(path).then(
            ({ summaries }) => {
                if (!cancelled) {
                    setWorkspaceSummaries(
                        new Map(
                            summaries.map((summary) => [
                                summary.workspaceId,
                                summary,
                            ]),
                        ),
                    );
                }
            },
            (failure: unknown) => {
                if (!cancelled) {
                    const message =
                        failure instanceof Error
                            ? failure.message
                            : String(failure);
                    setWorkspaceSummaries(
                        new Map(
                            visibleWorkspaces.map((workspace) => [
                                workspace.id,
                                {
                                    variables: 0,
                                    qualifiers: 0,
                                    catalogs: 0,
                                    schemas: 0,
                                    error: message,
                                },
                            ]),
                        ),
                    );
                }
            },
        );
        return () => {
            cancelled = true;
        };
    }, [selectedScreen, data, sourceTreeFilterId]);

    if (loading && !data) {
        return <LoadingScreen />;
    }
    if (error || !data) {
        return (
            <main className="fault-page">
                <div className="fault-panel">
                    <span className="label">console</span>
                    <h1>The console data failed to load.</h1>
                    <div className="banner banner-err">
                        <TriangleAlert aria-hidden size={16} />
                        <span>{error ?? "Unknown error."}</span>
                    </div>
                </div>
            </main>
        );
    }

    const sourceTrees = data.sourceTrees;
    const workspaces = data.workspaces;
    const filterSourceTree = sourceTreeFilterId
        ? (sourceTrees.find(
              (sourceTree) => sourceTree.id === sourceTreeFilterId,
          ) ?? null)
        : null;
    const visibleWorkspaces = filterSourceTree
        ? workspaces.filter(
              (workspace) => workspace.sourceTreeId === filterSourceTree.id,
          )
        : workspaces;
    const branches = data.branches;
    const activeBranches = branches.filter(
        ({ branch }) => branch.status === "active",
    );
    const branchesWithPullRequests = branches.filter(
        ({ branch }) => branch.prUrl !== null,
    );

    return (
        <RefreshScope onRefresh={reload}>
            <AppShell
                crumbs={[
                    {
                        label: "console",
                        href:
                            selectedScreen === "configuration-sources"
                                ? undefined
                                : "/app",
                    },
                    ...(filterSourceTree
                        ? [{ label: "workspaces", href: "/app/workspaces" }]
                        : []),
                ]}
                nav={
                    <>
                        <NavGroupLabel>Console</NavGroupLabel>
                        <NavLink
                            active={selectedScreen === "configuration-sources"}
                            count={sourceTrees.length}
                            href={appScreenHref("configuration-sources")}
                            icon={<FolderGit2 aria-hidden size={16} />}
                            label="Configuration Sources"
                        />
                        <NavLink
                            active={selectedScreen === "workspaces"}
                            count={workspaces.length}
                            href={appScreenHref("workspaces")}
                            icon={<Layers aria-hidden size={16} />}
                            label="Workspaces"
                        />
                        <NavLink
                            active={selectedScreen === "branches"}
                            count={branches.length}
                            href={appScreenHref("branches")}
                            icon={<GitBranch aria-hidden size={16} />}
                            label="Branches"
                        />
                        <NavLink
                            active={selectedScreen === "activity"}
                            href={appScreenHref("activity")}
                            icon={<History aria-hidden size={16} />}
                            label="Activity"
                        />
                    </>
                }
                title={SCREEN_TITLES[selectedScreen]}
                user={user}
            >
                {selectedScreen === "configuration-sources" ? (
                    <SourceTreesScreen sourceTrees={sourceTrees} />
                ) : null}
                {selectedScreen === "workspaces" ? (
                    <WorkspacesScreen
                        branches={branches}
                        filterSourceTree={filterSourceTree}
                        workspaceSummaries={workspaceSummaries}
                        workspaces={visibleWorkspaces}
                    />
                ) : null}
                {selectedScreen === "branches" ? (
                    <BranchesScreen branches={branches} />
                ) : null}
                {selectedScreen === "activity" ? (
                    <ActivityScreen
                        branches={branches}
                        activeBranchesCount={activeBranches.length}
                        branchesWithPullRequestsCount={
                            branchesWithPullRequests.length
                        }
                        sourceTreesCount={sourceTrees.length}
                    />
                ) : null}
            </AppShell>
        </RefreshScope>
    );
}

function SourceTreesScreen({
    sourceTrees,
}: {
    sourceTrees: SourceTreeWithWorkspaces[];
}) {
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Configuration Sources</h1>
                <p className="hint">
                    rototo discovers workspaces by scanning a configuration
                    source for{" "}
                    <span className="mono">rototo-workspace.toml</span> files.
                    Register a GitHub repo, local folder, git remote, or archive
                    this console can read.
                </p>
            </div>
            <SourceTreeRegistrationForm />
            {sourceTrees.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <FolderGit2 aria-hidden size={18} />
                    </span>
                    <p>
                        No configuration sources yet. Add one above to discover
                        workspaces.
                    </p>
                </div>
            ) : (
                <SearchableList
                    className="card-grid"
                    emptyLabel="No configuration sources match that search."
                    label="Search configuration sources"
                    placeholder="Search configuration sources"
                >
                    {sourceTrees.map((sourceTree) => (
                        <article
                            className="card source-tree-card"
                            data-search={`${sourceTree.displayName} ${sourceTree.source} ${sourceTree.defaultRevision} ${sourceTreeKindLabel(sourceTree.kind)}`}
                            key={sourceTree.id}
                        >
                            <div className="card-head">
                                <div className="card-head-text">
                                    <h3>
                                        <Link
                                            className="card-stretch"
                                            href={`/app/workspaces?sourceTree=${sourceTree.id}`}
                                            title={`Workspaces in ${sourceTree.displayName}`}
                                        >
                                            {sourceTree.displayName}
                                        </Link>
                                    </h3>
                                    <span className="kv">
                                        <span>
                                            revision{" "}
                                            <span className="mono">
                                                {sourceTree.defaultRevision}
                                            </span>
                                        </span>
                                        <span>
                                            {sourceTreeKindLabel(
                                                sourceTree.kind,
                                            )}
                                        </span>
                                    </span>
                                </div>
                                <span className="card-actions">
                                    {!sourceTree.capabilities.canBranch ? (
                                        <span className="pill pill-neutral">
                                            read-only
                                        </span>
                                    ) : null}
                                    <span className="pill pill-sea">
                                        <span className="d" />
                                        {sourceTree.workspaces.length}{" "}
                                        {sourceTree.workspaces.length === 1
                                            ? "workspace"
                                            : "workspaces"}
                                    </span>
                                    <RefreshSourceTreeButton
                                        sourceTreeId={sourceTree.id}
                                        sourceTreeName={sourceTree.displayName}
                                    />
                                    <RemoveSourceTreeButton
                                        sourceTreeId={sourceTree.id}
                                        sourceTreeName={sourceTree.displayName}
                                    />
                                </span>
                            </div>
                            <div className="kv">
                                <span>
                                    updated {formatDate(sourceTree.updatedAt)}
                                </span>
                                {sourceTree.lastDiscoveredAt ? (
                                    <span>
                                        discovered{" "}
                                        {formatDate(
                                            sourceTree.lastDiscoveredAt,
                                        )}
                                    </span>
                                ) : null}
                            </div>
                        </article>
                    ))}
                </SearchableList>
            )}
        </section>
    );
}

function WorkspacesScreen({
    branches,
    filterSourceTree,
    workspaceSummaries,
    workspaces,
}: {
    branches: BranchEntry[];
    filterSourceTree: SourceTreeWithWorkspaces | null;
    workspaceSummaries: Map<string, WorkspaceSummary>;
    workspaces: WorkspaceRecord[];
}) {
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Workspaces</h1>
                <p className="hint">
                    Each workspace is a{" "}
                    <span className="mono">rototo-workspace.toml</span> root
                    discovered in a registered configuration source. Open one to
                    inspect and edit it.
                </p>
            </div>
            {filterSourceTree ? (
                <div className="action-row">
                    <span className="pill pill-sea">
                        <span className="d" />
                        configuration source: {filterSourceTree.displayName}
                    </span>
                    <Link
                        className="btn btn-ghost btn-sm"
                        href="/app/workspaces"
                    >
                        Clear filter
                    </Link>
                </div>
            ) : null}
            {workspaces.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <Layers aria-hidden size={18} />
                    </span>
                    <p>
                        {filterSourceTree
                            ? `No workspaces discovered in ${filterSourceTree.displayName}. Re-scan it from the configuration sources screen after adding rototo-workspace.toml.`
                            : "Nothing to configure… yet. Register a configuration source to discover workspaces."}
                    </p>
                </div>
            ) : (
                <SearchableList
                    className="row-list"
                    emptyLabel="No workspaces match that search."
                    label="Search workspaces"
                    placeholder="Search workspaces"
                >
                    {workspaces.map((workspace) => (
                        <WorkspaceRow
                            data-search={`${workspace.sourceTreeLabel} ${workspace.path} ${workspace.revision}`}
                            branchesCount={
                                branches.filter(
                                    (entry) =>
                                        entry.workspace.id === workspace.id,
                                ).length
                            }
                            key={workspace.id}
                            summary={workspaceSummaries.get(workspace.id)}
                            workspace={workspace}
                        />
                    ))}
                </SearchableList>
            )}
        </section>
    );
}

function WorkspaceRow({
    branchesCount,
    summary,
    workspace,
    "data-search": dataSearch,
}: {
    branchesCount: number;
    summary: WorkspaceSummary | undefined;
    workspace: WorkspaceRecord;
    "data-search": string;
}) {
    const navigate = useNavigate();
    const [opening, setOpening] = useState(false);
    const href = `/app/workspaces/${workspace.slug}`;

    function openWorkspace(event: MouseEvent<HTMLAnchorElement>) {
        if (!shouldHandleClientNavigation(event)) {
            return;
        }
        event.preventDefault();
        setOpening(true);
        window.requestAnimationFrame(() => navigate(href));
    }

    return (
        <Link
            aria-busy={opening}
            className="row"
            data-loading={opening ? "true" : undefined}
            data-search={dataSearch}
            href={href}
            onClick={openWorkspace}
        >
            <span className="row-icon">
                <Boxes aria-hidden size={16} />
            </span>
            <span className="row-text">
                <span className="row-title mono">{workspace.path}</span>
                <span className="row-sub">{workspace.sourceTreeLabel}</span>
                <span
                    aria-busy={summary ? undefined : true}
                    className="kv workspace-summary-line"
                >
                    {summary ? (
                        summary.error ? (
                            <span>inventory unavailable</span>
                        ) : (
                            <>
                                <span>
                                    {countLabel(summary.variables, "variable")}
                                </span>
                                <span>
                                    {countLabel(
                                        summary.qualifiers,
                                        "qualifier",
                                    )}
                                </span>
                                <span>
                                    {countLabel(summary.catalogs, "catalog")}
                                </span>
                                <span>
                                    {countLabel(summary.schemas, "schema")}
                                </span>
                            </>
                        )
                    ) : (
                        <>
                            <span
                                aria-hidden="true"
                                className="skeleton workspace-summary-chip"
                            />
                            <span
                                aria-hidden="true"
                                className="skeleton workspace-summary-chip"
                            />
                            <span
                                aria-hidden="true"
                                className="skeleton workspace-summary-chip"
                            />
                            <span
                                aria-hidden="true"
                                className="skeleton workspace-summary-chip"
                            />
                        </>
                    )}
                </span>
            </span>
            <span className="row-side">
                {opening ? (
                    <span className="row-loading">
                        <span className="spin" />
                        Opening
                    </span>
                ) : (
                    <>
                        {branchesCount > 0 ? (
                            <span className="pill pill-neutral">
                                {branchesCount}{" "}
                                {branchesCount === 1 ? "branch" : "branches"}
                            </span>
                        ) : null}
                        <ChevronRight aria-hidden className="muted" size={15} />
                    </>
                )}
            </span>
        </Link>
    );
}

function shouldHandleClientNavigation(
    event: MouseEvent<HTMLAnchorElement>,
): boolean {
    return (
        !event.defaultPrevented &&
        event.button === 0 &&
        !event.metaKey &&
        !event.altKey &&
        !event.ctrlKey &&
        !event.shiftKey
    );
}

function BranchesScreen({ branches }: { branches: BranchEntry[] }) {
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Branches</h1>
                <p className="hint">
                    Every branch is a real branch in the workspace configuration
                    source. Edits commit to the branch; publishing opens a pull
                    request.
                </p>
            </div>
            {branches.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <GitBranch aria-hidden size={18} />
                    </span>
                    <p>
                        No branches yet. Open a workspace and start editing to
                        create one.
                    </p>
                </div>
            ) : (
                <SearchableList
                    className="row-list"
                    emptyLabel="No branches match that search."
                    label="Search branches"
                    placeholder="Search branches"
                >
                    {branches.map(({ branch, workspace }) => (
                        <div
                            className="row"
                            data-search={`${workspace.sourceTreeLabel} ${workspace.path} ${branch.branch} ${branch.status} ${branch.prState ?? ""}`}
                            key={branch.id}
                        >
                            <span className="row-icon">
                                <GitBranch aria-hidden size={16} />
                            </span>
                            <span className="row-text">
                                <Link
                                    className="row-title mono row-link"
                                    href={`/app/workspaces/${workspace.slug}/branches/${branch.id}`}
                                >
                                    {branch.branch}
                                </Link>
                                <span className="row-sub">
                                    <Link
                                        href={`/app/workspaces/${workspace.slug}`}
                                    >
                                        {workspace.sourceTreeLabel} ·{" "}
                                        {workspace.path}
                                    </Link>
                                </span>
                            </span>
                            <span className="row-side">
                                <BranchStatusPill branch={branch} />
                                <Link
                                    aria-label={`Open branch ${branch.branch}`}
                                    className="muted"
                                    href={`/app/workspaces/${workspace.slug}/branches/${branch.id}`}
                                >
                                    <ChevronRight aria-hidden size={15} />
                                </Link>
                            </span>
                        </div>
                    ))}
                </SearchableList>
            )}
        </section>
    );
}

function ActivityScreen({
    branches,
    activeBranchesCount,
    branchesWithPullRequestsCount,
    sourceTreesCount,
}: {
    branches: BranchEntry[];
    activeBranchesCount: number;
    branchesWithPullRequestsCount: number;
    sourceTreesCount: number;
}) {
    const recentFirst = [...branches].sort(
        (left, right) =>
            Date.parse(branchUpdatedAt(right.branch)) -
            Date.parse(branchUpdatedAt(left.branch)),
    );
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Activity</h1>
                <p className="hint">
                    Every branch across your workspaces, most recently updated
                    first.
                </p>
            </div>
            <div className="stat-grid">
                <div className="stat-card">
                    <span className="label">active branches</span>
                    <span className="stat-value">{activeBranchesCount}</span>
                </div>
                <div className="stat-card">
                    <span className="label">branches with PRs</span>
                    <span className="stat-value">
                        {branchesWithPullRequestsCount}
                    </span>
                </div>
                <div className="stat-card">
                    <span className="label">configuration sources</span>
                    <span className="stat-value">{sourceTreesCount}</span>
                </div>
            </div>
            {recentFirst.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <History aria-hidden size={18} />
                    </span>
                    <p>
                        No branches yet. Open a workspace and start editing to
                        create one.
                    </p>
                </div>
            ) : (
                <SearchableList
                    className="row-list"
                    emptyLabel="No activity matches that search."
                    label="Search activity"
                    placeholder="Search activity"
                >
                    {recentFirst.map(({ branch, workspace }) => (
                        <div
                            className="row"
                            data-search={`${workspace.sourceTreeLabel} ${workspace.path} ${branch.branch} ${branch.status} ${branch.prUrl ?? ""} ${branch.prState ?? ""}`}
                            key={branch.id}
                        >
                            <span className="row-icon">
                                <History aria-hidden size={16} />
                            </span>
                            <span className="row-text">
                                <Link
                                    className="row-title mono row-link"
                                    href={`/app/workspaces/${workspace.slug}/branches/${branch.id}`}
                                >
                                    {branch.branch}
                                </Link>
                                <span className="row-sub">
                                    <Link
                                        href={`/app/workspaces/${workspace.slug}`}
                                    >
                                        {workspace.path}
                                    </Link>{" "}
                                    · updated{" "}
                                    {formatDate(branchUpdatedAt(branch))}
                                    {branch.prUrl ? (
                                        <>
                                            {" · "}
                                            <a
                                                href={branch.prUrl}
                                                rel="noreferrer"
                                                target="_blank"
                                            >
                                                {branch.prUrl.replace(
                                                    "https://github.com/",
                                                    "",
                                                )}
                                            </a>
                                        </>
                                    ) : null}
                                </span>
                            </span>
                            <span className="row-side">
                                <BranchStatusPill branch={branch} />
                                <Link
                                    aria-label={`Open branch ${branch.branch}`}
                                    className="muted"
                                    href={`/app/workspaces/${workspace.slug}/branches/${branch.id}`}
                                >
                                    <ChevronRight aria-hidden size={15} />
                                </Link>
                            </span>
                        </div>
                    ))}
                </SearchableList>
            )}
        </section>
    );
}

function countLabel(count: number, noun: string): string {
    return `${count} ${count === 1 ? noun : `${noun}s`}`;
}

function appScreenHref(screen: AppScreen): string {
    return `/app/${screen}`;
}

function sourceTreeKindLabel(kind: SourceTreeWithWorkspaces["kind"]): string {
    switch (kind) {
        case "gitHub":
            return "GitHub";
        case "gitRemote":
            return "git remote";
        case "localFolder":
            return "local folder";
        case "archive":
            return "archive";
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
