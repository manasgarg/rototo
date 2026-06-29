import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { RototoError, Package } from "../dist/index.js";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const CASES = resolve(ROOT, "tests/sdk-contract/cases.jsonl");

for (const contractCase of contractCases()) {
    test(`shared contract: ${contractCase.name}`, async () => {
        if (contractCase.expect.ok) {
            const actual = await runCase(contractCase);
            assertExpectedSubset(actual, contractCase.expect);
        } else {
            await assert.rejects(
                () => runCase(contractCase),
                (error) =>
                    error instanceof RototoError &&
                    error.message.includes(contractCase.expect.error.contains),
            );
        }
    });
}

async function runCase(contractCase) {
    const operation = contractCase.operation;
    const packageSource = resolve(ROOT, contractCase.package);

    if (operation === "load_package") {
        await Package.load(packageSource);
        return { ok: true };
    }

    if (operation === "lint_package") {
        const pkg = await Package.inspect(packageSource);
        const lint = await pkg.lint();
        return { diagnostics: lint.diagnostics.length };
    }

    if (operation === "resolve_variable") {
        const pkg = await Package.load(packageSource);
        return pkg.resolveVariable(contractCase.id, contractCase.context ?? {});
    }

    if (operation === "resolve_qualifier") {
        const pkg = await Package.load(packageSource);
        return pkg.resolveQualifier(
            contractCase.id,
            contractCase.context ?? {},
        );
    }

    if (operation === "package_identity") {
        const pkg = await Package.load(packageSource);
        const identity = pkg.identity();
        return {
            releaseId: identity.releaseId,
            immutable: identity.immutable,
        };
    }

    throw new Error(`unsupported contract operation: ${operation}`);
}

function assertExpectedSubset(actual, expect) {
    if ("diagnostics" in expect) {
        assert.equal(actual.diagnostics, expect.diagnostics);
    }
    if ("result" in expect) {
        assertSubset(actual, expect.result);
    }
}

function assertSubset(actual, expected) {
    if (expected && typeof expected === "object" && !Array.isArray(expected)) {
        assert.equal(typeof actual, "object");
        for (const [key, value] of Object.entries(expected)) {
            const actualKey = key in actual ? key : snakeToCamel(key);
            assert.ok(actualKey in actual, `missing key ${key}`);
            assertSubset(actual[actualKey], value);
        }
    } else {
        assert.deepEqual(actual, expected);
    }
}

function snakeToCamel(key) {
    return key.replace(/_([a-z])/g, (_, character) => character.toUpperCase());
}

function contractCases() {
    return readFileSync(CASES, "utf8")
        .split(/\r?\n/)
        .filter((line) => line.trim())
        .map((line) => JSON.parse(line));
}
