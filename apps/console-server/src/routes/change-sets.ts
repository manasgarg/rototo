// The change-set API. Authorization is two layers deep on purpose: every
// mutation recomputes decide() (the console's prediction), and the write
// itself runs with the actor's own GitHub token, so GitHub stays the
// authority (design/console-git-ops.md rule 5). Ownership — author edits,
// author plus collaborators after sharing — is console bookkeeping the
// routes enforce.

import { Hono } from "hono";
import type { Context } from "hono";

import type { ActingCredential } from "../app-credential.ts";
import {
    approvalPolicy,
    contributorsOf,
    policyStatus,
} from "../approvals.ts";
import { branchFor, repoId, type EditInput } from "../change-sets.ts";
import type { ConsoleContext } from "../context.ts";
import type { Subject } from "../decide.ts";
import { ApiError } from "../errors.ts";
import { isPin } from "../packages.ts";
import { buildReview } from "../review.ts";
import type { ChangeSetRow, SourceTreeRow } from "../store.ts";

export function changeSetRoutes(ctx: ConsoleContext): Hono {
    const app = new Hono();

    app.onError((error, c) => {
        if (error instanceof ApiError) {
            return c.json(
                {
                    error: {
                        message: error.message,
                        ...(error.conflictPaths === undefined
                            ? {}
                            : { paths: error.conflictPaths }),
                    },
                },
                error.status as 400,
            );
        }
        return c.json({ error: { message: error.message } }, 500);
    });

    const subjectOf = (c: Context): Subject => {
        const subject = ctx.subjectFor(c.req.header("cookie"));
        if (subject === null) {
            throw new ApiError(401, "sign in first");
        }
        return subject;
    };

    const treeOf = (treeId: string): SourceTreeRow => {
        const tree = ctx.store.getSourceTree(treeId);
        if (tree === null) {
            throw new ApiError(404, "no such source tree");
        }
        return tree;
    };

    const decide = async (
        subject: Subject,
        action: "view" | "propose",
        tree: SourceTreeRow,
    ): Promise<void> => {
        const verdict = await ctx.decision.decide(subject, action, {
            kind: "source-tree",
            sourceTree: tree.id,
        });
        if (verdict.allow) {
            return;
        }
        // A propose grant scoped to a package (or entity) within the tree
        // still lets its holder carry a change set — the branch and PR are
        // vehicles; the entity-level checks bind what the edits may touch.
        if (action === "propose" && subject.kind === "principal") {
            const scoped = ctx.store
                .grantsForPrincipal(subject.id)
                .some(
                    (grant) =>
                        ["propose", "approve", "administer"].includes(
                            grant.action,
                        ) &&
                        (grant.resource.startsWith(`package:${tree.id}/`) ||
                            grant.resource.startsWith(`entity:${tree.id}/`)),
                );
            if (scoped) {
                return;
            }
        }
        throw new ApiError(403, verdict.reason);
    };

    const credentialOf = async (
        subject: Subject,
        tree: SourceTreeRow,
    ): Promise<ActingCredential> => {
        const credential = await ctx.actingCredential(subject, tree);
        if (credential === null) {
            throw new ApiError(
                403,
                "no credential can act on this tree: link a GitHub identity or install the console's GitHub App",
            );
        }
        return credential;
    };

    const displayOf = (subject: Subject): string => {
        if (subject.kind === "local") {
            return "local";
        }
        return (
            ctx.store.getPrincipal(subject.id)?.displayName ?? subject.id
        );
    };

    const changeSetOf = (c: Context): ChangeSetRow => {
        const row = ctx.store.getChangeSet(c.req.param("id") ?? "");
        if (row === null) {
            throw new ApiError(404, "no such change set");
        }
        return row;
    };

    const body = async (c: Context): Promise<Record<string, unknown>> => {
        const parsed = (await c.req.json().catch(() => null)) as Record<
            string,
            unknown
        > | null;
        if (parsed === null) {
            throw new ApiError(400, "expected a JSON body");
        }
        return parsed;
    };

    app.post("/source-trees/:tree/change-sets", async (c) => {
        const subject = subjectOf(c);
        const tree = treeOf(c.req.param("tree"));
        await decide(subject, "propose", tree);
        const input = await body(c);
        if (typeof input.title !== "string" || input.title.trim() === "") {
            throw new ApiError(400, "title is required");
        }
        if (input.baseRef !== undefined && typeof input.baseRef !== "string") {
            throw new ApiError(400, "baseRef must be a string");
        }
        const row = await ctx.changeSets.create({
            tree,
            title: input.title.trim(),
            baseRef: input.baseRef as string | undefined,
            author: ctx.subjectId(subject),
            credential: await credentialOf(subject, tree),
        });
        return c.json(payload(row), 201);
    });

    app.get("/source-trees/:tree/change-sets", async (c) => {
        const subject = subjectOf(c);
        const tree = treeOf(c.req.param("tree"));
        await decide(subject, "view", tree);
        // Everyone who can see the tree sees every change set: no secret
        // drafts, a change set is a proposal by construction.
        return c.json({
            changeSets: ctx.store
                .listChangeSets(tree.id)
                .map((row) => payload(row)),
        });
    });

    app.get("/change-sets/:id", async (c) => {
        const subject = subjectOf(c);
        const changeSet = changeSetOf(c);
        const tree = treeOf(changeSet.sourceTreeId);
        await decide(subject, "view", tree);
        return c.json({
            changeSet: payload(changeSet),
            events: ctx.store.listChangeSetEvents(changeSet.id),
            collaborators: ctx.store.listChangeSetCollaborators(changeSet.id),
        });
    });

    // The three-delta review: what changed, what it does (with the
    // denominator stated), whether it is healthy. A read: anyone who can
    // view the tree reads the review, because approving happens on GitHub
    // in this tranche and informed eyes are the whole point.
    app.get("/change-sets/:id/review", async (c) => {
        const subject = subjectOf(c);
        const changeSet = changeSetOf(c);
        const tree = treeOf(changeSet.sourceTreeId);
        await decide(subject, "view", tree);
        const credential = await credentialOf(subject, tree);
        const review = await buildReview(
            { git: ctx.git, stager: ctx.stager },
            { tree, changeSet, token: credential.token },
        );
        // The approval requirements ride along, so the reviewer sees what
        // this change must satisfy and what is still missing.
        const policy = await approvalPolicy(
            { git: ctx.git, stager: ctx.stager },
            tree,
            changeSet,
            credential.token,
        );
        return c.json({
            changeSet: payload(changeSet),
            review,
            approvals: ctx.store.listApprovals(changeSet.id),
            contributors: contributorsOf(ctx.store, changeSet),
            policy: {
                ...policy,
                ...policyStatus(ctx.store, changeSet, policy),
            },
        });
    });

    app.post("/change-sets/:id/edits", async (c) => {
        const subject = subjectOf(c);
        const changeSet = changeSetOf(c);
        const tree = treeOf(changeSet.sourceTreeId);
        await decide(subject, "propose", tree);
        const actor = ctx.subjectId(subject);
        if (!ctx.changeSets.canEdit(changeSet, actor)) {
            throw new ApiError(
                403,
                "only the author or a collaborator edits a change set",
            );
        }
        const input = await body(c);
        const edit = editInput(input);
        const credential = await credentialOf(subject, tree);
        // On the App path the console is the enforcement point (rule 5):
        // before anything is committed, every computed operation is checked
        // against the actor's grants at entity scope, quantized to the
        // entity address. GitHub enforces nothing for the App token beyond
        // the installation itself.
        const enforce =
            credential.mode !== "app"
                ? undefined
                : async (records: { address: string }[]): Promise<void> => {
                      for (const record of records) {
                          const entity = record.address.split("#")[0] as string;
                          const verdict = await ctx.decision.decide(
                              subject,
                              "propose",
                              {
                                  kind: "entity",
                                  sourceTree: tree.id,
                                  path: edit.packagePath,
                                  entity,
                              },
                          );
                          if (!verdict.allow) {
                              throw new ApiError(
                                  403,
                                  `the change to ${entity} exceeds your grants: ${verdict.reason}`,
                              );
                          }
                      }
                  };
        const result = await ctx.changeSets.edit({
            changeSet,
            tree,
            edit,
            actor,
            actorDisplay: displayOf(subject),
            credential,
            enforce,
        });
        return c.json(result);
    });

    app.post("/change-sets/:id/submit", async (c) => {
        const subject = subjectOf(c);
        const changeSet = changeSetOf(c);
        const tree = treeOf(changeSet.sourceTreeId);
        await decide(subject, "propose", tree);
        const actor = ctx.subjectId(subject);
        requireAuthor(changeSet, actor, "submit");
        const input = await body(c);
        const credential = await credentialOf(subject, tree);
        const pull = await ctx.changeSets.submit({
            changeSet,
            tree,
            body: typeof input.body === "string" ? input.body : undefined,
            actor,
            actorDisplay: displayOf(subject),
            credential,
        });
        // Fill the observed columns right away so the response and the next
        // render carry the PR; the reconciler stays their only writer.
        const fresh = await ctx.reconciler.reconcile(
            ctx.store.getChangeSet(changeSet.id) as ChangeSetRow,
            credential.token,
        );
        return c.json({ changeSet: payload(fresh), pull });
    });

    app.post("/change-sets/:id/abandon", async (c) => {
        const subject = subjectOf(c);
        const changeSet = changeSetOf(c);
        const tree = treeOf(changeSet.sourceTreeId);
        await decide(subject, "propose", tree);
        const actor = ctx.subjectId(subject);
        requireAuthor(changeSet, actor, "abandon");
        await ctx.changeSets.abandon({
            changeSet,
            tree,
            actor,
            credential: await credentialOf(subject, tree),
        });
        return c.json({
            changeSet: payload(
                ctx.store.getChangeSet(changeSet.id) as ChangeSetRow,
            ),
        });
    });

    app.post("/change-sets/:id/collaborators", async (c) => {
        const subject = subjectOf(c);
        const changeSet = changeSetOf(c);
        const tree = treeOf(changeSet.sourceTreeId);
        await decide(subject, "view", tree);
        const actor = ctx.subjectId(subject);
        requireAuthor(changeSet, actor, "share");
        const input = await body(c);
        if (typeof input.principalId !== "string") {
            throw new ApiError(400, "principalId is required");
        }
        if (
            ctx.config.authMode === "team" &&
            ctx.store.getPrincipal(input.principalId) === null
        ) {
            throw new ApiError(404, "no such principal");
        }
        ctx.changeSets.share({
            changeSet,
            principalId: input.principalId,
            actor,
        });
        return c.json({
            collaborators: ctx.store.listChangeSetCollaborators(changeSet.id),
        });
    });

    // A manual nudge: "check this one sooner". The reconciler stays
    // idempotent, so anyone who can view may nudge.
    app.post("/change-sets/:id/reconcile", async (c) => {
        const subject = subjectOf(c);
        const changeSet = changeSetOf(c);
        const tree = treeOf(changeSet.sourceTreeId);
        await decide(subject, "view", tree);
        const fresh = await ctx.reconciler.reconcile(
            changeSet,
            (await credentialOf(subject, tree)).token,
        );
        return c.json({ changeSet: payload(fresh) });
    });

    // Approving records the act, comments on the PR (the copy the fire
    // drill can rebuild from), and — once every requirement is satisfied —
    // merges. decide() carries the contributors, so helping write a change
    // disqualifies approving it (the two-person default under Backend B).
    app.post("/change-sets/:id/approve", async (c) => {
        const subject = subjectOf(c);
        const changeSet = changeSetOf(c);
        const tree = treeOf(changeSet.sourceTreeId);
        if (changeSet.state !== "proposed") {
            throw new ApiError(
                409,
                `change set ${changeSet.id} is ${changeSet.state}; submit it first`,
            );
        }
        const contributors = contributorsOf(ctx.store, changeSet);
        const verdict = await ctx.decision.decide(
            subject,
            "approve",
            { kind: "source-tree", sourceTree: tree.id },
            { contributors },
        );
        if (!verdict.allow) {
            throw new ApiError(403, verdict.reason);
        }
        const actor = ctx.subjectId(subject);
        const credential = await credentialOf(subject, tree);
        ctx.store.addApproval(changeSet.id, actor);
        ctx.store.appendChangeSetEvent(changeSet.id, actor, "approved", null);
        if (changeSet.prNumber !== null) {
            await ctx.git
                .commentOnPull(
                    credential.token,
                    repoId(tree),
                    changeSet.prNumber,
                    `Approved by ${displayOf(subject)} via the rototo console.`,
                )
                .catch(() => {});
        }
        const policy = await approvalPolicy(
            { git: ctx.git, stager: ctx.stager },
            tree,
            changeSet,
            credential.token,
        );
        const status = policyStatus(ctx.store, changeSet, policy);
        if (!status.satisfied) {
            return c.json({
                recorded: true,
                merged: false,
                waitingOn: status.missing,
            });
        }
        const merged = await mergeChangeSet(changeSet, tree, credential, actor);
        return c.json({ recorded: true, merged: true, mergeSha: merged });
    });

    // The explicit merge: the same policy gate when the App acts; a user's
    // own token leaves enforcement to GitHub, exactly like `git push`.
    app.post("/change-sets/:id/merge", async (c) => {
        const subject = subjectOf(c);
        const changeSet = changeSetOf(c);
        const tree = treeOf(changeSet.sourceTreeId);
        if (changeSet.state !== "proposed" || changeSet.prNumber === null) {
            throw new ApiError(
                409,
                `change set ${changeSet.id} has no open pull request`,
            );
        }
        await decide(subject, "view", tree);
        const credential = await credentialOf(subject, tree);
        if (credential.mode === "app") {
            const policy = await approvalPolicy(
                { git: ctx.git, stager: ctx.stager },
                tree,
                changeSet,
                credential.token,
            );
            const status = policyStatus(ctx.store, changeSet, policy);
            if (!status.satisfied) {
                throw new ApiError(
                    409,
                    `approval policy is not satisfied: ${status.missing.join("; ")}`,
                );
            }
        }
        const merged = await mergeChangeSet(
            changeSet,
            tree,
            credential,
            ctx.subjectId(subject),
        );
        return c.json({ merged: true, mergeSha: merged });
    });

    async function mergeChangeSet(
        changeSet: ChangeSetRow,
        tree: SourceTreeRow,
        credential: ActingCredential,
        actor: string,
    ): Promise<string> {
        const sha = await ctx.git.mergePull(
            credential.token,
            repoId(tree),
            changeSet.prNumber as number,
            `${changeSet.title} (#${changeSet.prNumber})`,
        );
        ctx.store.appendChangeSetEvent(
            changeSet.id,
            actor,
            "merged",
            JSON.stringify({ sha, via: "console" }),
        );
        await ctx.reconciler.reconcile(
            ctx.store.getChangeSet(changeSet.id) as ChangeSetRow,
            credential.token,
        );
        return sha;
    }

    return app;
}

