// Sign-in, enrollment, and identity linking. GitHub keeps its bespoke
// OAuth2 dance (it has no OIDC login, and its token doubles as an acting
// credential); everything else arrives through one generic OIDC provider.
// Completing authentication never grants access by itself — enrollment
// policy decides (src/enroll.ts) — and linking requires a signed-in
// principal, never an email match (design/console-identity-authz.md 3.3).

import { createHash, randomBytes } from "node:crypto";

import { Hono } from "hono";
import type { Context } from "hono";

import type { ServerConfig } from "../config.ts";
import { enrollOrSignIn } from "../enroll.ts";
import { exchangeOAuthCode, type GitHubFacts } from "../github.ts";
import type { OidcExchange, OidcProvider } from "../oidc.ts";
import {
    OAUTH_STATE_COOKIE,
    SESSION_COOKIE,
    SESSION_TTL_MS,
    cookieValue,
    endSession,
    issueSession,
    sessionPrincipalId,
    setCookie,
} from "../sessions.ts";
import type { IdentitySnapshot, Store } from "../store.ts";
import type { TokenCrypto } from "../token-crypto.ts";

export const AUTH_INTENT_COOKIE = "rototo_console_auth_intent";
export const OIDC_NONCE_COOKIE = "rototo_console_oidc_nonce";
export const INVITE_COOKIE = "rototo_console_invite";
const GITHUB_OAUTH_SCOPES = "read:user repo";

export type AuthDeps = {
    config: ServerConfig;
    store: Store;
    github: GitHubFacts;
    oauthExchange: (code: string) => Promise<string>;
    // Null when no OIDC provider is configured.
    oidc: OidcProvider | null;
    // The test seam; defaults to the provider's own exchange.
    oidcExchange: OidcExchange | null;
    tokenCrypto: () => TokenCrypto;
};

