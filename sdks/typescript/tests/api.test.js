import assert from "node:assert/strict";
import { test } from "node:test";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { RototoError, Package } from "../dist/index.js";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const EXAMPLES_BASIC = resolve(ROOT, "examples/basic");

test("package exposes TypeScript runtime resolution API", async () => {
    const pkg = await Package.load(EXAMPLES_BASIC);

    const variable = pkg.resolveVariable("premium-message", {
        user: { tier: "premium" },
    });
    const qualifier = pkg.resolveQualifier("premium-users", {
        user: { tier: "premium" },
    });

    assert.equal(variable.id, "premium-message");
    assert.deepEqual(variable.source, { kind: "literal" });
    assert.equal(variable.value, "Welcome back, premium member.");
    assert.equal(qualifier, true);
});

test("inspected package can lint but not resolve", async () => {
    const pkg = await Package.inspect(EXAMPLES_BASIC);
    const lint = await pkg.lint();

    assert.deepEqual(lint.diagnostics, []);
    assert.throws(
        () => pkg.resolveVariable("premium-message", {}),
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
        () => pkg.resolveVariable("premium-message", ["not", "an", "object"]),
        (error) =>
            error instanceof RototoError &&
            error.message.includes("resolve context must be a JSON object"),
    );
});

test("context validation can be skipped", async () => {
    const pkg = await Package.load(EXAMPLES_BASIC);

    const result = pkg.resolveVariable(
        "premium-message",
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
