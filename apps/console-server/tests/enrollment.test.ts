// Phase B identity: OIDC sign-in through the injectable exchange,
// enrollment policies (completing authentication grants nothing), the
// invitation loop with initial groups and grants, identity linking, and the
// admin API with its audit trail.

import assert from "node:assert/strict";
import { test } from "node:test";

import type { OidcClaims } from "../src/oidc.ts";
import { SESSION_COOKIE } from "../src/sessions.ts";
import {
    FakeGitHub,
    json,
    mutationHeaders,
    teamHarness,
    type TeamHarness,
} from "./helpers.ts";

const OIDC_CONFIG = {
    issuer: "https://sso.acme.test",
    clientId: "console-client",
    clientSecret: "console-secret",
    displayName: "Acme SSO",
};

function oidcHarness(
    overrides: Record<string, unknown>,
    claimsByCode: Map<string, OidcClaims>,
): TeamHarness {
    return teamHarness(
        { oidc: OIDC_CONFIG, ...overrides },
        {
            oidcExchange: async (code) => {
                const claims = claimsByCode.get(code);
                if (claims === undefined) {
                    throw new Error("unknown code");
                }
                return claims;
            },
        },
    );
}

function claimsFor(
    subject: string,
    email: string,
    verified = true,
): OidcClaims {
    return {
        issuer: OIDC_CONFIG.issuer,
        subject,
        email,
        emailVerified: verified,
        name: `User ${subject}`,
        picture: null,
    };
}

// Walks the two-step flow: start (collecting the state/nonce/intent
// cookies), then callback with a code the fake exchange recognizes.
async function completeOidc(
    harness: TeamHarness,
    code: string,
    startQuery = "",
    extraCookies: Record<string, string> = {},
): Promise<Response> {
    const start = await harness.app.fetch(
        new Request(`http://console.test/api/auth/oidc/start${startQuery}`, {
            headers:
                Object.keys(extraCookies).length > 0
                    ? { cookie: cookieHeader(extraCookies) }
                    : {},
        }),
    );
    assert.equal(start.status, 302, await start.clone().text());
    const cookies = { ...cookiesOf(start), ...extraCookies };
    const state = cookies.rototo_console_oauth_state as string;
    return harness.app.fetch(
        new Request(
            `http://console.test/api/auth/oidc/callback?code=${code}&state=${state}`,
            { headers: { cookie: cookieHeader(cookies) } },
        ),
    );
}

function cookiesOf(response: Response): Record<string, string> {
    const jar: Record<string, string> = {};
    for (const header of response.headers.getSetCookie()) {
        const pair = header.split(";")[0] as string;
        const eq = pair.indexOf("=");
        jar[pair.slice(0, eq)] = pair.slice(eq + 1);
    }
    return jar;
}

function cookieHeader(jar: Record<string, string>): string {
    return Object.entries(jar)
        .map(([name, value]) => `${name}=${value}`)
        .join("; ");
}

test("groups rename and delete, invitations revoke, identities unlink", async () => {
    const harness = teamHarness();
    const admin = harness.signIn({ login: "admin", token: "admin-token" });
    harness.store.insertGrant({
        granteeKind: "principal",
        granteeId: admin.principalId,
        action: "administer",
        resource: "deployment",
        createdBy: null,
    });
    const post = (path: string, payload: unknown): Promise<Response> =>
        Promise.resolve(
            harness.app.fetch(
                new Request(`http://console.test/api${path}`, {
                    method: "POST",
                    headers: mutationHeaders(admin.headers),
                    body: JSON.stringify(payload),
                }),
            ),
        );

    // Groups: rename in place (names are labels, not addresses); delete
    // refuses while a grant references the group, and works after revoke.
    const group = (await json(await post("/admin/groups", { name: "ops" })))
        .group;
    const renamed = await json(
        await post(`/admin/groups/${group.id}/update`, {
            name: "platform_ops",
            description: "who runs prod",
        }),
    );
    assert.equal(renamed.group.name, "platform_ops");
    assert.equal(renamed.group.description, "who runs prod");
    const granted = await json(
        await post("/admin/grants", {
            granteeKind: "group",
            granteeId: group.id,
            action: "view",
            resource: "deployment",
        }),
    );
    const blocked = await post(`/admin/groups/${group.id}/delete`, {});
    assert.equal(blocked.status, 409);
    assert.match((await json(blocked)).error.message, /revoke them first/);
    await post(`/admin/grants/${granted.grant.id}/revoke`, {});
    const deleted = await post(`/admin/groups/${group.id}/delete`, {});
    assert.equal(deleted.status, 200, await deleted.clone().text());
    assert.equal(harness.store.getGroup(group.id), null);

    // Invitations: a pending one hard-deletes; nothing else references it.
    const invited = await json(
        await post("/admin/invitations", { email: "sam@acme.com" }),
    );
    const revoked = await post(
        `/admin/invitations/${invited.invitation.id}/revoke`,
        {},
    );
    assert.equal(revoked.status, 200, await revoked.clone().text());
    assert.equal(harness.store.listInvitations().length, 0);

    // Identities: the last one refuses (that would be a disable wearing a
    // disguise); a second identity unlinks cleanly.
    const solo = harness.store.identitiesForPrincipal(admin.principalId);
    const refused = await post(`/admin/identities/${solo[0]?.id}/unlink`, {});
    assert.equal(refused.status, 409);
    assert.match((await json(refused)).error.message, /last identity/);
    const second = harness.store.attachIdentity(
        admin.principalId,
        {
            provider: "oidc",
            subject: "admin-okta-sub",
            login: null,
            email: "admin@acme.com",
            emailVerified: true,
            name: "Admin",
            avatarUrl: null,
        },
        null,
    );
    const unlinked = await post(`/admin/identities/${second.id}/unlink`, {});
    assert.equal(unlinked.status, 200, await unlinked.clone().text());
    assert.equal(
        harness.store.identitiesForPrincipal(admin.principalId).length,
        1,
    );
});

