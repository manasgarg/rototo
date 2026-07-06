// The C6 gate (design/console-implementation-plan.md): the flag-rollout
// walkthrough. A release manager takes the new_editor flag from dark ship
// to 10% to 50% to launched through the flags surface, consulting the
// lit-up graph (the batch preview that powers it) and the impact panel
// (the three-delta review) along the way. The status the surface shows at
// each step is derived by the flags experience's own logic — imported from
// the extension, fed the wire shapes this server actually produced —
// which, together with the contract-proof script, is the contract proof:
// the experience works with zero private APIs.

import assert from "node:assert/strict";
import { after, test } from "node:test";

import {
    deriveFlagStatus,
    dialOperations,
    type FlagShape,
    type RolloutView,
} from "../../console-web/src/extensions/flags/logic.ts";
import { BUDGETS_MS } from "../src/budgets.ts";
import { native } from "../src/native.ts";
import { SURFACES_SCHEMA } from "../src/surfaces.ts";
import { gitHarness, json } from "./helpers.ts";

const harness = gitHarness();
after(() => harness.cleanup());

const release = harness.signIn({
    login: "rin-release",
    token: "release-token",
    displayName: "Rin (Release)",
});
harness.github.grantRepo("release-token", "acme/config", {
    pull: true,
    push: true,
});
const RELEASE_NATIVE = native.buildProfile() === "release";

// The slice a platform team lands in code review: the flag wired to a
// rollout layer (dark: allocation drafted at 10%, default false), and the
// flags surface binding both.
const LAYER_TOML = `schema_version = 1

description = "New editor rollout, diverted by user id"
unit = "context.user.id"
buckets = 1000

[[allocation]]
id = "new_editor"
status = "draft"

[[allocation.arm]]
name = "treatment"
buckets = "0-99"
`;
const FLAG_TOML = `schema_version = 1

description = "The rebuilt editor experience"
type = "bool"

[resolve]
method = "allocation"
allocation = "new_editor"
default = false

[[resolve.assign]]
arm = "treatment"
value = true
`;
const SURFACE_TOML = `kind = "flags"
title = "Flags"
description = "Release flags and their rollouts."
audience = ["internal"]

[[bind]]
target = "variable=new_editor"

[[bind]]
target = "layer=rollout"
`;
harness.fakeGit.commitDirect("main", "add the new_editor flag", [
    { path: "packages/basic/layers/rollout.toml", content: LAYER_TOML },
    {
        path: "packages/basic/variables/new_editor.toml",
        content: FLAG_TOML,
    },
    {
        path: "packages/basic/model/catalogs/console/surfaces.schema.json",
        content: `${JSON.stringify(SURFACES_SCHEMA, null, 2)}\n`,
    },
    {
        path: "packages/basic/data/catalogs/console/surfaces/flags.toml",
        content: SURFACE_TOML,
    },
]);

// A schema-valid request context; each probe varies only the unit the
// layer hashes.
function contextFor(userId: string): Record<string, unknown> {
    return {
        user: {
            id: userId,
            tier: "free",
            role: "member",
            email_domain: "example.com",
            language: "en",
            session_count: 1,
        },
        account: { plan: "starter", seats: 3 },
        cart: { total_usd: 0 },
        device: { platform: "web" },
        request: { country: "DE" },
    };
}

