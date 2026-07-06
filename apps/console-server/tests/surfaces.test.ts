// Surfaces (design/console-surfaces.md): catalog entries in console/surfaces
// render at floor fidelity with validation on load, the read side attached,
// and cold-start suggestions that draft real change sets. The three-delta
// review (design/console-system-view.md) gets its own coverage here too:
// semantic diff, resolution impact with an honest denominator, lint delta.

import assert from "node:assert/strict";
import { after, test } from "node:test";

import { SURFACES_SCHEMA } from "../src/surfaces.ts";
import { gitHarness, json } from "./helpers.ts";

const harness = gitHarness();
after(() => harness.cleanup());

const dev = harness.signIn({ login: "dev", token: "dev-token" });
harness.github.grantRepo("dev-token", "acme/config", {
    pull: true,
    push: true,
});

const SURFACE_TOML = `kind = "table"
title = "Tenant limits"
description = "Account limit profiles per plan."
audience = ["internal"]
approval = "role:plan_admins"

[[bind]]
target = "variable=tenant_limits"

[[bind]]
target = "variable=premium_users"

[[bind]]
target = "catalog=tenant_limits"
editable_fields = ["limits", "metadata"]
can_add = true
`;

const BROKEN_SURFACE_TOML = `title = "Broken"

[[bind]]
target = "variable=does_not_exist"
`;

// Seed the vendored schema plus two surfaces onto main, the way a
// surface-creating change set would have landed them.
const surfacePin = harness.fakeGit.commitDirect("main", "add surfaces", [
    {
        path: "packages/basic/model/catalogs/console/surfaces.schema.json",
        content: `${JSON.stringify(SURFACES_SCHEMA, null, 2)}\n`,
    },
    {
        path: "packages/basic/data/catalogs/console/surfaces/tenant_limits.toml",
        content: SURFACE_TOML,
    },
    {
        path: "packages/basic/data/catalogs/console/surfaces/broken.toml",
        content: BROKEN_SURFACE_TOML,
    },
]);

const packageQuery = (pin: string) =>
    `path=${encodeURIComponent(harness.packagePath)}&pin=${pin}`;

test("the surface list validates on load and stays honest about problems", async () => {
    const response = await harness.get(
        `/api/source-trees/${harness.tree.id}/surfaces?${packageQuery(surfacePin)}`,
        dev.headers,
    );
    assert.equal(response.status, 200, await response.clone().text());
    const body = await json(response);

    assert.equal(body.surfaces.length, 2);
    const limits = body.surfaces.find((s: any) => s.id === "tenant_limits");
    assert.equal(limits.title, "Tenant limits");
    assert.equal(limits.kind, "table");
    assert.equal(limits.approval, "role:plan_admins");
    // No experience renders "table" until C6; the surface says so and
    // renders on the floor. Degradation is information, not an error.
    assert.ok(
        limits.diagnostics.some(
            (d: any) => d.severity === "info" && /floor/.test(d.message),
        ),
    );
    assert.ok(
        !limits.diagnostics.some((d: any) => d.severity === "error"),
        JSON.stringify(limits.diagnostics),
    );

    const broken = body.surfaces.find((s: any) => s.id === "broken");
    assert.ok(
        broken.diagnostics.some(
            (d: any) =>
                d.severity === "error" && /does_not_exist/.test(d.message),
        ),
    );

    // The vendored schema matches what this console ships: no freshness
    // noise, and no suggestions while surfaces exist.
    assert.deepEqual(body.diagnostics, []);
    assert.deepEqual(body.suggestions, []);
});

test("one surface renders at floor fidelity with its read side", async () => {
    const response = await harness.get(
        `/api/source-trees/${harness.tree.id}/surface?${packageQuery(surfacePin)}&id=tenant_limits`,
        dev.headers,
    );
    assert.equal(response.status, 200, await response.clone().text());
    const body = await json(response);

    // The floor: a catalog-typed variable is a select over entry ids, a
    // bool variable is a toggle, a bound catalog is a table with
    // schema-driven field controls bounded by editable_fields.
    const [variable, flag, catalog] = body.items;
    assert.equal(variable.kind, "variable");
    assert.equal(variable.control.control, "select");
    assert.deepEqual(
        [...variable.control.options].sort(),
        ["enterprise", "growth", "starter"],
    );
    assert.equal(flag.kind, "variable");
    assert.equal(flag.id, "premium_users");
    assert.equal(flag.control.control, "toggle");
    assert.equal(catalog.kind, "catalog");
    assert.equal(catalog.entries.length, 3);
    assert.equal(catalog.canAdd, true);
    assert.deepEqual(
        catalog.fields.map((field: any) => field.field),
        ["limits", "metadata"],
    );

    // The read side: bound-file history reaches back to the seed commit;
    // nothing is pending yet.
    assert.ok(body.history.length >= 1);
    assert.ok(
        body.history.some((commit: any) => /seed/.test(commit.message)),
    );
    assert.deepEqual(body.pending, []);
    assert.deepEqual(body.upcoming, []);
});

