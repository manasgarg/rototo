import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { RefreshingPackage, RototoError } from "../dist/index.js";

test("refreshing package resolves and shuts down", async () => {
    const root = mkdtempSync(join(tmpdir(), "rototo-typescript-refresh-"));
    try {
        writeFileSync(
            join(root, "rototo-package.toml"),
            "schema_version = 1\n",
        );
        mkdirSync(join(root, "variables"));
        writeFileSync(
            join(root, "variables", "message.toml"),
            `schema_version = 1
type = "string"

[resolve]
default = "hello"
`,
        );

        const pkg = await RefreshingPackage.load(root, {
            periodSeconds: 60,
        });
        const resolution = pkg.resolveVariable("message", {});
        const status = await pkg.status();

        assert.equal(resolution.value, "hello");
        assert.equal(status.consecutiveFailures, 0);

        await pkg.shutdown();
        assert.throws(
            () => pkg.resolveVariable("message", {}),
            (error) =>
                error instanceof RototoError &&
                error.message.includes("refreshing package has been shut down"),
        );
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});

test("refreshing package exposes identity, snapshot, and events", async () => {
    const root = mkdtempSync(join(tmpdir(), "rototo-typescript-events-"));
    const writePackage = (message) => {
        writeFileSync(
            join(root, "rototo-package.toml"),
            "schema_version = 1\n",
        );
        writeFileSync(
            join(root, "variables", "message.toml"),
            `schema_version = 1\ntype = "string"\n\n[resolve]\ndefault = "${message}"\n`,
        );
    };
    try {
        mkdirSync(join(root, "variables"));
        writePackage("hello");

        const pkg = await RefreshingPackage.load(root);

        const identity = await pkg.identity();
        assert.equal(identity.releaseId, null); // local source has no fingerprint
        assert.ok(identity.source.length > 0);

        const snapshot = await pkg.snapshot();
        assert.notEqual(snapshot.lastSuccess, null);
        assert.equal(snapshot.lastEvent.eventType, "loaded");

        const received = [];
        const pump = (async () => {
            for await (const event of pkg.refreshEvents()) {
                received.push(event);
            }
        })();

        await new Promise((resolve) => setTimeout(resolve, 20));
        writePackage("updated");
        const outcome = await pkg.refreshNow();
        assert.equal(outcome, "refreshed");

        await new Promise((resolve) => setTimeout(resolve, 50));
        await pkg.shutdown();
        await pump;

        const refreshed = received.find((e) => e.eventType === "refreshed");
        assert.ok(refreshed, "a refreshed event was delivered");
        assert.equal(refreshed.schemaVersion, 1);
        assert.equal(refreshed.outcome, "refreshed");
        assert.equal(refreshed.sdk.language, "rust");
        assert.ok(refreshed.current);
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});

test("refreshing package streams package-driven trace events", async () => {
    const root = mkdtempSync(join(tmpdir(), "rototo-typescript-trace-"));
    try {
        mkdirSync(join(root, "variables"));
        writeFileSync(
            join(root, "rototo-package.toml"),
            "schema_version = 1\n\n[[trace]]\nwhen = 'env.resolving.variable == \"message\"'\n",
        );
        writeFileSync(
            join(root, "variables", "message.toml"),
            'schema_version = 1\ntype = "string"\n\n[resolve]\ndefault = "hello"\n',
        );

        const pkg = await RefreshingPackage.load(root);
        const received = [];
        const pump = (async () => {
            for await (const item of pkg.traceEvents()) {
                received.push(item);
            }
        })();

        await new Promise((resolve) => setTimeout(resolve, 20));
        pkg.resolveVariable("message", {});

        await new Promise((resolve) => setTimeout(resolve, 50));
        await pkg.shutdown();
        await pump;

        const trace = received.find((item) => item.kind === "trace");
        assert.ok(trace, "a package-driven trace event was delivered");
        assert.equal(trace.trace.targetId, "message");
        assert.equal(trace.trace.targetKind, "variable");
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});

test("refreshing package emits a trace for a per-call trace request", async () => {
    const root = mkdtempSync(join(tmpdir(), "rototo-typescript-call-trace-"));
    try {
        // No [[trace]] policy: the trace is requested by the call itself.
        mkdirSync(join(root, "variables"));
        writeFileSync(
            join(root, "rototo-package.toml"),
            "schema_version = 1\n",
        );
        writeFileSync(
            join(root, "variables", "message.toml"),
            'schema_version = 1\ntype = "string"\n\n[resolve]\ndefault = "hello"\n',
        );

        const pkg = await RefreshingPackage.load(root);
        const received = [];
        const pump = (async () => {
            for await (const item of pkg.traceEvents()) {
                received.push(item);
            }
        })();

        await new Promise((resolve) => setTimeout(resolve, 20));
        pkg.resolveVariable("message", {}, { trace: true });

        await new Promise((resolve) => setTimeout(resolve, 50));
        await pkg.shutdown();
        await pump;

        const trace = received.find((item) => item.kind === "trace");
        assert.ok(trace, "a per-call trace event was delivered");
        assert.equal(trace.trace.targetId, "message");
        assert.equal(trace.trace.targetKind, "variable");
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});
