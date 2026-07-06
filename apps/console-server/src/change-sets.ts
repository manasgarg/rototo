// The life of a change set (design/console-git-ops.md): one branch, at most
// one PR, four states. This service writes the intent side — branches,
// commits, PRs, diary entries — always with the acting user's own token, so
// GitHub enforces permissions and the console only predicts.
//
// The commit path is rule 3 made real: a whole edit plan lands as one
// commit, moved onto the branch with a compare-and-swap. Every plan carries
// the pin it was computed against; when the branch has moved past that pin,
// intervening commits touching disjoint files rebase the plan automatically,
// and overlapping ones reject with "changed under you".

import { randomBytes } from "node:crypto";

import type { ActingCredential } from "./app-credential.ts";
import { ApiError } from "./errors.ts";
import type { GitOps, RepoId } from "./git.ts";
import {
    native,
    type ChangeRecordJson,
    type EditPlanJson,
    type JsonValue,
    type PackageLintJson,
} from "./native.ts";
import { PackageStager } from "./packages.ts";
import type { ChangeSetRow, SourceTreeRow, Store } from "./store.ts";

export const BRANCH_PREFIX = "rototo-console/";
// The machine-readable PR body line the fire drill rebuilds from.
export const CHANGE_SET_MARKER = "Rototo-Change-Set:";

export function branchFor(changeSetId: string): string {
    return `${BRANCH_PREFIX}${changeSetId}`;
}

export function parseChangeSetMarker(body: string): string | null {
    for (const line of body.split("\n")) {
        const trimmed = line.trim();
        if (trimmed.startsWith(CHANGE_SET_MARKER)) {
            const id = trimmed.slice(CHANGE_SET_MARKER.length).trim();
            return id === "" ? null : id;
        }
    }
    return null;
}

// One save: either structured operations against a package, or the raw-text
// escape hatch shipping whole files. Paths in `files`/`deletes` are relative
// to the package root, like the engine's plan paths.
export type EditInput = {
    packagePath: string;
    expectedPin: string;
    summary?: string;
    operations?: JsonValue[];
    files?: { path: string; content: string }[];
    deletes?: string[];
};

export type EditResult = {
    pin: string;
    records: ChangeRecordJson[];
    lint: PackageLintJson;
};

type ChangeSetsDeps = {
    store: Store;
    git: GitOps;
    stager: PackageStager;
};

export class ChangeSets {
    private readonly deps: ChangeSetsDeps;

    constructor(deps: ChangeSetsDeps) {
        this.deps = deps;
    }

    async create(input: {
        tree: SourceTreeRow;
        title: string;
        baseRef?: string;
        author: string;
        credential: ActingCredential;
    }): Promise<ChangeSetRow> {
        const { store, git } = this.deps;
        const repo = repoId(input.tree);
        const baseRef = input.baseRef ?? input.tree.defaultBranch ?? "main";
        const basePin = await git.getRef(input.credential.token, repo, baseRef);
        if (basePin === null) {
            throw new ApiError(
                404,
                `base ref ${baseRef} does not exist on ${repo.owner}/${repo.name}`,
            );
        }
        const id = `cs_${randomBytes(5).toString("hex")}`;
        await git.createRef(input.credential.token, repo, branchFor(id), basePin);
        const row = store.insertChangeSet({
            id,
            sourceTreeId: input.tree.id,
            title: input.title,
            authorPrincipal: input.author,
            actingMode: input.credential.mode,
            baseRef,
            baseShaAtCreation: basePin,
            state: "draft",
        });
        store.appendChangeSetEvent(
            id,
            input.author,
            "created",
            JSON.stringify({ branch: branchFor(id), base: baseRef, basePin }),
        );
        return row;
    }

