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
import { RemoveRepoButton } from "@/components/remove-repo-button";
import { RepoRegistrationForm } from "@/components/repo-registration-form";
import { SearchableList } from "@/components/searchable-list";
import { DraftStatusPill } from "@/components/status-pills";
import { api, useApi } from "@/lib/api";
import { Link } from "@/lib/link";
import { useShellUser } from "@/lib/me";
import { RefreshScope } from "@/lib/refresh";
import type {
    ConsoleData,
    DraftSessionRecord,
    RepoWithWorkspaces,
    WorkspaceRecord,
    WorkspaceSummary,
    WorkspaceSummariesData,
} from "@/lib/types";

export type AppScreen = "repositories" | "workspaces" | "drafts" | "activity";

type DraftEntry = { draft: DraftSessionRecord; workspace: WorkspaceRecord };

const SCREEN_TITLES: Record<AppScreen, string> = {
    repositories: "Repositories",
    workspaces: "Workspaces",
    drafts: "Drafts",
    activity: "Activity",
};

export function ConsoleScreen({ screen }: { screen: AppScreen }) {
    const [query] = useSearchParams();
    const repoFilterId = query.get("repo");
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
        const activeRepoId =
            repoFilterId && data.repos.some((repo) => repo.id === repoFilterId)
                ? repoFilterId
                : null;
        const visibleWorkspaces = activeRepoId
            ? data.workspaces.filter(
                  (workspace) => workspace.repoId === activeRepoId,
              )
            : data.workspaces;
        const path = activeRepoId
            ? `/api/workspaces/summaries?repoId=${encodeURIComponent(activeRepoId)}`
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
    }, [selectedScreen, data, repoFilterId]);

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

    const repos = data.repos;
    const workspaces = data.workspaces;
    const filterRepo = repoFilterId
        ? (repos.find((repo) => repo.id === repoFilterId) ?? null)
        : null;
    const visibleWorkspaces = filterRepo
        ? workspaces.filter((workspace) => workspace.repoId === filterRepo.id)
        : workspaces;
    const drafts = data.drafts;
    const openDrafts = drafts.filter(({ draft }) => draft.status === "open");
    const publishedDrafts = drafts.filter(
        ({ draft }) => draft.status === "published",
    );

    return (
        <RefreshScope onRefresh={reload}>
            <AppShell
                crumbs={[
                    {
                        label: "console",
                        href:
                            selectedScreen === "repositories"
                                ? undefined
                                : "/app",
                    },
                    ...(filterRepo
                        ? [{ label: "workspaces", href: "/app/workspaces" }]
                        : []),
                ]}
                nav={
                    <>
                        <NavGroupLabel>Console</NavGroupLabel>
                        <NavLink
                            active={selectedScreen === "repositories"}
                            count={repos.length}
                            href={appScreenHref("repositories")}
                            icon={<FolderGit2 aria-hidden size={16} />}
                            label="Repositories"
                        />
                        <NavLink
                            active={selectedScreen === "workspaces"}
                            count={workspaces.length}
                            href={appScreenHref("workspaces")}
                            icon={<Layers aria-hidden size={16} />}
                            label="Workspaces"
                        />
                        <NavLink
                            active={selectedScreen === "drafts"}
                            count={drafts.length}
                            href={appScreenHref("drafts")}
                            icon={<GitBranch aria-hidden size={16} />}
                            label="Drafts"
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
                {selectedScreen === "repositories" ? (
                    <RepositoriesScreen repos={repos} />
                ) : null}
                {selectedScreen === "workspaces" ? (
                    <WorkspacesScreen
                        drafts={drafts}
                        filterRepo={filterRepo}
                        workspaceSummaries={workspaceSummaries}
                        workspaces={visibleWorkspaces}
                    />
                ) : null}
                {selectedScreen === "drafts" ? (
                    <DraftsScreen drafts={drafts} />
                ) : null}
                {selectedScreen === "activity" ? (
                    <ActivityScreen
                        drafts={drafts}
                        openDraftsCount={openDrafts.length}
                        publishedDraftsCount={publishedDrafts.length}
                        reposCount={repos.length}
                    />
                ) : null}
            </AppShell>
        </RefreshScope>
    );
}

