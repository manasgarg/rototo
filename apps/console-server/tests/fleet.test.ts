// Ring 2 (design/console-system-view.md): fleet health is the validity
// facet across a base's overlays, and the matrix is the execution facet —
// one context resolved across every member of the composition. Both are
// composition of reads the console already does, fanned out and summarized.

import assert from "node:assert/strict";
import { after, test } from "node:test";

import { gitHarness, json } from "./helpers.ts";

const harness = gitHarness();
after(() => harness.cleanup());

const dev = harness.signIn({ login: "dev", token: "dev-token" });
harness.github.grantRepo("dev-token", "acme/config", {
    pull: true,
    push: true,
});

// Two tenant overlays extend the seeded base: one healthy, one carrying a
// variable that cannot lint.
const pin = harness.fakeGit.commitDirect("main", "add tenant overlays", [
    {
        path: "packages/tenant_a/rototo-package.toml",
        content: 'schema_version = 1\nextends = ["../basic"]\n',
    },
    {
        path: "packages/tenant_b/rototo-package.toml",
        content: 'schema_version = 1\nextends = ["../basic"]\n',
    },
    {
        path: "packages/tenant_b/variables/broken.toml",
        content:
            'schema_version = 1\ntype = "mystery"\n\n[resolve]\ndefault = false\n',
    },
]);

const base = `/api/source-trees/${harness.tree.id}`;
const packageQuery = `path=${encodeURIComponent(harness.packagePath)}&pin=${pin}`;

test("fleet health lints every overlay of the base and says which fail", async () => {
    const response = await harness.get(
        `${base}/fleet?${packageQuery}`,
        dev.headers,
    );
    assert.equal(response.status, 200, await response.clone().text());
    const body = await json(response);

    assert.deepEqual(body.overlays.map((overlay: any) => overlay.path).sort(), [
        "packages/tenant_a",
        "packages/tenant_b",
    ]);
    assert.equal(body.failing, 1);
    const healthy = body.overlays.find(
        (overlay: any) => overlay.path === "packages/tenant_a",
    );
    assert.equal(healthy.ok, true);
    assert.equal(healthy.errors, 0);
    const broken = body.overlays.find(
        (overlay: any) => overlay.path === "packages/tenant_b",
    );
    assert.equal(broken.ok, false);
    assert.ok(broken.errors >= 1);
});

test("the matrix resolves one context across the base and its overlays", async () => {
    const response = await harness.post(
        `${base}/matrix?${packageQuery}`,
        {
            context: {
                user: {
                    id: "user-1",
                    tier: "premium",
                    role: "admin",
                    email_domain: "example.com",
                    language: "en",
                    session_count: 1,
                },
                account: { plan: "enterprise", seats: 250 },
                cart: { total_usd: 300 },
                device: { platform: "web" },
                request: { country: "DE" },
            },
            variables: ["premium_users"],
        },
        dev.headers,
    );
    assert.equal(response.status, 200, await response.clone().text());
    const body = await json(response);

    assert.deepEqual(
        body.columns.map((column: any) => column.path),
        ["packages/basic", "packages/tenant_a", "packages/tenant_b"],
    );
    for (const path of ["packages/basic", "packages/tenant_a"]) {
        const column = body.columns.find((entry: any) => entry.path === path);
        const outcome = column.outcomes.find(
            (entry: any) => entry.id === "premium_users",
        );
        assert.equal(
            outcome?.value,
            true,
            `${path}: ${JSON.stringify(column)}`,
        );
    }
    // The broken overlay cannot compile; its column says so instead of
    // going silent.
    const broken = body.columns.find(
        (entry: any) => entry.path === "packages/tenant_b",
    );
    assert.ok(
        broken.failure !== undefined ||
            broken.outcomes.some((entry: any) => entry.error !== null),
        JSON.stringify(broken),
    );
});
