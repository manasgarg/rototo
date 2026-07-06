// The C5 gate — the acceptance test this whole redesign aims at
// (design/console-implementation-plan.md): a pricing manager with no
// GitHub account signs in with SSO, edits through a surface, sees impact
// with its confidence stated, submits; an approver approves; the App
// merges; the audit trail names everyone. Performed through the HTTP
// surface, timed, and kept as a regression gate from here on.

import assert from "node:assert/strict";
import { after, test } from "node:test";

import { BUDGETS_MS } from "../src/budgets.ts";
import { native } from "../src/native.ts";
import { SESSION_COOKIE } from "../src/sessions.ts";
import { SURFACES_SCHEMA } from "../src/surfaces.ts";
import { gitHarness, json, mutationHeaders } from "./helpers.ts";

const OIDC_CONFIG = {
    issuer: "https://sso.acme.test",
    clientId: "console-client",
    clientSecret: "console-secret",
    displayName: "Acme SSO",
};

// The App is installed on the repo; the fake mints a token FakeGit accepts.
const harness = gitHarness(
    { oidc: OIDC_CONFIG },
    {
        oidcExchange: async (code) => {
            assert.equal(code, "priya-code");
            return {
                issuer: OIDC_CONFIG.issuer,
                subject: "priya-okta-sub",
                email: "priya@acme.com",
                emailVerified: true,
                name: "Priya Sharma",
                picture: null,
            };
        },
        appCredentials: {
            installationToken: async () => "app-installation-token",
        },
    },
);
after(() => harness.cleanup());
const RELEASE_NATIVE = native.buildProfile() === "release";

// The platform team's pricing slice, already on main (same shape the C4
// gate proved through the GitHub-holding PM).
const surfacePin = harness.fakeGit.commitDirect("main", "add pricing", [
    {
        path: "packages/basic/model/catalogs/plans.schema.json",
        content: `{
  "type": "object",
  "required": ["title", "monthly_price_usd"],
  "properties": {
    "title": { "type": "string" },
    "monthly_price_usd": { "type": "number", "minimum": 0 }
  },
  "additionalProperties": false
}
`,
    },
    {
        path: "packages/basic/data/catalogs/plans/starter.toml",
        content: 'title = "Starter"\nmonthly_price_usd = 9\n',
    },
    {
        path: "packages/basic/data/catalogs/plans/pro.toml",
        content: 'title = "Pro"\nmonthly_price_usd = 49\n',
    },
    {
        path: "packages/basic/variables/active_plan.toml",
        content: `schema_version = 1

description = "The plan checkout charges for"
type = "catalog=plans"

[resolve]
default = "starter"

[[resolve.rule]]
when = 'context.user.tier == "premium"'
value = "pro"
`,
    },
    {
        path: "packages/basic/model/catalogs/console/surfaces.schema.json",
        content: `${JSON.stringify(SURFACES_SCHEMA, null, 2)}\n`,
    },
    {
        path: "packages/basic/data/catalogs/console/surfaces/pricing.toml",
        content: `kind = "table"
title = "Pricing"
description = "Plans and prices."
audience = ["internal"]
approval = "role:pricing_admins"

[[bind]]
target = "catalog=plans"
editable_fields = ["monthly_price_usd"]
`,
    },
]);

