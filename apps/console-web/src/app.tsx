// The shared home plus its lenses: Domain is the surfaces floor, Changes is
// the change-set list with the three-delta review, History browses any pin,
// Model is the workbench. What a user can do is decided server-side;
// everything rendered here is explanation.

import { useEffect, useState } from "react";

import { RototoMark } from "@/components/rototo-mark";
import { fetchMe, type MeResponse, type SourceTreeSummary } from "@/lib/api";
import { navigate, useHashPath } from "@/lib/router";
import { ChangeSetPage, ChangesPage } from "@/pages/changes";
import { SurfacesPage } from "@/pages/surfaces";
import { WorkbenchPage } from "@/pages/workbench";

type Route =
    | { page: "home" }
    | { page: "workbench"; treeId: string }
    | { page: "surfaces"; treeId: string }
    | { page: "history"; treeId: string }
    | { page: "changes"; treeId: string }
    | { page: "change-set"; id: string };

function parseRoute(path: string): Route {
    const segments = path.split("/").filter((segment) => segment !== "");
    if (segments[0] === "trees" && segments[1] !== undefined) {
        return segments[2] === "changes"
            ? { page: "changes", treeId: segments[1] }
            : segments[2] === "history"
              ? { page: "history", treeId: segments[1] }
              : segments[2] === "surfaces"
                ? { page: "surfaces", treeId: segments[1] }
                : { page: "workbench", treeId: segments[1] };
    }
    if (segments[0] === "change-sets" && segments[1] !== undefined) {
        return { page: "change-set", id: segments[1] };
    }
    return { page: "home" };
}

export function App() {
    const [me, setMe] = useState<MeResponse | null>(null);
    const [error, setError] = useState<string | null>(null);
    const route = parseRoute(useHashPath());

    useEffect(() => {
        fetchMe().then(setMe, (err: Error) => setError(err.message));
    }, []);

    const treeId =
        route.page === "workbench" ||
        route.page === "changes" ||
        route.page === "history" ||
        route.page === "surfaces"
            ? route.treeId
            : (me?.capabilities?.sourceTrees[0]?.id ?? null);

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
                    <div className="label nav-group-label">Lenses</div>
                    <Lens
                        label="Domain"
                        active={route.page === "surfaces"}
                        disabled={treeId === null}
                        onClick={() => navigate(`/trees/${treeId}/surfaces`)}
                        title={
                            treeId === null
                                ? "Register a source tree first"
                                : undefined
                        }
                    />
                    <Lens
                        label="Changes"
                        active={
                            route.page === "changes" ||
                            route.page === "change-set"
                        }
                        disabled={treeId === null}
                        onClick={() => navigate(`/trees/${treeId}/changes`)}
                        title={
                            treeId === null
                                ? "Register a source tree first"
                                : undefined
                        }
                    />
                    <Lens
                        label="History"
                        active={route.page === "history"}
                        disabled={treeId === null}
                        onClick={() => navigate(`/trees/${treeId}/history`)}
                        title={
                            treeId === null
                                ? "Register a source tree first"
                                : undefined
                        }
                    />
                    <Lens
                        label="Model"
                        active={route.page === "workbench"}
                        disabled={treeId === null}
                        onClick={() => navigate(`/trees/${treeId}`)}
                        title={
                            treeId === null
                                ? "Register a source tree first"
                                : undefined
                        }
                    />
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
                    <div className="crumbs">
                        <a className="label" href="#/">
                            Home
                        </a>
                        {route.page !== "home" ? (
                            <>
                                <span className="crumb-sep">/</span>
                                <span className="label">
                                    {route.page === "workbench"
                                        ? "Model"
                                        : route.page === "surfaces"
                                          ? "Domain"
                                          : route.page === "history"
                                            ? "History"
                                            : route.page === "change-set"
                                              ? "Change set"
                                              : "Changes"}
                                </span>
                            </>
                        ) : null}
                    </div>
                    <div className="topbar-actions" />
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
                        ) : route.page === "workbench" ? (
                            <WorkbenchPage me={me} treeId={route.treeId} />
                        ) : route.page === "surfaces" ? (
                            <SurfacesPage me={me} treeId={route.treeId} />
                        ) : route.page === "history" ? (
                            <WorkbenchPage
                                me={me}
                                treeId={route.treeId}
                                initialView="history"
                            />
                        ) : route.page === "changes" ? (
                            <ChangesPage me={me} treeId={route.treeId} />
                        ) : route.page === "change-set" ? (
                            <ChangeSetPage id={route.id} />
                        ) : (
                            <Home me={me} />
                        )}
                    </div>
                </main>
            </div>
        </div>
    );
}

function Lens({
    label,
    active,
    disabled,
    onClick,
    title,
}: {
    label: string;
    active?: boolean;
    disabled?: boolean;
    onClick?: () => void;
    title?: string;
}) {
    if (disabled === true) {
        return (
            <span className="nav-item" aria-disabled="true" title={title}>
                <span className="nav-item-text">{label}</span>
            </span>
        );
    }
    return (
        <button
            className="nav-item"
            data-active={active === true ? "true" : undefined}
            onClick={onClick}
            title={title}
        >
            <span className="nav-item-text">{label}</span>
        </button>
    );
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

function Home({ me }: { me: MeResponse }) {
    if (me.principal === null) {
        return (
            <div className="card">
                <h1>Sign in</h1>
                <p className="hint">
                    This console runs in team mode; sign in with GitHub to see
                    the packages your repositories give you.
                </p>
                {me.signIn?.github ? (
                    <a
                        className="btn btn-primary"
                        href="/api/auth/github/start"
                    >
                        Sign in with GitHub
                    </a>
                ) : null}
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
                    packages are discovered. Register a GitHub repository
                    through the API and it appears here with what you can do to
                    it.
                </p>
            </div>
        );
    }
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
        </div>
    );
}

function SourceTreeCard({ tree }: { tree: SourceTreeSummary }) {
    const name =
        tree.kind === "github" ? `${tree.owner}/${tree.name}` : tree.id;
    const verbs = ["view", "propose", "approve", "administer"] as const;
    return (
        <div className="card">
            <div className="card-head">
                <span className="mono">{name}</span>
                <span className="card-actions">
                    <button
                        className="btn btn-secondary btn-sm"
                        onClick={() => navigate(`/trees/${tree.id}`)}
                    >
                        Workbench
                    </button>
                    <button
                        className="btn btn-ghost btn-sm"
                        onClick={() => navigate(`/trees/${tree.id}/changes`)}
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
