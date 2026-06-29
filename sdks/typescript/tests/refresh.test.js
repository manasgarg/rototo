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
