// The C1 gate: rendered capability always matches a recomputed server
// decision. Whatever /api/me and /api/source-trees claim a principal can
// do, decide() must independently agree with — allow, backend, and reason.
// If rendering ever grows its own permission shortcut, these tests break.

import assert from "node:assert/strict";
import { test } from "node:test";

import { ACTIONS, type Resource } from "../src/decide.ts";
import { json, mutationHeaders, teamHarness } from "./helpers.ts";

async function assertRenderedMatchesDecision(
    harness: ReturnType<typeof teamHarness>,
    principalId: string,
    rendered: Record<
        string,
        { allow: boolean; backend: unknown; reason: unknown }
    >,
    resource: Resource,
): Promise<void> {
    for (const action of ACTIONS) {
        const recomputed = await harness.app.decision.decide(
            { kind: "principal", id: principalId },
            action,
            resource,
        );
        assert.deepEqual(
            rendered[action],
            recomputed,
            `rendered ${action} on ${JSON.stringify(resource)} drifted from decide()`,
        );
    }
}

test("every capability /api/me renders matches a recomputed decide()", async () => {
    const harness = teamHarness();
    const dev = harness.signIn({ login: "dev", token: "dev-token" });
    const readOnly = harness.signIn({ login: "reader", token: "reader-token" });

    const treeA = harness.store.insertSourceTree({
        kind: "github",
        owner: "acme",
        name: "config",
        defaultBranch: "main",
        createdBy: null,
    });
    const treeB = harness.store.insertSourceTree({
        kind: "github",
        owner: "acme",
        name: "infra",
        defaultBranch: "main",
        createdBy: null,
    });
    harness.github.grantRepo("dev-token", "acme/config", {
        pull: true,
        push: true,
    });
    harness.github.grantRepo("dev-token", "acme/infra", {
        pull: true,
        push: true,
        admin: true,
    });
    harness.github.grantRepo("reader-token", "acme/config", { pull: true });

    for (const user of [dev, readOnly]) {
        const response = await harness.app.fetch(
            new Request("http://console.test/api/me", {
                headers: user.headers,
            }),
        );
        assert.equal(response.status, 200);
        const body = await json(response);
        assert.equal(body.enrollment, "enrolled");

        await assertRenderedMatchesDecision(
            harness,
            user.principalId,
            body.capabilities.deployment,
            { kind: "deployment" },
        );
        for (const tree of body.capabilities.sourceTrees) {
            await assertRenderedMatchesDecision(
                harness,
                user.principalId,
                tree.capabilities,
                { kind: "source-tree", sourceTree: tree.id },
            );
        }
    }

    // The reader sees only the tree GitHub shows them.
    const readerMe = await json(
        await harness.app.fetch(
            new Request("http://console.test/api/me", {
                headers: readOnly.headers,
            }),
        ),
    );
    const readerTrees = readerMe.capabilities.sourceTrees.map(
        (tree: { id: string }) => tree.id,
    );
    assert.deepEqual(readerTrees, [treeA.id]);

    // The dev sees both.
    const devMe = await json(
        await harness.app.fetch(
            new Request("http://console.test/api/me", {
                headers: dev.headers,
            }),
        ),
    );
    const devTrees = devMe.capabilities.sourceTrees.map(
        (tree: { id: string }) => tree.id,
    );
    assert.deepEqual(devTrees.sort(), [treeA.id, treeB.id].sort());
});

test("capabilities stay honest when the underlying facts change", async () => {
    const harness = teamHarness();
    const dev = harness.signIn({ login: "dev", token: "dev-token" });
    const tree = harness.store.insertSourceTree({
        kind: "github",
        owner: "acme",
        name: "config",
        defaultBranch: "main",
        createdBy: null,
    });
    harness.github.grantRepo("dev-token", "acme/config", {
        pull: true,
        push: true,
    });

    const before = await json(
        await harness.app.fetch(
            new Request("http://console.test/api/me", {
                headers: dev.headers,
            }),
        ),
    );
    assert.equal(
        before.capabilities.sourceTrees[0].capabilities.propose.allow,
        true,
    );

    // GitHub offboards them: the fake has no TTL, so the very next render
    // must flip, and the recomputation must flip with it.
    harness.github.repos.clear();
    harness.github.grantRepo("dev-token", "acme/config", { pull: true });

    const after = await json(
        await harness.app.fetch(
            new Request("http://console.test/api/me", {
                headers: dev.headers,
            }),
        ),
    );
    const rendered = after.capabilities.sourceTrees[0].capabilities;
    assert.equal(rendered.propose.allow, false);
    await assertRenderedMatchesDecision(harness, dev.principalId, rendered, {
        kind: "source-tree",
        sourceTree: tree.id,
    });
});

