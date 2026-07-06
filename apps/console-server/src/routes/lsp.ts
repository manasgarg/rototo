// The editing-session routes over the LSP bridge. A session is opened
// against a staged package root (tree + pin); afterwards the client speaks
// plain LSP methods with package-relative paths in every uri field. The
// mutation guard covers the POSTs, and the bridge itself refuses calls from
// any subject other than the opener.

import { Hono } from "hono";
import type { Context } from "hono";

import type { ConsoleContext } from "../context.ts";
import { ApiError } from "../errors.ts";
import { isPin } from "../packages.ts";

export function lspRoutes(ctx: ConsoleContext): Hono {
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

    const subjectKey = (c: Context): string => {
        const subject = ctx.subjectFor(c.req.header("cookie"));
        if (subject === null) {
            throw new ApiError(401, "sign in first");
        }
        return ctx.subjectId(subject);
    };

    // Opens an editing session on a package at a pin. View access is the
    // bar: the LSP only reads (overlays live in memory, never on disk).
    app.post("/source-trees/:tree/lsp-sessions", async (c) => {
        const tree = ctx.store.getSourceTree(c.req.param("tree") ?? "");
        if (tree === null) {
            throw new ApiError(404, "no such source tree");
        }
        const subject = ctx.subjectFor(c.req.header("cookie"));
        if (subject === null) {
            throw new ApiError(401, "sign in first");
        }
        const verdict = await ctx.decision.decide(subject, "view", {
            kind: "source-tree",
            sourceTree: tree.id,
        });
        if (!verdict.allow) {
            throw new ApiError(403, verdict.reason);
        }
        const credential = await ctx.actingCredential(subject, tree);
        if (credential === null) {
            throw new ApiError(
                403,
                "no GitHub credential is available to read this tree with",
            );
        }
        const token = credential.token;
        const body = (await c.req.json().catch(() => null)) as {
            path?: string;
            pin?: string;
        } | null;
        if (
            body === null ||
            typeof body.path !== "string" ||
            typeof body.pin !== "string" ||
            !isPin(body.pin)
        ) {
            throw new ApiError(400, "expected { path, pin } with a full SHA pin");
        }
        const treeRoot = await ctx.stager.stageTree(tree, body.pin, token);
        const packageRoot = ctx.stager.packageRoot(treeRoot, body.path);
        const session = await ctx.lsp.open(packageRoot, ctx.subjectId(subject));
        return c.json({ session, pin: body.pin, path: body.path }, 201);
    });

    // One LSP request (completion, hover, definition, references, ...).
    app.post("/lsp-sessions/:id/request", async (c) => {
        const key = subjectKey(c);
        const body = (await c.req.json().catch(() => null)) as {
            method?: string;
            params?: unknown;
        } | null;
        if (body === null || typeof body.method !== "string") {
            throw new ApiError(400, "expected { method, params? }");
        }
        const result = await ctx.lsp.request(
            c.req.param("id") ?? "",
            key,
            body.method,
            body.params ?? {},
        );
        return c.json({ result });
    });

    // One LSP notification (didOpen, didChange, didClose).
    app.post("/lsp-sessions/:id/notify", async (c) => {
        const key = subjectKey(c);
        const body = (await c.req.json().catch(() => null)) as {
            method?: string;
            params?: unknown;
        } | null;
        if (body === null || typeof body.method !== "string") {
            throw new ApiError(400, "expected { method, params? }");
        }
        await ctx.lsp.notify(
            c.req.param("id") ?? "",
            key,
            body.method,
            body.params ?? {},
        );
        return c.json({ ok: true });
    });

    // Queued server notifications since the last poll; diagnostics arrive
    // here after edits settle.
    app.get("/lsp-sessions/:id/notifications", (c) => {
        const key = subjectKey(c);
        const notifications = ctx.lsp.drain(c.req.param("id") ?? "", key);
        return c.json({ notifications });
    });

    app.post("/lsp-sessions/:id/close", async (c) => {
        const key = subjectKey(c);
        await ctx.lsp.close(c.req.param("id") ?? "", key);
        return c.json({ ok: true });
    });

    return app;
}
