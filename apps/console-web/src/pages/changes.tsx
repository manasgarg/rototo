// The Changes lens (tranche C2): every change set the tree's viewers can
// see — no secret drafts — with the observed facts the reconciler keeps
// current, the events diary, and the author's submit and abandon actions.
// Tranche C4 adds the three-delta review: what changed, what it does (with
// the denominator stated), whether it is healthy — the panels an approver
// reads before pressing merge on GitHub.

import { useCallback, useEffect, useRef, useState } from "react";

import {
    abandonChangeSet,
    approveChangeSet,
    changeSetCollaborator,
    fetchReview,
    listChangeSets,
    mergeChangeSet,
    readChangeSet,
    readPackage,
    reconcileChangeSet,
    retitleChangeSet,
    saveEdit,
    submitChangeSet,
    withdrawApproval,
    type ApprovalPolicyStatus,
    type ApprovalRecord,
    type ChangeSet,
    type ChangeSetCollaborator,
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
import { entityLabel, entitySteps } from "@/components/entity-link";
import { formatInstant } from "@/lib/format";
import { SearchableList } from "@/lib/ui-kit";
import { githubBranchUrl, githubCommitUrl } from "@/lib/github";
import {
    changeSetUrl,
    navigate,
    packageUrl,
    treeUrl,
    type AddressStep,
    type ViewState,
} from "@/lib/router";

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
                <SearchableList
                    label="Search change sets"
                    placeholder="Search change sets"
                    emptyLabel="No change set matches that search."
                    className="row-list"
                >
                    {changeSets.map((changeSet) => (
                        <a
                            className="row"
                            key={changeSet.id}
                            href={`#${changeSetUrl(treeId, changeSet.id)}`}
                            data-search={`${changeSet.title} ${changeSet.branch} ${changeSet.prNumber ?? ""} ${changeSet.authorPrincipal} ${changeSet.state}`}
                        >
                            <span className="row-text">
                                <span className="row-title">
                                    {changeSet.title}
                                </span>
                                <span className="row-sub mono">
                                    {changeSet.branch}
                                    {changeSet.prNumber !== null
                                        ? ` · PR #${changeSet.prNumber}`
                                        : ""}
                                    {` · ${changeSet.authorPrincipal}`}
                                </span>
                            </span>
                            <span className="row-side">
                                <StatePill changeSet={changeSet} />
                            </span>
                        </a>
                    ))}
                </SearchableList>
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
    const [titleDraft, setTitleDraft] = useState<string | null>(null);
    const [collaboratorDraft, setCollaboratorDraft] = useState("");

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
    const tree = me?.capabilities?.sourceTrees.find(
        (candidate) => candidate.id === changeSet.sourceTreeId,
    );
    const branchUrl =
        tree === undefined ? null : githubBranchUrl(tree, changeSet.branch);
    const headUrl =
        tree === undefined || changeSet.headSha === null
            ? null
            : githubCommitUrl(tree, changeSet.headSha);
    // The workbench opens with this change set active, so edits land here.
    const workbenchState: ViewState = {
        changeSetId: changeSet.id,
        pin: null,
        context: null,
    };
    const open = changeSet.state === "draft" || changeSet.state === "proposed";
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
                    {titleDraft !== null ? (
                        <form
                            className="inline-form"
                            onSubmit={(event) => {
                                event.preventDefault();
                                const title = titleDraft.trim();
                                setTitleDraft(null);
                                act(retitleChangeSet(changeSet.id, title));
                            }}
                        >
                            <input
                                autoFocus
                                className="input"
                                value={titleDraft}
                                onChange={(event) =>
                                    setTitleDraft(event.target.value)
                                }
                            />
                            <button
                                className="btn btn-primary btn-sm"
                                type="submit"
                                disabled={busy || titleDraft.trim() === ""}
                            >
                                Save
                            </button>
                            <button
                                className="btn btn-ghost btn-sm"
                                type="button"
                                onClick={() => setTitleDraft(null)}
                            >
                                Cancel
                            </button>
                        </form>
                    ) : (
                        <h1>{changeSet.title}</h1>
                    )}
                    <p className="changeset-meta">
                        <span className="pill pill-neutral mono branch-pill">
                            ⎇{" "}
                            {branchUrl !== null ? (
                                <a
                                    className="row-link"
                                    href={branchUrl}
                                    rel="noreferrer"
                                    target="_blank"
                                    title="Open this branch on GitHub"
                                >
                                    {changeSet.branch}
                                </a>
                            ) : (
                                changeSet.branch
                            )}
                            {changeSet.headSha !== null ? (
                                <>
                                    {" @ "}
                                    {headUrl !== null ? (
                                        <a
                                            className="row-link"
                                            href={headUrl}
                                            rel="noreferrer"
                                            target="_blank"
                                            title="Open the head commit on GitHub"
                                        >
                                            {changeSet.headSha.slice(0, 10)}
                                        </a>
                                    ) : (
                                        changeSet.headSha.slice(0, 10)
                                    )}
                                </>
                            ) : null}
                        </span>
                        <span className="dot-sep">·</span>
                        <span>
                            base{" "}
                            <span className="mono">{changeSet.baseRef}</span>
                        </span>
                        <span className="dot-sep">·</span>
                        <span>
                            author{" "}
                            <span className="mono">
                                {changeSet.authorPrincipal}
                            </span>
                        </span>
                        <span className="dot-sep">·</span>
                        <span>
                            {changeSet.prUrl !== null ? (
                                <a
                                    className="row-link"
                                    href={changeSet.prUrl}
                                    rel="noreferrer"
                                    target="_blank"
                                >
                                    PR #{changeSet.prNumber}
                                </a>
                            ) : (
                                "no PR yet"
                            )}
                        </span>
                        <span className="dot-sep">·</span>
                        <span>
                            observed{" "}
                            {changeSet.lastReconciledAt === null
                                ? "never"
                                : formatInstant(changeSet.lastReconciledAt)}
                        </span>
                    </p>
                </div>
                <div className="section-header-actions">
                    <StatePill changeSet={changeSet} />
                    <button
                        className="btn btn-secondary btn-sm"
                        title="Open the workbench with this change set active"
                        onClick={() =>
                            navigate(
                                treeUrl(changeSet.sourceTreeId, workbenchState),
                            )
                        }
                    >
                        Open workbench
                    </button>
                    {changeSet.state === "draft" ? (
                        <button
                            className="btn btn-primary btn-sm"
                            disabled={busy}
                            onClick={() => act(submitChangeSet(changeSet.id))}
                        >
                            Submit (open PR)
                        </button>
                    ) : null}
                    <ActionsMenu
                        disabled={busy}
                        items={[
                            ...(open
                                ? [
                                      {
                                          label: "Rename",
                                          onPick: () =>
                                              setTitleDraft(changeSet.title),
                                      },
                                      {
                                          label: "Abandon",
                                          danger: true,
                                          onPick: () =>
                                              act(
                                                  abandonChangeSet(
                                                      changeSet.id,
                                                  ),
                                              ),
                                      },
                                  ]
                                : []),
                            {
                                label: "Check GitHub now",
                                onPick: () =>
                                    act(reconcileChangeSet(changeSet.id)),
                            },
                        ]}
                    />
                </div>
            </div>

            {/* Sharing is a team-mode idea: local mode has one implicit
                principal, so the panel would only ever say no. */}
            {open && me?.authMode === "team" ? (
                <details className="card variable-disclosure">
                    <summary>
                        <span className="row-text">
                            <span className="row-title">Collaborators</span>
                            <span className="row-sub">
                                {detail.collaborators.length === 0
                                    ? "None yet; the author edits alone."
                                    : detail.collaborators
                                          .map(collaboratorName)
                                          .join(", ")}
                            </span>
                        </span>
                    </summary>
                    {detail.collaborators.length > 0 ? (
                        <div className="row-list">
                            {detail.collaborators.map((collaborator) => (
                                <div
                                    className="row row-static"
                                    key={collaborator.principalId}
                                >
                                    <span className="row-text">
                                        <span className="row-title">
                                            {collaboratorName(collaborator)}
                                        </span>
                                        {collaborator.login !== null ? (
                                            <span className="row-sub mono">
                                                @{collaborator.login}
                                            </span>
                                        ) : null}
                                    </span>
                                    <span className="row-side">
                                        <button
                                            className="btn btn-icon btn-sm btn-remove"
                                            disabled={busy}
                                            title="Remove; edits they already made stay"
                                            onClick={() =>
                                                act(
                                                    changeSetCollaborator(
                                                        changeSet.id,
                                                        {
                                                            principalId:
                                                                collaborator.principalId,
                                                            remove: true,
                                                        },
                                                    ),
                                                )
                                            }
                                        >
                                            ×
                                        </button>
                                    </span>
                                </div>
                            ))}
                        </div>
                    ) : null}
                    <form
                        className="action-row"
                        onSubmit={(event) => {
                            event.preventDefault();
                            const login = collaboratorDraft.trim();
                            setCollaboratorDraft("");
                            act(changeSetCollaborator(changeSet.id, { login }));
                        }}
                    >
                        <input
                            aria-label="Teammate's GitHub login"
                            className="input mono"
                            placeholder="GitHub login"
                            value={collaboratorDraft}
                            onChange={(event) =>
                                setCollaboratorDraft(event.target.value)
                            }
                        />
                        <button
                            className="btn btn-secondary btn-sm"
                            type="submit"
                            disabled={busy || collaboratorDraft.trim() === ""}
                        >
                            Add collaborator
                        </button>
                    </form>
                </details>
            ) : null}

            {open ? (
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
                            <span className="tl-when">
                                {formatInstant(event.at)}
                            </span>
                        </span>
                    </div>
                ))}
            </div>
        </div>
    );
}

