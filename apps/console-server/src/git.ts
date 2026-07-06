// Git-data operations against GitHub (design/console-git-ops.md): refs,
// one-commit edit plans, compare, and pull requests. Everything here acts
// with an explicit caller-supplied token — in C2 that is always a user's own
// token, so GitHub itself enforces permissions and this layer only moves
// files and refs.
//
// updateRef is the compare-and-swap that makes rule 3 ("one edit, one
// commit") atomic: our new commit's parent is the head we read, and GitHub's
// fast-forward check refuses the update when the branch has moved past it.

const GITHUB_API = "https://api.github.com";
const GITHUB_USER_AGENT = "rototo-console";

export type RepoId = { owner: string; name: string };

// One edit plan as a commit: repo-relative paths, opaque contents.
export type CommitInput = {
    parent: string;
    message: string;
    writes: { path: string; content: string }[];
    deletes: string[];
};

export type PullRecord = {
    number: number;
    url: string;
    state: "open" | "closed";
    merged: boolean;
    title: string;
    body: string;
    headRef: string;
    headSha: string;
    baseRef: string;
    authorLogin: string | null;
    // GitHub's mergeable_state; "dirty" means the branch conflicts.
    mergeableState: string;
};

export type CompareResult = {
    aheadBy: number;
    behindBy: number;
    // The merge base's sha: what a review diffs `head` against, so base
    // drift never shows up as part of the change under review.
    mergeBase: string;
    // Paths changed on `head` since the merge base (GitHub's three-dot
    // compare); what the staleness check intersects with a plan's paths.
    files: string[];
};

// One commit in a branch's history; what the console's time views list.
export type CommitRecord = {
    sha: string;
    message: string;
    authorName: string | null;
    // The committer date, RFC3339. `until` filters on it, which is how
    // "what was this value on March 3rd" finds its pin.
    date: string;
};

export type ListCommitsOptions = {
    ref: string;
    // Restrict to commits touching this path (a package directory).
    path?: string;
    // Only commits at or before this RFC3339 instant.
    until?: string;
    perPage?: number;
};

export interface GitOps {
    // The pin a branch points at, or null when the branch does not exist.
    getRef(token: string, repo: RepoId, branch: string): Promise<string | null>;
    createRef(
        token: string,
        repo: RepoId,
        branch: string,
        sha: string,
    ): Promise<void>;
    // Fast-forward only; false means the head moved and the caller should
    // re-read and retry (or give up).
    updateRef(
        token: string,
        repo: RepoId,
        branch: string,
        sha: string,
    ): Promise<boolean>;
    deleteRef(token: string, repo: RepoId, branch: string): Promise<void>;
    // Blobs, tree, commit — one call here, three at GitHub. Returns the new
    // commit sha; nothing moves until updateRef succeeds.
    createCommit(
        token: string,
        repo: RepoId,
        commit: CommitInput,
    ): Promise<string>;
    compare(
        token: string,
        repo: RepoId,
        base: string,
        head: string,
    ): Promise<CompareResult>;
    createPull(
        token: string,
        repo: RepoId,
        input: { title: string; body: string; head: string; base: string },
    ): Promise<PullRecord>;
    getPull(
        token: string,
        repo: RepoId,
        number: number,
    ): Promise<PullRecord | null>;
    // The newest pull (any state) whose head is the given branch.
    pullForBranch(
        token: string,
        repo: RepoId,
        branch: string,
    ): Promise<PullRecord | null>;
    closePull(token: string, repo: RepoId, number: number): Promise<void>;
    // Branches under a prefix; what the fire drill walks.
    listBranches(
        token: string,
        repo: RepoId,
        prefix: string,
    ): Promise<{ name: string; sha: string }[]>;
    // History newest-first from a ref, optionally path-scoped and bounded
    // by an instant.
    listCommits(
        token: string,
        repo: RepoId,
        options: ListCommitsOptions,
    ): Promise<CommitRecord[]>;
}

export class GitHubGit implements GitOps {
    async getRef(
        token: string,
        repo: RepoId,
        branch: string,
    ): Promise<string | null> {
        const response = await this.request(
            token,
            "GET",
            `/repos/${repo.owner}/${repo.name}/git/ref/heads/${branch}`,
        );
        if (response.status === 404) {
            return null;
        }
        await ensureOk(response, "resolve ref");
        const body = (await response.json()) as { object: { sha: string } };
        return body.object.sha;
    }