test("invite-only: completing SSO authentication grants nothing", async () => {
    const claims = new Map([["code-1", claimsFor("u-1", "sam@acme.com")]]);
    const harness = oidcHarness({}, claims);
    const callback = await completeOidc(harness, "code-1");
    assert.equal(callback.status, 302);
    assert.match(
        callback.headers.get("location") ?? "",
        /not-enrolled/,
        "an uninvited identity lands on the not-enrolled screen",
    );
    // No principal row was created; curious visitors leave no residue
    // under invite-only.
    assert.equal(harness.store.listPrincipals().length, 0);
});

test("the invitation loop: redeem by link, initial groups and grants apply", async () => {
    const claims = new Map([
        ["code-priya", claimsFor("priya-sub", "priya@acme.com")],
    ]);
    const harness = oidcHarness({}, claims);
    // The administrator: enrolled directly, deployment administer.
    const admin = harness.signIn({ login: "admin", token: "admin-token" });
    harness.store.insertGrant({
        granteeKind: "principal",
        granteeId: admin.principalId,
        action: "administer",
        resource: "deployment",
        createdBy: null,
    });
    const group = await json(
        await harness.app.fetch(
            new Request("http://console.test/api/admin/groups", {
                method: "POST",
                headers: mutationHeaders(admin.headers),
                body: JSON.stringify({ name: "pricing_editors" }),
            }),
        ),
    );

    const tree = harness.store.insertSourceTree({
        kind: "github",
        owner: "acme",
        name: "config",
        defaultBranch: "main",
        createdBy: null,
    });
    const invited = await json(
        await harness.app.fetch(
            new Request("http://console.test/api/admin/invitations", {
                method: "POST",
                headers: mutationHeaders(admin.headers),
                body: JSON.stringify({
                    email: "priya@acme.com",
                    providerRestriction: "oidc",
                    initialGroups: ["pricing_editors"],
                    initialGrants: [
                        {
                            action: "propose",
                            resource: `source-tree:${tree.id}`,
                        },
                    ],
                }),
            }),
        ),
    );
    assert.ok(invited.token);
    assert.match(invited.link, /invite=/);

    // Priya redeems through the link: token in the start query, SSO dance,
    // session out the other side.
    const callback = await completeOidc(
        harness,
        "code-priya",
        `?invite=${invited.token}`,
    );
    assert.equal(callback.status, 302);
    const session = cookiesOf(callback)[SESSION_COOKIE];
    assert.ok(session, "redeeming issues a session");

    const me = await json(
        await harness.app.fetch(
            new Request("http://console.test/api/me", {
                headers: { cookie: `${SESSION_COOKIE}=${session}` },
            }),
        ),
    );
    assert.equal(me.principal.displayName, "User priya-sub");
    assert.equal(me.identities[0].provider, "oidc");
    // The initial grant reaches decisions immediately.
    const treeSummary = me.capabilities.sourceTrees.find(
        (entry: any) => entry.id === tree.id,
    );
    assert.equal(treeSummary.capabilities.propose.allow, true);
    assert.equal(treeSummary.capabilities.propose.backend, "grant");
    // The group membership landed too.
    assert.deepEqual(harness.store.listGroupMembers(group.group.id), [
        me.principal.id,
    ]);
    // Single use: the invitation is spent.
    assert.equal(
        harness.store.listInvitations()[0]?.redeemedBy,
        me.principal.id,
    );
    // The audit trail names the acts.
    const events = harness.store.listAudit().map((row) => row.event);
    assert.ok(events.includes("invitation.create"));
    assert.ok(events.includes("principal.enroll"));
    assert.ok(events.includes("invitation.redeem"));
});

