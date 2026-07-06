// The Changes lens (tranche C2): every change set the tree's viewers can
// see — no secret drafts — with the observed facts the reconciler keeps
// current, the events diary, and the author's submit and abandon actions.
// Tranche C4 adds the three-delta review: what changed, what it does (with
// the denominator stated), whether it is healthy — the panels an approver
// reads before pressing merge on GitHub.

import { useCallback, useEffect, useState } from "react";

import {
    abandonChangeSet,
    approveChangeSet,
    fetchReview,
    listChangeSets,
    mergeChangeSet,
    readChangeSet,
    readPackage,
    reconcileChangeSet,
    saveEdit,
    submitChangeSet,
    type ApprovalPolicyStatus,
    type ApprovalRecord,
    type ChangeSet,
    type ChangeSetDetail,
    type ChangeSetReview,
    type ContextImpact,
    type LintDiagnostic,
    type MeResponse,
    type PackageReview,
    type RedactedPackageReview,
    type ReviewContext,
    type SemanticChange,
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

export function ChangeSetPage({
    id,
    me,
}: {
    id: string;
    me: MeResponse | null;
}) {
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

            {changeSet.state === "draft" || changeSet.state === "proposed" ? (
                <ReviewPanel
                    changeSet={changeSet}
                    me={me}
                    onChanged={refresh}
                />
            ) : null}

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

// --- the three-delta review (tranche C4) ---

function ReviewPanel({
    changeSet,
    me,
    onChanged,
}: {
    changeSet: ChangeSet;
    me: MeResponse | null;
    onChanged: () => void;
}) {
    const [review, setReview] = useState<ChangeSetReview | null>(null);
    const [approvals, setApprovals] = useState<ApprovalRecord[]>([]);
    const [contributors, setContributors] = useState<string[]>([]);
    const [policy, setPolicy] = useState<ApprovalPolicyStatus | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [acting, setActing] = useState(false);

    const load = useCallback(() => {
        fetchReview(changeSet.id).then(
            (response) => {
                setReview(response.review);
                setApprovals(response.approvals);
                setContributors(response.contributors);
                setPolicy(response.policy);
            },
            (failure: Error) => setError(failure.message),
        );
    }, [changeSet.id]);
    useEffect(() => {
        load();
    }, [load]);

    if (error !== null) {
        return <div className="banner banner-warn">Review: {error}</div>;
    }
    if (review === null) {
        return <p className="muted">Computing the review…</p>;
    }
    const act = (action: Promise<unknown>) => {
        setActing(true);
        action
            .then(
                () => {
                    setError(null);
                    load();
                    onChanged();
                },
                (failure: Error) => setError(failure.message),
            )
            .finally(() => setActing(false));
    };
    const isContributor =
        me?.principal != null && contributors.includes(me.principal.id);

    return (
        <>
            {changeSet.state === "proposed" && policy !== null ? (
                <div className="card">
                    <h2>Approval</h2>
                    {policy.satisfied ? (
                        <p className="hint">
                            Every requirement is satisfied; merging will land
                            the change.
                        </p>
                    ) : (
                        <div>
                            {policy.missing.map((line, index) => (
                                <p className="diagnostic" key={index}>
                                    <span className="pill pill-info">
                                        waiting on
                                    </span>{" "}
                                    {line}
                                </p>
                            ))}
                        </div>
                    )}
                    {approvals.length > 0 ? (
                        <p className="hint">
                            Approved by{" "}
                            {approvals
                                .map((approval) => approval.principalId)
                                .join(", ")}
                        </p>
                    ) : null}
                    <div className="card-actions">
                        {isContributor ? (
                            <span
                                className="hint"
                                title="The two-person rule: everyone who committed is disqualified"
                            >
                                You contributed to this change, so you cannot
                                approve it.
                            </span>
                        ) : (
                            <button
                                className="btn btn-primary btn-sm"
                                disabled={acting}
                                onClick={() =>
                                    act(approveChangeSet(changeSet.id))
                                }
                            >
                                Approve
                            </button>
                        )}
                        {policy.satisfied ? (
                            <button
                                className="btn btn-secondary btn-sm"
                                disabled={acting}
                                onClick={() =>
                                    act(mergeChangeSet(changeSet.id))
                                }
                            >
                                Merge
                            </button>
                        ) : null}
                    </div>
                </div>
            ) : null}
            {review.packages.length === 0 ? (
                <div className="card">
                    <h2>Review</h2>
                    <p className="hint">
                        No package changes yet; the review fills in as commits
                        land on the branch.
                    </p>
                </div>
            ) : null}
            {review.packages.map((pkg) =>
                "redacted" in pkg ? (
                    <RedactedPackageView key={pkg.path} pkg={pkg} />
                ) : (
                    <PackageReviewView
                        key={pkg.path}
                        changeSet={changeSet}
                        headPin={review.headPin}
                        pkg={pkg}
                        onPromoted={() => {
                            load();
                            onChanged();
                        }}
                    />
                ),
            )}
        </>
    );
}

// Existence is disclosed; content is not. The count keeps the reviewer
// honest about what they cannot judge.
function RedactedPackageView({ pkg }: { pkg: RedactedPackageReview }) {
    return (
        <div className="card">
            <h3 className="mono">{pkg.path}</h3>
            <p className="hint">
                This change also touches {pkg.files} file
                {pkg.files === 1 ? "" : "s"} in a package you cannot view.
            </p>
        </div>
    );
}

function PackageReviewView({
    changeSet,
    headPin,
    pkg,
    onPromoted,
}: {
    changeSet: ChangeSet;
    headPin: string;
    pkg: PackageReview;
    onPromoted: () => void;
}) {
    return (
        <div className="section">
            {pkg.surfaces.length > 0 ? (
                <div className="card">
                    <h3>Surfaces this change touches</h3>
                    {pkg.surfaces.map((surface) => (
                        <p className="diagnostic" key={surface.id}>
                            <strong>{surface.title}</strong>
                            {surface.approval !== null ? (
                                <span
                                    className="pill pill-info"
                                    title="Declared approval requirement; GitHub enforces merges in this phase, so this informs the approver"
                                >
                                    requires {surface.approval}
                                </span>
                            ) : null}
                            {surface.caution !== null ? (
                                <span className="hint"> {surface.caution}</span>
                            ) : null}
                        </p>
                    ))}
                </div>
            ) : null}

            <div className="card">
                <h3>What changed</h3>
                {pkg.changes.length === 0 ? (
                    <p className="hint">No semantic changes.</p>
                ) : (
                    <div className="row-list">
                        {pkg.changes.map((change, index) => (
                            <SemanticChangeRow key={index} change={change} />
                        ))}
                    </div>
                )}
            </div>

            <div className="card">
                <h3>What it does</h3>
                <ImpactView
                    changeSet={changeSet}
                    headPin={headPin}
                    pkg={pkg}
                    onPromoted={onPromoted}
                />
            </div>

            <div className="card">
                <h3>Whether it is healthy</h3>
                <LintDeltaView lint={pkg.lint} />
            </div>
        </div>
    );
}

function SemanticChangeRow({ change }: { change: SemanticChange }) {
    return (
        <div className="row row-static">
            <span className="row-text">
                <span className="row-title mono">
                    {change.kind.replaceAll("_", " ")}
                </span>
                <span className="row-sub mono">
                    {describeTarget(change.target)}
                </span>
            </span>
            <span className="row-side mono review-values">
                {change.before !== undefined ? (
                    <span className="review-before">{clip(change.before)}</span>
                ) : null}
                {change.before !== undefined && change.after !== undefined
                    ? " → "
                    : null}
                {change.after !== undefined ? (
                    <span className="review-after">{clip(change.after)}</span>
                ) : null}
            </span>
        </div>
    );
}

// The execution delta: which outcomes change, under which contexts, and —
// always — against how much evidence. Synthetic contexts are labeled, and a
// gap they reveal converts to a real sample in this same change set.
function ImpactView({
    changeSet,
    headPin,
    pkg,
    onPromoted,
}: {
    changeSet: ChangeSet;
    headPin: string;
    pkg: PackageReview;
    onPromoted: () => void;
}) {
    const [promoting, setPromoting] = useState<string | null>(null);
    const [note, setNote] = useState<string | null>(null);

    if (pkg.impactError !== null) {
        return (
            <div className="banner banner-warn">
                Resolution impact could not run: {pkg.impactError}
            </div>
        );
    }
    const changed = pkg.contextImpacts.filter(
        (impact) => impact.impacts.length > 0,
    );
    const contextsByLabel = new Map(
        pkg.contexts.map((context) => [context.label, context]),
    );

    // Promote a synthetic context to a saved sample inside this change set:
    // the sample corpus grows as a side effect of review.
    const promote = async (context: ReviewContext) => {
        setPromoting(context.label);
        setNote(null);
        try {
            const detail = await readPackage(
                changeSet.sourceTreeId,
                pkg.path,
                headPin,
            );
            const evaluationContext = detail.model.evaluationContexts[0]?.id;
            if (evaluationContext === undefined) {
                throw new Error(
                    "the package declares no evaluation context to hold the sample",
                );
            }
            await saveEdit(changeSet.id, {
                packagePath: pkg.path,
                expectedPin: headPin,
                operations: [
                    {
                        op: "create_sample",
                        context: evaluationContext,
                        key: sampleKeyFor(context.label),
                        content: context.context,
                    },
                ],
                summary: `Add sample from review (${context.label})`,
            });
            onPromoted();
        } catch (failure) {
            setNote(
                failure instanceof Error ? failure.message : String(failure),
            );
        } finally {
            setPromoting(null);
        }
    };

    return (
        <>
            <p className="hint">
                Against {pkg.denominator.samples} sample context
                {pkg.denominator.samples === 1 ? "" : "s"}
                {pkg.denominator.synthesized > 0
                    ? ` and ${pkg.denominator.synthesized} synthesized boundary context${pkg.denominator.synthesized === 1 ? "" : "s"}`
                    : ""}
                .{" "}
                {pkg.denominator.variables.map((variable) => {
                    const covered = variable.rules.filter(
                        (rule) => rule.covered,
                    ).length;
                    return (
                        <span key={variable.id}>
                            Samples exercise {covered} of{" "}
                            {variable.rules.length} rule
                            {variable.rules.length === 1 ? "" : "s"} on{" "}
                            <span className="mono">{variable.id}</span>
                            {variable.defaultCovered
                                ? ", including the default."
                                : "; no sample exercises its default."}{" "}
                        </span>
                    );
                })}
            </p>
            {note !== null ? (
                <div className="banner banner-err">{note}</div>
            ) : null}
            {changed.length === 0 ? (
                <p className="hint">
                    No outcome changes under any of these contexts
                    {pkg.denominator.samples + pkg.denominator.synthesized === 0
                        ? " — but no context ran, so this says nothing"
                        : ""}
                    .
                </p>
            ) : (
                changed.map((impact) => (
                    <ContextImpactView
                        key={impact.context}
                        impact={impact}
                        context={contextsByLabel.get(impact.context) ?? null}
                        promoting={promoting === impact.context}
                        onPromote={promote}
                    />
                ))
            )}
        </>
    );
}

function ContextImpactView({
    impact,
    context,
    promoting,
    onPromote,
}: {
    impact: ContextImpact;
    context: ReviewContext | null;
    promoting: boolean;
    onPromote: (context: ReviewContext) => void;
}) {
    const synthetic = context?.source === "synthetic";
    return (
        <div className="impact-context">
            <div className="impact-context-head">
                <span
                    className={`pill ${synthetic ? "pill-warn" : "pill-sea"}`}
                >
                    {synthetic ? "synthetic" : "sample"}
                </span>
                <span className="mono">{stripLabel(impact.context)}</span>
                <span className="hint">
                    {impact.impacts.length} change
                    {impact.impacts.length === 1 ? "" : "s"} across{" "}
                    {impact.compared} compared
                </span>
                {synthetic && context !== null ? (
                    <button
                        className="btn btn-secondary btn-sm"
                        disabled={promoting}
                        title="No saved sample covers this behavior; add this context as a sample in this change set"
                        onClick={() => onPromote(context)}
                    >
                        {promoting ? "Promoting…" : "Promote to sample"}
                    </button>
                ) : null}
            </div>
            {impact.impacts.map((outcome) => (
                <p className="diagnostic mono" key={outcome.variable}>
                    {outcome.variable}:{" "}
                    <span className="review-before">
                        {outcome.before !== undefined
                            ? clip(outcome.before.value)
                            : (outcome.before_error ?? "(absent)")}
                    </span>{" "}
                    →{" "}
                    <span className="review-after">
                        {outcome.after !== undefined
                            ? clip(outcome.after.value)
                            : (outcome.after_error ?? "(absent)")}
                    </span>
                </p>
            ))}
        </div>
    );
}

function LintDeltaView({
    lint,
}: {
    lint: { introduced: LintDiagnostic[]; resolved: LintDiagnostic[] };
}) {
    if (lint.introduced.length === 0 && lint.resolved.length === 0) {
        return (
            <p className="hint">
                Lint is unchanged: nothing introduced, nothing resolved.
            </p>
        );
    }
    return (
        <>
            {lint.introduced.map((diagnostic, index) => (
                <p className="diagnostic" key={`i${index}`}>
                    <span
                        className={`pill ${diagnostic.severity === "error" ? "pill-err" : "pill-warn"}`}
                    >
                        introduces {diagnostic.severity}
                    </span>{" "}
                    {diagnostic.rule !== undefined ? (
                        <span className="mono">{diagnostic.rule} </span>
                    ) : null}
                    {diagnostic.message}
                </p>
            ))}
            {lint.resolved.map((diagnostic, index) => (
                <p className="diagnostic" key={`r${index}`}>
                    <span className="pill pill-ok">resolves</span>{" "}
                    {diagnostic.rule !== undefined ? (
                        <span className="mono">{diagnostic.rule} </span>
                    ) : null}
                    {diagnostic.message}
                </p>
            ))}
        </>
    );
}

function describeTarget(target: SemanticChange["target"]): string {
    const entity = target.entity;
    const parts = Object.entries(entity)
        .filter(([key]) => key !== "kind")
        .map(([, value]) => String(value));
    const field = target.field as
        { kind?: string; path?: string[] } | undefined;
    const pointer =
        field?.path !== undefined && Array.isArray(field.path)
            ? `#/${field.path.join("/")}`
            : "";
    return `${entity.kind} ${parts.join(" / ")}${pointer}`;
}

function stripLabel(label: string): string {
    return label.replace(/^(sample|synthetic):/, "");
}

function sampleKeyFor(label: string): string {
    return stripLabel(label)
        .toLowerCase()
        .replace(/[^a-z0-9_]+/g, "_")
        .replace(/_+/g, "_")
        .replace(/^_|_$/g, "");
}

function clip(value: unknown): string {
    const text = JSON.stringify(value) ?? String(value);
    return text.length > 32 ? `${text.slice(0, 32)}…` : text;
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
