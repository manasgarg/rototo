// Links out to the source tree's GitHub home. The console never writes
// through these; they exist so every sha, branch, and repository the UI
// mentions can be checked at the source of truth.

type TreeIdentity = {
    kind: "github" | "local";
    owner: string | null;
    name: string | null;
};

export function githubRepoUrl(tree: TreeIdentity): string | null {
    if (tree.kind !== "github" || tree.owner === null || tree.name === null) {
        return null;
    }
    return `https://github.com/${tree.owner}/${tree.name}`;
}

export function githubCommitUrl(
    tree: TreeIdentity,
    sha: string,
): string | null {
    const repo = githubRepoUrl(tree);
    return repo === null ? null : `${repo}/commit/${sha}`;
}

export function githubBranchUrl(
    tree: TreeIdentity,
    branch: string,
): string | null {
    const repo = githubRepoUrl(tree);
    return repo === null ? null : `${repo}/tree/${encodeURIComponent(branch)}`;
}
