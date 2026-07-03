import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { test } from "node:test";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { RototoError, Package } from "../dist/index.js";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const EXAMPLES_BASIC = resolve(ROOT, "examples/basic");

test("package exposes TypeScript runtime resolution API", async () => {
    const pkg = await Package.load(EXAMPLES_BASIC);

    const variable = pkg.resolveVariable("premium_message", {
        user: { tier: "premium" },
    });
    const condition = pkg.resolveVariable("premium_users", {
        user: { tier: "premium" },
    });

    assert.equal(variable.id, "premium_message");
    assert.deepEqual(variable.source, { kind: "literal" });
    assert.equal(variable.value, "Welcome back, premium member.");
    assert.equal(condition.value, true);
});

test("resolution can be tenant-scoped", async () => {
    const root = mkdtempSync(join(tmpdir(), "rototo-tenant-"));
    try {
        writeFileSync(join(root, "rototo-package.toml"), "schema_version = 1\n");
        mkdirSync(join(root, "variables"));
        writeFileSync(
            join(root, "variables", "greeting.toml"),
            [
                "schema_version = 1",
                'type = "string"',
                "",
                "[resolve]",
                'default = "hello"',
                "",
                "[[resolve.rule]]",
                "when = 'env.tenant == \"acme\"'",
                'value = "hello acme"',
                "",
            ].join("\n"),
        );
        const pkg = await Package.load(root);

        const scoped = pkg.resolveVariable("greeting", {}, { tenant: "acme" });
        assert.equal(scoped.value, "hello acme");

        const other = pkg.resolveVariable("greeting", {}, { tenant: "globex" });
        assert.equal(other.value, "hello");

        // Without a tenant, a rule that reads env.tenant fails loudly instead
        // of comparing against null.
        assert.throws(
            () => pkg.resolveVariable("greeting", {}),
            (error) =>
                error instanceof RototoError &&
                error.message.includes("resolution is not tenant-scoped"),
        );
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});

test("inspected package can lint but not resolve", async () => {
    const pkg = await Package.inspect(EXAMPLES_BASIC);
    const lint = await pkg.lint();

    assert.deepEqual(lint.diagnostics, []);
    assert.throws(
        () => pkg.resolveVariable("premium_message", {}),
        (error) =>
            error instanceof RototoError &&
            error.message.includes(
                "package was loaded without a runtime model",
            ),
    );
});

test("context must be a JSON object", async () => {
    const pkg = await Package.load(EXAMPLES_BASIC);

    assert.throws(
        () => pkg.resolveVariable("premium_message", ["not", "an", "object"]),
        (error) =>
            error instanceof RototoError &&
            error.message.includes("evaluation context must be a JSON object"),
    );
});

test("context validation can be skipped", async () => {
    const pkg = await Package.load(EXAMPLES_BASIC);

    const result = pkg.resolveVariable(
        "premium_message",
        { user: { tier: { bad: "shape" } } },
        { validateContext: false },
    );

    assert.deepEqual(result.source, { kind: "literal" });
});

test("load rejects invalid lint mode", async () => {
    await assert.rejects(
        () => Package.load(EXAMPLES_BASIC, { lint: "warn" }),
        (error) =>
            error instanceof RototoError &&
            error.message.includes("lint must be 'deny' or 'skip'"),
    );
});
