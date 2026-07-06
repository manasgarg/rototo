// The read side (tranche C3): the context inventory, the lenient batch
// preview behind the lit-up graph, upcoming changes, package history with
// an `until` bound, the composition tree, and the in-process LSP bridge.

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

const base = `/api/source-trees/${harness.tree.id}`;
const packageQuery = (pin: string) =>
    `path=${encodeURIComponent(harness.packagePath)}&pin=${pin}`;

test("the context inventory lists saved samples and synthesized boundaries", async () => {
    const response = await harness.get(
        `${base}/contexts?${packageQuery(harness.basePin)}`,
        dev.headers,
    );
    assert.equal(response.status, 200, await response.clone().text());
    const body = await json(response);

    const sample = body.samples.find(
        (entry: any) => entry.key === "premium_enterprise",
    );
    assert.equal(sample.evaluationContext, "request");
    assert.equal(sample.context.user.tier, "premium");

    // Synthesized contexts come from the fixtures machinery: per behavior
    // case, a context that exercises it plus the expected outcome. The
    // rule boundaries the sample corpus misses are covered here.
    assert.ok(body.synthesized.length > 0);
    const laneCase = body.synthesized.find(
        (entry: any) =>
            entry.target.id === "lane_dev" && entry.expect.value === true,
    );
    assert.equal(laneCase.context.lane, "dev");
});

test("the batch preview is honest about a partial context", async () => {
    const sample = (
        await json(
            await harness.get(
                `${base}/contexts?${packageQuery(harness.basePin)}`,
                dev.headers,
            ),
        )
    ).samples[0];

    const response = await harness.post(
        `${base}/preview?${packageQuery(harness.basePin)}`,
        { context: sample.context },
        dev.headers,
    );
    assert.equal(response.status, 200, await response.clone().text());
    const body = await json(response);

    const outcome = (id: string) =>
        body.outcomes.find((entry: any) => entry.id === id);
    // What the context covers resolves and carries its full trace.
    assert.equal(outcome("premium_users").trace.resolution.value, true);
    assert.equal(outcome("premium_users").trace.rules[0].matched, true);
    // What it cannot cover fails alone, with the missing key named, and
    // the batch as a whole still answers.
    assert.match(outcome("lane_dev").error, /No such key: lane/);
    assert.equal(outcome("lane_dev").trace, undefined);

    // A synthesized context from the inventory closes that exact gap.
    const contexts = await json(
        await harness.get(
            `${base}/contexts?${packageQuery(harness.basePin)}`,
            dev.headers,
        ),
    );
    const laneCase = contexts.synthesized.find(
        (entry: any) =>
            entry.target.id === "lane_dev" && entry.expect.value === true,
    );
    const laneResponse = await json(
        await harness.post(
            `${base}/preview?${packageQuery(harness.basePin)}`,
            { context: laneCase.context },
            dev.headers,
        ),
    );
    const lane = laneResponse.outcomes.find(
        (entry: any) => entry.id === "lane_dev",
    );
    assert.equal(lane.trace.resolution.value, true);
});

test("upcoming changes surface unpassed env.now boundaries", async () => {
    // The base package schedules nothing.
    const quiet = await json(
        await harness.get(
            `${base}/upcoming?${packageQuery(harness.basePin)}`,
            dev.headers,
        ),
    );
    assert.deepEqual(quiet.changes, []);

    // Someone lands a scheduled banner: on between two future instants.
    const pin = harness.fakeGit.commitDirect("main", "add holiday banner", [
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
    ]);
    const body = await json(
        await harness.get(
            `${base}/upcoming?${packageQuery(pin)}`,
            dev.headers,
        ),
    );
    assert.equal(body.changes.length, 2);
    const [opens, closes] = body.changes;
    assert.equal(opens.variable, "holiday_banner");
    assert.equal(opens.boundary, "2099-12-20T00:00:00Z");
    assert.equal(opens.comparison, "timeBetween");
    assert.deepEqual(opens.site, { kind: "rule", index: 0 });
    assert.equal(closes.boundary, "2100-01-05T00:00:00Z");
});