test("domain allowlist enrolls verified emails with zero grants", async () => {
    const claims = new Map([
        ["code-bob", claimsFor("bob-sub", "bob@acme.com")],
        ["code-eve", claimsFor("eve-sub", "eve@acme.com", false)],
    ]);
    const harness = oidcHarness(
        { enrollment: "domain-allowlist", enrollmentDomains: ["acme.com"] },
        claims,
    );
    const bob = await completeOidc(harness, "code-bob");
    assert.ok(cookiesOf(bob)[SESSION_COOKIE], "verified domain enrolls");
    assert.equal(harness.store.listPrincipals().length, 1);

    // An unverified email is display data only; it enrolls nothing.
    const eve = await completeOidc(harness, "code-eve");
    assert.match(eve.headers.get("location") ?? "", /not-enrolled/);
    assert.equal(harness.store.listPrincipals().length, 1);
});

test("linking attaches a GitHub credential to an SSO principal, never by email", async () => {
    const claims = new Map([
        ["code-dev", claimsFor("dev-sub", "dev@acme.com")],
    ]);
    const harness = teamHarness(
        { oidc: OIDC_CONFIG, enrollment: "open" },
        {
            oidcExchange: async (code) => {
                const found = claims.get(code);
                if (found === undefined) {
                    throw new Error("unknown code");
                }
                return found;
            },
            oauthExchange: async () => "gh-token",
        },
    );
    (harness.github as FakeGitHub).viewers.set("gh-token", {
        id: 900,
        login: "dev",
        name: "Dev",
        // Same email on both providers — and still two principals unless
        // linked explicitly while signed in.
        email: "dev@acme.com",
        avatarUrl: null,
    });

    const signedIn = await completeOidc(harness, "code-dev");
    const session = cookiesOf(signedIn)[SESSION_COOKIE] as string;

    // The link flow: start with link=1 while signed in, then the callback
    // attaches the GitHub identity (and its credential) to this principal.
    const start = await harness.app.fetch(
        new Request("http://console.test/api/auth/github/start?link=1", {
            headers: { cookie: `${SESSION_COOKIE}=${session}` },
        }),
    );
    assert.equal(start.status, 302, await start.clone().text());
    const jar: Record<string, string> = {
        ...cookiesOf(start),
        [SESSION_COOKIE]: session,
    };
    const state = jar.rototo_console_oauth_state as string;
    const oauthDeps = harness.app; // callback goes through the same app
    const callback = await oauthDeps.fetch(
        new Request(
            `http://console.test/api/auth/github/callback?code=gh-code&state=${state}`,
            { headers: { cookie: cookieHeader(jar) } },
        ),
    );
    assert.equal(callback.status, 302, await callback.clone().text());

    const me = await json(
        await harness.app.fetch(
            new Request("http://console.test/api/me", {
                headers: { cookie: `${SESSION_COOKIE}=${session}` },
            }),
        ),
    );
    assert.equal(me.identities.length, 2);
    const github = me.identities.find(
        (identity: any) => identity.provider === "github",
    );
    assert.equal(github.hasCredential, true);
    // Linking without a session is refused outright.
    const anonymous = await harness.app.fetch(
        new Request("http://console.test/api/auth/github/start?link=1"),
    );
    assert.equal(anonymous.status, 401);
});

test("the admin API gates on administer and audits every act", async () => {
    const harness = teamHarness();
    const admin = harness.signIn({ login: "root", token: "root-token" });
    harness.store.insertGrant({
        granteeKind: "principal",
        granteeId: admin.principalId,
        action: "administer",
        resource: "deployment",
        createdBy: null,
    });
    const outsider = harness.signIn({ login: "eve", token: "eve-token" });

    const denied = await harness.app.fetch(
        new Request("http://console.test/api/admin/groups", {
            method: "POST",
            headers: mutationHeaders(outsider.headers),
            body: JSON.stringify({ name: "ops" }),
        }),
    );
    assert.equal(denied.status, 403);

    // A grant on an unparseable or entity-scoped resource is refused; the
    // v1 admin surface stops at package scope on purpose.
    const badResource = await harness.app.fetch(
        new Request("http://console.test/api/admin/grants", {
            method: "POST",
            headers: mutationHeaders(admin.headers),
            body: JSON.stringify({
                granteeKind: "principal",
                granteeId: admin.principalId,
                action: "view",
                resource: "entity:st_1/pkg/variable=x",
            }),
        }),
    );
    assert.equal(badResource.status, 400);

    // Disable a principal: their decisions die with their sessions.
    const disable = await harness.app.fetch(
        new Request(
            `http://console.test/api/admin/principals/${outsider.principalId}/status`,
            {
                method: "POST",
                headers: mutationHeaders(admin.headers),
                body: JSON.stringify({ status: "disabled" }),
            },
        ),
    );
    assert.equal(disable.status, 200);
    const afterDisable = await harness.app.fetch(
        new Request("http://console.test/api/me", {
            headers: outsider.headers,
        }),
    );
    const body = await json(afterDisable);
    assert.equal(body.principal, null, "disabling killed the session");

    const events = harness.store.listAudit().map((row) => row.event);
    assert.ok(events.includes("principal.disable"));
});
