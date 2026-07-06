// The Changes lens (tranche C2): every change set the tree's viewers can
// see — no secret drafts — with the observed facts the reconciler keeps
// current, the events diary, and the author's submit and abandon actions.

import { useCallback, useEffect, useState } from "react";

import {
    abandonChangeSet,
    listChangeSets,
    readChangeSet,
    reconcileChangeSet,
    submitChangeSet,
    type ChangeSet,
    type ChangeSetDetail,
    type MeResponse,
} from "@/lib/api";
import { navigate } from "@/lib/router";

export function ChangesPage({
    me,
    treeId,
}: {
    me: MeResponse;
    treeId: string;
}) {
    const tree = me.capabilities?.sourceTrees.find(
        (candidate) => candidate.id === treeId,
    );
    const [changeSets, setChangeSets] = useState<ChangeSet[] | null>(null);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        listChangeSets(treeId).then(
            (response) => setChangeSets(response.changeSets),
            (failure: Error) => setError(failure.message),
        );
    }, [treeId]);

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

    return (
        <div className="section">
            <div className="section-header">
                <div className="section-header-text">
                    <h1>Change sets</h1>
                    <p className="hint">{treeName}</p>
                </div>
                <button
                    className="btn btn-secondary btn-sm"
                    onClick={() => navigate(`/trees/${treeId}`)}
                >
                    Workbench
                </button>
            </div>
            {error !== null ? (
                <div className="banner banner-err">{error}</div>
            ) : null}
            {changeSets === null ? (
                <p className="muted">Loading…</p>
            ) : changeSets.length === 0 ? (
                <div className="card">
                    <h2>No change sets yet</h2>
                    <p className="hint">
                        Start one from the workbench: pick a variable, start a
                        change set, save an edit. Every save is a commit on the
                        change set's branch.
                    </p>
                </div>
            ) : (
                <div className="row-list">
                    {changeSets.map((changeSet) => (
                        <button
                            className="row"
                            key={changeSet.id}
                            onClick={() =>
                                navigate(`/change-sets/${changeSet.id}`)
                            }
                        >
                            <span className="row-text">
                                <span className="row-title">
                                    {changeSet.title}
                                </span>
                                <span className="row-sub mono">
                                    {changeSet.branch}
                                </span>
                            </span>
                            <span className="row-side">
                                <StatePill changeSet={changeSet} />
                            </span>
                        </button>
                    ))}
                </div>
            )}
        </div>
    );
}

export function ChangeSetPage({ id }: { id: string }) {
    const [detail, setDetail] = useState<ChangeSetDetail | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);

    const refresh = useCallback(() => {
        readChangeSet(id).then(setDetail, (failure: Error) =>
            setError(failure.message),
        );
    }, [id]);
    useEffect(() => {
        refresh();
    }, [refresh]);

    if (error !== null) {
        return (
            <div className="card">
                <h1>Change set unavailable</h1>
                <p className="hint">{error}</p>
            </div>
        );
    }
    if (detail === null) {
        return <p className="muted">Loading…</p>;
    }
    const changeSet = detail.changeSet;
    const act = (action: Promise<unknown>) => {
        setBusy(true);
        action
            .then(
                () => {
                    setError(null);
                    refresh();
                },
                (failure: Error) => setError(failure.message),
            )
            .finally(() => setBusy(false));
    };

    return (
        <div className="section">
            <div className="section-header">
                <div className="section-header-text">
                    <h1>{changeSet.title}</h1>
                    <p className="hint mono">
                        {changeSet.branch}
                        {changeSet.headSha !== null
                            ? ` @ ${changeSet.headSha.slice(0, 10)}`
                            : ""}
                    </p>
                </div>
                <StatePill changeSet={changeSet} />
            </div>

            <div className="card">
                <div className="meta-grid">
                    <div className="meta-item">
                        <span className="label">Base</span>
                        <span className="meta-value mono">
                            {changeSet.baseRef}
                        </span>
                    </div>
                    <div className="meta-item">
                        <span className="label">Author</span>
                        <span className="meta-value mono">
                            {changeSet.authorPrincipal}
                        </span>
                    </div>
                    <div className="meta-item">
                        <span className="label">Pull request</span>
                        <span className="meta-value">
                            {changeSet.prUrl !== null ? (
                                <a
                                    className="pill-link"
                                    href={changeSet.prUrl}
                                    rel="noreferrer"
                                    target="_blank"
                                >
                                    #{changeSet.prNumber}
                                </a>
                            ) : (
                                "not yet"
                            )}
                        </span>
                    </div>
                    <div className="meta-item">
                        <span className="label">Observed</span>
                        <span className="meta-value">
                            {changeSet.lastReconciledAt ?? "never"}
                        </span>
                    </div>
                </div>
                <div className="card-actions">
                    {changeSet.state === "draft" ? (
                        <button
                            className="btn btn-primary btn-sm"
                            disabled={busy}
                            onClick={() => act(submitChangeSet(changeSet.id))}
                        >
                            Submit (open PR)
                        </button>
                    ) : null}
                    {changeSet.state === "draft" ||
                    changeSet.state === "proposed" ? (
                        <button
                            className="btn btn-danger btn-sm"
                            disabled={busy}
                            onClick={() => act(abandonChangeSet(changeSet.id))}
                        >
                            Abandon
                        </button>
                    ) : null}
                    <button
                        className="btn btn-ghost btn-sm"
                        disabled={busy}
                        onClick={() => act(reconcileChangeSet(changeSet.id))}
                    >
                        Check GitHub now
                    </button>
                    <button
                        className="btn btn-ghost btn-sm"
                        onClick={() =>
                            navigate(`/trees/${changeSet.sourceTreeId}`)
                        }
                    >
                        Open workbench
                    </button>
                </div>
            </div>

            <div className="section-header-text">
                <h2>Diary</h2>
                <p className="hint">
                    Append-only: what happened, who did it, what we observed.
                </p>
            </div>
            <div className="timeline">
                {detail.events.map((event) => (
                    <div className="tl-row" key={event.id}>
                        <span className="tl-icon" aria-hidden>
                            •
                        </span>
                        <span className="tl-body">
                            <span className="tl-detail">
                                <strong>{event.event}</strong>
                                {event.actor !== null
                                    ? ` by ${event.actor}`
                                    : " (observed)"}
                                {event.detail !== null ? (
                                    <span className="mono muted">
                                        {" "}
                                        {event.detail}
                                    </span>
                                ) : null}
                            </span>
                            <span className="tl-when">{event.at}</span>
                        </span>
                    </div>
                ))}
            </div>
        </div>
    );
}

function StatePill({ changeSet }: { changeSet: ChangeSet }) {
    const kind =
        changeSet.state === "merged"
            ? "pill-ok"
            : changeSet.state === "abandoned"
              ? "pill-neutral"
              : changeSet.state === "proposed"
                ? "pill-info"
                : "pill-sea";
    return (
        <span>
            <span className={`pill ${kind}`}>{changeSet.state}</span>
            {changeSet.behindBase ? (
                <span className="pill pill-warn" title="The base branch moved">
                    behind base
                </span>
            ) : null}
            {changeSet.conflicted ? (
                <span
                    className="pill pill-err"
                    title="GitHub reports the branch cannot merge cleanly"
                >
                    conflicted
                </span>
            ) : null}
        </span>
    );
}