test("history answers 'what was this value on March 3rd'", async () => {
    // A value that changed over time, with real dated commits.
    const file = "packages/basic/variables/payments_risk_threshold.toml";
    const at = (value: string) =>
        harness.fakeGit
            .readFileAt(harness.basePin, file)
            .replace(/^default = .*$/m, `default = ${value}`);
    harness.fakeGit.commitDirect(
        "main",
        "raise threshold for february",
        [{ path: file, content: at("0.9") }],
        [],
        "2026-02-10T09:00:00Z",
    );
    harness.fakeGit.commitDirect(
        "main",
        "lower threshold in april",
        [{ path: file, content: at("0.4") }],
        [],
        "2026-04-01T09:00:00Z",
    );

    // The full package history, newest first.
    const all = await json(
        await harness.get(
            `${base}/history?path=${encodeURIComponent(harness.packagePath)}`,
            dev.headers,
        ),
    );
    assert.ok(all.commits.length >= 3);
    assert.equal(all.commits[0].message, "lower threshold in april");

    // Bounded by the instant: the newest commit at or before March 3rd is
    // the pin to read the package at.
    const march = await json(
        await harness.get(
            `${base}/history?path=${encodeURIComponent(harness.packagePath)}&until=2026-03-03T00:00:00Z`,
            dev.headers,
        ),
    );
    assert.equal(march.commits[0].message, "raise threshold for february");
    const pin = march.commits[0].sha;
    const detail = await json(
        await harness.get(
            `${base}/package?${packageQuery(pin)}`,
            dev.headers,
        ),
    );
    const variable = detail.model.variables.find(
        (entry: any) => entry.id === "payments_risk_threshold",
    );
    assert.equal(variable.resolve.default.value, 0.9);
});

test("the composition tree infers extends edges from discovery", async () => {
    const pin = harness.fakeGit.commitDirect("main", "add tenant overlay", [
        {
            path: "packages/overlay/rototo-package.toml",
            content: 'schema_version = 1\nextends = ["../basic"]\n',
        },
    ]);
    const body = await json(
        await harness.get(`${base}/composition?ref=${pin}`, dev.headers),
    );
    assert.deepEqual(
        body.nodes.map((node: any) => node.path),
        ["packages/basic", "packages/overlay"],
    );
    assert.deepEqual(body.edges, [
        {
            from: "packages/overlay",
            source: "../basic",
            to: "packages/basic",
        },
    ]);
});

test("the LSP bridge serves overlay-aware reads on package-relative paths", async (t) => {
    const open = await harness.post(
        `${base}/lsp-sessions`,
        { path: harness.packagePath, pin: harness.basePin },
        dev.headers,
    );
    assert.equal(open.status, 201, await open.clone().text());
    const { session } = await json(open);
    // Close even on failure: a live session holds a pending native read
    // that would keep the test process alive.
    t.after(async () => {
        await harness.post(
            `/api/lsp-sessions/${session}/close`,
            {},
            dev.headers,
        );
    });

    // The premium_users file, opened with an unsaved edit that breaks the
    // type. Diagnostics must reflect the buffer, not the staged file.
    await harness.post(
        `/api/lsp-sessions/${session}/notify`,
        {
            method: "textDocument/didOpen",
            params: {
                textDocument: {
                    uri: "variables/premium_users.toml",
                    languageId: "toml",
                    version: 1,
                    text: 'schema_version = 1\ntype = "mystery"\n\n[resolve]\ndefault = false\n',
                },
            },
        },
        dev.headers,
    );

    // Diagnostics arrive from a debounced background build; poll briefly.
    let diagnostics: any = null;
    for (let attempt = 0; attempt < 50 && diagnostics === null; attempt++) {
        await new Promise((resolve) => setTimeout(resolve, 100));
        const drained = await json(
            await harness.get(
                `/api/lsp-sessions/${session}/notifications`,
                dev.headers,
            ),
        );
        diagnostics =
            drained.notifications.find(
                (entry: any) =>
                    entry.method === "textDocument/publishDiagnostics" &&
                    entry.params.uri === "variables/premium_users.toml" &&
                    entry.params.diagnostics.length > 0,
            ) ?? null;
    }
    assert.ok(diagnostics, "no diagnostics arrived for the broken overlay");
    assert.equal(
        diagnostics.params.diagnostics[0].code,
        "rototo/variable-unknown-type",
    );

    // Definition across files: the reference in checkout_redesign's rule
    // jumps to the condition variable it composes.
    const definition = await json(
        await harness.post(
            `/api/lsp-sessions/${session}/request`,
            {
                method: "textDocument/definition",
                params: {
                    textDocument: { uri: "variables/eu_premium_users.toml" },
                    position: { line: 8, character: 20 },
                },
            },
            dev.headers,
        ),
    );
    assert.ok(
        typeof definition.result.uri === "string" &&
            definition.result.uri.startsWith("variables/"),
        JSON.stringify(definition),
    );

    // Another signed-in user cannot touch this session.
    const outsider = harness.signIn({ login: "peek", token: "peek-token" });
    harness.github.grantRepo("peek-token", "acme/config", { pull: true });
    const denied = await harness.get(
        `/api/lsp-sessions/${session}/notifications`,
        outsider.headers,
    );
    assert.equal(denied.status, 404);
});