// A collaborator's human name: the display name, the GitHub login, or as a
// last resort the principal id.
function collaboratorName(collaborator: ChangeSetCollaborator): string {
    return (
        collaborator.displayName ??
        collaborator.login ??
        collaborator.principalId
    );
}

// The header's overflow menu: the secondary actions (rename, abandon,
// reconcile) that would otherwise crowd the primary pair.
function ActionsMenu({
    disabled,
    items,
}: {
    disabled: boolean;
    items: { label: string; danger?: boolean; onPick: () => void }[];
}) {
    const [open, setOpen] = useState(false);
    const wrapper = useRef<HTMLDivElement>(null);
    if (items.length === 0) {
        return null;
    }
    return (
        <div
            className="menu-control"
            ref={wrapper}
            onBlur={(event) => {
                if (!wrapper.current?.contains(event.relatedTarget as Node)) {
                    setOpen(false);
                }
            }}
        >
            <button
                aria-expanded={open}
                aria-label="More actions"
                className="btn btn-secondary btn-sm"
                disabled={disabled}
                type="button"
                onClick={() => setOpen(!open)}
            >
                …
            </button>
            {open ? (
                <div className="menu" role="menu">
                    {items.map((item) => (
                        <button
                            className={
                                item.danger === true
                                    ? "menu-item menu-item-danger"
                                    : "menu-item"
                            }
                            key={item.label}
                            role="menuitem"
                            type="button"
                            onClick={() => {
                                setOpen(false);
                                item.onPick();
                            }}
                        >
                            {item.label}
                        </button>
                    ))}
                </div>
            ) : null}
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
    const myId =
        me?.principal?.id ?? (me?.authMode === "local" ? "local" : null);
    const isContributor = myId !== null && contributors.includes(myId);
    const hasApproved =
        myId !== null &&
        approvals.some((approval) => approval.principalId === myId);

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
                        ) : hasApproved ? (
                            <button
                                className="btn btn-ghost btn-sm"
                                disabled={acting}
                                title="Changed your mind? Approvals withdraw while the change set is still proposed."
                                onClick={() =>
                                    act(withdrawApproval(changeSet.id))
                                }
                            >
                                Withdraw approval
                            </button>
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
    // Review links open the workbench on the change set's branch, so what
    // the reviewer inspects is what the change set holds.
    const reviewState: ViewState = {
        changeSetId: changeSet.id,
        pin: null,
        context: null,
    };
    const hrefEntity = (steps: AddressStep[]): string =>
        `#${packageUrl(
            changeSet.sourceTreeId,
            pkg.path,
            { kind: "address", steps },
            reviewState,
        )}`;
    return (
        <div className="section">
            {/* The root package's path is ".", which reads as a stray dot;
                the heading only earns its place when packages need telling
                apart. */}
            {pkg.path !== "." ? (
                <div className="section-header-text">
                    <h3>
                        <a
                            className="row-link mono"
                            href={`#${packageUrl(
                                changeSet.sourceTreeId,
                                pkg.path,
                                { kind: "overview" },
                                reviewState,
                            )}`}
                            title="Open this package in the workbench, on this change set"
                        >
                            {pkg.path}
                        </a>
                    </h3>
                </div>
            ) : null}
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
                <div className="change-summary-head">
                    <h3>Changes</h3>
                    <ChangeCounts changes={pkg.changes} />
                </div>
                {pkg.changes.length === 0 ? (
                    <p className="hint">No semantic changes.</p>
                ) : (
                    <div className="change-list">
                        {pkg.changes.map((change, index) => (
                            <SemanticChangeRow
                                key={index}
                                change={change}
                                hrefEntity={hrefEntity}
                            />
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
                    hrefEntity={hrefEntity}
                    onPromoted={onPromoted}
                />
            </div>

            {pkg.lint.introduced.length === 0 &&
            pkg.lint.resolved.length === 0 ? (
                <div className="card lint-quiet">
                    <span className="label">lint</span>
                    <span className="hint">
                        Unchanged: this change introduces no findings and
                        resolves none.
                    </span>
                </div>
            ) : (
                <div className="card">
                    <h3>Lint</h3>
                    <LintDeltaView lint={pkg.lint} />
                </div>
            )}
        </div>
    );
}

// Whether a change adds, removes, or reshapes its target, read off the
// stable kind suffix.
function changeOp(kind: string): "add" | "del" | "mod" {
    return kind.endsWith("_added")
        ? "add"
        : kind.endsWith("_removed")
          ? "del"
          : "mod";
}

// The plan-style totals beside the Changes heading: how much of each verb,
// at a glance, before reading a single row.
function ChangeCounts({ changes }: { changes: SemanticChange[] }) {
    const count = (op: "add" | "del" | "mod") =>
        changes.filter((change) => changeOp(change.kind) === op).length;
    return (
        <span className="label">
            {count("mod")} changed · {count("add")} added · {count("del")}{" "}
            removed
        </span>
    );
}

// The changed value as unified-diff lines: the removed shape in red, the
// added shape in green, keyed by the JSON path segment the change names.
// Composite values pretty-print so a nested change is still readable.
function DiffLines({ change }: { change: SemanticChange }) {
    const path = (change.target.field as { path?: string[] } | undefined)?.path;
    const key = path !== undefined ? (path[path.length - 1] ?? null) : null;
    const side = (
        op: "add" | "del",
        value: unknown,
    ): { op: "add" | "del"; text: string }[] => {
        const text = JSON.stringify(value, null, 2) ?? "null";
        return text.split("\n").map((line, index) => ({
            op,
            text: index === 0 && key !== null ? `"${key}": ${line}` : line,
        }));
    };
    const lines = [
        ...(change.before !== undefined ? side("del", change.before) : []),
        ...(change.after !== undefined ? side("add", change.after) : []),
    ];
    if (lines.length === 0) {
        return null;
    }
    return (
        <div className="change-diff">
            {lines.map((line, index) => (
                <div className="change-diff-line" data-op={line.op} key={index}>
                    <span aria-hidden="true" className="change-diff-gutter">
                        {line.op === "del" ? "-" : "+"}
                    </span>
                    {line.text}
                </div>
            ))}
        </div>
    );
}

function SemanticChangeRow({
    change,
    hrefEntity,
}: {
    change: SemanticChange;
    hrefEntity: (steps: AddressStep[]) => string;
}) {
    const steps = entitySteps(change.target.entity);
    // Review targets carry snake_case kinds; the entity-link helpers accept
    // both spellings. The change kind itself rides on the marker's tooltip.
    const label = entityLabel(change.target.entity);
    const path = (change.target.field as { path?: string[] } | undefined)?.path;
    const op = changeOp(change.kind);
    return (
        <div className="change-row" data-op={op}>
            <div
                className="change-row-head"
                title={change.kind.replaceAll("_", " ")}
            >
                <span aria-hidden="true" className="change-marker mono">
                    {op === "add" ? "+" : op === "del" ? "-" : "~"}
                </span>
                <span className="change-path mono">
                    {steps === null ? (
                        label
                    ) : (
                        <a href={hrefEntity(steps)}>{label}</a>
                    )}
                    {(path ?? []).map((segment, index) => (
                        <span key={index}>
                            <span className="crumb-sep"> › </span>
                            {segment}
                        </span>
                    ))}
                </span>
                <span className="mono review-values">
                    {change.before !== undefined ? (
                        <span className="review-before">
                            {clip(change.before)}
                        </span>
                    ) : null}
                    {change.before !== undefined && change.after !== undefined
                        ? " → "
                        : null}
                    {change.after !== undefined ? (
                        <span className="review-after">
                            {clip(change.after)}
                        </span>
                    ) : null}
                </span>
            </div>
            <DiffLines change={change} />
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
    hrefEntity,
    onPromoted,
}: {
    changeSet: ChangeSet;
    headPin: string;
    pkg: PackageReview;
    hrefEntity: (steps: AddressStep[]) => string;
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
                            <a
                                className="row-link mono"
                                href={hrefEntity([
                                    { class: "variable", id: variable.id },
                                ])}
                            >
                                {variable.id}
                            </a>
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
                        hrefEntity={hrefEntity}
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
    hrefEntity,
    onPromote,
}: {
    impact: ContextImpact;
    context: ReviewContext | null;
    promoting: boolean;
    hrefEntity: (steps: AddressStep[]) => string;
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
                    <a
                        className="row-link"
                        href={hrefEntity([
                            { class: "variable", id: outcome.variable },
                        ])}
                    >
                        {outcome.variable}
                    </a>
                    :{" "}
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

// The lint delta when there is one; an unchanged delta renders as the
// quiet one-line strip in PackageReviewView instead of a card.
function LintDeltaView({
    lint,
}: {
    lint: { introduced: LintDiagnostic[]; resolved: LintDiagnostic[] };
}) {
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
