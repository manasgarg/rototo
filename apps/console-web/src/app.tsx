// The shell: one information hierarchy (tree -> package -> entity) made
// visible three ways at once — the URL (lib/router.ts owns the grammar),
// the left nav (scope pickers plus scoped sections), and the breadcrumbs
// (clickable prefixes of the path). View state (change set, pin, context)
// renders as topbar chips, not crumbs, because it parameterizes every
// level rather than naming one. What a user can do is decided server-side;
// everything rendered here is explanation.

import { useCallback, useEffect, useState } from "react";

import { RototoMark } from "@/components/rototo-mark";
import {
    fetchMe,
    listPackages,
    type MeResponse,
    type SourceTreeSummary,
} from "@/lib/api";
import {
    adminUrl,
    changeSetUrl,
    changesUrl,
    CLASS_LABELS,
    changesActive,
    homeUrl,
    navigate,
    packageUrl,
    parseHash,
    treeUrl,
    useHashPath,
    type PackageView,
    type Route,
    type ViewState,
} from "@/lib/router";
import { AdminPage } from "@/pages/admin";
import { ChangeSetPage, ChangesPage } from "@/pages/changes";
import { SurfacesPage } from "@/pages/surfaces";
import { TreeHomePage } from "@/pages/tree";
import { WorkbenchPage } from "@/pages/workbench";

