import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { RefreshingWorkspace, RototoError } from "../dist/index.js";

test("refreshing workspace resolves and shuts down", async () => {
    const root = mkdtempSync(join(tmpdir(), "rototo-typescript-refresh-"));
    try {
        writeFileSync(
            join(root, "rototo-workspace.toml"),
            "schema_version = 1\n",
        );
        mkdirSync(join(root, "variables"));
        writeFileSync(
            join(root, "variables", "message.toml"),
            `schema_version = 1
type = "string"

[values]
default = "hello"

[resolve]
default = "default"
`,
        );

        const workspace = await RefreshingWorkspace.load(root, {
            periodSeconds: 60,
        });
        const resolution = await workspace.resolveVariable("message", {});
        const status = await workspace.status();

        assert.equal(resolution.value, "hello");
        assert.equal(status.consecutiveFailures, 0);

        await workspace.shutdown();
        await assert.rejects(
            () => workspace.resolveVariable("message", {}),
            (error) =>
                error instanceof RototoError &&
                error.message.includes(
                    "refreshing workspace has been shut down",
                ),
        );
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});
