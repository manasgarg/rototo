// A GitOps implementation over a real local bare repository, driven with
// git plumbing. Refs, commits, fast-forward checks, and compares are real
// git; pull requests are an in-memory ledger with the same observable facts
// GitHub reports. The bare repo doubles as the PinStore remote, so staged
// reads flow through the same content the fake mutates.

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import type {
    CommitInput,
    CommitRecord,
    CompareResult,
    GitOps,
    ListCommitsOptions,
    PullRecord,
    RepoId,
} from "../src/git.ts";

const COMMIT_ENV = {
    GIT_AUTHOR_NAME: "Rototo Test",
    GIT_AUTHOR_EMAIL: "rototo@example.com",
    GIT_COMMITTER_NAME: "Rototo Test",
    GIT_COMMITTER_EMAIL: "rototo@example.com",
};

export class FakeGit implements GitOps {
    // The bare repository; also the remote staged reads fetch from.
    readonly gitDir: string;
    private readonly scratch: string;
    private readonly pulls: PullRecord[] = [];
    private nextPullNumber = 1;
    private readonly conflictedPulls = new Set<number>();
    private nextIndex = 0;
    // Failure injection: make the next N ref updates report "head moved",
    // the way GitHub refuses a non-fast-forward during a write race.
    failNextRefUpdates = 0;
    // Optional token -> login mapping so PRs carry an author.
    readonly tokenLogins = new Map<string, string>();

    private constructor(scratch: string, gitDir: string) {
        this.scratch = scratch;
        this.gitDir = gitDir;
    }

    static init(): FakeGit {
        const scratch = mkdtempSync(path.join(tmpdir(), "rototo-fake-git-"));
        const gitDir = path.join(scratch, "repo.git");
        execFileSync("git", [
            "init",
            "--bare",
            "--quiet",
            "-b",
            "main",
            gitDir,
        ]);
        execFileSync("git", [
            "--git-dir",
            gitDir,
            "config",
            "uploadpack.allowReachableSHA1InWant",
            "true",
        ]);
        return new FakeGit(scratch, gitDir);
    }

    cleanup(): void {
        rmSync(this.scratch, { recursive: true, force: true });
    }

    // Seeds a branch with a directory's contents under a prefix; the
    // fixture equivalent of a repository's existing history.
    seedBranch(branch: string, sourceDir: string, prefix: string): string {
        const writes: { path: string; content: string }[] = [];
        for (const entry of readdirSync(sourceDir, {
            withFileTypes: true,
            recursive: true,
        })) {
            if (!entry.isFile() || entry.name.startsWith(".")) {
                continue;
            }
            const absolute = path.join(entry.parentPath, entry.name);
            const relative = path
                .relative(sourceDir, absolute)
                .split(path.sep)
                .join("/");
            writes.push({
                path: `${prefix}/${relative}`,
                content: readFileSync(absolute, "utf8"),
            });
        }
        const sha = this.commitRaw(null, `seed ${prefix}`, writes, []);
        this.plumb(["update-ref", `refs/heads/${branch}`, sha]);
        return sha;
    }

    // An "external" push: someone edits in vim and pushes (rule 6). The
    // optional date backdates the commit, so history tests can ask "what
    // was this value on March 3rd" against a real dated log.
    commitDirect(
        branch: string,
        message: string,
        writes: { path: string; content: string }[],
        deletes: string[] = [],
        date?: string,
    ): string {
        const parent = this.refSha(branch);
        if (parent === null) {
            throw new Error(`no such branch: ${branch}`);
        }
        const sha = this.commitRaw(parent, message, writes, deletes, date);
        this.plumb(["update-ref", `refs/heads/${branch}`, sha, parent]);
        return sha;
    }

    // An "external" merge: someone presses the button on GitHub. Squash
    // semantics with delete-branch-on-merge, like the deployments we care
    // about configure.
    mergePull(number: number): string {
        const pull = this.pulls.find((entry) => entry.number === number);
        if (pull === undefined || pull.state !== "open") {
            throw new Error(`pull #${number} is not open`);
        }
        const head = this.refSha(pull.headRef);
        const base = this.refSha(pull.baseRef);
        if (head === null || base === null) {
            throw new Error(`pull #${number} refs are gone`);
        }
        // Tests keep the base linear; a diverged base would need a real
        // merge and GitHub would be computing it, not us.
        this.assertAncestor(base, head, "base diverged; fake cannot merge");
        const tree = this.plumb(["rev-parse", `${head}^{tree}`]);
        const sha = this.plumb(
            [
                "commit-tree",
                tree,
                "-p",
                base,
                "-m",
                `${pull.title} (#${number})`,
            ],
            undefined,
        );
        this.plumb(["update-ref", `refs/heads/${pull.baseRef}`, sha, base]);
        pull.merged = true;
        pull.state = "closed";
        pull.headSha = head;
        this.plumb(["update-ref", "-d", `refs/heads/${pull.headRef}`]);
        return sha;
    }