    // Applies one save as one commit on the change-set branch. When the App
    // credential acts, the commit message carries the Acting-For trailer:
    // git-level attribution reconstructed deliberately, since the token
    // itself no longer names the person.
    async edit(input: {
        changeSet: ChangeSetRow;
        tree: SourceTreeRow;
        edit: EditInput;
        actor: string;
        actorDisplay: string;
        credential: ActingCredential;
        // Called with the computed change records before anything is
        // committed; throwing here refuses the edit. The App path uses it
        // for entity-scoped grant enforcement (rule 5).
        enforce?: (records: ChangeRecordJson[]) => Promise<void>;
    }): Promise<EditResult> {
        const { store, git, stager } = this.deps;
        const { changeSet, tree, edit } = input;
        const token = input.credential.token;
        if (changeSet.state !== "draft" && changeSet.state !== "proposed") {
            throw new ApiError(
                409,
                `change set ${changeSet.id} is ${changeSet.state} and no longer editable`,
            );
        }
        const repo = repoId(tree);
        const branch = branchFor(changeSet.id);
        const packagePath = edit.packagePath;
        const prefix =
            packagePath === "." || packagePath === ""
                ? ""
                : `${packagePath.replace(/\/+$/, "")}/`;

        // The plan at a pin. The structured path stages the tree and asks
        // the engine (pure; nothing written); the raw path is the plan.
        const planAt = async (
            pin: string,
        ): Promise<{ plan: EditPlanJson; records: ChangeRecordJson[] }> => {
            if (edit.operations !== undefined) {
                const treeRoot = await stager.stageTree(tree, pin, token);
                const packageRoot = stager.packageRoot(treeRoot, packagePath);
                return native.applyEdit(packageRoot, edit.operations);
            }
            return {
                plan: {
                    writes: edit.files ?? [],
                    deletes: edit.deletes ?? [],
                },
                records: [],
            };
        };
        const repoPaths = (plan: EditPlanJson): string[] => [
            ...plan.writes.map((write) => prefix + write.path),
            ...plan.deletes.map((del) => prefix + del),
        ];

        for (let attempt = 0; attempt < 3; attempt++) {
            const head = await git.getRef(token, repo, branch);
            if (head === null) {
                throw new ApiError(
                    409,
                    `the branch for change set ${changeSet.id} is gone; it may have been abandoned`,
                );
            }
            if (head !== edit.expectedPin) {
                // The staleness check runs against the plan as the client
                // computed it, at the pin the client saw.
                const basis = await planAt(edit.expectedPin);
                const intervening = await git.compare(
                    token,
                    repo,
                    edit.expectedPin,
                    head,
                );
                const planned = new Set(repoPaths(basis.plan));
                const overlap = intervening.files.filter((file) =>
                    planned.has(file),
                );
                if (overlap.length > 0) {
                    throw new ApiError(
                        409,
                        "the branch changed under you; reload and reapply your edit",
                        overlap,
                    );
                }
            }
            const outcome = await planAt(head);
            if (
                outcome.plan.writes.length === 0 &&
                outcome.plan.deletes.length === 0
            ) {
                throw new ApiError(400, "the edit produced no file changes");
            }
            if (input.enforce !== undefined) {
                await input.enforce(outcome.records);
            }
            const summary =
                input.edit.summary ?? summarize(outcome, packagePath);
            const message =
                input.credential.mode === "app"
                    ? `${summary}\n\nActing-For: ${input.actor} (${input.actorDisplay})`
                    : summary;
            const commit = await git.createCommit(token, repo, {
                parent: head,
                message,
                writes: outcome.plan.writes.map((write) => ({
                    path: prefix + write.path,
                    content: write.content,
                })),
                deletes: outcome.plan.deletes.map((del) => prefix + del),
            });
            if (!(await git.updateRef(token, repo, branch, commit))) {
                // Someone moved the head between our read and our write;
                // loop, re-read, rebuild the commit on the new head.
                continue;
            }
            store.appendChangeSetEvent(
                changeSet.id,
                input.actor,
                "committed",
                JSON.stringify({ sha: commit, message: summary, packagePath }),
            );
            // Lint on the post-edit stage: every save runs it, and the form
            // path gets its diagnostics from here.
            const treeRoot = await stager.stageTree(tree, commit, token);
            const lint = await native.lintPackage(
                stager.packageRoot(treeRoot, packagePath),
            );
            return { pin: commit, records: outcome.records, lint };
        }
        throw new ApiError(
            409,
            "the branch kept moving during the write (three attempts); reload and retry",
        );
    }

