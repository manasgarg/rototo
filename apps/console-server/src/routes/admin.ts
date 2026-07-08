// The admin surface: principals, groups, grants, and invitations, every
// mutation gated by administer at the relevant scope and recorded in
// authz_audit. Grants are allow-only triples; the only deny mechanisms are
// grant absence and principal disablement, which keeps a grant set
// auditable by reading it top to bottom.

import { randomBytes } from "node:crypto";

import { Hono } from "hono";
import type { Context } from "hono";

import type { ConsoleContext } from "../context.ts";
import {
    ACTIONS,
    parseResource,
    type Action,
    type Subject,
} from "../decide.ts";
import { ApiError } from "../errors.ts";
import { hashInvite } from "./auth.ts";
import { grantDiagnostics } from "../grant-diagnostics.ts";

export function adminRoutes(ctx: ConsoleContext): Hono {
    const app = new Hono();

    app.onError((error, c) => {
        if (error instanceof ApiError) {
            return c.json(
                { error: { message: error.message } },
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

    // Most administration is deployment-scoped; grant creation re-checks at
    // the grant's own resource, so a source-tree administrator can delegate
    // within their tree without holding the deployment.
    const requireAdminister = async (
        subject: Subject,
        resource: Parameters<typeof ctx.decision.decide>[2],
    ): Promise<void> => {
        const verdict = await ctx.decision.decide(
            subject,
            "administer",
            resource,
        );
        if (!verdict.allow) {
            throw new ApiError(403, verdict.reason);
        }
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

    // The admin's view of source trees keeps deregistered rows visible:
    // they are audit, and re-registration reactivates them.
    app.get("/admin/source-trees", async (c) => {
        const subject = subjectOf(c);
        await requireAdminister(subject, { kind: "deployment" });
        return c.json({
            sourceTrees: ctx.store.listSourceTrees(true).map((tree) => ({
                id: tree.id,
                kind: tree.kind,
                owner: tree.owner,
                name: tree.name,
                defaultBranch: tree.defaultBranch,
                status: tree.status,
                createdAt: tree.createdAt,
            })),
        });
    });

    app.get("/admin/principals", async (c) => {
        const subject = subjectOf(c);
        await requireAdminister(subject, { kind: "deployment" });
        return c.json({
            principals: ctx.store.listPrincipals().map((principal) => ({
                ...principal,
                identities: ctx.store
                    .identitiesForPrincipal(principal.id)
                    .map((identity) => ({
                        provider: identity.provider,
                        login: identity.login,
                        email: identity.email,
                        hasCredential: identity.credentialCiphertext !== null,
                    })),
                groups: ctx.store
                    .groupsForPrincipal(principal.id)
                    .map((group) => group.name),
            })),
        });
    });

    app.post("/admin/principals/:id/status", async (c) => {
        const subject = subjectOf(c);
        await requireAdminister(subject, { kind: "deployment" });
        const principal = ctx.store.getPrincipal(c.req.param("id"));
        if (principal === null) {
            throw new ApiError(404, "no such principal");
        }
        const input = await body(c);
        if (input.status !== "active" && input.status !== "disabled") {
            throw new ApiError(400, "status must be active or disabled");
        }
        ctx.store.setPrincipalStatus(principal.id, input.status);
        ctx.store.appendAudit(
            ctx.subjectId(subject),
            input.status === "disabled"
                ? "principal.disable"
                : "principal.enable",
            JSON.stringify({ principal: principal.id }),
        );
        return c.json({ ok: true });
    });

    app.get("/admin/groups", async (c) => {
        const subject = subjectOf(c);
        await requireAdminister(subject, { kind: "deployment" });
        return c.json({
            groups: ctx.store.listGroups().map((group) => ({
                ...group,
                members: ctx.store.listGroupMembers(group.id),
            })),
        });
    });

    app.post("/admin/groups", async (c) => {
        const subject = subjectOf(c);
        await requireAdminister(subject, { kind: "deployment" });
        const input = await body(c);
        if (typeof input.name !== "string" || input.name.trim() === "") {
            throw new ApiError(400, "name is required");
        }
        const name = input.name.trim();
        if (!/^[a-z0-9_]+$/.test(name)) {
            throw new ApiError(
                400,
                "group names are lowercase snake_case; surface approval roles reference them",
            );
        }
        if (ctx.store.getGroupByName(name) !== null) {
            throw new ApiError(409, `group ${name} already exists`);
        }
        const group = ctx.store.createGroup(
            name,
            typeof input.description === "string" ? input.description : null,
        );
        ctx.store.appendAudit(
            ctx.subjectId(subject),
            "group.create",
            JSON.stringify({ group: group.id, name }),
        );
        return c.json({ group }, 201);
    });

    app.post("/admin/groups/:id/members", async (c) => {
        const subject = subjectOf(c);
        await requireAdminister(subject, { kind: "deployment" });
        const group = ctx.store.getGroup(c.req.param("id"));
        if (group === null) {
            throw new ApiError(404, "no such group");
        }
        const input = await body(c);
        if (typeof input.principalId !== "string") {
            throw new ApiError(400, "principalId is required");
        }
        if (ctx.store.getPrincipal(input.principalId) === null) {
            throw new ApiError(404, "no such principal");
        }
        if (input.remove === true) {
            ctx.store.removeGroupMember(group.id, input.principalId);
        } else {
            ctx.store.addGroupMember(group.id, input.principalId);
        }
        ctx.store.appendAudit(
            ctx.subjectId(subject),
            input.remove === true ? "group.member.remove" : "group.member.add",
            JSON.stringify({ group: group.id, principal: input.principalId }),
        );
        return c.json({ members: ctx.store.listGroupMembers(group.id) });
    });

    app.get("/admin/grants", async (c) => {
        const subject = subjectOf(c);
        await requireAdminister(subject, { kind: "deployment" });
        return c.json({ grants: ctx.store.listGrants() });
    });

    app.post("/admin/grants", async (c) => {
        const subject = subjectOf(c);
        const input = await body(c);
        if (
            input.granteeKind !== "principal" &&
            input.granteeKind !== "group"
        ) {
            throw new ApiError(400, "granteeKind must be principal or group");
        }
        if (typeof input.granteeId !== "string") {
            throw new ApiError(400, "granteeId is required");
        }
        const exists =
            input.granteeKind === "principal"
                ? ctx.store.getPrincipal(input.granteeId) !== null
                : ctx.store.getGroup(input.granteeId) !== null;
        if (!exists) {
            throw new ApiError(404, `no such ${input.granteeKind}`);
        }
        if (
            typeof input.action !== "string" ||
            !(ACTIONS as readonly string[]).includes(input.action)
        ) {
            throw new ApiError(
                400,
                `action must be one of ${ACTIONS.join(", ")}`,
            );
        }
        if (typeof input.resource !== "string") {
            throw new ApiError(400, "resource is required");
        }
        const resource = parseResource(input.resource);
        if (resource === null) {
            throw new ApiError(
                400,
                "resource must be deployment, source-tree:<id>, or package:<tree>/<path>",
            );
        }
        // Creating a grant on a resource requires administer on that
        // resource or above.
        await requireAdminister(subject, resource);
        const grant = ctx.store.insertGrant({
            granteeKind: input.granteeKind,
            granteeId: input.granteeId,
            action: input.action as Action,
            resource: input.resource,
            createdBy: ctx.subjectId(subject),
        });
        ctx.store.appendAudit(
            ctx.subjectId(subject),
            "grant.create",
            JSON.stringify({
                grant: grant.id,
                grantee: input.granteeId,
                action: input.action,
                resource: input.resource,
            }),
        );
        return c.json({ grant }, 201);
    });

    app.post("/admin/grants/:id/revoke", async (c) => {
        const subject = subjectOf(c);
        const grant = ctx.store.getGrant(c.req.param("id"));
        if (grant === null) {
            throw new ApiError(404, "no such grant");
        }
        const resource = parseResource(grant.resource) ?? {
            kind: "deployment" as const,
        };
        await requireAdminister(subject, resource);
        ctx.store.deleteGrant(grant.id);
        ctx.store.appendAudit(
            ctx.subjectId(subject),
            "grant.revoke",
            JSON.stringify({ grant: grant.id, resource: grant.resource }),
        );
        return c.json({ ok: true });
    });

    app.get("/admin/invitations", async (c) => {
        const subject = subjectOf(c);
        await requireAdminister(subject, { kind: "deployment" });
        return c.json({
            invitations: ctx.store.listInvitations().map((invitation) => ({
                // The token hash never leaves; the token itself was shown
                // once at creation.
                id: invitation.id,
                email: invitation.email,
                providerRestriction: invitation.providerRestriction,
                initialGroups: invitation.initialGroups,
                initialGrants: invitation.initialGrants,
                expiresAt: invitation.expiresAt,
                redeemedBy: invitation.redeemedBy,
                createdAt: invitation.createdAt,
            })),
        });
    });

    app.post("/admin/invitations", async (c) => {
        const subject = subjectOf(c);
        await requireAdminister(subject, { kind: "deployment" });
        const input = await body(c);
        if (typeof input.email !== "string" || !input.email.includes("@")) {
            throw new ApiError(400, "email is required");
        }
        const initialGrants: { action: string; resource: string }[] = [];
        for (const raw of Array.isArray(input.initialGrants)
            ? input.initialGrants
            : []) {
            const entry = raw as { action?: unknown; resource?: unknown };
            if (
                typeof entry.action !== "string" ||
                !(ACTIONS as readonly string[]).includes(entry.action) ||
                typeof entry.resource !== "string"
            ) {
                throw new ApiError(
                    400,
                    "initialGrants must be { action, resource } pairs",
                );
            }
            const resource = parseResource(entry.resource);
            if (resource === null) {
                throw new ApiError(
                    400,
                    `unparseable grant resource: ${entry.resource}`,
                );
            }
            // Inviting with grants is granting: the same scope check applies.
            await requireAdminister(subject, resource);
            initialGrants.push({
                action: entry.action,
                resource: entry.resource,
            });
        }
        const initialGroups: string[] = [];
        for (const raw of Array.isArray(input.initialGroups)
            ? input.initialGroups
            : []) {
            if (typeof raw !== "string") {
                continue;
            }
            const group =
                ctx.store.getGroup(raw) ?? ctx.store.getGroupByName(raw);
            if (group === null) {
                throw new ApiError(404, `no such group: ${raw}`);
            }
            initialGroups.push(group.id);
        }
        const ttlDays =
            typeof input.ttlDays === "number" && input.ttlDays > 0
                ? input.ttlDays
                : 14;
        const token = randomBytes(24).toString("base64url");
        const invitation = ctx.store.createInvitation({
            email: input.email,
            providerRestriction:
                input.providerRestriction === "github" ||
                input.providerRestriction === "oidc"
                    ? input.providerRestriction
                    : null,
            initialGroups,
            initialGrants,
            tokenHash: hashInvite(token),
            expiresAt: new Date(
                Date.now() + ttlDays * 24 * 60 * 60 * 1000,
            ).toISOString(),
            createdBy: ctx.subjectId(subject),
        });
        ctx.store.appendAudit(
            ctx.subjectId(subject),
            "invitation.create",
            JSON.stringify({ invitation: invitation.id, email: input.email }),
        );
        // v1 shows the link to the administrator; delivery is out of band.
        return c.json(
            {
                invitation: { id: invitation.id, email: invitation.email },
                token,
                link: `${ctx.config.publicUrl}/api/auth/oidc/start?invite=${token}`,
            },
            201,
        );
    });

    // Grant configuration validated like a package: incoherence reported,
    // nothing failed.
    app.get("/admin/diagnostics", async (c) => {
        const subject = subjectOf(c);
        await requireAdminister(subject, { kind: "deployment" });
        return c.json({ diagnostics: grantDiagnostics(ctx.store) });
    });

    return app;
}
