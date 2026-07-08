// The tree home: the packages a source tree holds, each a doorway into that
// package's hierarchy. A single-package tree forwards straight to its
// package overview; this page is the fork for trees that hold several.

import { useEffect } from "react";

import type { MeResponse } from "@/lib/api";
import { githubRepoUrl } from "@/lib/github";
import { SearchableList } from "@/lib/ui-kit";
import {
    changesUrl,
    navigate,
    packageUrl,
    redirect,
    type ViewState,
} from "@/lib/router";

export function TreeHomePage({
    me,
    treeId,
    state,
    packages,
}: {
    me: MeResponse;
    treeId: string;
    state: ViewState;
    packages: string[] | null;
}) {
    const tree = me.capabilities?.sourceTrees.find(
        (candidate) => candidate.id === treeId,
    );

    // The forward keeps the URL's view state, so a link like
    // /trees/st_x?cs=cs_y lands on the package with the change set active.
    useEffect(() => {
        if (packages !== null && packages.length === 1) {
            redirect(
                packageUrl(
                    treeId,
                    packages[0] as string,
                    { kind: "overview" },
                    state,
                ),
            );
        }
    }, [packages, treeId, state]);

    if (tree === undefined) {
        return (
            <div className="card">
                <h1>Not visible</h1>
                <p className="hint">
                    This source tree does not exist or is not visible to you.
                </p>
            </div>
        );
    }
    const treeName =
        tree.kind === "github" ? `${tree.owner}/${tree.name}` : tree.id;
    const repoUrl = githubRepoUrl(tree);

    return (
        <div className="section">
            <div className="section-header">
                <div className="section-header-text">
                    <h1>{treeName}</h1>
                    <p className="hint">
                        The packages discovered in this tree.
                        {repoUrl !== null ? (
                            <>
                                {" "}
                                <a
                                    className="pill-link"
                                    href={repoUrl}
                                    rel="noreferrer"
                                    target="_blank"
                                >
                                    GitHub ↗
                                </a>
                            </>
                        ) : null}
                    </p>
                </div>
                <button
                    className="btn btn-secondary btn-sm"
                    onClick={() => navigate(changesUrl(treeId))}
                >
                    Change sets
                </button>
            </div>
            {packages === null ? (
                <p className="muted">Loading…</p>
            ) : packages.length === 0 ? (
                <div className="card">
                    <h2>No packages</h2>
                    <p className="hint">
                        No <span className="mono">rototo-package.toml</span> was
                        found anywhere in this tree.
                    </p>
                </div>
            ) : (
                <SearchableList
                    label="Search packages"
                    placeholder="Search packages"
                    emptyLabel="No package matches that search."
                    className="row-list"
                >
                    {packages.map((path) => (
                        <a
                            className="row"
                            key={path}
                            href={`#${packageUrl(treeId, path, { kind: "overview" }, state)}`}
                            data-search={path}
                        >
                            <span className="row-text">
                                <span className="row-title mono">
                                    {path === "." ? "(root package)" : path}
                                </span>
                            </span>
                        </a>
                    ))}
                </SearchableList>
            )}
        </div>
    );
}