function RepositoriesScreen({ repos }: { repos: RepoWithWorkspaces[] }) {
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Repositories</h1>
                <p className="hint">
                    rototo discovers workspaces by scanning a repository for{" "}
                    <span className="mono">rototo-workspace.toml</span> files.
                    Register a repository your GitHub account can read.
                </p>
            </div>
            <RepoRegistrationForm />
            {repos.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <FolderGit2 aria-hidden size={18} />
                    </span>
                    <p>
                        No repositories yet. Add one above to discover
                        workspaces.
                    </p>
                </div>
            ) : (
                <SearchableList
                    className="card-grid"
                    emptyLabel="No repositories match that search."
                    label="Search repositories"
                    placeholder="Search repositories"
                >
                    {repos.map((repo) => (
                        <article
                            className="card repo-card"
                            data-search={`${repo.owner}/${repo.name} ${repo.defaultRef}`}
                            key={repo.id}
                        >
                            <div className="card-head">
                                <div className="card-head-text">
                                    <h3>
                                        <Link
                                            className="card-stretch"
                                            href={`/app/workspaces?repo=${repo.id}`}
                                            title={`Workspaces in ${repo.owner}/${repo.name}`}
                                        >
                                            {repo.owner}/{repo.name}
                                        </Link>
                                    </h3>
                                    <span className="kv">
                                        <span>
                                            ref{" "}
                                            <span className="mono">
                                                {repo.defaultRef}
                                            </span>
                                        </span>
                                    </span>
                                </div>
                                <span className="card-actions">
                                    <span className="pill pill-sea">
                                        <span className="d" />
                                        {repo.workspaces.length}{" "}
                                        {repo.workspaces.length === 1
                                            ? "workspace"
                                            : "workspaces"}
                                    </span>
                                    <RemoveRepoButton
                                        repoId={repo.id}
                                        repoName={`${repo.owner}/${repo.name}`}
                                    />
                                </span>
                            </div>
                            <div className="kv">
                                <span>
                                    updated {formatDate(repo.updatedAt)}
                                </span>
                                {repo.lastDiscoveredAt ? (
                                    <span>
                                        discovered{" "}
                                        {formatDate(repo.lastDiscoveredAt)}
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
    drafts,
    filterRepo,
    workspaceSummaries,
    workspaces,
}: {
    drafts: DraftEntry[];
    filterRepo: RepoWithWorkspaces | null;
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
                    discovered in a registered repository. Open one to inspect
                    and edit it.
                </p>
            </div>
            {filterRepo ? (
                <div className="action-row">
                    <span className="pill pill-sea">
                        <span className="d" />
                        repository: {filterRepo.owner}/{filterRepo.name}
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
                        {filterRepo
                            ? `No workspaces discovered in ${filterRepo.owner}/${filterRepo.name}. Re-scan it from the repositories screen after adding rototo-workspace.toml.`
                            : "Nothing to configure… yet. Register a repository to discover workspaces."}
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
                            data-search={`${workspace.owner}/${workspace.name} ${workspace.path} ${workspace.ref}`}
                            draftsCount={
                                drafts.filter(
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
    draftsCount,
    summary,
    workspace,
    "data-search": dataSearch,
}: {
    draftsCount: number;
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
                <span className="row-sub">
                    {workspace.owner}/{workspace.name}
                </span>
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
                        {draftsCount > 0 ? (
                            <span className="pill pill-neutral">
                                {draftsCount}{" "}
                                {draftsCount === 1 ? "draft" : "drafts"}
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

function DraftsScreen({ drafts }: { drafts: DraftEntry[] }) {
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Drafts</h1>
                <p className="hint">
                    Every draft is a real branch in the workspace repository.
                    Edits commit to the branch; publishing opens a pull request.
                </p>
            </div>
            {drafts.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <GitBranch aria-hidden size={18} />
                    </span>
                    <p>
                        No draft branches yet. Open a workspace and start
                        editing to create one.
                    </p>
                </div>
            ) : (
                <SearchableList
                    className="row-list"
                    emptyLabel="No drafts match that search."
                    label="Search drafts"
                    placeholder="Search drafts"
                >
                    {drafts.map(({ draft, workspace }) => (
                        <div
                            className="row"
                            data-search={`${workspace.owner}/${workspace.name} ${workspace.path} ${draft.branch} ${draft.status} ${draft.prState ?? ""}`}
                            key={draft.id}
                        >
                            <span className="row-icon">
                                <GitBranch aria-hidden size={16} />
                            </span>
                            <span className="row-text">
                                <Link
                                    className="row-title mono row-link"
                                    href={`/app/workspaces/${workspace.slug}/drafts/${draft.id}`}
                                >
                                    {draft.branch}
                                </Link>
                                <span className="row-sub">
                                    <Link
                                        href={`/app/workspaces/${workspace.slug}`}
                                    >
                                        {workspace.owner}/{workspace.name} ·{" "}
                                        {workspace.path}
                                    </Link>
                                </span>
                            </span>
                            <span className="row-side">
                                <DraftStatusPill draft={draft} />
                                <Link
                                    aria-label={`Open draft ${draft.branch}`}
                                    className="muted"
                                    href={`/app/workspaces/${workspace.slug}/drafts/${draft.id}`}
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
    drafts,
    openDraftsCount,
    publishedDraftsCount,
    reposCount,
}: {
    drafts: DraftEntry[];
    openDraftsCount: number;
    publishedDraftsCount: number;
    reposCount: number;
}) {
    const recentFirst = [...drafts].sort(
        (left, right) =>
            Date.parse(right.draft.updatedAt) -
            Date.parse(left.draft.updatedAt),
    );
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Activity</h1>
                <p className="hint">
                    Every draft across your workspaces, most recently updated
                    first.
                </p>
            </div>
            <div className="stat-grid">
                <div className="stat-card">
                    <span className="label">open drafts</span>
                    <span className="stat-value">{openDraftsCount}</span>
                </div>
                <div className="stat-card">
                    <span className="label">published drafts</span>
                    <span className="stat-value">{publishedDraftsCount}</span>
                </div>
                <div className="stat-card">
                    <span className="label">repositories</span>
                    <span className="stat-value">{reposCount}</span>
                </div>
            </div>
            {recentFirst.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <History aria-hidden size={18} />
                    </span>
                    <p>
                        No drafts yet. Open a workspace and start editing to
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
                    {recentFirst.map(({ draft, workspace }) => (
                        <div
                            className="row"
                            data-search={`${workspace.owner}/${workspace.name} ${workspace.path} ${draft.branch} ${draft.status} ${draft.prUrl ?? ""} ${draft.prState ?? ""}`}
                            key={draft.id}
                        >
                            <span className="row-icon">
                                <History aria-hidden size={16} />
                            </span>
                            <span className="row-text">
                                <Link
                                    className="row-title mono row-link"
                                    href={`/app/workspaces/${workspace.slug}/drafts/${draft.id}`}
                                >
                                    {draft.branch}
                                </Link>
                                <span className="row-sub">
                                    <Link
                                        href={`/app/workspaces/${workspace.slug}`}
                                    >
                                        {workspace.path}
                                    </Link>{" "}
                                    · updated {formatDate(draft.updatedAt)}
                                    {draft.prUrl ? (
                                        <>
                                            {" · "}
                                            <a
                                                href={draft.prUrl}
                                                rel="noreferrer"
                                                target="_blank"
                                            >
                                                {draft.prUrl.replace(
                                                    "https://github.com/",
                                                    "",
                                                )}
                                            </a>
                                        </>
                                    ) : null}
                                </span>
                            </span>
                            <span className="row-side">
                                <DraftStatusPill draft={draft} />
                                <Link
                                    aria-label={`Open draft ${draft.branch}`}
                                    className="muted"
                                    href={`/app/workspaces/${workspace.slug}/drafts/${draft.id}`}
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
    return screen === "repositories" ? "/app" : `/app/${screen}`;
}

function formatDate(value: string): string {
    return new Intl.DateTimeFormat("en", {
        dateStyle: "medium",
        timeStyle: "short",
    }).format(new Date(value));
}