test("the full Priya walkthrough, end to end and timed", async (t) => {
    const startedAt = performance.now();
    const timings: Record<string, number> = {};
    const timed = async <T>(
        name: string,
        run: () => Promise<T>,
    ): Promise<T> => {
        const started = performance.now();
        const value = await run();
        timings[name] = performance.now() - started;
        return value;
    };

    // --- The administrator sets the stage: a group for the approval role,
    // the approver in it with an approve grant, and Priya's invitation
    // carrying her propose grant.
    const admin = harness.signIn({ login: "root", token: "root-token" });
    harness.store.insertGrant({
        granteeKind: "principal",
        granteeId: admin.principalId,
        action: "administer",
        resource: "deployment",
        createdBy: null,
    });
    const approver = harness.signIn({
        login: "alex",
        token: "alex-token",
        displayName: "Alex (Approver)",
    });
    const group = await json(
        await harness.post(
            "/api/admin/groups",
            { name: "pricing_admins" },
            admin.headers,
        ),
    );
    await harness.post(
        `/api/admin/groups/${group.group.id}/members`,
        { principalId: approver.principalId },
        admin.headers,
    );
    await harness.post(
        "/api/admin/grants",
        {
            granteeKind: "group",
            granteeId: group.group.id,
            action: "approve",
            resource: `source-tree:${harness.tree.id}`,
        },
        admin.headers,
    );
    const invited = await json(
        await harness.post(
            "/api/admin/invitations",
            {
                email: "priya@acme.com",
                providerRestriction: "oidc",
                initialGrants: [
                    {
                        action: "propose",
                        resource: `source-tree:${harness.tree.id}`,
                    },
                ],
            },
            admin.headers,
        ),
    );

    // --- Priya signs in with SSO through her invitation link. She has no
    // GitHub account; her identity is the verified ID token.
    const priyaSession = await timed("SSO sign-in + enrollment", async () => {
        const start = await harness.app.fetch(
            new Request(
                `http://console.test/api/auth/oidc/start?invite=${invited.token}`,
            ),
        );
        assert.equal(start.status, 302);
        const jar: Record<string, string> = {};
        for (const header of start.headers.getSetCookie()) {
            const pair = header.split(";")[0] as string;
            const eq = pair.indexOf("=");
            jar[pair.slice(0, eq)] = pair.slice(eq + 1);
        }
        const callback = await harness.app.fetch(
            new Request(
                `http://console.test/api/auth/oidc/callback?code=priya-code&state=${jar.rototo_console_oauth_state}`,
                {
                    headers: {
                        cookie: Object.entries(jar)
                            .map(([name, value]) => `${name}=${value}`)
                            .join("; "),
                    },
                },
            ),
        );
        assert.equal(callback.status, 302, await callback.clone().text());
        const session = callback.headers
            .getSetCookie()
            .map((header) => header.split(";")[0] as string)
            .find((pair) => pair.startsWith(`${SESSION_COOKIE}=`));
        assert.ok(session, "enrollment issued a session");
        return { cookie: session as string };
    });
    const priya = { headers: { cookie: priyaSession.cookie } };

    const me = await json(
        await harness.app.fetch(
            new Request("http://console.test/api/me", {
                headers: priya.headers,
            }),
        ),
    );
    assert.equal(me.principal.displayName, "Priya Sharma");
    assert.equal(me.identities[0].provider, "oidc");
    const treeSummary = me.capabilities.sourceTrees.find(
        (entry: any) => entry.id === harness.tree.id,
    );
    assert.equal(treeSummary.capabilities.propose.allow, true);
    assert.equal(treeSummary.capabilities.propose.backend, "grant");
    const priyaId = me.principal.id as string;

    // Priya is a pricing admin herself — she approves other people's price
    // changes. The two-person rule below is what keeps that from mattering
    // for her own.
    await harness.post(
        `/api/admin/groups/${group.group.id}/members`,
        { principalId: priyaId },
        admin.headers,
    );

    // --- Priya opens the pricing surface (the App token reads for her).
    const surfaceUrl = `/api/source-trees/${harness.tree.id}/surface?path=${encodeURIComponent(
        harness.packagePath,
    )}&pin=${surfacePin}&id=pricing`;
    await harness.get(surfaceUrl, priya.headers);
    const surface = await timed("open surface (warm)", async () => {
        const response = await harness.get(surfaceUrl, priya.headers);
        assert.equal(response.status, 200, await response.clone().text());
        return json(response);
    });
    const table = surface.items.find((item: any) => item.kind === "catalog");
    assert.deepEqual(table.fields, [
        { field: "monthly_price_usd", control: "number" },
    ]);

    // --- She edits the price. The change set acts through the App, and
    // the commit says so in git itself.
    const changeSet = await json(
        await harness.post(
            `/api/source-trees/${harness.tree.id}/change-sets`,
            { title: "Raise Pro to $59" },
            priya.headers,
        ),
    );
    assert.equal(changeSet.actingMode, "app");
    const edited = await timed("save price edit", async () => {
        return json(
            await harness.post(
                `/api/change-sets/${changeSet.id}/edits`,
                {
                    packagePath: harness.packagePath,
                    expectedPin: changeSet.baseShaAtCreation,
                    operations: [
                        {
                            op: "set_field",
                            target: "catalog=plans:entry=pro#/monthly_price_usd",
                            value: 59,
                        },
                    ],
                    summary: "Raise Pro to $59",
                },
                priya.headers,
            ),
        );
    });
    assert.equal(edited.records[0].operation, "set_field");
    const commitMessage = harness.fakeGit.commitMessage(edited.pin);
    assert.match(
        commitMessage,
        new RegExp(`Acting-For: ${priyaId} \\(Priya Sharma\\)`),
        "the App-authored commit names the person in git itself",
    );

    // --- She submits; the PR carries the marker and the attribution.
    const submitted = await json(
        await harness.post(
            `/api/change-sets/${changeSet.id}/submit`,
            { body: "Pro goes to $59 next cycle." },
            priya.headers,
        ),
    );
    assert.equal(submitted.changeSet.state, "proposed");
    const pull = await harness.fakeGit.getPull(
        "",
        { owner: "acme", name: "config" },
        submitted.pull.number,
    );
    assert.match(pull?.body ?? "", /Rototo-Change-Set:/);
    assert.match(pull?.body ?? "", /Acting-For:/);

    // --- She reads the impact, with its confidence stated. (First read
    // stages the trees; the budget is judged on the warm read, like every
    // preview-class budget.)
    await harness.get(`/api/change-sets/${changeSet.id}/review`, priya.headers);
    const review = await timed("review (impact w/ confidence)", async () => {
        return json(
            await harness.get(
                `/api/change-sets/${changeSet.id}/review`,
                priya.headers,
            ),
        );
    });
    const pkg = review.review.packages[0];
    assert.ok(!("redacted" in pkg));
    const priceChange = pkg.changes.find(
        (change: any) => change.kind === "catalog_entry_changed",
    );
    assert.equal(priceChange.before, 49);
    assert.equal(priceChange.after, 59);
    assert.ok(pkg.denominator.samples >= 1);
    const coverage = pkg.denominator.variables.find(
        (variable: any) => variable.id === "active_plan",
    );
    assert.equal(coverage.defaultCovered, false);
    // The policy names what this change must satisfy, before anyone asks.
    assert.deepEqual(review.policy.requirements, [
        { kind: "role", role: "pricing_admins", surfaces: ["pricing"] },
    ]);
    assert.equal(review.policy.satisfied, false);

    // --- The two-person rule: Priya cannot approve her own change.
    const selfApprove = await harness.app.fetch(
        new Request(
            `http://console.test/api/change-sets/${changeSet.id}/approve`,
            {
                method: "POST",
                headers: mutationHeaders(priya.headers),
                body: JSON.stringify({}),
            },
        ),
    );
    assert.equal(selfApprove.status, 403);
    assert.match(
        (await json(selfApprove)).error.message,
        /second person/,
    );

    // --- The approver approves; policy is satisfied; the merge lands.
    const approved = await timed("approve + merge", async () => {
        const response = await harness.post(
            `/api/change-sets/${changeSet.id}/approve`,
            {},
            approver.headers,
        );
        assert.equal(response.status, 200, await response.clone().text());
        return json(response);
    });
    assert.equal(approved.recorded, true);
    assert.equal(approved.merged, true);

    // The PR timeline keeps the approval (the fire drill's copy).
    const comments =
        harness.fakeGit.pullComments.get(submitted.pull.number) ?? [];
    assert.ok(
        comments.some((comment) => /Approved by Alex/.test(comment)),
        JSON.stringify(comments),
    );

    // --- The world after: merged state, the price on main, and an audit
    // trail that names everyone.
    const observed = await json(
        await harness.get(`/api/change-sets/${changeSet.id}`, priya.headers),
    );
    assert.equal(observed.changeSet.state, "merged");
    const mainPin = await harness.fakeGit.getRef(
        "",
        { owner: "acme", name: "config" },
        "main",
    );
    assert.match(
        harness.fakeGit.readFileAt(
            mainPin as string,
            "packages/basic/data/catalogs/plans/pro.toml",
        ),
        /monthly_price_usd = 59/,
    );
    const diary = observed.events.map((event: any) => [
        event.event,
        event.actor,
    ]);
    assert.deepEqual(diary[0], ["created", priyaId]);
    assert.ok(
        diary.some(
            ([event, actor]: string[]) =>
                event === "committed" && actor === priyaId,
        ),
    );
    assert.ok(
        diary.some(
            ([event, actor]: string[]) =>
                event === "approved" && actor === approver.principalId,
        ),
    );
    assert.ok(
        diary.some(
            ([event, actor]: string[]) =>
                event === "merged" && actor === approver.principalId,
        ),
    );
    const audit = harness.store.listAudit().map((row) => row.event);
    for (const expected of [
        "group.create",
        "group.member.add",
        "grant.create",
        "invitation.create",
        "principal.enroll",
        "invitation.redeem",
    ]) {
        assert.ok(audit.includes(expected), `audit misses ${expected}`);
    }

    // The timing report and the budgets.
    const total = performance.now() - startedAt;
    for (const [name, value] of Object.entries(timings)) {
        t.diagnostic(`${name}: ${value.toFixed(1)}ms`);
    }
    t.diagnostic(`walkthrough total: ${total.toFixed(1)}ms`);
    if (RELEASE_NATIVE) {
        assert.ok(
            (timings["open surface (warm)"] as number) < BUDGETS_MS.preview,
            `surface read ${(timings["open surface (warm)"] as number).toFixed(1)}ms exceeds ${BUDGETS_MS.preview}ms`,
        );
        assert.ok(
            (timings["save price edit"] as number) < BUDGETS_MS.saveAck,
            `save ack ${(timings["save price edit"] as number).toFixed(1)}ms exceeds ${BUDGETS_MS.saveAck}ms`,
        );
        assert.ok(
            (timings["review (impact w/ confidence)"] as number) <
                BUDGETS_MS.preview,
            `review ${(timings["review (impact w/ confidence)"] as number).toFixed(1)}ms exceeds ${BUDGETS_MS.preview}ms`,
        );
    } else {
        t.diagnostic("debug native build: budgets measured, not gated");
    }
});
