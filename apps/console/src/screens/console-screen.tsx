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
    ConsoleState,
    SourceTreeWithPackages,
    PackageRecord,
    PackageSummary,
    PackageSummariesData,
} from "@/lib/types";

/** App shell tab id accepted from route state. */
export type AppScreen =
    | "configuration-sources"
    | "packages"
    | "branches"
    | "activity";

/** Active branch row paired with package metadata for dashboard lists. */
type BranchEntry = { branch: BranchRecord; package: PackageRecord };

const SCREEN_TITLES: Record<AppScreen, string> = {
    "configuration-sources": "Configuration Sources",
    packages: "Packages",
    branches: "Branches",
    activity: "Activity",
};

export function ConsoleScreen({ screen }: { screen: AppScreen }) {
    const [query] = useSearchParams();
    const sourceTreeFilterId = query.get("sourceTree");
    const user = useShellUser();
    const { data, error, loading, reload } =
        useApi<ConsoleData>("/api/console");
    const [packageSummaries, setPackageSummaries] = useState<
        Map<string, PackageSummary>
    >(new Map());

    const selectedScreen = screen;

    // The packages screen decorates each row with inventory counts; those
    // stage remote sources, so one batched API request keeps browser fan-out
    // and layout shifts under control.
    useEffect(() => {
        if (selectedScreen !== "packages" || !data) {
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
        const visiblePackages = activeSourceTreeId
            ? data.packages.filter(
                  (pkg) => pkg.sourceTreeId === activeSourceTreeId,
              )
            : data.packages;
        const path = activeSourceTreeId
            ? `/api/packages/summaries?sourceTreeId=${encodeURIComponent(activeSourceTreeId)}`
            : "/api/packages/summaries";
        setPackageSummaries(new Map());
        void api<PackageSummariesData>(path).then(
            ({ summaries }) => {
                if (!cancelled) {
                    setPackageSummaries(
                        new Map(
                            summaries.map((summary) => [
                                summary.packageId,
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
                    setPackageSummaries(
                        new Map(
                            visiblePackages.map((pkg) => [
                                pkg.id,
                                {
                                    variables: 0,
                                    qualifiers: 0,
                                    catalogs: 0,
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
    const packages = data.packages;
    const filterSourceTree = sourceTreeFilterId
        ? (sourceTrees.find(
              (sourceTree) => sourceTree.id === sourceTreeFilterId,
          ) ?? null)
        : null;
    const visiblePackages = filterSourceTree
        ? packages.filter((pkg) => pkg.sourceTreeId === filterSourceTree.id)
        : packages;
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
                        ? [{ label: "packages", href: "/app/packages" }]
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
                            active={selectedScreen === "packages"}
                            count={packages.length}
                            href={appScreenHref("packages")}
                            icon={<Layers aria-hidden size={16} />}
                            label="Packages"
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
                    <SourceTreesScreen
                        consoleState={data.state}
                        sourceTrees={sourceTrees}
                    />
                ) : null}
                {selectedScreen === "packages" ? (
                    <PackagesScreen
                        branches={branches}
                        filterSourceTree={filterSourceTree}
                        packageSummaries={packageSummaries}
                        packages={visiblePackages}
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
    consoleState,
    sourceTrees,
}: {
    consoleState: ConsoleState;
    sourceTrees: SourceTreeWithPackages[];
}) {
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Configuration Sources</h1>
                <p className="hint">
                    {consoleState.fixedPackage
                        ? "This console is scoped to the package source it was started with."
                        : "rototo discovers packages by scanning a configuration source for "}
                    {!consoleState.fixedPackage ? (
                        <>
                            <span className="mono">rototo-package.toml</span>{" "}
                            files. Register a GitHub repo, local folder, git
                            remote, or archive this console can read.
                        </>
                    ) : null}
                </p>
            </div>
            {consoleState.canManageSourceTrees ? (
                <SourceTreeRegistrationForm />
            ) : null}
            {sourceTrees.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <FolderGit2 aria-hidden size={18} />
                    </span>
                    <p>
                        {consoleState.fixedPackage
                            ? "No packages were discovered in the startup configuration source."
                            : "No configuration sources yet. Add one above to discover packages."}
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
                                            href={`/app/packages?sourceTree=${sourceTree.id}`}
                                            title={`Packages in ${sourceTree.displayName}`}
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
                                        {sourceTree.packages.length}{" "}
                                        {sourceTree.packages.length === 1
                                            ? "package"
                                            : "packages"}
                                    </span>
                                    <RefreshSourceTreeButton
                                        sourceTreeId={sourceTree.id}
                                        sourceTreeName={sourceTree.displayName}
                                    />
                                    {consoleState.canManageSourceTrees ? (
                                        <RemoveSourceTreeButton
                                            sourceTreeId={sourceTree.id}
                                            sourceTreeName={
                                                sourceTree.displayName
                                            }
                                        />
                                    ) : null}
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

function PackagesScreen({
    branches,
    filterSourceTree,
    packageSummaries,
    packages,
}: {
    branches: BranchEntry[];
    filterSourceTree: SourceTreeWithPackages | null;
    packageSummaries: Map<string, PackageSummary>;
    packages: PackageRecord[];
}) {
    return (
        <section className="section">
            <div className="section-header-text">
                <h1>Packages</h1>
                <p className="hint">
                    Each package is a{" "}
                    <span className="mono">rototo-package.toml</span> root
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
                    <Link className="btn btn-ghost btn-sm" href="/app/packages">
                        Clear filter
                    </Link>
                </div>
            ) : null}
            {packages.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <Layers aria-hidden size={18} />
                    </span>
                    <p>
                        {filterSourceTree
                            ? `No packages discovered in ${filterSourceTree.displayName}. Re-scan it from the configuration sources screen after adding rototo-package.toml.`
                            : "Nothing to configure… yet. Register a configuration source to discover packages."}
                    </p>
                </div>
            ) : (
                <SearchableList
                    className="row-list"
                    emptyLabel="No packages match that search."
                    label="Search packages"
                    placeholder="Search packages"
                >
                    {packages.map((pkg) => (
                        <PackageRow
                            data-search={`${pkg.sourceTreeLabel} ${pkg.displayPath} ${pkg.path} ${pkg.revision}`}
                            branchesCount={
                                branches.filter(
                                    (entry) => entry.package.id === pkg.id,
                                ).length
                            }
                            key={pkg.id}
                            summary={packageSummaries.get(pkg.id)}
                            pkg={pkg}
                        />
                    ))}
                </SearchableList>
            )}
        </section>
    );
}

function PackageRow({
    branchesCount,
    summary,
    pkg,
    "data-search": dataSearch,
}: {
    branchesCount: number;
    summary: PackageSummary | undefined;
    pkg: PackageRecord;
    "data-search": string;
}) {
    const navigate = useNavigate();
    const [opening, setOpening] = useState(false);
    const href = `/app/packages/${pkg.slug}`;

    function openPackage(event: MouseEvent<HTMLAnchorElement>) {
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
            onClick={openPackage}
        >
            <span className="row-icon">
                <Boxes aria-hidden size={16} />
            </span>
            <span className="row-text">
                <span className="row-title mono">{pkg.displayPath}</span>
                <span className="row-sub">{pkg.sourceTreeLabel}</span>
                <span
                    aria-busy={summary ? undefined : true}
                    className="kv package-summary-line"
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
                            </>
                        )
                    ) : (
                        <>
                            <span
                                aria-hidden="true"
                                className="skeleton package-summary-chip"
                            />
                            <span
                                aria-hidden="true"
                                className="skeleton package-summary-chip"
                            />
                            <span
                                aria-hidden="true"
                                className="skeleton package-summary-chip"
                            />
                            <span
                                aria-hidden="true"
                                className="skeleton package-summary-chip"
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
                    Every branch is a real branch in the package configuration
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
                        No branches yet. Open a package and start editing to
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
                    {branches.map(({ branch, package: pkg }) => (
                        <div
                            className="row"
                            data-search={`${pkg.sourceTreeLabel} ${pkg.displayPath} ${pkg.path} ${branch.branch} ${branch.status} ${branch.prState ?? ""}`}
                            key={branch.id}
                        >
                            <span className="row-icon">
                                <GitBranch aria-hidden size={16} />
                            </span>
                            <span className="row-text">
                                <Link
                                    className="row-title mono row-link"
                                    href={`/app/packages/${pkg.slug}/branches/${branch.id}`}
                                >
                                    {branch.branch}
                                </Link>
                                <span className="row-sub">
                                    <Link href={`/app/packages/${pkg.slug}`}>
                                        {pkg.sourceTreeLabel} ·{" "}
                                        {pkg.displayPath}
                                    </Link>
                                </span>
                            </span>
                            <span className="row-side">
                                <BranchStatusPill branch={branch} />
                                <Link
                                    aria-label={`Open branch ${branch.branch}`}
                                    className="muted"
                                    href={`/app/packages/${pkg.slug}/branches/${branch.id}`}
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
                    Every branch across your packages, most recently updated
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
                        No branches yet. Open a package and start editing to
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
                    {recentFirst.map(({ branch, package: pkg }) => (
                        <div
                            className="row"
                            data-search={`${pkg.sourceTreeLabel} ${pkg.displayPath} ${pkg.path} ${branch.branch} ${branch.status} ${branch.prUrl ?? ""} ${branch.prState ?? ""}`}
                            key={branch.id}
                        >
                            <span className="row-icon">
                                <History aria-hidden size={16} />
                            </span>
                            <span className="row-text">
                                <Link
                                    className="row-title mono row-link"
                                    href={`/app/packages/${pkg.slug}/branches/${branch.id}`}
                                >
                                    {branch.branch}
                                </Link>
                                <span className="row-sub">
                                    <Link href={`/app/packages/${pkg.slug}`}>
                                        {pkg.displayPath}
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
                                    href={`/app/packages/${pkg.slug}/branches/${branch.id}`}
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

function sourceTreeKindLabel(kind: SourceTreeWithPackages["kind"]): string {
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