    markConflicted(number: number): void {
        this.conflictedPulls.add(number);
    }

    readFileAt(pin: string, filePath: string): string {
        return this.plumb(["show", `${pin}:${filePath}`]);
    }

    commitCount(ref: string): number {
        return Number(this.plumb(["rev-list", "--count", ref]));
    }

    changedFiles(commit: string): string[] {
        const output = this.plumb([
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            commit,
        ]);
        return output === "" ? [] : output.split("\n");
    }

    // --- GitOps ---

    async getRef(
        _token: string,
        _repo: RepoId,
        branch: string,
    ): Promise<string | null> {
        return this.refSha(branch);
    }

    async createRef(
        _token: string,
        _repo: RepoId,
        branch: string,
        sha: string,
    ): Promise<void> {
        if (this.refSha(branch) !== null) {
            throw new Error(`branch already exists: ${branch}`);
        }
        this.plumb(["update-ref", `refs/heads/${branch}`, sha]);
    }

    async updateRef(
        _token: string,
        _repo: RepoId,
        branch: string,
        sha: string,
    ): Promise<boolean> {
        if (this.failNextRefUpdates > 0) {
            this.failNextRefUpdates--;
            return false;
        }
        const current = this.refSha(branch);
        if (current === null) {
            return false;
        }
        if (current !== sha && !this.isAncestor(current, sha)) {
            // GitHub's force=false refusal: not a fast forward.
            return false;
        }
        this.plumb(["update-ref", `refs/heads/${branch}`, sha, current]);
        return true;
    }

    async deleteRef(
        _token: string,
        _repo: RepoId,
        branch: string,
    ): Promise<void> {
        if (this.refSha(branch) !== null) {
            this.plumb(["update-ref", "-d", `refs/heads/${branch}`]);
        }
    }

    async createCommit(
        _token: string,
        _repo: RepoId,
        commit: CommitInput,
    ): Promise<string> {
        return this.commitRaw(
            commit.parent,
            commit.message,
            commit.writes,
            commit.deletes,
        );
    }

    async compare(
        _token: string,
        _repo: RepoId,
        base: string,
        head: string,
    ): Promise<CompareResult> {
        const mergeBase = this.plumb(["merge-base", base, head]);
        const aheadBy = Number(
            this.plumb(["rev-list", "--count", `${mergeBase}..${head}`]),
        );
        const behindBy = Number(
            this.plumb(["rev-list", "--count", `${mergeBase}..${base}`]),
        );
        const diff = this.plumb(["diff", "--name-only", mergeBase, head]);
        return {
            aheadBy,
            behindBy,
            mergeBase,
            files: diff === "" ? [] : diff.split("\n"),
        };
    }

    async createPull(
        token: string,
        _repo: RepoId,
        input: { title: string; body: string; head: string; base: string },
    ): Promise<PullRecord> {
        const headSha = this.refSha(input.head);
        if (headSha === null) {
            throw new Error(`no such branch: ${input.head}`);
        }
        const number = this.nextPullNumber++;
        const pull: PullRecord = {
            number,
            url: `https://github.example/pulls/${number}`,
            state: "open",
            merged: false,
            title: input.title,
            body: input.body,
            headRef: input.head,
            headSha,
            baseRef: input.base,
            authorLogin: this.tokenLogins.get(token) ?? null,
            mergeableState: "clean",
        };
        this.pulls.push(pull);
        return this.snapshot(pull);
    }

    async getPull(
        _token: string,
        _repo: RepoId,
        number: number,
    ): Promise<PullRecord | null> {
        const pull = this.pulls.find((entry) => entry.number === number);
        return pull === undefined ? null : this.snapshot(pull);
    }

    async pullForBranch(
        _token: string,
        _repo: RepoId,
        branch: string,
    ): Promise<PullRecord | null> {
        const matches = this.pulls.filter((entry) => entry.headRef === branch);
        const pull = matches[matches.length - 1];
        return pull === undefined ? null : this.snapshot(pull);
    }