    async submit(input: {
        changeSet: ChangeSetRow;
        tree: SourceTreeRow;
        body?: string;
        actor: string;
        actorDisplay: string;
        credential: ActingCredential;
    }): Promise<{ number: number; url: string }> {
        const { store, git } = this.deps;
        const { changeSet, tree } = input;
        const token = input.credential.token;
        if (changeSet.state !== "draft") {
            throw new ApiError(
                409,
                `change set ${changeSet.id} is ${changeSet.state}, not draft`,
            );
        }
        const repo = repoId(tree);
        const branch = branchFor(changeSet.id);
        const head = await git.getRef(token, repo, branch);
        if (head === null) {
            throw new ApiError(
                409,
                `the branch for change set ${changeSet.id} is gone`,
            );
        }
        const actingFor =
            input.credential.mode === "app"
                ? `\nActing-For: ${input.actor} (${input.actorDisplay})`
                : "";
        const body =
            `${input.body ?? ""}\n\n${CHANGE_SET_MARKER} ${changeSet.id}${actingFor}`.trim();
        const pull = await git.createPull(token, repo, {
            title: changeSet.title,
            body,
            head: branch,
            base: changeSet.baseRef,
        });
        store.setChangeSetState(changeSet.id, "proposed");
        store.appendChangeSetEvent(
            changeSet.id,
            input.actor,
            "submitted",
            JSON.stringify({ pr: pull.number, url: pull.url }),
        );
        return { number: pull.number, url: pull.url };
    }

    async abandon(input: {
        changeSet: ChangeSetRow;
        tree: SourceTreeRow;
        actor: string;
        credential: ActingCredential;
    }): Promise<void> {
        const { store, git } = this.deps;
        const { changeSet, tree } = input;
        const token = input.credential.token;
        if (changeSet.state !== "draft" && changeSet.state !== "proposed") {
            throw new ApiError(
                409,
                `change set ${changeSet.id} is already ${changeSet.state}`,
            );
        }
        const repo = repoId(tree);
        const pull =
            changeSet.prNumber === null
                ? await git.pullForBranch(token, repo, branchFor(changeSet.id))
                : await git.getPull(token, repo, changeSet.prNumber);
        if (pull !== null && pull.state === "open") {
            await git.closePull(token, repo, pull.number);
        }
        await git.deleteRef(token, repo, branchFor(changeSet.id));
        store.setChangeSetState(changeSet.id, "abandoned");
        store.appendChangeSetEvent(
            changeSet.id,
            input.actor,
            "abandoned",
            null,
        );
    }

    // Sharing adds collaborators; collaborators edit (the author alone
    // shares, submits, and abandons).
    share(input: {
        changeSet: ChangeSetRow;
        principalId: string;
        actor: string;
    }): void {
        const { store } = this.deps;
        store.addChangeSetCollaborator(
            input.changeSet.id,
            input.principalId,
            input.actor,
        );
        store.appendChangeSetEvent(
            input.changeSet.id,
            input.actor,
            "shared",
            JSON.stringify({ with: input.principalId }),
        );
    }

    canEdit(changeSet: ChangeSetRow, principalId: string): boolean {
        if (changeSet.authorPrincipal === principalId) {
            return true;
        }
        return this.deps.store
            .listChangeSetCollaborators(changeSet.id)
            .some((row) => row.principalId === principalId);
    }
}

export function repoId(tree: SourceTreeRow): RepoId {
    if (tree.kind !== "github" || tree.owner === null || tree.name === null) {
        throw new ApiError(
            400,
            `source tree ${tree.id} is not a GitHub repository; change sets need one`,
        );
    }
    return { owner: tree.owner, name: tree.name };
}

// The commit summary when the client sends none: intent from the change
// records, or the touched files on the raw path.
function summarize(
    outcome: { plan: EditPlanJson; records: ChangeRecordJson[] },
    packagePath: string,
): string {
    const first = outcome.records[0];
    if (first !== undefined) {
        const rest =
            outcome.records.length > 1
                ? ` (+${outcome.records.length - 1} more)`
                : "";
        return `${first.operation} ${first.address}${rest}`;
    }
    const paths = [
        ...outcome.plan.writes.map((write) => write.path),
        ...outcome.plan.deletes,
    ];
    return `Edit ${packagePath === "." ? "" : `${packagePath}: `}${paths.join(", ")}`;
}