function requireAuthor(
    changeSet: ChangeSetRow,
    actor: string,
    verb: string,
): void {
    if (changeSet.authorPrincipal !== actor) {
        throw new ApiError(403, `only the author may ${verb} a change set`);
    }
}

function editInput(input: Record<string, unknown>): EditInput {
    if (typeof input.packagePath !== "string" || input.packagePath === "") {
        throw new ApiError(400, "packagePath is required");
    }
    if (typeof input.expectedPin !== "string" || !isPin(input.expectedPin)) {
        throw new ApiError(
            400,
            "expectedPin must be the full commit SHA the edit was computed against",
        );
    }
    const hasOperations = Array.isArray(input.operations);
    const hasFiles = Array.isArray(input.files) || Array.isArray(input.deletes);
    if (hasOperations === hasFiles) {
        throw new ApiError(
            400,
            "send either operations (the structured path) or files/deletes (the raw-text path)",
        );
    }
    if (hasFiles) {
        for (const file of (input.files as unknown[]) ?? []) {
            const entry = file as { path?: unknown; content?: unknown };
            if (
                typeof entry.path !== "string" ||
                typeof entry.content !== "string"
            ) {
                throw new ApiError(
                    400,
                    "files must be { path, content } objects",
                );
            }
        }
        for (const del of (input.deletes as unknown[]) ?? []) {
            if (typeof del !== "string") {
                throw new ApiError(400, "deletes must be path strings");
            }
        }
    }
    return {
        packagePath: input.packagePath,
        expectedPin: input.expectedPin,
        summary: typeof input.summary === "string" ? input.summary : undefined,
        operations: hasOperations
            ? (input.operations as EditInput["operations"])
            : undefined,
        files: hasFiles
            ? ((input.files as EditInput["files"]) ?? [])
            : undefined,
        deletes: hasFiles
            ? ((input.deletes as string[] | undefined) ?? [])
            : undefined,
    };
}

function payload(row: ChangeSetRow): Record<string, unknown> {
    return { ...row, branch: branchFor(row.id) };
}
