// The C2 gate (design/console-implementation-plan.md): the developer
// walkthrough, performed through the HTTP surface and timed. Browse a
// package, edit a variable through the form path (operations, not text),
// watch the change set become a PR, merge it on GitHub, watch the
// reconciler observe it. The save-ack budget gates here, against a release
// build of the bindings, over the same machinery a real save runs — only
// the GitHub network hop is local.

import assert from "node:assert/strict";
import { after, test } from "node:test";

import { BUDGETS_MS } from "../src/budgets.ts";
import { native } from "../src/native.ts";
import { gitHarness, json } from "./helpers.ts";

const harness = gitHarness();
after(() => harness.cleanup());

const dev = harness.signIn({ login: "dev", token: "dev-token" });
harness.github.grantRepo("dev-token", "acme/config", {
    pull: true,
    push: true,
});
const REPO = { owner: "acme", name: "config" };
const RELEASE_NATIVE = native.buildProfile() === "release";

test("the developer walkthrough, end to end and timed", async (t) => {
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

    // 1. Browse: the tree's packages at main, ref resolved to a pin.
    const listing = await timed("browse packages", async () => {
        const response = await harness.get(
            `/api/source-trees/${harness.tree.id}/packages?ref=main`,
            dev.headers,
        );
        assert.equal(response.status, 200, await response.clone().text());
        return json(response);
    });
    assert.deepEqual(
        listing.packages.map((entry: any) => entry.path),
        ["packages/basic"],
    );
    assert.equal(listing.pin, harness.basePin);

    // 2. Read the package: the semantic model names the variable and its
    // current default.
    const packageUrl = (pin: string) =>
        `/api/source-trees/${harness.tree.id}/package?path=${encodeURIComponent(
            harness.packagePath,
        )}&pin=${pin}`;
    const detail = await timed("read package", async () => {
        const response = await harness.get(
            packageUrl(listing.pin),
            dev.headers,
        );
        assert.equal(response.status, 200, await response.clone().text());
        return json(response);
    });
    const premium = detail.model.variables.find(
        (variable: any) => variable.id === "premium_users",
    );
    assert.equal(premium.resolve.default.value, false);

    // 3. Start a change set.
    const changeSet = await timed("create change set", async () => {
        const response = await harness.post(
            `/api/source-trees/${harness.tree.id}/change-sets`,
            { title: "Enable premium users" },
            dev.headers,
        );
        assert.equal(response.status, 201, await response.clone().text());
        return json(response);
    });

    // 4. The form edit: the editor submits operations and one save is one
    // commit. Measured repeatedly for the save-ack p95 (each save commits,
    // stages the new pin, and lints — the full acknowledgement).
    let pin = changeSet.baseShaAtCreation as string;
    const saveSamples: number[] = [];
    for (let i = 0; i < 11; i++) {
        const value = i % 2 === 0;
        const started = performance.now();
        const response = await harness.post(
            `/api/change-sets/${changeSet.id}/edits`,
            {
                packagePath: harness.packagePath,
                expectedPin: pin,
                operations: [
                    { op: "set_default", variable: "premium_users", value },
                ],
                summary: `Set premium_users default to ${value}`,
            },
            dev.headers,
        );
        saveSamples.push(performance.now() - started);
        assert.equal(response.status, 200, await response.clone().text());
        const result = await json(response);
        pin = result.pin;
        assert.deepEqual(
            result.lint.diagnostics.filter(
                (diagnostic: any) => diagnostic.severity === "error",
            ),
            [],
        );
    }
    timings["save (last)"] = saveSamples[saveSamples.length - 1] as number;
    // 11 saves, i ends at 10, value ends true: the walkthrough's edit.

    // 5. The workbench re-renders at the new pin: the edit is visible.
    const edited = await timed("re-read at new pin", async () => {
        return json(await harness.get(packageUrl(pin), dev.headers));
    });
    const editedPremium = edited.model.variables.find(
        (variable: any) => variable.id === "premium_users",
    );
    assert.equal(editedPremium.resolve.default.value, true);

    // 6. Submit: the change set becomes a PR carrying the marker.
    const submitted = await timed("submit", async () => {
        return json(
            await harness.post(
                `/api/change-sets/${changeSet.id}/submit`,
                { body: "Turns premium on by default." },
                dev.headers,
            ),
        );
    });
    assert.equal(submitted.changeSet.state, "proposed");
    assert.ok(submitted.pull.number > 0);

    // 7. Merge on GitHub — outside the console, like the gate demands.
    harness.fakeGit.externalMerge(submitted.pull.number);

    // 8. The reconciler observes the merge on its normal pass.
    await timed("reconcile", () => harness.app.reconciler.reconcileAll());
    const observed = await json(
        await harness.get(`/api/change-sets/${changeSet.id}`, dev.headers),
    );
    assert.equal(observed.changeSet.state, "merged");
    assert.equal(observed.changeSet.observedVia, "reconciler");
    const diary = observed.events.map((event: any) => event.event);
    assert.equal(diary[0], "created");
    assert.ok(diary.includes("committed"));
    assert.ok(diary.includes("submitted"));
    assert.equal(diary[diary.length - 1], "merged");

    // 9. Main carries the change.
    const mainPin = await harness.fakeGit.getRef("", REPO, "main");
    assert.match(
        harness.fakeGit.readFileAt(
            mainPin as string,
            "packages/basic/variables/premium_users.toml",
        ),
        /default = true/,
    );

    // The timing report, and the save-ack budget.
    saveSamples.sort((a, b) => a - b);
    const saveP95 = saveSamples[
        Math.min(
            saveSamples.length - 1,
            Math.ceil(saveSamples.length * 0.95) - 1,
        )
    ] as number;
    const total = performance.now() - startedAt;
    for (const [name, value] of Object.entries(timings)) {
        t.diagnostic(`${name}: ${value.toFixed(1)}ms`);
    }
    t.diagnostic(
        `save ack: p95 ${saveP95.toFixed(1)}ms over ${saveSamples.length} saves (budget ${BUDGETS_MS.saveAck}ms)`,
    );
    t.diagnostic(`walkthrough total: ${total.toFixed(1)}ms`);
    if (RELEASE_NATIVE) {
        assert.ok(
            saveP95 < BUDGETS_MS.saveAck,
            `save-ack p95 ${saveP95.toFixed(1)}ms exceeds ${BUDGETS_MS.saveAck}ms`,
        );
    } else {
        t.diagnostic("debug native build: save-ack budget measured, not gated");
    }
});
