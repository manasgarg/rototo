// Server spine behavior: the mutation-guard invariants and the GitHub
// sign-in flow (enrollment, credential storage, admin bootstrap).

import assert from "node:assert/strict";
import { test } from "node:test";

import { buildApp, CONSOLE_HEADER } from "../src/app.ts";
import {
    cookieValue,
    OAUTH_STATE_COOKIE,
    SESSION_COOKIE,
} from "../src/sessions.ts";
import { Store } from "../src/store.ts";
import { TokenCrypto } from "../src/token-crypto.ts";
import {
    FakeGitHub,
    json,
    mutationHeaders,
    teamConfig,
    teamHarness,
    TEST_KEY,
} from "./helpers.ts";

test("mutating routes require the console header", async () => {
    const harness = teamHarness();
    const response = await harness.app.fetch(
        new Request("http://console.test/api/auth/logout", {
            method: "POST",
        }),
    );
    assert.equal(response.status, 403);
    const body = await json(response);
    assert.match(body.error.message, /x-rototo-console/);
});

test("mutating routes reject origins outside the allowlist", async () => {
    const harness = teamHarness();
    const response = await harness.app.fetch(
        new Request("http://console.test/api/auth/logout", {
            method: "POST",
            headers: {
                [CONSOLE_HEADER]: "1",
                origin: "https://evil.example",
            },
        }),
    );
    assert.equal(response.status, 403);
    const body = await json(response);
    assert.match(body.error.message, /origin .* is not allowed/);

    const allowed = await harness.app.fetch(
        new Request("http://console.test/api/auth/logout", {
            method: "POST",
            headers: {
                [CONSOLE_HEADER]: "1",
                origin: harness.config.allowedOrigins[0]!,
            },
        }),
    );
    assert.equal(allowed.status, 200);
});

test("reads pass without the header; signed-out /api/me offers sign-in", async () => {
    const harness = teamHarness();
    const response = await harness.app.fetch(
        new Request("http://console.test/api/me"),
    );
    assert.equal(response.status, 200);
    const body = await json(response);
    assert.equal(body.principal, null);
    assert.deepEqual(body.signIn, { github: true, oidc: null });
});

test("GitHub sign-in enrolls, stores an encrypted credential, and bootstraps admins", async () => {
    const config = teamConfig({ admins: ["github:priya"] });
    const store = new Store(null);
    const github = new FakeGitHub();
    github.viewers.set("oauth-token", {
        id: 501,
        login: "priya",
        name: "Priya",
        email: "priya@example.com",
        avatarUrl: null,
    });
    const app = buildApp({
        config,
        store,
        github,
        oauthExchange: async (code) => {
            assert.equal(code, "oauth-code");
            return "oauth-token";
        },
    });

    const start = await app.fetch(
        new Request("http://console.test/api/auth/github/start"),
    );
    assert.equal(start.status, 302);
    const stateCookie = start.headers.get("set-cookie") ?? "";
    const state = cookieValue(stateCookie, OAUTH_STATE_COOKIE);
    assert.ok(state !== null);
    const authorize = new URL(start.headers.get("location")!);
    assert.equal(authorize.searchParams.get("state"), state);

    const callback = await app.fetch(
        new Request(
            `http://console.test/api/auth/github/callback?code=oauth-code&state=${state}`,
            { headers: { cookie: `${OAUTH_STATE_COOKIE}=${state}` } },
        ),
    );
    assert.equal(callback.status, 302);
    const sessionCookie = callback.headers.get("set-cookie") ?? "";
    const session = cookieValue(sessionCookie, SESSION_COOKIE);
    assert.ok(session !== null);

    // Enrollment: principal + identity keyed by the stable subject id.
    const identity = store.getIdentity("github", "501");
    assert.ok(identity !== null);
    assert.equal(identity.login, "priya");
    assert.ok(identity.credentialCiphertext !== null);
    // The credential is encrypted at rest and decrypts to the OAuth token.
    const crypto = TokenCrypto.fromEnvValue(TEST_KEY);
    assert.equal(crypto.decrypt(identity.credentialCiphertext), "oauth-token");

    // The bootstrap admin grant landed and shows up in decisions.
    const me = await json(
        await app.fetch(
            new Request("http://console.test/api/me", {
                headers: { cookie: `${SESSION_COOKIE}=${session}` },
            }),
        ),
    );
    assert.equal(me.principal.displayName, "Priya");
    assert.equal(me.capabilities.deployment.administer.allow, true);
    assert.equal(me.capabilities.deployment.administer.backend, "grant");

    // Signing in again reuses the principal; no duplicate enrollment.
    const secondState = "fixed-state";
    const second = await app.fetch(
        new Request(
            `http://console.test/api/auth/github/callback?code=oauth-code&state=${secondState}`,
            { headers: { cookie: `${OAUTH_STATE_COOKIE}=${secondState}` } },
        ),
    );
    assert.equal(second.status, 302);
    assert.equal(
        store.getIdentity("github", "501")?.principalId,
        identity.principalId,
    );
});

test("state mismatch rejects the OAuth callback", async () => {
    const harness = teamHarness();
    const response = await harness.app.fetch(
        new Request(
            "http://console.test/api/auth/github/callback?code=x&state=forged",
            { headers: { cookie: `${OAUTH_STATE_COOKIE}=real` } },
        ),
    );
    assert.equal(response.status, 400);
});

test("logout ends the session", async () => {
    const harness = teamHarness();
    const dev = harness.signIn({ login: "dev", token: "dev-token" });
    const logout = await harness.app.fetch(
        new Request("http://console.test/api/auth/logout", {
            method: "POST",
            headers: mutationHeaders(dev.headers),
        }),
    );
    assert.equal(logout.status, 200);
    const me = await json(
        await harness.app.fetch(
            new Request("http://console.test/api/me", {
                headers: dev.headers,
            }),
        ),
    );
    assert.equal(me.principal, null);
});
