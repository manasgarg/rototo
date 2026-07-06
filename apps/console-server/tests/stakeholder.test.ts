// The C4 gate (design/console-implementation-plan.md): the
// stakeholder-with-GitHub walkthrough, performed through the HTTP surface
// and timed. A PM edits a price through the floor surface — never touching
// rototo vocabulary beyond what the surface shows — and an approver reads
// all three deltas (what changed, what it does with its denominator,
// whether it is healthy) before merging on GitHub. This is Priya's journey
// minus "no GitHub account"; role-based enforcement of the surface's
// approval field waits for C5, so GitHub stays the authority throughout.

import assert from "node:assert/strict";
import { after, test } from "node:test";

import { BUDGETS_MS } from "../src/budgets.ts";
import { native } from "../src/native.ts";
import { SURFACES_SCHEMA } from "../src/surfaces.ts";
import { gitHarness, json } from "./helpers.ts";

const harness = gitHarness();
after(() => harness.cleanup());

const pm = harness.signIn({
    login: "priya-pm",
    token: "pm-token",
    displayName: "Priya (PM)",
});
const approver = harness.signIn({
    login: "alex-approver",
    token: "approver-token",
    displayName: "Alex (Approver)",
});
harness.github.grantRepo("pm-token", "acme/config", {
    pull: true,
    push: true,
});
harness.github.grantRepo("approver-token", "acme/config", {
    pull: true,
    push: true,
});
const RELEASE_NATIVE = native.buildProfile() === "release";

// The pricing slice a platform team would have landed: a plans catalog, a
// variable selecting from it, and the surface that shows it to the PM.
const PLANS_SCHEMA = `{
  "description": "Subscription plans",
  "type": "object",
  "required": ["title", "monthly_price_usd"],
  "properties": {
    "title": { "type": "string" },
    "monthly_price_usd": { "type": "number", "minimum": 0 }
  },
  "additionalProperties": false
}
`;
const SURFACE_TOML = `kind = "table"
title = "Pricing"
description = "Plans and prices."
audience = ["internal"]
approval = "role:pricing_admins"
caution = "Price changes reach checkout on the next package refresh."

[[bind]]
target = "catalog=plans"
editable_fields = ["monthly_price_usd"]
`;
const surfacePin = harness.fakeGit.commitDirect("main", "add pricing", [
    {
        path: "packages/basic/model/catalogs/plans.schema.json",
        content: PLANS_SCHEMA,
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
        content: SURFACE_TOML,
    },
]);