export function App() {
    const [me, setMe] = useState<MeResponse | null>(null);
    const [error, setError] = useState<string | null>(null);
    const { route, state } = parseHash(useHashPath());

    // The nav renders from /api/me, so anything that changes the caller's
    // capabilities (registering a source tree, granting) re-fetches it.
    const refreshMe = useCallback(() => {
        fetchMe().then(setMe, (err: Error) => setError(err.message));
    }, []);
    useEffect(() => {
        refreshMe();
    }, [refreshMe]);

    const trees = me?.capabilities?.sourceTrees ?? [];
    const routeTreeId = "treeId" in route ? route.treeId : null;
    const treeId = routeTreeId ?? trees[0]?.id ?? null;

    // Packages of the tree in scope, for the nav picker and section links.
    // This listing is ref-agnostic (default branch); pages fetch their own
    // branch-aware listings.
    const [navPackages, setNavPackages] = useState<{
        treeId: string;
        paths: string[];
    } | null>(null);
    useEffect(() => {
        if (treeId === null || me?.capabilities == null) {
            setNavPackages(null);
            return;
        }
        let stale = false;
        listPackages(treeId).then(
            (response) => {
                if (!stale) {
                    setNavPackages({
                        treeId,
                        paths: response.packages.map((entry) => entry.path),
                    });
                }
            },
            () => {
                if (!stale) {
                    setNavPackages(null);
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, me]);

    const packages =
        navPackages !== null && navPackages.treeId === treeId
            ? navPackages.paths
            : null;
    const packagePath =
        route.page === "package" ? route.packagePath : (packages?.[0] ?? null);
    // Links into the package the user is already inside keep the view
    // state; links that change scope start clean.
    const packageState =
        route.page === "package" && route.packagePath === packagePath
            ? state
            : undefined;
    const packageHref = (view: PackageView): string | null =>
        treeId === null || packagePath === null
            ? null
            : packageUrl(treeId, packagePath, view, packageState);
    const view = route.page === "package" ? route.view : null;
    const addressClass =
        view?.kind === "address" ? (view.steps[0]?.class ?? null) : null;

    return (
        <div className="shell">
            <aside className="sidebar">
                <a className="brand" href="#/">
                    <span className="brand-mark">
                        <RototoMark />
                    </span>
                    <span className="brand-name">rototo</span>
                </a>
                <nav className="side-nav">
                    {trees.length > 0 && treeId !== null ? (
                        <div className="nav-scope">
                            <select
                                className="input"
                                title="Source tree"
                                value={treeId}
                                onChange={(event) =>
                                    navigate(treeUrl(event.target.value))
                                }
                            >
                                {trees.map((tree) => (
                                    <option key={tree.id} value={tree.id}>
                                        {treeLabel(tree)}
                                    </option>
                                ))}
                            </select>
                            {packages !== null &&
                            packages.length > 1 &&
                            packagePath !== null ? (
                                <select
                                    className="input"
                                    title="Package"
                                    value={packagePath}
                                    onChange={(event) =>
                                        navigate(
                                            packageUrl(
                                                treeId,
                                                event.target.value,
                                                { kind: "overview" },
                                                // The change set and pin are
                                                // tree-scoped and survive the
                                                // move; the chosen context is
                                                // package-scoped and does not.
                                                route.page === "package"
                                                    ? {
                                                          ...state,
                                                          context: null,
                                                      }
                                                    : undefined,
                                            ),
                                        )
                                    }
                                >
                                    {packages.map((path) => (
                                        <option key={path} value={path}>
                                            {path === "." ? "(root)" : path}
                                        </option>
                                    ))}
                                </select>
                            ) : null}
                        </div>
                    ) : null}
                    {packagePath !== null ? (
                        <>
                            <div className="label nav-group-label">Package</div>
                            <NavItem
                                label="Overview"
                                on={view?.kind === "overview"}
                                to={packageHref({ kind: "overview" })}
                            />
                            <NavItem
                                label="Surfaces"
                                on={view?.kind === "surfaces"}
                                to={packageHref({
                                    kind: "surfaces",
                                    surfaceId: null,
                                })}
                            />
                            <NavItem
                                label="Variables"
                                on={addressClass === "variable"}
                                to={packageHref(collection("variable"))}
                            />
                            <NavItem
                                label="Catalogs"
                                on={addressClass === "catalog"}
                                to={packageHref(collection("catalog"))}
                            />
                            <NavItem
                                label="Lists"
                                on={addressClass === "list"}
                                to={packageHref(collection("list"))}
                            />
                            <NavItem
                                label="Contexts"
                                on={addressClass === "evaluation-context"}
                                to={packageHref(
                                    collection("evaluation-context"),
                                )}
                            />
                            <NavItem
                                label="Files"
                                on={view?.kind === "files"}
                                to={packageHref({ kind: "files", file: null })}
                            />
                            <NavItem
                                label="History"
                                on={view?.kind === "history"}
                                to={packageHref({ kind: "history" })}
                            />
                        </>
                    ) : null}
                    {treeId !== null ? (
                        <>
                            <div className="label nav-group-label">Tree</div>
                            <NavItem
                                label="Change sets"
                                on={changesActive(route)}
                                to={changesUrl(treeId)}
                            />
                        </>
                    ) : null}
                    {me?.capabilities?.deployment.administer.allow === true ? (
                        <>
                            <div className="label nav-group-label">Console</div>
                            <NavItem
                                label="Admin"
                                on={route.page === "admin"}
                                to={adminUrl()}
                            />
                        </>
                    ) : null}
                </nav>
                <SideUser me={me} />
            </aside>
            <div className="main">
                <header className="topbar">
                    <a
                        className="topbar-brand"
                        href="#/"
                        title="rototo console"
                    >
                        <RototoMark size={24} />
                    </a>
                    <Crumbs route={route} state={state} trees={trees} />
                    <div className="topbar-actions">
                        {state.context !== null ? (
                            <span
                                className="pill pill-neutral mono"
                                title="The chosen context; every execution view resolves under it"
                            >
                                {state.context}
                            </span>
                        ) : null}
                        {state.changeSetId !== null ? (
                            <span
                                className="pill pill-sea mono"
                                title="Edits accumulate on this change set"
                            >
                                {state.changeSetId}
                            </span>
                        ) : null}
                        {state.pin !== null ? (
                            <span
                                className="pill pill-info mono"
                                title="Viewing this historical pin; editing is off"
                            >
                                @{state.pin.slice(0, 10)}
                            </span>
                        ) : null}
                    </div>
                </header>
                <main className="content">
                    <div className="content-inner">
                        {error !== null ? (
                            <div className="card">
                                <h1>Console server unreachable</h1>
                                <p className="hint">
                                    {error}. Start it with{" "}
                                    <span className="mono">
                                        npm --prefix apps/console-server run dev
                                    </span>
                                    .
                                </p>
                            </div>
                        ) : me === null ? (
                            <p className="muted">Loading…</p>
                        ) : route.page === "package" ? (
                            route.view.kind === "surfaces" ? (
                                <SurfacesPage
                                    me={me}
                                    treeId={route.treeId}
                                    packagePath={route.packagePath}
                                    surfaceId={route.view.surfaceId}
                                    state={state}
                                />
                            ) : (
                                <WorkbenchPage
                                    me={me}
                                    treeId={route.treeId}
                                    packagePath={route.packagePath}
                                    view={route.view}
                                    state={state}
                                />
                            )
                        ) : route.page === "tree" ? (
                            <TreeHomePage
                                me={me}
                                treeId={route.treeId}
                                packages={
                                    navPackages?.treeId === route.treeId
                                        ? navPackages.paths
                                        : null
                                }
                            />
                        ) : route.page === "changes" ? (
                            <ChangesPage me={me} treeId={route.treeId} />
                        ) : route.page === "change-set" ? (
                            <ChangeSetPage id={route.changeSetId} me={me} />
                        ) : route.page === "admin" ? (
                            <AdminPage
                                me={me}
                                onCapabilitiesChanged={refreshMe}
                            />
                        ) : route.page === "not-enrolled" ? (
                            <NotEnrolled />
                        ) : (
                            <Home me={me} />
                        )}
                    </div>
                </main>
            </div>
        </div>
    );
}

function collection(className: string): PackageView {
    return { kind: "address", steps: [{ class: className, id: "" }] };
}

function treeLabel(tree: SourceTreeSummary): string {
    return tree.kind === "github" ? `${tree.owner}/${tree.name}` : tree.id;
}

function NavItem({
    label,
    on,
    to,
}: {
    label: string;
    on?: boolean;
    to: string | null;
}) {
    if (to === null) {
        return (
            <span className="nav-item" aria-disabled="true">
                <span className="nav-item-text">{label}</span>
            </span>
        );
    }
    return (
        <button
            className="nav-item"
            data-on={on === true ? "true" : undefined}
            onClick={() => navigate(to)}
        >
            <span className="nav-item-text">{label}</span>
        </button>
    );
}

// Breadcrumbs are the URL, humanized: each crumb is a clickable prefix of
// the containment path, and the last one is where you stand.
type Crumb = { label: string; to: string | null; mono?: boolean };

function Crumbs({
    route,
    state,
    trees,
}: {
    route: Route;
    state: ViewState;
    trees: SourceTreeSummary[];
}) {
    const parts = crumbsFor(route, state, trees);
    return (
        <div className="crumbs">
            {parts.map((part, index) => {
                const last = index === parts.length - 1;
                const className = `label${part.mono === true ? " mono" : ""}`;
                return (
                    <span key={index} className="crumb">
                        {index > 0 ? (
                            <span className="crumb-sep">/</span>
                        ) : null}
                        {last || part.to === null ? (
                            <span className={className}>{part.label}</span>
                        ) : (
                            <a className={className} href={`#${part.to}`}>
                                {part.label}
                            </a>
                        )}
                    </span>
                );
            })}
        </div>
    );
}

function crumbsFor(
    route: Route,
    state: ViewState,
    trees: SourceTreeSummary[],
): Crumb[] {
    const parts: Crumb[] = [{ label: "Home", to: homeUrl() }];
    if (route.page === "admin") {
        parts.push({ label: "Admin", to: null });
        return parts;
    }
    if (!("treeId" in route)) {
        return parts;
    }
    const tree = trees.find((candidate) => candidate.id === route.treeId);
    parts.push({
        label: tree !== undefined ? treeLabel(tree) : route.treeId,
        to: treeUrl(route.treeId),
        mono: true,
    });
    if (route.page === "changes") {
        parts.push({ label: "Change sets", to: null });
    } else if (route.page === "change-set") {
        parts.push({ label: "Change sets", to: changesUrl(route.treeId) });
        parts.push({
            label: route.changeSetId,
            to: changeSetUrl(route.treeId, route.changeSetId),
            mono: true,
        });
    } else if (route.page === "package") {
        const at = (view: PackageView): string =>
            packageUrl(route.treeId, route.packagePath, view, state);
        parts.push({
            label: route.packagePath === "." ? "package" : route.packagePath,
            to: at({ kind: "overview" }),
            mono: route.packagePath !== ".",
        });
        const view = route.view;
        if (view.kind === "surfaces") {
            parts.push({
                label: "Surfaces",
                to: at({ kind: "surfaces", surfaceId: null }),
            });
            if (view.surfaceId !== null) {
                parts.push({ label: view.surfaceId, to: null, mono: true });
            }
        } else if (view.kind === "files") {
            parts.push({
                label: "Files",
                to: at({ kind: "files", file: null }),
            });
            if (view.file !== null) {
                parts.push({ label: view.file, to: null, mono: true });
            }
        } else if (view.kind === "history") {
            parts.push({ label: "History", to: null });
        } else if (view.kind === "address") {
            view.steps.forEach((step, index) => {
                if (index === 0) {
                    parts.push({
                        label: CLASS_LABELS[step.class] ?? step.class,
                        to: at({
                            kind: "address",
                            steps: [{ class: step.class, id: "" }],
                        }),
                    });
                }
                if (step.id !== "") {
                    parts.push({
                        label: step.id,
                        to: at({
                            kind: "address",
                            steps: view.steps.slice(0, index + 1),
                        }),
                        mono: true,
                    });
                }
            });
        }
    }
    return parts;
}

function SideUser({ me }: { me: MeResponse | null }) {
    if (me?.principal == null) {
        return null;
    }
    const initials = me.principal.displayName.slice(0, 2);
    return (
        <div className="side-user">
            <span className="avatar-fallback">{initials}</span>
            <span className="side-user-name">{me.principal.displayName}</span>
        </div>
    );
}

function NotEnrolled() {
    return (
        <div className="card">
            <h1>Not enrolled</h1>
            <p className="hint">
                You signed in, but this identity is not enrolled here.
                Completing authentication never grants access by itself; ask an
                administrator for an invitation, then open its link.
            </p>
        </div>
    );
}

function Home({ me }: { me: MeResponse }) {
    if (me.principal === null) {
        return (
            <div className="card">
                <h1>Sign in</h1>
                <p className="hint">
                    This console runs in team mode; what you can see and do is
                    decided per person.
                </p>
                <div className="action-row">
                    {me.signIn?.oidc != null ? (
                        <a
                            className="btn btn-primary"
                            href="/api/auth/oidc/start"
                        >
                            Sign in with {me.signIn.oidc.displayName}
                        </a>
                    ) : null}
                    {me.signIn?.github ? (
                        <a
                            className={`btn ${me.signIn?.oidc != null ? "btn-secondary" : "btn-primary"}`}
                            href="/api/auth/github/start"
                        >
                            Sign in with GitHub
                        </a>
                    ) : null}
                </div>
            </div>
        );
    }
    const trees = me.capabilities?.sourceTrees ?? [];
    if (trees.length === 0) {
        return (
            <div className="card">
                <h1>Nothing here yet</h1>
                <p className="hint">
                    The shared home fills up as source trees are registered and
                    packages are discovered. An administrator registers a GitHub
                    repository from the Admin page (or through the API) and it
                    appears here with what you can do to it.
                </p>
            </div>
        );
    }
    const hasGithubCredential = (me.identities ?? []).some(
        (identity) =>
            identity.provider === "github" && identity.hasCredential === true,
    );
    return (
        <div className="section">
            <div className="section-header-text">
                <h1>Source trees</h1>
                <p className="hint">
                    What you can do here is decided server-side; these pills
                    only explain it.
                </p>
            </div>
            {trees.map((tree) => (
                <SourceTreeCard key={tree.id} tree={tree} />
            ))}
            {!hasGithubCredential && me.signIn?.github ? (
                <div className="card">
                    <p className="hint">
                        Writes act through the console's GitHub App on your
                        behalf. Linking your own GitHub account makes commits
                        yours directly.{" "}
                        <a
                            className="pill-link"
                            href="/api/auth/github/start?link=1"
                        >
                            Link GitHub
                        </a>
                    </p>
                </div>
            ) : null}
        </div>
    );
}

function SourceTreeCard({ tree }: { tree: SourceTreeSummary }) {
    const verbs = ["view", "propose", "approve", "administer"] as const;
    return (
        <div className="card">
            <div className="card-head">
                <span className="mono">{treeLabel(tree)}</span>
                <span className="card-actions">
                    <button
                        className="btn btn-secondary btn-sm"
                        onClick={() => navigate(treeUrl(tree.id))}
                    >
                        Open
                    </button>
                    <button
                        className="btn btn-ghost btn-sm"
                        onClick={() => navigate(changesUrl(tree.id))}
                    >
                        Change sets
                    </button>
                </span>
            </div>
            <div>
                {verbs.map((verb) => {
                    const decision = tree.capabilities[verb];
                    return (
                        <span
                            key={verb}
                            className={`pill ${decision.allow ? "pill-ok" : "pill-neutral"}`}
                            title={decision.reason}
                        >
                            {verb}
                        </span>
                    );
                })}
            </div>
        </div>
    );
}