test("a change set touching bound entities shows up as pending on the surface", async () => {
    const created = await json(
        await harness.post(
            `/api/source-trees/${harness.tree.id}/change-sets`,
            { title: "Raise growth limits" },
            dev.headers,
        ),
    );
    const edited = await json(
        await harness.post(
            `/api/change-sets/${created.id}/edits`,
            {
                packagePath: harness.packagePath,
                expectedPin: created.baseShaAtCreation,
                operations: [
                    {
                        op: "set_field",
                        target: "catalog=tenant_limits:entry=growth#/limits/projects",
                        value: 50,
                    },
                ],
            },
            dev.headers,
        ),
    );
    assert.ok(edited.pin);

    const surface = await json(
        await harness.get(
            `/api/source-trees/${harness.tree.id}/surface?${packageQuery(surfacePin)}&id=tenant_limits`,
            dev.headers,
        ),
    );
    assert.ok(
        surface.pending.some((row: any) => row.id === created.id),
        JSON.stringify(surface.pending),
    );

    // The three-delta review of that change set.
    const review = await json(
        await harness.get(`/api/change-sets/${created.id}/review`, dev.headers),
    );
    assert.equal(review.review.packages.length, 1);
    const pkg = review.review.packages[0];
    assert.equal(pkg.path, harness.packagePath);

    // Structure delta: the entry field change, addressed semantically.
    assert.ok(
        pkg.changes.some((change: any) => change.kind === "catalog_entry_changed"),
        JSON.stringify(pkg.changes.map((c: any) => c.kind)),
    );

    // Execution delta with its denominator. The saved sample is an
    // enterprise account, so the growth edit changes nothing for it — and
    // that is precisely why the synthesized contexts exist: one of them
    // resolves growth and catches the change.
    const sampleImpact = pkg.contextImpacts.find((impact: any) =>
        impact.context.startsWith("sample:"),
    );
    assert.ok(sampleImpact);
    assert.ok(
        !sampleImpact.impacts.some(
            (impact: any) => impact.variable === "tenant_limits",
        ),
        "the enterprise sample does not exercise the growth entry",
    );
    const syntheticHit = pkg.contextImpacts.find(
        (impact: any) =>
            impact.context.startsWith("synthetic:tenant_limits/") &&
            impact.impacts.some(
                (outcome: any) =>
                    outcome.variable === "tenant_limits" &&
                    outcome.after?.value?.limits?.projects === 50,
            ),
    );
    assert.ok(
        syntheticHit,
        `no synthetic context caught the growth change: ${JSON.stringify(
            pkg.contextImpacts.map((impact: any) => impact.context),
        )}`,
    );
    assert.ok(pkg.denominator.samples >= 1);
    assert.ok(pkg.denominator.synthesized >= 1);
    const coverage = pkg.denominator.variables.find(
        (variable: any) => variable.id === "tenant_limits",
    );
    assert.ok(coverage);
    assert.equal(coverage.defaultCovered, false);

    // Validity delta: this edit neither introduces nor resolves anything.
    assert.deepEqual(pkg.lint.introduced, []);
    assert.deepEqual(pkg.lint.resolved, []);

    // The touched surface, with the approval requirement it declares.
    // Rendered and informative; GitHub stays the authority in this tranche.
    const touched = pkg.surfaces.find((s: any) => s.id === "tenant_limits");
    assert.equal(touched.approval, "role:plan_admins");

    // Clean up: abandon so later tests see no pending change sets.
    await harness.post(
        `/api/change-sets/${created.id}/abandon`,
        {},
        dev.headers,
    );
});

test("cold start proposes surfaces and the first one vendors the schema", async () => {
    const bare = gitHarness();
    try {
        const user = bare.signIn({ login: "pm", token: "pm-token" });
        bare.github.grantRepo("pm-token", "acme/config", {
            pull: true,
            push: true,
        });
        const listing = await json(
            await bare.get(
                `/api/source-trees/${bare.tree.id}/surfaces?path=${encodeURIComponent(
                    bare.packagePath,
                )}&pin=${bare.basePin}`,
                user.headers,
            ),
        );
        assert.deepEqual(listing.surfaces, []);
        const suggestion = listing.suggestions.find(
            (entry: any) => entry.id === "tenant_limits",
        );
        assert.ok(suggestion, JSON.stringify(listing.suggestions));
        assert.equal(suggestion.kind, "table");
        // Bool variables suggest a flags surface too.
        assert.ok(
            listing.suggestions.some((entry: any) => entry.kind === "flags"),
        );
        // The suggestion's first operation carries the schema in; nobody
        // copies files.
        assert.equal(suggestion.operations[0].op, "create_catalog");
        assert.equal(suggestion.operations[0].id, "console/surfaces");

        // Accepting the suggestion is an ordinary change set.
        const created = await json(
            await bare.post(
                `/api/source-trees/${bare.tree.id}/change-sets`,
                { title: "Add the tenant limits surface" },
                user.headers,
            ),
        );
        const edited = await json(
            await bare.post(
                `/api/change-sets/${created.id}/edits`,
                {
                    packagePath: bare.packagePath,
                    expectedPin: created.baseShaAtCreation,
                    operations: suggestion.operations,
                },
                user.headers,
            ),
        );
        assert.deepEqual(
            edited.lint.diagnostics.filter(
                (diagnostic: any) => diagnostic.severity === "error",
            ),
            [],
        );

        const after = await json(
            await bare.get(
                `/api/source-trees/${bare.tree.id}/surfaces?path=${encodeURIComponent(
                    bare.packagePath,
                )}&pin=${edited.pin}`,
                user.headers,
            ),
        );
        assert.equal(after.surfaces.length, 1);
        assert.equal(after.surfaces[0].id, "tenant_limits");
        assert.deepEqual(after.suggestions, []);
        // The vendored schema is exactly the one this console ships.
        assert.deepEqual(after.diagnostics, []);
    } finally {
        bare.cleanup();
    }
});