test("the stakeholder walkthrough, end to end and timed", async (t) => {
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
    const surfacesUrl = (pin: string) =>
        `/api/source-trees/${harness.tree.id}/surfaces?path=${encodeURIComponent(
            harness.packagePath,
        )}&pin=${pin}`;
    const surfaceUrl = (pin: string) =>
        `/api/source-trees/${harness.tree.id}/surface?path=${encodeURIComponent(
            harness.packagePath,
        )}&pin=${pin}&id=pricing`;

    // 1. The PM opens the domain lens and finds Pricing. (A first read
    // stages the pin; the interaction budget is judged on the warm read,
    // which is what every click after the first one costs.)
    await harness.get(surfacesUrl(surfacePin), pm.headers);
    const surfaces = await timed("surface list (warm)", async () => {
        const response = await harness.get(surfacesUrl(surfacePin), pm.headers);
        assert.equal(response.status, 200, await response.clone().text());
        return json(response);
    });
    const pricing = surfaces.surfaces.find((s: any) => s.id === "pricing");
    assert.equal(pricing.title, "Pricing");
    assert.ok(
        !pricing.diagnostics.some((d: any) => d.severity === "error"),
        JSON.stringify(pricing.diagnostics),
    );

    // 2. The floor renders the surface: plans as a table, the price as a
    // number control, nothing else editable. The caution shows near edits.
    const surface = await timed("open surface", async () => {
        return json(await harness.get(surfaceUrl(surfacePin), pm.headers));
    });
    assert.equal(surface.surface.caution?.includes("checkout"), true);
    const table = surface.items.find((item: any) => item.kind === "catalog");
    assert.equal(table.id, "plans");
    assert.deepEqual(
        table.entries.map((entry: any) => entry.key),
        ["pro", "starter"],
    );
    assert.deepEqual(table.fields, [
        { field: "monthly_price_usd", control: "number" },
    ]);

    // 3. The PM raises the Pro price through the control's one operation.
    const changeSet = await timed("create change set", async () => {
        const response = await harness.post(
            `/api/source-trees/${harness.tree.id}/change-sets`,
            { title: "Raise Pro to $59" },
            pm.headers,
        );
        assert.equal(response.status, 201, await response.clone().text());
        return json(response);
    });
    const edited = await timed("save price edit", async () => {
        const response = await harness.post(
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
            pm.headers,
        );
        assert.equal(response.status, 200, await response.clone().text());
        return json(response);
    });
    assert.deepEqual(
        edited.lint.diagnostics.filter(
            (diagnostic: any) => diagnostic.severity === "error",
        ),
        [],
    );
    assert.equal(edited.records[0].operation, "set_field");

    // 4. The PM submits; the surface shows the pending change set.
    const submitted = await timed("submit", async () => {
        return json(
            await harness.post(
                `/api/change-sets/${changeSet.id}/submit`,
                { body: "Pro goes to $59 next cycle." },
                pm.headers,
            ),
        );
    });
    assert.equal(submitted.changeSet.state, "proposed");
    const surfaceAfter = await json(
        await harness.get(surfaceUrl(surfacePin), pm.headers),
    );
    assert.ok(
        surfaceAfter.pending.some((row: any) => row.id === changeSet.id),
        JSON.stringify(surfaceAfter.pending),
    );

    // 5. The approver reads the three deltas. Timed over warm repeats: the
    // review is a preview-class read and gates against that budget.
    const reviewOnce = async () => {
        const response = await harness.get(
            `/api/change-sets/${changeSet.id}/review`,
            approver.headers,
        );
        assert.equal(response.status, 200, await response.clone().text());
        return json(response);
    };
    const review = await timed("review (first)", reviewOnce);
    const reviewSamples: number[] = [];
    for (let i = 0; i < 8; i++) {
        const started = performance.now();
        await reviewOnce();
        reviewSamples.push(performance.now() - started);
    }

    assert.equal(review.review.basePin, changeSet.baseShaAtCreation);
    const pkg = review.review.packages[0];
    assert.equal(pkg.path, harness.packagePath);

    // What changed: the price, semantically addressed, 49 → 59.
    const priceChange = pkg.changes.find(
        (change: any) => change.kind === "catalog_entry_changed",
    );
    assert.ok(priceChange, JSON.stringify(pkg.changes));
    assert.equal(priceChange.before, 49);
    assert.equal(priceChange.after, 59);

    // What it does: active_plan serves $59 wherever it resolves, and the
    // panel states its denominator instead of implying safety.
    assert.equal(pkg.impactError, null);
    const sampleImpact = pkg.contextImpacts.find((impact: any) =>
        impact.context.startsWith("sample:"),
    );
    assert.ok(sampleImpact);
    const served = sampleImpact.impacts.find(
        (outcome: any) => outcome.variable === "active_plan",
    );
    assert.equal(served.before.value.monthly_price_usd, 49);
    assert.equal(served.after.value.monthly_price_usd, 59);
    assert.ok(pkg.denominator.samples >= 1);
    const coverage = pkg.denominator.variables.find(
        (variable: any) => variable.id === "active_plan",
    );
    assert.ok(coverage, JSON.stringify(pkg.denominator.variables));
    assert.ok(coverage.sampleCount >= 1);
    // The premium sample exercises the premium rule; nothing exercises the
    // default — and the panel says so instead of implying safety.
    assert.deepEqual(coverage.rules, [{ index: 0, covered: true }]);
    assert.equal(coverage.defaultCovered, false);

    // Whether it is healthy: nothing introduced, nothing resolved.
    assert.deepEqual(pkg.lint.introduced, []);
    assert.deepEqual(pkg.lint.resolved, []);

    // The touched surface names its approval requirement to the approver.
    const touched = pkg.surfaces.find((s: any) => s.id === "pricing");
    assert.equal(touched.approval, "role:pricing_admins");

    // 6. Informed, the approver merges on GitHub (Backend A: GitHub is the
    // authority in this tranche) and the reconciler observes it.
    harness.fakeGit.externalMerge(submitted.pull.number);
    await timed("reconcile", () => harness.app.reconciler.reconcileAll());
    const observed = await json(
        await harness.get(`/api/change-sets/${changeSet.id}`, approver.headers),
    );
    assert.equal(observed.changeSet.state, "merged");

    // 7. Main carries the new price.
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

    // The timing report and the budgets.
    reviewSamples.sort((a, b) => a - b);
    const reviewP95 = reviewSamples[
        Math.min(
            reviewSamples.length - 1,
            Math.ceil(reviewSamples.length * 0.95) - 1,
        )
    ] as number;
    const total = performance.now() - startedAt;
    for (const [name, value] of Object.entries(timings)) {
        t.diagnostic(`${name}: ${value.toFixed(1)}ms`);
    }
    t.diagnostic(
        `review: p95 ${reviewP95.toFixed(1)}ms over ${reviewSamples.length} warm runs (budget ${BUDGETS_MS.preview}ms)`,
    );
    t.diagnostic(`walkthrough total: ${total.toFixed(1)}ms`);
    if (RELEASE_NATIVE) {
        assert.ok(
            (timings["surface list (warm)"] as number) <
                BUDGETS_MS.interaction,
            `surface list ${(timings["surface list (warm)"] as number).toFixed(1)}ms exceeds ${BUDGETS_MS.interaction}ms`,
        );
        assert.ok(
            reviewP95 < BUDGETS_MS.preview,
            `review p95 ${reviewP95.toFixed(1)}ms exceeds ${BUDGETS_MS.preview}ms`,
        );
    } else {
        t.diagnostic("debug native build: budgets measured, not gated");
    }
});
