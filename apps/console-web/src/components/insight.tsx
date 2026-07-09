// The remaining read-side panels (tranche C3): upcoming changes (the time
// facet) and package history with the "what was this value on March 3rd"
// bound.

import { useEffect, useState } from "react";

import { ExpressionText } from "@/components/entity-link";
import {
    fetchHistory,
    fetchUpcoming,
    type CommitRecord,
    type SourceTreeSummary,
    type UpcomingChange,
} from "@/lib/api";
import { formatInstant } from "@/lib/format";
import { githubCommitUrl } from "@/lib/github";
import { SearchableList } from "@/lib/ui-kit";
import type { AddressStep } from "@/lib/router";

// Behavior that changes with no commit and no deploy: env.now boundaries
// that have not passed. Empty means nothing is scheduled, and the panel
// says so only when asked (it hides when quiet).
export function UpcomingPanel({
    treeId,
    packagePath,
    pin,
    hrefEntity,
}: {
    treeId: string;
    packagePath: string;
    pin: string;
    hrefEntity: (steps: AddressStep[]) => string;
}) {
    const [changes, setChanges] = useState<UpcomingChange[] | null>(null);
    useEffect(() => {
        let stale = false;
        fetchUpcoming(treeId, packagePath, pin).then(
            (response) => {
                if (!stale) {
                    setChanges(response.changes);
                }
            },
            () => {
                if (!stale) {
                    setChanges([]);
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, packagePath, pin]);

    if (changes === null || changes.length === 0) {
        return null;
    }
    return (
        <>
            <div className="section-header-text">
                <h2>Scheduled to change</h2>
                <p className="hint">
                    These flips happen on their own when the instant passes; no
                    commit, no deploy.
                </p>
            </div>
            <div className="row-list">
                {changes.map((change, index) => (
                    <div className="row row-static" key={index}>
                        <span className="row-text">
                            <a
                                className="row-link row-title mono"
                                href={hrefEntity([
                                    { class: "variable", id: change.variable },
                                ])}
                            >
                                {change.variable}
                            </a>
                            <span className="row-sub mono">
                                {siteLabel(change.site)}:{" "}
                                <ExpressionText
                                    text={change.expression}
                                    hrefFor={hrefEntity}
                                />
                            </span>
                        </span>
                        <span className="row-side">
                            <span className="pill pill-info mono">
                                {change.boundary}
                            </span>
                        </span>
                    </div>
                ))}
            </div>
        </>
    );
}

function siteLabel(site: UpcomingChange["site"]): string {
    switch (site.kind) {
        case "rule":
            return `rule ${site.index}`;
        case "queryFilter":
            return "query filter";
        case "querySort":
            return "query sort";
    }
}

// Package history, and the compliance question: pick an instant, get the
// pin that was in force, browse the package as it was.
export function HistoryPanel({
    treeId,
    tree,
    packagePath,
    viewingPin,
    onViewPin,
}: {
    treeId: string;
    tree: SourceTreeSummary | undefined;
    packagePath: string;
    viewingPin: string | null;
    onViewPin: (pin: string | null) => void;
}) {
    const [commits, setCommits] = useState<CommitRecord[] | null>(null);
    const [until, setUntil] = useState("");
    const [bound, setBound] = useState<string | undefined>(undefined);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        let stale = false;
        setCommits(null);
        fetchHistory(treeId, packagePath, bound).then(
            (response) => {
                if (!stale) {
                    setCommits(response.commits);
                }
            },
            (failure: Error) => {
                if (!stale) {
                    setError(failure.message);
                }
            },
        );
        return () => {
            stale = true;
        };
    }, [treeId, packagePath, bound]);

    return (
        <div className="section">
            <div className="section-header-text">
                <h2>History</h2>
                <p className="hint">
                    Every commit that touched this package. Ask for an instant
                    to see what was in force then; a commit opens the package as
                    it was.
                </p>
            </div>
            <form
                className="action-row"
                onSubmit={(event) => {
                    event.preventDefault();
                    setBound(
                        until.trim() === ""
                            ? undefined
                            : normalizeInstant(until.trim()),
                    );
                }}
            >
                <span className="label">On this date</span>
                <input
                    className="input mono history-instant"
                    placeholder="2026-03-03 or 2026-03-03T12:00:00Z"
                    value={until}
                    onChange={(event) => setUntil(event.target.value)}
                />
                <button className="btn btn-secondary btn-sm" type="submit">
                    Look up
                </button>
                {bound !== undefined ? (
                    <button
                        className="btn btn-ghost btn-sm"
                        type="button"
                        onClick={() => {
                            setBound(undefined);
                            setUntil("");
                        }}
                    >
                        Clear
                    </button>
                ) : null}
            </form>
            {error !== null ? (
                <div className="banner banner-err">{error}</div>
            ) : null}
            {commits === null ? (
                <p className="muted">Loading…</p>
            ) : commits.length === 0 ? (
                <p className="hint">
                    No commits touched this package before that instant.
                </p>
            ) : (
                <SearchableList
                    label="Search commits"
                    placeholder="Search commits"
                    emptyLabel="No commit matches that search."
                    className="row-list"
                >
                    {commits.map((commit, index) => {
                        const current = viewingPin === commit.sha;
                        const inForce = bound !== undefined && index === 0;
                        const commitUrl =
                            tree === undefined
                                ? null
                                : githubCommitUrl(tree, commit.sha);
                        return (
                            <div
                                className="row row-static"
                                data-active={current ? "true" : undefined}
                                key={commit.sha}
                                data-search={`${commit.message.split("\n")[0]} ${commit.sha} ${commit.authorName ?? ""} ${formatInstant(commit.date)}`}
                            >
                                <span className="row-text">
                                    <button
                                        className="row-link row-title"
                                        title={
                                            current
                                                ? "Back to now"
                                                : "View the package as it was after this commit"
                                        }
                                        onClick={() =>
                                            onViewPin(
                                                current ? null : commit.sha,
                                            )
                                        }
                                    >
                                        {commit.message.split("\n")[0]}
                                    </button>
                                    <span className="row-sub mono">
                                        {commitUrl !== null ? (
                                            <a
                                                href={commitUrl}
                                                rel="noreferrer"
                                                target="_blank"
                                                title="Open this commit on GitHub"
                                            >
                                                {commit.sha.slice(0, 10)}
                                            </a>
                                        ) : (
                                            commit.sha.slice(0, 10)
                                        )}{" "}
                                        · {formatInstant(commit.date)}
                                        {commit.authorName !== null
                                            ? ` · ${commit.authorName}`
                                            : ""}
                                    </span>
                                </span>
                                <span className="row-side">
                                    {inForce ? (
                                        <span className="pill pill-ok">
                                            in force then
                                        </span>
                                    ) : null}
                                    {current ? (
                                        <span className="pill pill-info">
                                            viewing
                                        </span>
                                    ) : null}
                                </span>
                            </div>
                        );
                    })}
                </SearchableList>
            )}
        </div>
    );
}

// A date alone means "end of that day, UTC".
function normalizeInstant(value: string): string {
    return /^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T23:59:59Z` : value;
}
