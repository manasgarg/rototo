// The latency harness (C1 gate): named budgets, measured p95 over warm
// paths, asserted in CI. Native-backed budgets only gate against a release
// build of the bindings; a debug native module measures and reports.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";

import { serve } from "@hono/node-server";

import { BUDGETS_MS } from "../src/budgets.ts";
import { native } from "../src/native.ts";
import { teamHarness } from "./helpers.ts";

const REPO_ROOT = path.resolve(import.meta.dirname, "../../..");
const BASIC = path.join(REPO_ROOT, "examples/basic");
const SAMPLE_CONTEXT = {
    ...JSON.parse(
        readFileSync(
            path.join(
                BASIC,
                "model/context/request-samples/premium_enterprise.json",
            ),
            "utf8",
        ),
    ),
    // The batch trace needs every key the rule expressions read.
    lane: "prod",
};

const RELEASE_NATIVE = native.buildProfile() === "release";

async function p95(
    label: string,
    iterations: number,
    warmup: number,
    run: () => Promise<unknown>,
): Promise<number> {
    for (let i = 0; i < warmup; i++) {
        await run();
    }
    const samples: number[] = [];
    for (let i = 0; i < iterations; i++) {
        const started = performance.now();
        await run();
        samples.push(performance.now() - started);
    }
    samples.sort((a, b) => a - b);
    const value =
        samples[
            Math.min(samples.length - 1, Math.ceil(samples.length * 0.95) - 1)
        ]!;
    console.log(
        `latency ${label}: p95 ${value.toFixed(1)}ms (min ${samples[0]!.toFixed(1)}ms, max ${samples[samples.length - 1]!.toFixed(1)}ms)`,
    );
    return value;
}

test("interaction budget: /api/me over a real socket", async () => {
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
    void tree;

    const server = serve({ fetch: harness.app.fetch, port: 0 });
    const address = server.address();
    assert.ok(address !== null && typeof address === "object");
    const base = `http://127.0.0.1:${address.port}`;
    try {
        const value = await p95("interaction /api/me", 50, 5, async () => {
            const response = await fetch(`${base}/api/me`, {
                headers: dev.headers,
            });
            assert.equal(response.status, 200);
            await response.json();
        });
        assert.ok(
            value < BUDGETS_MS.interaction,
            `interaction p95 ${value.toFixed(1)}ms exceeds ${BUDGETS_MS.interaction}ms`,
        );
    } finally {
        server.close();
    }
});

test("preview budget: batch traced resolution on a cached package", async () => {
    const value = await p95(
        "preview traceResolutions(examples/basic)",
        20,
        3,
        () => native.traceResolutions(BASIC, SAMPLE_CONTEXT),
    );
    if (!RELEASE_NATIVE) {
        console.log(
            "debug native build: preview budget measured but not gated",
        );
        return;
    }
    assert.ok(
        value < BUDGETS_MS.preview,
        `preview p95 ${value.toFixed(1)}ms exceeds ${BUDGETS_MS.preview}ms`,
    );
});

test("save-ack budget is declared and waiting for the C2 save path", () => {
    // No console save path exists in C1. The budget is declared so the
    // harness carries it; C2's walkthrough gate measures it end to end.
    assert.equal(BUDGETS_MS.saveAck, 300);
});
