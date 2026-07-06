// The fire drill (design/console-git-ops.md rule 1): the store is
// bookkeeping, so a lost database rebuilds from GitHub. Walk the
// rototo-console/* branches, find each one's PR, read the
// Rototo-Change-Set marker, and map PR facts back onto the four states.
// What genuinely cannot come back is the diary (and, later, approvals) —
// which is why the console also writes into the PR timeline.

import { BRANCH_PREFIX, parseChangeSetMarker } from "./change-sets.ts";
import type { GitOps, RepoId } from "./git.ts";
import type {
    ChangeSetRow,
    ChangeSetState,
    SourceTreeRow,
    Store,
} from "./store.ts";

export async function rebuildChangeSets(input: {
    store: Store;
    git: GitOps;
    tree: SourceTreeRow;
    token: string;
}): Promise<ChangeSetRow[]> {
    const { store, git, tree, token } = input;
    if (tree.kind !== "github" || tree.owner === null || tree.name === null) {
        return [];
    }
    const repo: RepoId = { owner: tree.owner, name: tree.name };
    const rebuilt: ChangeSetRow[] = [];
    for (const branch of await git.listBranches(token, repo, BRANCH_PREFIX)) {
        const branchId = branch.name.slice(BRANCH_PREFIX.length);
        const pull = await git.pullForBranch(token, repo, branch.name);
        // The branch name and the marker both carry the id; the marker wins
        // because it is what a human would have to forge to confuse us.
        const id =
            (pull === null ? null : parseChangeSetMarker(pull.body)) ??
            branchId;
        if (store.getChangeSet(id) !== null) {
            continue;
        }
        let state: ChangeSetState = "draft";
        if (pull !== null) {
            state = pull.merged
                ? "merged"
                : pull.state === "open"
                  ? "proposed"
                  : "abandoned";
        }
        // Authorship rebuilds best-effort from the PR author's login; the
        // principal itself may not have re-enrolled yet.
        const identity =
            pull === null || pull.authorLogin === null
                ? null
                : store.identityByLogin("github", pull.authorLogin);
        const author = identity?.principalId ?? "unknown";
        const row = store.insertChangeSet({
            id,
            sourceTreeId: tree.id,
            title: pull?.title ?? id,
            authorPrincipal: author,
            actingMode: "user",
            baseRef: pull?.baseRef ?? tree.defaultBranch ?? "main",
            baseShaAtCreation: null,
            state,
        });
        store.appendChangeSetEvent(
            id,
            null,
            "rebuilt",
            JSON.stringify({
                branch: branch.name,
                pr: pull?.number ?? null,
                state,
            }),
        );
        rebuilt.push(row);
    }
    return rebuilt;
}
