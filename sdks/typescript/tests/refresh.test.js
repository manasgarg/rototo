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
