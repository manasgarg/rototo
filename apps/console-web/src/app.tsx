// The shared home, empty (tranche C1). One home for every persona; lenses
// (domain, changes, history, model) arrive with the tranches that give them
// content. What exists now: the shell, sign-in, and an honest capability
// rendering of the source trees the server shows us.

import { useEffect, useState } from "react";

import { RototoMark } from "@/components/rototo-mark";
import { fetchMe, type MeResponse, type SourceTreeSummary } from "@/lib/api";

const LENSES = ["Domain", "Changes", "History", "Model"] as const;

export function App() {
    const [me, setMe] = useState<MeResponse | null>(null);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        fetchMe().then(setMe, (err: Error) => setError(err.message));
    }, []);

    return (
        <div className="shell">
            <aside className="sidebar">
                <a className="brand" href="/">
                    <span className="brand-mark">
                        <RototoMark />
                    </span>
                    <span className="brand-name">rototo</span>
                </a>
                <nav className="side-nav">
                    <div className="label nav-group-label">Lenses</div>
                    {LENSES.map((lens) => (
                        <span
                            key={lens}
                            className="nav-item"
                            aria-disabled="true"
                            title="Arrives with the next tranches"
                        >
                            <span className="nav-item-text">{lens}</span>
                        </span>
                    ))}
                </nav>
                <SideUser me={me} />
            </aside>
            <div className="main">
                <header className="topbar">
                    <a className="topbar-brand" href="/" title="rototo console">
                        <RototoMark size={24} />
                    </a>
                    <div className="crumbs">
                        <span className="label">Home</span>
                    </div>
                    <div className="topbar-actions" />
                </header>
                <main className="content">
                    <div className="content-inner">
                        <Home me={me} error={error} />
                    </div>
                </main>
            </div>
        </div>
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

function Home({ me, error }: { me: MeResponse | null; error: string | null }) {
    if (error !== null) {
        return (
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
        );
    }
    if (me === null) {
        return <p className="muted">Loading…</p>;
    }
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
                    packages are discovered. Registration and change sets land
                    in the next tranche; this build carries the identity spine
                    and the decision seam underneath them.
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
            <span className="mono">{name}</span>
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