    async createRef(
        token: string,
        repo: RepoId,
        branch: string,
        sha: string,
    ): Promise<void> {
        const response = await this.request(
            token,
            "POST",
            `/repos/${repo.owner}/${repo.name}/git/refs`,
            { ref: `refs/heads/${branch}`, sha },
        );
        await ensureOk(response, "create branch");
    }

    async updateRef(
        token: string,
        repo: RepoId,
        branch: string,
        sha: string,
    ): Promise<boolean> {
        const response = await this.request(
            token,
            "PATCH",
            `/repos/${repo.owner}/${repo.name}/git/refs/heads/${branch}`,
            { sha, force: false },
        );
        // 422 is GitHub refusing a non-fast-forward: the head moved.
        if (response.status === 422) {
            return false;
        }
        await ensureOk(response, "move branch");
        return true;
    }

    async deleteRef(
        token: string,
        repo: RepoId,
        branch: string,
    ): Promise<void> {
        const response = await this.request(
            token,
            "DELETE",
            `/repos/${repo.owner}/${repo.name}/git/refs/heads/${branch}`,
        );
        // Already gone is fine; deletion is idempotent here.
        if (response.status !== 404 && response.status !== 422) {
            await ensureOk(response, "delete branch");
        }
    }

    async createCommit(
        token: string,
        repo: RepoId,
        commit: CommitInput,
    ): Promise<string> {
        const base = `/repos/${repo.owner}/${repo.name}`;
        const parentResponse = await this.request(
            token,
            "GET",
            `${base}/git/commits/${commit.parent}`,
        );
        await ensureOk(parentResponse, "read parent commit");
        const parent = (await parentResponse.json()) as {
            tree: { sha: string };
        };

        const entries: Record<string, unknown>[] = commit.writes.map(
            (write) => ({
                path: write.path,
                mode: "100644",
                type: "blob",
                content: write.content,
            }),
        );
        for (const path of commit.deletes) {
            entries.push({ path, mode: "100644", type: "blob", sha: null });
        }
        const treeResponse = await this.request(
            token,
            "POST",
            `${base}/git/trees`,
            { base_tree: parent.tree.sha, tree: entries },
        );
        await ensureOk(treeResponse, "build tree");
        const tree = (await treeResponse.json()) as { sha: string };

        const commitResponse = await this.request(
            token,
            "POST",
            `${base}/git/commits`,
            {
                message: commit.message,
                tree: tree.sha,
                parents: [commit.parent],
            },
        );
        await ensureOk(commitResponse, "create commit");
        return ((await commitResponse.json()) as { sha: string }).sha;
    }

    async compare(
        token: string,
        repo: RepoId,
        base: string,
        head: string,
    ): Promise<CompareResult> {
        const response = await this.request(
            token,
            "GET",
            `/repos/${repo.owner}/${repo.name}/compare/${base}...${head}?per_page=100`,
        );
        await ensureOk(response, "compare commits");
        const body = (await response.json()) as {
            ahead_by: number;
            behind_by: number;
            merge_base_commit: { sha: string };
            files?: { filename: string }[];
        };
        return {
            aheadBy: body.ahead_by,
            behindBy: body.behind_by,
            mergeBase: body.merge_base_commit.sha,
            files: (body.files ?? []).map((file) => file.filename),
        };
    }

    async createPull(
        token: string,
        repo: RepoId,
        input: { title: string; body: string; head: string; base: string },
    ): Promise<PullRecord> {
        const response = await this.request(
            token,
            "POST",
            `/repos/${repo.owner}/${repo.name}/pulls`,
            input,
        );
        await ensureOk(response, "open pull request");
        return pullFromWire((await response.json()) as Record<string, unknown>);
    }

    async getPull(
        token: string,
        repo: RepoId,
        number: number,
    ): Promise<PullRecord | null> {
        const response = await this.request(
            token,
            "GET",
            `/repos/${repo.owner}/${repo.name}/pulls/${number}`,
        );
        if (response.status === 404) {
            return null;
        }
        await ensureOk(response, "read pull request");
        return pullFromWire((await response.json()) as Record<string, unknown>);
    }