test("the flag-rollout walkthrough: dark to 10% to 50% to launched", async (t) => {
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

    const treeApi = `/api/source-trees/${harness.tree.id}`;
    const packageQuery = (pin: string) =>
        `path=${encodeURIComponent(harness.packagePath)}&pin=${pin}`;

    const mainPin = async (): Promise<string> => {
        const listing = await json(
            await harness.get(`${treeApi}/packages`, release.headers),
        );
        return listing.pin;
    };

    // What the flags experience derives at a pin, from exactly the wire
    // shapes the surface route serves.
    const flagStatus = async (pin: string) => {
        const body = await json(
            await harness.get(
                `${treeApi}/surface?${packageQuery(pin)}&id=flags`,
                release.headers,
            ),
        );
        const item = body.items.find(
            (entry: any) =>
                entry.kind === "variable" && entry.id === "new_editor",
        );
        assert.ok(item, JSON.stringify(body.items));
        return {
            body,
            status: deriveFlagStatus(item as FlagShape, body.now),
        };
    };

    // The lit-up graph's data source: the whole package resolved under one
    // context. Returns new_editor's value and stable bucket for a user.
    const probe = async (pin: string, userId: string) => {
        const response = await harness.post(
            `${treeApi}/preview?${packageQuery(pin)}`,
            { context: contextFor(userId) },
            release.headers,
        );
        assert.equal(response.status, 200, await response.clone().text());
        const body = await json(response);
        const outcome = body.outcomes.find(
            (entry: any) => entry.id === "new_editor",
        );
        assert.ok(outcome?.trace, JSON.stringify(outcome));
        return {
            value: outcome.trace.resolution.value as boolean,
            bucket: outcome.trace.allocation?.bucket as number | undefined,
        };
    };

    // One rollout step: a change set carrying the operations the surface's
    // controls emit, submitted and merged through the console.
    const applyStep = async (
        title: string,
        operations: Record<string, unknown>[],
    ): Promise<string> => {
        const created = await json(
            await harness.post(
                `${treeApi}/change-sets`,
                { title },
                release.headers,
            ),
        );
        const edited = await harness.post(
            `/api/change-sets/${created.id}/edits`,
            {
                packagePath: harness.packagePath,
                expectedPin: created.baseShaAtCreation,
                operations,
                summary: title,
            },
            release.headers,
        );
        assert.equal(edited.status, 200, await edited.clone().text());
        await harness.post(
            `/api/change-sets/${created.id}/submit`,
            {},
            release.headers,
        );
        const merged = await json(
            await harness.post(
                `/api/change-sets/${created.id}/merge`,
                {},
                release.headers,
            ),
        );
        assert.equal(merged.merged, true, JSON.stringify(merged));
        return mainPin();
    };

    // --- Dark ship. The flag exists, the surface shows it, nobody has it.
    const darkPin = await mainPin();
    await flagStatus(darkPin); // cold read stages the pin
    // The interaction budget is judged on the surface list, the click-rate
    // read; the detail read (history and pending attached) is timed below.
    const list = await timed("surface list (warm)", async () => {
        const response = await harness.get(
            `${treeApi}/surfaces?${packageQuery(darkPin)}`,
            release.headers,
        );
        assert.equal(response.status, 200, await response.clone().text());
        return json(response);
    });
    assert.ok(list.surfaces.some((surface: any) => surface.id === "flags"));
    const dark = await timed("surface (warm)", () => flagStatus(darkPin));
    assert.equal(dark.status.state, "dark");
    assert.match(
        dark.status.state === "dark" ? dark.status.summary : "",
        /dark/,
    );
    // The layer item renders beside the flag (the floor's allocation list
    // is also what the experience's layer card reads).
    const layerItem = dark.body.items.find(
        (entry: any) => entry.kind === "layer" && entry.id === "rollout",
    );
    assert.deepEqual(layerItem.allocations[0].variables, ["new_editor"]);
    // The graph agrees: dark means off for everyone, enrolled or not.
    assert.equal((await probe(darkPin, "user-1")).value, false);

    // --- Start the rollout: draft -> running, exactly the operation the
    // dial's Start button emits.
    const tenPin = await timed("start rollout (save+merge)", () =>
        applyStep("Start the new_editor rollout", [
            {
                op: "set_allocation_status",
                layer: "rollout",
                id: "new_editor",
                status: "running",
            },
        ]),
    );
    const ten = await flagStatus(tenPin);
    assert.equal(ten.status.state, "partial");
    if (ten.status.state !== "partial" || ten.status.rollout === null) {
        assert.fail(JSON.stringify(ten.status));
    }
    assert.equal(ten.status.rollout.percent, 10);
    assert.match(ten.status.summary, /10% of everyone else/);

    // Consult the graph: buckets are stable, arms decide. Probe users until
    // both classes show up; every probed user obeys bucket < 100 <=> on.
    const probes = new Map<string, number>();
    let treatmentUser: string | null = null;
    let controlUser: string | null = null;
    for (
        let i = 0;
        i < 200 && (treatmentUser === null || controlUser === null);
        i++
    ) {
        const userId = `user-${i}`;
        const { value, bucket } = await probe(tenPin, userId);
        assert.ok(bucket !== undefined);
        probes.set(userId, bucket as number);
        assert.equal(
            value,
            (bucket as number) < 100,
            `${userId} bucket ${bucket} value ${value} at 10%`,
        );
        if ((bucket as number) < 100) {
            treatmentUser = treatmentUser ?? userId;
        }
        if ((bucket as number) >= 500) {
            controlUser = controlUser ?? userId;
        }
    }
    assert.ok(treatmentUser !== null && controlUser !== null);

    // --- Grow to 50% with the dial's exact operations (the extension's
    // own computation, running against the server's wire shapes).
    const dialOps = dialOperations(ten.status.rollout as RolloutView, 500);
    const fiftyChange = await json(
        await harness.post(
            `${treeApi}/change-sets`,
            { title: "Roll new_editor out to 50%" },
            release.headers,
        ),
    );
    const fiftyEdit = await timed("dial to 50% (save)", async () => {
        const response = await harness.post(
            `/api/change-sets/${fiftyChange.id}/edits`,
            {
                packagePath: harness.packagePath,
                expectedPin: fiftyChange.baseShaAtCreation,
                operations: dialOps,
                summary: "Roll new_editor out to 50%",
            },
            release.headers,
        );
        assert.equal(response.status, 200, await response.clone().text());
        return json(response);
    });
    assert.deepEqual(
        fiftyEdit.lint.diagnostics.filter(
            (diagnostic: any) => diagnostic.severity === "error",
        ),
        [],
    );
    await harness.post(
        `/api/change-sets/${fiftyChange.id}/submit`,
        {},
        release.headers,
    );

    // Consult the impact panel before merging. The semantic diff knows a
    // growing arm from a reassignment: expansion keeps every enrolled
    // unit's value, so the review says "expanded", not "reassigned".
    const review = await timed("review (warm)", async () => {
        await harness.get(
            `/api/change-sets/${fiftyChange.id}/review`,
            release.headers,
        );
        return json(
            await harness.get(
                `/api/change-sets/${fiftyChange.id}/review`,
                release.headers,
            ),
        );
    });
    const pkg = review.review.packages[0];
    assert.ok(
        pkg.changes.some(
            (change: any) => change.kind === "allocation_arms_expanded",
        ),
        JSON.stringify(pkg.changes.map((change: any) => change.kind)),
    );
    assert.ok(
        !pkg.changes.some(
            (change: any) => change.kind === "allocation_arms_reassigned",
        ),
    );
    // The impact panel states its basis, and the touched surface is named.
    assert.ok(pkg.denominator.samples >= 1);
    assert.deepEqual(pkg.lint.introduced, []);
    assert.ok(pkg.surfaces.some((surface: any) => surface.id === "flags"));

    const merged = await json(
        await harness.post(
            `/api/change-sets/${fiftyChange.id}/merge`,
            {},
            release.headers,
        ),
    );
    assert.equal(merged.merged, true);
    const fiftyPin = await mainPin();

    const fifty = await flagStatus(fiftyPin);
    assert.equal(
        fifty.status.state === "partial" ? fifty.status.rollout?.percent : 0,
        50,
    );

    // Buckets never move: everyone who had it at 10% still has it at 50%,
    // and the boundary is now 500.
    for (const [userId, bucket] of probes) {
        const { value } = await probe(fiftyPin, userId);
        assert.equal(value, bucket < 500, `${userId} bucket ${bucket} at 50%`);
    }

    // --- Launch: default true, allocation concluded. The toggle plus the
    // dial's Conclude, in one change set.
    const launchPin = await timed("launch (save+merge)", () =>
        applyStep("Launch new_editor", [
            { op: "set_default", variable: "new_editor", value: true },
            {
                op: "set_allocation_status",
                layer: "rollout",
                id: "new_editor",
                status: "concluded",
            },
        ]),
    );
    const launched = await flagStatus(launchPin);
    assert.equal(launched.status.state, "on");
    assert.match(
        launched.status.state === "on" ? launched.status.summary : "",
        /launched/,
    );
    for (const userId of [treatmentUser, controlUser]) {
        assert.equal(
            (await probe(launchPin, userId as string)).value,
            true,
            `${userId} after launch`,
        );
    }

    // Main carries the launched flag.
    assert.match(
        harness.fakeGit.readFileAt(
            launchPin,
            "packages/basic/variables/new_editor.toml",
        ),
        /default = true/,
    );

    const total = performance.now() - startedAt;
    for (const [name, value] of Object.entries(timings)) {
        t.diagnostic(`${name}: ${value.toFixed(1)}ms`);
    }
    t.diagnostic(`walkthrough total: ${total.toFixed(1)}ms`);
    if (RELEASE_NATIVE) {
        assert.ok(
            (timings["surface list (warm)"] as number) < BUDGETS_MS.interaction,
            `surface list ${(timings["surface list (warm)"] as number).toFixed(1)}ms exceeds ${BUDGETS_MS.interaction}ms`,
        );
        assert.ok(
            (timings["dial to 50% (save)"] as number) < BUDGETS_MS.saveAck,
            `save ${(timings["dial to 50% (save)"] as number).toFixed(1)}ms exceeds ${BUDGETS_MS.saveAck}ms`,
        );
        assert.ok(
            (timings["review (warm)"] as number) < BUDGETS_MS.preview,
            `review ${(timings["review (warm)"] as number).toFixed(1)}ms exceeds ${BUDGETS_MS.preview}ms`,
        );
    } else {
        t.diagnostic("debug native build: budgets measured, not gated");
    }
});