test("mutations recompute decide() server-side: registration needs administer", async () => {
    const harness = teamHarness();
    const dev = harness.signIn({ login: "dev", token: "dev-token" });

    const denied = await harness.app.fetch(
        new Request("http://console.test/api/source-trees", {
            method: "POST",
            headers: mutationHeaders(dev.headers),
            body: JSON.stringify({
                kind: "github",
                owner: "acme",
                name: "config",
            }),
        }),
    );
    assert.equal(denied.status, 403);

    // A deployment-scope administer grant flips the same request to 201.
    harness.store.insertGrant({
        granteeKind: "principal",
        granteeId: dev.principalId,
        action: "administer",
        resource: "deployment",
        createdBy: null,
    });
    const allowed = await harness.app.fetch(
        new Request("http://console.test/api/source-trees", {
            method: "POST",
            headers: mutationHeaders(dev.headers),
            body: JSON.stringify({
                kind: "github",
                owner: "acme",
                name: "config",
            }),
        }),
    );
    assert.equal(allowed.status, 201);
});

test("a tree's branch updates; deregistering hides it until re-registration", async () => {
    const harness = teamHarness();
    const admin = harness.signIn({ login: "root", token: "root-token" });
    harness.store.insertGrant({
        granteeKind: "principal",
        granteeId: admin.principalId,
        action: "administer",
        resource: "deployment",
        createdBy: null,
    });
    const register = () =>
        harness.app.fetch(
            new Request("http://console.test/api/source-trees", {
                method: "POST",
                headers: mutationHeaders(admin.headers),
                body: JSON.stringify({
                    kind: "github",
                    owner: "acme",
                    name: "config",
                }),
            }),
        );
    const registered = await json(await register());
    assert.equal(registered.status, "active");

    // The default branch is the one updatable fact.
    const updated = await json(
        await harness.app.fetch(
            new Request(
                `http://console.test/api/source-trees/${registered.id}/default-branch`,
                {
                    method: "POST",
                    headers: mutationHeaders(admin.headers),
                    body: JSON.stringify({ defaultBranch: "release" }),
                },
            ),
        ),
    );
    assert.equal(updated.defaultBranch, "release");

    // Deregistering is a soft status: hidden from listings, kept on the
    // admin roster, and new change sets are refused.
    const deregistered = await json(
        await harness.app.fetch(
            new Request(
                `http://console.test/api/source-trees/${registered.id}/deregister`,
                {
                    method: "POST",
                    headers: mutationHeaders(admin.headers),
                    body: JSON.stringify({}),
                },
            ),
        ),
    );
    assert.equal(deregistered.status, "deregistered");
    assert.equal(harness.store.listSourceTrees().length, 0);
    assert.equal(harness.store.listSourceTrees(true).length, 1);
    const roster = await json(
        await harness.app.fetch(
            new Request("http://console.test/api/admin/source-trees", {
                headers: admin.headers,
            }),
        ),
    );
    assert.equal(roster.sourceTrees[0].status, "deregistered");
    const blocked = await harness.app.fetch(
        new Request(
            `http://console.test/api/source-trees/${registered.id}/change-sets`,
            {
                method: "POST",
                headers: mutationHeaders(admin.headers),
                body: JSON.stringify({ title: "Too late" }),
            },
        ),
    );
    assert.equal(blocked.status, 409);

    // Registering the same repository reactivates the same row: (kind,
    // owner, name) is the tree's identity.
    const again = await json(await register());
    assert.equal(again.id, registered.id);
    assert.equal(again.status, "active");
});

test("a disabled principal loses every capability mid-session", async () => {
    const harness = teamHarness();
    const dev = harness.signIn({ login: "dev", token: "dev-token" });
    harness.store.setPrincipalStatus(dev.principalId, "disabled");
    const response = await harness.app.fetch(
        new Request("http://console.test/api/me", { headers: dev.headers }),
    );
    const body = await json(response);
    // Disabling killed the session, so the principal is signed out.
    assert.equal(body.principal, null);
});