    async pullForBranch(
        token: string,
        repo: RepoId,
        branch: string,
    ): Promise<PullRecord | null> {
        const response = await this.request(
            token,
            "GET",
            `/repos/${repo.owner}/${repo.name}/pulls?head=${repo.owner}:${branch}&state=all&sort=created&direction=desc&per_page=1`,
        );
        await ensureOk(response, "find pull request");
        const pulls = (await response.json()) as Record<string, unknown>[];
        return pulls.length === 0
            ? null
            : pullFromWire(pulls[0] as Record<string, unknown>);
    }

    async closePull(
        token: string,
        repo: RepoId,
        number: number,
    ): Promise<void> {
        const response = await this.request(
            token,
            "PATCH",
            `/repos/${repo.owner}/${repo.name}/pulls/${number}`,
            { state: "closed" },
        );
        await ensureOk(response, "close pull request");
    }

    async listBranches(
        token: string,
        repo: RepoId,
        prefix: string,
    ): Promise<{ name: string; sha: string }[]> {
        const response = await this.request(
            token,
            "GET",
            `/repos/${repo.owner}/${repo.name}/git/matching-refs/heads/${prefix}?per_page=100`,
        );
        await ensureOk(response, "list branches");
        const refs = (await response.json()) as {
            ref: string;
            object: { sha: string };
        }[];
        return refs.map((ref) => ({
            name: ref.ref.replace(/^refs\/heads\//, ""),
            sha: ref.object.sha,
        }));
    }

    async listCommits(
        token: string,
        repo: RepoId,
        options: ListCommitsOptions,
    ): Promise<CommitRecord[]> {
        const query = new URLSearchParams({
            sha: options.ref,
            per_page: String(options.perPage ?? 50),
        });
        if (options.path !== undefined) {
            query.set("path", options.path);
        }
        if (options.until !== undefined) {
            query.set("until", options.until);
        }
        const response = await this.request(
            token,
            "GET",
            `/repos/${repo.owner}/${repo.name}/commits?${query.toString()}`,
        );
        await ensureOk(response, "list commits");
        const commits = (await response.json()) as {
            sha: string;
            commit: {
                message: string;
                author: { name?: string } | null;
                committer: { date: string } | null;
            };
        }[];
        return commits.map((entry) => ({
            sha: entry.sha,
            message: entry.commit.message,
            authorName: entry.commit.author?.name ?? null,
            date: entry.commit.committer?.date ?? "",
        }));
    }

    private request(
        token: string,
        method: string,
        path: string,
        body?: unknown,
    ): Promise<Response> {
        return fetch(`${GITHUB_API}${path}`, {
            method,
            headers: {
                accept: "application/vnd.github+json",
                authorization: `Bearer ${token}`,
                "user-agent": GITHUB_USER_AGENT,
                "x-github-api-version": "2022-11-28",
                ...(body === undefined
                    ? {}
                    : { "content-type": "application/json" }),
            },
            body: body === undefined ? undefined : JSON.stringify(body),
        });
    }
}

function pullFromWire(wire: Record<string, unknown>): PullRecord {
    const head = wire.head as { ref: string; sha: string };
    const base = wire.base as { ref: string };
    const user = wire.user as { login?: string } | null;
    return {
        number: wire.number as number,
        url: wire.html_url as string,
        state: wire.state as "open" | "closed",
        merged: Boolean(wire.merged ?? wire.merged_at),
        title: (wire.title as string | null) ?? "",
        body: (wire.body as string | null) ?? "",
        headRef: head.ref,
        headSha: head.sha,
        baseRef: base.ref,
        authorLogin: user?.login ?? null,
        mergeableState: (wire.mergeable_state as string | null) ?? "unknown",
    };
}

async function ensureOk(response: Response, doing: string): Promise<void> {
    if (response.ok) {
        return;
    }
    let message = "";
    try {
        const body = (await response.json()) as { message?: string };
        message = body.message ?? "";
    } catch {
        // Non-JSON error body; the status alone will have to explain.
    }
    throw new Error(
        `GitHub refused to ${doing} (${response.status}${message === "" ? "" : `: ${message}`})`,
    );
}
