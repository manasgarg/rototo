// The C3 gate (design/console-implementation-plan.md): the understanding
// walkthrough, performed through the HTTP surface and timed. Two questions,
// answered without touching the CLI:
//
//   1. "What does this package do for this context?" — the context picker's
//      inventory, one lenient batch preview (honest about what the context
//      cannot cover), and a synthesized boundary context that closes the gap.
//   2. "What was this value on March 3rd?" — history bounded by the instant,
//      the pin in force, and the package read as it was then.
//
// The preview budget gates here against a release build of the bindings, on
// a warm (already staged) pin.

import assert from "node:assert/strict";
import { after, test } from "node:test";

import { BUDGETS_MS } from "../src/budgets.ts";
import { native } from "../src/native.ts";
import { gitHarness, json } from "./helpers.ts";

const harness = gitHarness();
after(() => harness.cleanup());

const dev = harness.signIn({ login: "dev", token: "dev-token" });
harness.github.grantRepo("dev-token", "acme/config", { pull: true });
const RELEASE_NATIVE = native.buildProfile() === "release";

const base = `/api/source-trees/${harness.tree.id}`;
const packageQuery = (pin: string) =>
    `path=${encodeURIComponent(harness.packagePath)}&pin=${pin}`;

test("the understanding walkthrough, end to end and timed", async (t) => {
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

    // Someone scheduled a holiday banner and adjusted a threshold over the
    // months; the history this walkthrough reads is real and dated.
    const threshold = "packages/basic/variables/payments_risk_threshold.toml";
    const thresholdAt = (value: string) =>
        harness.fakeGit
            .readFileAt(harness.basePin, threshold)
            .replace(/^default = .*$/m, `default = ${value}`);
    harness.fakeGit.commitDirect(
        "main",
        "tighten risk threshold for february",
        [{ path: threshold, content: thresholdAt("0.9") }],
        [],
        "2026-02-10T09:00:00Z",
    );
    harness.fakeGit.commitDirect(
        "main",
        "relax risk threshold for spring",
        [{ path: threshold, content: thresholdAt("0.4") }],
        [],
        "2026-04-01T09:00:00Z",
    );
    harness.fakeGit.commitDirect(
        "main",
        "schedule the holiday banner",
        [
            {
                path: "packages/basic/variables/holiday_banner.toml",
                content: [
                    "schema_version = 1",
                    'description = "Holiday banner, scheduled ahead of time"',
                    'type = "bool"',
                    "",
                    "[resolve]",
                    "default = false",
                    "",
                    "[[resolve.rule]]",
                    `when = 'timeBetween(env.now, "2099-12-20T00:00:00Z", "2100-01-05T00:00:00Z")'`,
                    "value = true",
                ].join("\n"),
            },
        ],
        [],
        "2026-06-01T09:00:00Z",
    );

    // 1. Browse: packages at main, ref resolved to a pin.
    const listing = await timed("browse packages", async () => {
        const response = await harness.get(
            `${base}/packages?ref=main`,
            dev.headers,
        );
        assert.equal(response.status, 200, await response.clone().text());
        return json(response);
    });
    const pin = listing.pin as string;

    // 2. The context picker's inventory: the saved sample plus synthesized
    // boundary contexts from the package's own rules.
    const inventory = await timed("context inventory", async () => {
        return json(
            await harness.get(
                `${base}/contexts?${packageQuery(pin)}`,
                dev.headers,
            ),
        );
    });
    const sample = inventory.samples.find(
        (entry: any) => entry.key === "premium_enterprise",
    );
    assert.ok(sample, "the saved sample is in the picker");
    assert.ok(inventory.synthesized.length > 0);

    // 3. "What does this package do for this context?" One lenient batch
    // resolves everything; the answer is honest about what the sample
    // cannot cover instead of failing or implying safety.
    const preview = await timed("batch preview (sample)", async () => {
        const response = await harness.post(
            `${base}/preview?${packageQuery(pin)}`,
            { context: sample.context },
            dev.headers,
        );
        assert.equal(response.status, 200, await response.clone().text());
        return json(response);
    });
    const outcome = (id: string) =>
        preview.outcomes.find((entry: any) => entry.id === id);
    assert.equal(outcome("premium_users").trace.resolution.value, true);
    assert.equal(outcome("enterprise_accounts").trace.resolution.value, true);
    assert.match(
        outcome("lane_dev").error,
        /reads context\.lane, which the given context does not carry/,
    );
    const resolved = preview.outcomes.filter(
        (entry: any) => entry.trace !== undefined,
    );
    assert.ok(resolved.length >= 4, "the batch still answers what it can");

    // 4. The gap the sample leaves is closed by a synthesized boundary
    // context, straight from the picker's inventory.
    const laneCase = inventory.synthesized.find(
        (entry: any) =>
            entry.target.id === "lane_dev" && entry.expect.value === true,
    );
    const lanePreview = await timed("batch preview (synthetic)", async () => {
        return json(
            await harness.post(
                `${base}/preview?${packageQuery(pin)}`,
                { context: laneCase.context },
                dev.headers,
            ),
        );
    });
    const lane = lanePreview.outcomes.find(
        (entry: any) => entry.id === "lane_dev",
    );
    assert.equal(lane.trace.resolution.value, true);
    assert.equal(lane.trace.rules[0].matched, true);

    // 5. The model's reference edges are what the lit-up graph draws; the
    // preview above is its light.
    const detail = await timed("read package", async () => {
        return json(
            await harness.get(
                `${base}/package?${packageQuery(pin)}`,
                dev.headers,
            ),
        );
    });
    const variableEdges = detail.model.references.filter(
        (reference: any) =>
            reference.from.kind === "variable" &&
            reference.to.kind === "variable",
    );
    assert.ok(
        variableEdges.some(
            (reference: any) =>
                reference.from.id === "eu_premium_users" &&
                reference.to.id === "premium_users",
        ),
        "condition composition shows up as graph edges",
    );

    // 6. The time facet: the scheduled banner shows before it happens.
    const upcoming = await timed("upcoming changes", async () => {
        return json(
            await harness.get(
                `${base}/upcoming?${packageQuery(pin)}`,
                dev.headers,
            ),
        );
    });
    assert.equal(upcoming.changes.length, 2);
    assert.equal(upcoming.changes[0].variable, "holiday_banner");
    assert.equal(upcoming.changes[0].boundary, "2099-12-20T00:00:00Z");

    // 7. "What was this value on March 3rd?" History bounded by the
    // instant names the pin in force; the package reads as it was then.
    const march = await timed("history until March 3rd", async () => {
        return json(
            await harness.get(
                `${base}/history?path=${encodeURIComponent(harness.packagePath)}&until=2026-03-03T00:00:00Z`,
                dev.headers,
            ),
        );
    });
    assert.equal(
        march.commits[0].message,
        "tighten risk threshold for february",
    );
    const marchPin = march.commits[0].sha as string;
    const marchDetail = await timed("read package at March pin", async () => {
        return json(
            await harness.get(
                `${base}/package?${packageQuery(marchPin)}`,
                dev.headers,
            ),
        );
    });
    const marchThreshold = marchDetail.model.variables.find(
        (variable: any) => variable.id === "payments_risk_threshold",
    );
    assert.equal(marchThreshold.resolve.default.value, 0.9);

    // And the value actually served then. The threshold's rules read
    // context.lane, which the sample does not carry — the picker's custom
    // JSON path covers it, exactly what the honest error above asks for.
    const marchContext = { ...sample.context, lane: "live" };
    const marchPreview = await json(
        await harness.post(
            `${base}/preview?${packageQuery(marchPin)}`,
            { context: marchContext },
            dev.headers,
        ),
    );
    const served = marchPreview.outcomes.find(
        (entry: any) => entry.id === "payments_risk_threshold",
    );
    // The whole answer, not just a number: this cart was high-value, so
    // the review rule served 0.70 — while the default in force that day
    // was the February 0.9, which the trace also carries.
    assert.equal(served.trace.resolution.value, 0.7);
    const marchMatched = served.trace.rules.find((rule: any) => rule.matched);
    assert.match(marchMatched.condition, /high_value_cart/);
    assert.equal(served.trace.default_value, 0.9);

    // The preview budget, p95 over a warm pin — the number the plan gates.
    const samples: number[] = [];
    for (let i = 0; i < 12; i++) {
        const started = performance.now();
        const response = await harness.post(
            `${base}/preview?${packageQuery(pin)}`,
            { context: sample.context },
            dev.headers,
        );
        assert.equal(response.status, 200);
        await response.arrayBuffer();
        samples.push(performance.now() - started);
    }
    samples.sort((a, b) => a - b);
    const previewP95 = samples[
        Math.min(samples.length - 1, Math.ceil(samples.length * 0.95) - 1)
    ] as number;

    const total = performance.now() - startedAt;
    for (const [name, value] of Object.entries(timings)) {
        t.diagnostic(`${name}: ${value.toFixed(1)}ms`);
    }
    t.diagnostic(
        `preview: p95 ${previewP95.toFixed(1)}ms over ${samples.length} runs (budget ${BUDGETS_MS.preview}ms)`,
    );
    t.diagnostic(`walkthrough total: ${total.toFixed(1)}ms`);
    if (RELEASE_NATIVE) {
        assert.ok(
            previewP95 < BUDGETS_MS.preview,
            `preview p95 ${previewP95.toFixed(1)}ms exceeds ${BUDGETS_MS.preview}ms`,
        );
    } else {
        t.diagnostic("debug native build: preview budget measured, not gated");
    }
});
