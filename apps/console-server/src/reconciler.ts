// The reconciler (design/console-git-ops.md rule 4): a loop with one job —
// make our rows match GitHub. It is the single writer of the observed
// columns (prNumber through observedVia), it never copies state from
// anything but a direct GitHub read, and every pass is idempotent, so
// running it twice, late, or after a crash changes nothing.

import { branchFor } from "./change-sets.ts";
import type { GitOps, RepoId } from "./git.ts";
import type { ChangeSetRow, ChangeSetState, Store } from "./store.ts";

type ReconcilerDeps = {
    store: Store;
    git: GitOps;
    // The token a pass reads GitHub with; in C2 the change-set author's own
    // credential. Null means we cannot look right now — skip, stay stale.
    tokenFor: (changeSet: ChangeSetRow) => Promise<string | null>;
};

export class Reconciler {
    private readonly deps: ReconcilerDeps;
    private timer: NodeJS.Timeout | null = null;

    constructor(deps: ReconcilerDeps) {
        this.deps = deps;
    }

    // Every few minutes; a webhook nudge would call reconcileAll sooner.
    start(intervalMs: number): void {
        if (this.timer !== null) {
            return;
        }
        this.timer = setInterval(() => {
            void this.reconcileAll().catch(() => {
                // A failed pass is harmless; the next one re-derives
                // everything from GitHub.
            });
        }, intervalMs);
        this.timer.unref();
    }

    stop(): void {
        if (this.timer !== null) {
            clearInterval(this.timer);
            this.timer = null;
        }
    }

    async reconcileAll(): Promise<void> {
        for (const changeSet of this.deps.store.listOpenChangeSets()) {
            const token = await this.deps.tokenFor(changeSet);
            if (token === null) {
                continue;
            }
            await this.reconcile(changeSet, token);
        }
    }

    async reconcile(
        changeSet: ChangeSetRow,
        token: string,
    ): Promise<ChangeSetRow> {
        const { store, git } = this.deps;
        if (changeSet.state === "merged" || changeSet.state === "abandoned") {
            return changeSet;
        }
        const tree = store.getSourceTree(changeSet.sourceTreeId);
        if (
            tree === null ||
            tree.kind !== "github" ||
            tree.owner === null ||
            tree.name === null
        ) {
            return changeSet;
        }
        const repo: RepoId = { owner: tree.owner, name: tree.name };
        const branch = branchFor(changeSet.id);

        const branchSha = await git.getRef(token, repo, branch);
        const pull =
            changeSet.prNumber !== null
                ? await git.getPull(token, repo, changeSet.prNumber)
                : await git.pullForBranch(token, repo, branch);

        let state: ChangeSetState = changeSet.state;
        if (pull !== null && pull.merged) {
            state = "merged";
        } else if (pull !== null && pull.state === "closed") {
            state = "abandoned";
        } else if (pull !== null) {
            // A PR exists and is open; a draft with an externally opened PR
            // is a proposal in fact, so record it as one.
            state = "proposed";
        } else if (branchSha === null) {
            // No PR and the branch is gone: deleted externally.
            state = "abandoned";
        }

        let behindBase = false;
        let conflicted = false;
        if (state === "draft" || state === "proposed") {
            if (branchSha !== null) {
                const comparison = await git.compare(
                    token,
                    repo,
                    changeSet.baseRef,
                    branchSha,
                );
                behindBase = comparison.behindBy > 0;
            }
            conflicted = pull !== null && pull.mergeableState === "dirty";
        }

        store.updateChangeSetObserved(changeSet.id, {
            state,
            prNumber: pull?.number ?? changeSet.prNumber,
            prUrl: pull?.url ?? changeSet.prUrl,
            headSha: branchSha ?? pull?.headSha ?? changeSet.headSha,
            behindBase,
            conflicted,
            observedVia: "reconciler",
        });
        if (state !== changeSet.state) {
            store.appendChangeSetEvent(
                changeSet.id,
                null,
                state,
                JSON.stringify({
                    observedVia: "reconciler",
                    pr: pull?.number ?? null,
                }),
            );
        }
        return this.deps.store.getChangeSet(changeSet.id) as ChangeSetRow;
    }
}