    async closePull(
        _token: string,
        _repo: RepoId,
        number: number,
    ): Promise<void> {
        const pull = this.pulls.find((entry) => entry.number === number);
        if (pull === undefined || pull.state === "closed") {
            return;
        }
        pull.state = "closed";
    }

    async listCommits(
        _token: string,
        _repo: RepoId,
        options: ListCommitsOptions,
    ): Promise<CommitRecord[]> {
        const args = [
            "log",
            "--format=%H%x1f%an%x1f%cI%x1f%s",
            `--max-count=${options.perPage ?? 50}`,
        ];
        if (options.until !== undefined) {
            args.push(`--until=${options.until}`);
        }
        args.push(options.ref);
        if (options.path !== undefined) {
            args.push("--", options.path);
        }
        let output: string;
        try {
            output = this.plumb(args);
        } catch {
            return [];
        }
        if (output === "") {
            return [];
        }
        return output.split("\n").map((line) => {
            const [sha, authorName, date, message] = line.split("\x1f");
            return {
                sha: sha as string,
                message: message ?? "",
                authorName: authorName ?? null,
                date: date as string,
            };
        });
    }

    async listBranches(
        _token: string,
        _repo: RepoId,
        prefix: string,
    ): Promise<{ name: string; sha: string }[]> {
        const output = this.plumb([
            "for-each-ref",
            "--format=%(refname:short) %(objectname)",
            `refs/heads/${prefix}`,
        ]);
        if (output === "") {
            return [];
        }
        return output.split("\n").map((line) => {
            const [name, sha] = line.split(" ");
            return { name: name as string, sha: sha as string };
        });
    }

    // --- plumbing ---

    private commitRaw(
        parent: string | null,
        message: string,
        writes: { path: string; content: string }[],
        deletes: string[],
        date?: string,
    ): string {
        const indexFile = path.join(this.scratch, `index-${this.nextIndex++}`);
        const env = {
            ...process.env,
            ...COMMIT_ENV,
            ...(date === undefined
                ? {}
                : { GIT_AUTHOR_DATE: date, GIT_COMMITTER_DATE: date }),
            GIT_DIR: this.gitDir,
            GIT_INDEX_FILE: indexFile,
        };
        try {
            if (parent === null) {
                execFileSync("git", ["read-tree", "--empty"], { env });
            } else {
                execFileSync("git", ["read-tree", `${parent}^{tree}`], { env });
            }
            for (const write of writes) {
                const blob = execFileSync(
                    "git",
                    ["hash-object", "-w", "--stdin"],
                    { env, input: write.content },
                )
                    .toString()
                    .trim();
                execFileSync(
                    "git",
                    [
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        `100644,${blob},${write.path}`,
                    ],
                    { env },
                );
            }
            for (const del of deletes) {
                execFileSync("git", ["update-index", "--force-remove", del], {
                    env,
                });
            }
            const tree = execFileSync("git", ["write-tree"], { env })
                .toString()
                .trim();
            const args = ["commit-tree", tree, "-m", message];
            if (parent !== null) {
                args.splice(2, 0, "-p", parent);
            }
            return execFileSync("git", args, { env }).toString().trim();
        } finally {
            rmSync(indexFile, { force: true });
        }
    }

    private refSha(branch: string): string | null {
        try {
            return this.plumb([
                "rev-parse",
                "--verify",
                "--quiet",
                `refs/heads/${branch}`,
            ]);
        } catch {
            return null;
        }
    }

    private isAncestor(ancestor: string, descendant: string): boolean {
        try {
            this.plumb(["merge-base", "--is-ancestor", ancestor, descendant]);
            return true;
        } catch {
            return false;
        }
    }

    private assertAncestor(
        ancestor: string,
        descendant: string,
        message: string,
    ): void {
        if (!this.isAncestor(ancestor, descendant)) {
            throw new Error(message);
        }
    }

    private plumb(args: string[], input?: string): string {
        return execFileSync("git", ["--git-dir", this.gitDir, ...args], {
            env: { ...process.env, ...COMMIT_ENV },
            input,
        })
            .toString()
            .trim();
    }

    // An open PR reports live facts, the way GitHub does.
    private snapshot(pull: PullRecord): PullRecord {
        const headSha =
            pull.state === "open"
                ? (this.refSha(pull.headRef) ?? pull.headSha)
                : pull.headSha;
        return {
            ...pull,
            headSha,
            mergeableState:
                pull.state === "open" && this.conflictedPulls.has(pull.number)
                    ? "dirty"
                    : pull.state === "open"
                      ? "clean"
                      : "unknown",
        };
    }
}
