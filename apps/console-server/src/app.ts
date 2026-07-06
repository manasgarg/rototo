// Route wiring. Two invariants every route obeys:
//
// - Mutations require the `x-rototo-console` header plus an Origin check
//   (ported from the old console; CSRF cannot forge either).
// - Anything the browser renders as a capability came from `decide()`, and
//   the server recomputes `decide()` inside every mutation. Explanation is
//   never authority.

import os from "node:os";
import path from "node:path";

import { Hono } from "hono";

import { ChangeSets } from "./change-sets.ts";
import type { ServerConfig } from "./config.ts";
import type { ConsoleContext } from "./context.ts";
import {
    type Action,
    ACTIONS,
    type Decision,
    DecisionPoint,
    type Resource,
    type Subject,
    resourceString,
} from "./decide.ts";
import { type GitOps, GitHubGit } from "./git.ts";
import type { GitHubFacts } from "./github.ts";
import { LocalAuth } from "./local-auth.ts";
import { LspBridge } from "./lsp-bridge.ts";
import type { OidcExchange } from "./oidc.ts";
import { OidcProvider } from "./oidc.ts";
import { PackageStager } from "./packages.ts";
import { Reconciler } from "./reconciler.ts";
import { adminRoutes } from "./routes/admin.ts";
import { authRoutes, defaultOauthExchange } from "./routes/auth.ts";
import { changeSetRoutes } from "./routes/change-sets.ts";
import { lspRoutes } from "./routes/lsp.ts";
import { packageRoutes } from "./routes/packages.ts";
import { sessionPrincipalId } from "./sessions.ts";
import type { SourceTreeRow, Store } from "./store.ts";
import { TokenCrypto } from "./token-crypto.ts";

export const CONSOLE_HEADER = "x-rototo-console";

export type AppDeps = {
    config: ServerConfig;
    store: Store;
    github: GitHubFacts;
    // Test seam for the OAuth code exchange; defaults to the real GitHub
    // endpoint.
    oauthExchange?: (code: string) => Promise<string>;
    // Test seam for the OIDC code exchange; defaults to the configured
    // provider's own verified exchange.
    oidcExchange?: OidcExchange;
    // Git-data operations; tests substitute a local fake, production is the
    // GitHub REST implementation.
    git?: GitOps;
    // Where a source tree's git remote is; tests point at local bare repos.
    gitRemote?: (tree: SourceTreeRow) => string;
    // Where staged pins live; defaults under the data dir, or a per-process
    // scratch directory in ephemeral mode.
    pinCacheRoot?: string;
};

export type App = {
    fetch: (request: Request) => Response | Promise<Response>;
    decision: DecisionPoint;
    reconciler: Reconciler;
};

type CapabilitySummary = Record<Action, Decision>;

