import assert from "node:assert/strict";
import { test } from "node:test";
import { dirname, resolve } from "node:path";
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

test("scoped package tokens load and stay off local sources", async () => {
    const pkg = await Package.load(EXAMPLES_BASIC, {
        packageTokens: { "https://config.acme.com/team-a": "token" },
    });
    assert.equal(pkg.servedFallback, false);
});

test("bare and scoped package tokens are mutually exclusive", async () => {
    await assert.rejects(
        () =>
            Package.load(EXAMPLES_BASIC, {
                packageToken: "bare",
                packageTokens: { "https://config.acme.com": "scoped" },
            }),
        (error) =>
            error instanceof RototoError &&
            error.message.includes("cannot both be set"),
    );
});

test("scoped package token prefixes are validated", async () => {
    await assert.rejects(
        () =>
            Package.load(EXAMPLES_BASIC, {
                packageTokens: { "http://config.acme.com": "token" },
            }),
        (error) =>
            error instanceof RototoError &&
            error.message.includes("must start with https://"),
    );
});

test("reflection surface", async () => {
    const pkg = await Package.load(resolve(ROOT, "examples/billing"));

    assert.ok(pkg.listEnums().includes("plan_tiers"));
    const planTiers = pkg.readEnum("plan_tiers");
    assert.equal(planTiers.memberType, "string");
    assert.ok(planTiers.members.includes("business"));

    assert.ok(pkg.listEntries("features").includes("sso"));
    assert.equal(pkg.readEntry("features", "sso").name, "Single sign-on");

    assert.equal(
        pkg.resolveReference("catalog=features:entry=sso#/name"),
        "Single sign-on",
    );
    assert.equal(pkg.resolveEntryRef("sso#/name", ["features"]), "Single sign-on");

    assert.throws(
        () => pkg.resolveReference("catalog=features:entry=absent"),
        /does not resolve/,
    );
});