export function authRoutes(deps: AuthDeps): Hono {
    const { config, store } = deps;
    const secureCookies = config.publicUrl.startsWith("https://");
    const app = new Hono();

    // Shared start-flow bookkeeping: the state cookie, the intent (sign-in
    // or link, and link demands a session), and an optional invite token.
    const beginFlow = (
        c: Context,
    ): { state: string; error?: undefined } | { error: Response } => {
        const state = randomBytes(16).toString("base64url");
        const cookies = [
            setCookie(OAUTH_STATE_COOKIE, state, secureCookies, 600),
        ];
        const linking = c.req.query("link") === "1";
        if (linking) {
            const principal = sessionPrincipalId(store, c.req.header("cookie"));
            if (principal === null) {
                return {
                    error: c.json(
                        { error: { message: "sign in before linking" } },
                        401,
                    ),
                };
            }
        }
        cookies.push(
            setCookie(
                AUTH_INTENT_COOKIE,
                linking ? "link" : "signin",
                secureCookies,
                600,
            ),
        );
        const invite = c.req.query("invite");
        cookies.push(
            setCookie(INVITE_COOKIE, invite ?? "", secureCookies, 600),
        );
        for (const cookie of cookies) {
            c.header("set-cookie", cookie, { append: true });
        }
        return { state };
    };

    // The shared callback tail: link the identity to the signed-in
    // principal, or run enrollment and issue a session.
    const completeFlow = (
        c: Context,
        snapshot: IdentitySnapshot,
        credentialCiphertext: string | null,
    ): Response => {
        const cookieHeader = c.req.header("cookie");
        const intent = cookieValue(cookieHeader, AUTH_INTENT_COOKIE);
        if (intent === "link") {
            const principalId = sessionPrincipalId(store, cookieHeader);
            if (principalId === null) {
                return c.json(
                    { error: { message: "the linking session expired" } },
                    401,
                );
            }
            const existing = store.getIdentity(
                snapshot.provider,
                snapshot.subject,
            );
            if (existing !== null && existing.principalId !== principalId) {
                return c.json(
                    {
                        error: {
                            message:
                                "this identity already belongs to another principal",
                        },
                    },
                    409,
                );
            }
            if (existing !== null) {
                store.refreshIdentity(
                    existing.id,
                    snapshot,
                    credentialCiphertext,
                );
            } else {
                store.attachIdentity(
                    principalId,
                    snapshot,
                    credentialCiphertext,
                );
                store.appendAudit(
                    principalId,
                    "identity.link",
                    JSON.stringify({
                        provider: snapshot.provider,
                        subject: snapshot.subject,
                    }),
                );
            }
            return c.redirect(config.publicUrl, 302);
        }

        const invite = cookieValue(cookieHeader, INVITE_COOKIE);
        const outcome = enrollOrSignIn(store, config, {
            snapshot,
            credentialCiphertext,
            inviteTokenHash:
                invite === null || invite === "" ? null : hashInvite(invite),
        });
        if (outcome.kind === "not-enrolled") {
            return c.redirect(`${config.publicUrl}/#/not-enrolled`, 302);
        }
        const session = issueSession(store, outcome.principalId);
        c.header(
            "set-cookie",
            setCookie(
                SESSION_COOKIE,
                session,
                secureCookies,
                SESSION_TTL_MS / 1000,
            ),
        );
        return c.redirect(config.publicUrl, 302);
    };

    const checkState = (c: Context): boolean => {
        const expected = cookieValue(
            c.req.header("cookie"),
            OAUTH_STATE_COOKIE,
        );
        const state = c.req.query("state");
        return state !== undefined && expected !== null && state === expected;
    };

    app.get("/auth/github/start", (c) => {
        if (config.githubOAuth === null) {
            return c.json(
                { error: { message: "GitHub sign-in is not configured" } },
                404,
            );
        }
        const flow = beginFlow(c);
        if (flow.error !== undefined) {
            return flow.error;
        }
        const authorize = new URL("https://github.com/login/oauth/authorize");
        authorize.searchParams.set("client_id", config.githubOAuth.clientId);
        authorize.searchParams.set("scope", GITHUB_OAUTH_SCOPES);
        authorize.searchParams.set("state", flow.state);
        authorize.searchParams.set(
            "redirect_uri",
            `${config.publicUrl}/api/auth/github/callback`,
        );
        return c.redirect(authorize.toString(), 302);
    });

    app.get("/auth/github/callback", async (c) => {
        if (config.githubOAuth === null) {
            return c.json(
                { error: { message: "GitHub sign-in is not configured" } },
                404,
            );
        }
        const code = c.req.query("code");
        if (code === undefined || !checkState(c)) {
            return c.json(
                { error: { message: "OAuth state mismatch; retry sign-in" } },
                400,
            );
        }
        const token = await deps.oauthExchange(code);
        const viewer = await deps.github.viewer(token);
        const ciphertext = deps.tokenCrypto().encrypt(token);
        return completeFlow(
            c,
            {
                provider: "github",
                subject: String(viewer.id),
                login: viewer.login,
                email: viewer.email,
                // The REST viewer call does not assert verification; treat
                // the email as display data only.
                emailVerified: false,
                name: viewer.name,
                avatarUrl: viewer.avatarUrl,
            },
            ciphertext,
        );
    });

    app.get("/auth/oidc/start", async (c) => {
        if (deps.oidc === null || config.oidc === null) {
            return c.json(
                { error: { message: "SSO sign-in is not configured" } },
                404,
            );
        }
        const flow = beginFlow(c);
        if (flow.error !== undefined) {
            return flow.error;
        }
        const nonce = randomBytes(16).toString("base64url");
        c.header(
            "set-cookie",
            setCookie(OIDC_NONCE_COOKIE, nonce, secureCookies, 600),
            { append: true },
        );
        const redirectUri = `${config.publicUrl}/api/auth/oidc/callback`;
        // With an injected exchange (tests) the issuer is never contacted;
        // a synthetic authorize URL keeps the flow's cookie mechanics real.
        const authorize =
            deps.oidcExchange !== null
                ? syntheticAuthorizeUrl(
                      config.oidc,
                      redirectUri,
                      flow.state,
                      nonce,
                  )
                : await deps.oidc.authorizeUrl(redirectUri, flow.state, nonce);
        return c.redirect(authorize, 302);
    });

    app.get("/auth/oidc/callback", async (c) => {
        if (config.oidc === null) {
            return c.json(
                { error: { message: "SSO sign-in is not configured" } },
                404,
            );
        }
        const code = c.req.query("code");
        const nonce = cookieValue(c.req.header("cookie"), OIDC_NONCE_COOKIE);
        if (code === undefined || nonce === null || !checkState(c)) {
            return c.json(
                { error: { message: "OIDC state mismatch; retry sign-in" } },
                400,
            );
        }
        const exchange = deps.oidcExchange ?? deps.oidc?.exchange;
        if (exchange === undefined || exchange === null) {
            return c.json(
                { error: { message: "SSO sign-in is not configured" } },
                404,
            );
        }
        const claims = await exchange(
            code,
            `${config.publicUrl}/api/auth/oidc/callback`,
            nonce,
        );
        return completeFlow(
            c,
            {
                provider: "oidc",
                // Keyed by iss + sub, exactly as asserted in the verified
                // ID token; email and name are display snapshots.
                subject: `${claims.issuer}#${claims.subject}`,
                login: null,
                email: claims.email,
                emailVerified: claims.emailVerified,
                name: claims.name,
                avatarUrl: claims.picture,
            },
            null,
        );
    });

    app.post("/auth/logout", (c) => {
        endSession(store, c.req.header("cookie"));
        c.header("set-cookie", setCookie(SESSION_COOKIE, "", secureCookies, 0));
        return c.json({ ok: true });
    });

    return app;
}

export function defaultOauthExchange(
    config: ServerConfig,
): (code: string) => Promise<string> {
    return (code) =>
        exchangeOAuthCode(
            config.githubOAuth?.clientId ?? "",
            config.githubOAuth?.clientSecret ?? "",
            code,
        );
}

// Invitation tokens are delivered out of band; the store keeps only this
// hash, so a leaked database cannot mint redemptions.
export function hashInvite(token: string): string {
    return createHash("sha256").update(token).digest("hex");
}

function syntheticAuthorizeUrl(
    oidc: { issuer: string; clientId: string },
    redirectUri: string,
    state: string,
    nonce: string,
): string {
    const url = new URL(`${oidc.issuer}/authorize`);
    url.searchParams.set("response_type", "code");
    url.searchParams.set("client_id", oidc.clientId);
    url.searchParams.set("redirect_uri", redirectUri);
    url.searchParams.set("scope", "openid email profile");
    url.searchParams.set("state", state);
    url.searchParams.set("nonce", nonce);
    return url.toString();
}