export function buildApp(deps: AppDeps): App {
    const { config, store, github } = deps;
    const localAuth = new LocalAuth({
        explicitToken: config.packageToken,
        dataDir: config.dataDir,
        github,
    });

    let crypto: TokenCrypto | null = null;
    const tokenCrypto = (): TokenCrypto => {
        if (crypto === null) {
            if (config.tokenEncryptionKey === null) {
                throw new Error(
                    "ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY is required before GitHub sign-in",
                );
            }
            crypto = TokenCrypto.fromEnvValue(config.tokenEncryptionKey);
        }
        return crypto;
    };

    const decision = new DecisionPoint({
        authMode: config.authMode,
        store,
        github,
        tokenCrypto,
    });

    // The acting credential for a principal (user tokens only in C2): the
    // stored GitHub credential in team mode, the ambient token in local
    // mode. Null means the principal cannot act on GitHub right now.
    const principalToken = async (
        principalId: string,
    ): Promise<string | null> => {
        if (config.authMode === "local" || principalId === "local") {
            return (await localAuth.token())?.token ?? null;
        }
        const identity = store
            .identitiesForPrincipal(principalId)
            .find(
                (row) =>
                    row.provider === "github" &&
                    row.credentialCiphertext !== null,
            );
        if (identity === undefined) {
            return null;
        }
        try {
            return tokenCrypto().decrypt(
                identity.credentialCiphertext as string,
            );
        } catch {
            return null;
        }
    };

    const subjectId = (subject: Subject): string =>
        subject.kind === "principal" ? subject.id : "local";

    const git = deps.git ?? new GitHubGit();
    const stager = new PackageStager({
        cacheRoot:
            deps.pinCacheRoot ??
            (config.dataDir !== null
                ? path.join(config.dataDir, "pins")
                : path.join(os.tmpdir(), `rototo-console-pins-${process.pid}`)),
        remoteFor: deps.gitRemote,
    });
    const changeSets = new ChangeSets({ store, git, stager });
    const reconciler = new Reconciler({
        store,
        git,
        tokenFor: (changeSet) => principalToken(changeSet.authorPrincipal),
    });

    const app = new Hono();

    // The mutation guard: header plus Origin allowlist on anything unsafe.
    app.use("/api/*", async (c, next) => {
        const method = c.req.method.toUpperCase();
        if (["POST", "PUT", "PATCH", "DELETE"].includes(method)) {
            if (c.req.header(CONSOLE_HEADER) === undefined) {
                return c.json(
                    {
                        error: {
                            message: `missing ${CONSOLE_HEADER} request header`,
                        },
                    },
                    403,
                );
            }
            const origin = c.req.header("origin");
            if (
                origin !== undefined &&
                !config.allowedOrigins.includes(origin)
            ) {
                return c.json(
                    { error: { message: `origin ${origin} is not allowed` } },
                    403,
                );
            }
        }
        await next();
    });

    const subjectFor = (cookieHeader: string | undefined): Subject | null => {
        if (config.authMode === "local") {
            return { kind: "local" };
        }
        const principalId = sessionPrincipalId(store, cookieHeader);
        return principalId === null
            ? null
            : { kind: "principal", id: principalId };
    };

    const capabilities = async (
        subject: Subject,
        resource: Resource,
    ): Promise<CapabilitySummary> => {
        const summary = {} as CapabilitySummary;
        for (const action of ACTIONS) {
            summary[action] = await decision.decide(subject, action, resource);
        }
        return summary;
    };

    const treePayload = async (
        subject: Subject,
        tree: SourceTreeRow,
    ): Promise<Record<string, unknown>> => ({
        id: tree.id,
        kind: tree.kind,
        owner: tree.owner,
        name: tree.name,
        defaultBranch: tree.defaultBranch,
        resource: resourceString({
            kind: "source-tree",
            sourceTree: tree.id,
        }),
        capabilities: await capabilities(subject, {
            kind: "source-tree",
            sourceTree: tree.id,
        }),
    });

    const ctx: ConsoleContext = {
        config,
        store,
        decision,
        git,
        stager,
        changeSets,
        reconciler,
        lsp: new LspBridge(),
        subjectFor,
        subjectId,
        actingToken: (subject) => principalToken(subjectId(subject)),
    };
    app.route("/api", packageRoutes(ctx));
    app.route("/api", changeSetRoutes(ctx));
    app.route("/api", lspRoutes(ctx));
    app.route("/api", adminRoutes(ctx));
    app.route(
        "/api",
        authRoutes({
            config,
            store,
            github,
            oauthExchange: deps.oauthExchange ?? defaultOauthExchange(config),
            oidc: config.oidc === null ? null : new OidcProvider(config.oidc),
            oidcExchange: deps.oidcExchange ?? null,
            tokenCrypto,
        }),
    );

    app.get("/api/health", (c) => c.json({ ok: true }));

    app.get("/api/me", async (c) => {
        const subject = subjectFor(c.req.header("cookie"));

        if (config.authMode === "local") {
            const identity = await localAuth.localIdentity();
            const ambient = await localAuth.token();
            const displayName =
                identity.kind === "github"
                    ? (identity.name ?? identity.login)
                    : (identity.name ?? identity.email ?? "local git");
            return c.json({
                authMode: "local",
                principal: {
                    id: "local",
                    kind: "human",
                    displayName,
                    status: "active",
                },
                identities: [identity],
                enrollment: "enrolled",
                githubCredentialSource: ambient?.source ?? null,
                capabilities: {
                    deployment: await capabilities(
                        { kind: "local" },
                        {
                            kind: "deployment",
                        },
                    ),
                    sourceTrees: await Promise.all(
                        store
                            .listSourceTrees()
                            .map((tree) =>
                                treePayload({ kind: "local" }, tree),
                            ),
                    ),
                },
            });
        }

        const signIn = {
            github: config.githubOAuth !== null,
            oidc:
                config.oidc === null
                    ? null
                    : { displayName: config.oidc.displayName },
        };
        if (subject === null) {
            return c.json({
                authMode: "team",
                principal: null,
                identities: [],
                enrollment: null,
                signIn,
            });
        }
        const principal =
            subject.kind === "principal"
                ? store.getPrincipal(subject.id)
                : null;
        if (principal === null) {
            return c.json({
                authMode: "team",
                principal: null,
                identities: [],
                enrollment: null,
                signIn,
            });
        }
        const trees = store.listSourceTrees();
        const treeSummaries = [];
        for (const tree of trees) {
            const payload = await treePayload(subject, tree);
            const summary = payload.capabilities as CapabilitySummary;
            if (summary.view.allow) {
                treeSummaries.push(payload);
            }
        }
        return c.json({
            authMode: "team",
            principal: {
                id: principal.id,
                kind: principal.kind,
                displayName: principal.displayName,
                status: principal.status,
            },
            identities: store
                .identitiesForPrincipal(principal.id)
                .map((identity) => ({
                    provider: identity.provider,
                    subject: identity.subject,
                    login: identity.login,
                    name: identity.name,
                    email: identity.email,
                    emailVerified: identity.emailVerified,
                    avatarUrl: identity.avatarUrl,
                    hasCredential: identity.credentialCiphertext !== null,
                    lastSeenAt: identity.lastSeenAt,
                })),
            enrollment: "enrolled",
            capabilities: {
                deployment: await capabilities(subject, {
                    kind: "deployment",
                }),
                sourceTrees: treeSummaries,
            },
        });
    });

    app.get("/api/source-trees", async (c) => {
        const subject = subjectFor(c.req.header("cookie"));
        if (subject === null) {
            return c.json({ error: { message: "sign in first" } }, 401);
        }
        const trees = [];
        for (const tree of store.listSourceTrees()) {
            const payload = await treePayload(subject, tree);
            if ((payload.capabilities as CapabilitySummary).view.allow) {
                trees.push(payload);
            }
        }
        return c.json({ sourceTrees: trees });
    });

    app.post("/api/source-trees", async (c) => {
        const subject = subjectFor(c.req.header("cookie"));
        if (subject === null) {
            return c.json({ error: { message: "sign in first" } }, 401);
        }
        // Registration is a deployment-level act gated by administer.
        const verdict = await decision.decide(subject, "administer", {
            kind: "deployment",
        });
        if (!verdict.allow) {
            return c.json({ error: { message: verdict.reason } }, 403);
        }
        const body = (await c.req.json().catch(() => null)) as {
            kind?: string;
            owner?: string;
            name?: string;
            defaultBranch?: string;
        } | null;
        if (
            body === null ||
            body.kind !== "github" ||
            typeof body.owner !== "string" ||
            typeof body.name !== "string"
        ) {
            return c.json(
                {
                    error: {
                        message:
                            'expected { kind: "github", owner, name, defaultBranch? }',
                    },
                },
                400,
            );
        }
        // The default branch is a repo fact; fill it from GitHub when the
        // registration omits it, with the registrar's own credential.
        let defaultBranch = body.defaultBranch ?? null;
        if (defaultBranch === null) {
            const token = await principalToken(subjectId(subject));
            if (token !== null) {
                const facts = await github
                    .repoFacts(token, body.owner, body.name)
                    .catch(() => null);
                defaultBranch = facts?.defaultBranch ?? null;
            }
        }
        const tree = store.insertSourceTree({
            kind: "github",
            owner: body.owner,
            name: body.name,
            defaultBranch,
            createdBy: subject.kind === "principal" ? subject.id : "local",
        });
        return c.json(await treePayload(subject, tree), 201);
    });

    return { fetch: app.fetch, decision, reconciler };
}
