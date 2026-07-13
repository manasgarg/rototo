// Smoke over the internal binding surface: every function in the C1
// inventory answers against a real package. The exhaustive semantics live
// in the Rust tests; this proves the boundary carries them.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
    mkdtempSync,
    mkdirSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

import { native } from "../src/native.ts";

const REPO_ROOT = path.resolve(import.meta.dirname, "../../..");
const BASIC = path.join(REPO_ROOT, "examples/basic");
const SAMPLE_CONTEXT = JSON.parse(
    readFileSync(
        path.join(
            BASIC,
            "model/context/request-samples/premium_enterprise.json",
        ),
        "utf8",
    ),
);
// The batch trace evaluates every variable's rules, and rule expressions
// need the keys they read; the shipped sample omits `lane`.
const FULL_CONTEXT = { ...SAMPLE_CONTEXT, lane: "prod" };

test("version and build profile answer", () => {
    assert.match(native.version(), /^\d+\.\d+\.\d+/);
    assert.ok(["release", "debug"].includes(native.buildProfile()));
});

test("discoverPackages finds package roots, not their children", async () => {
    const tree = mkdtempSync(path.join(tmpdir(), "rototo-discover-"));
    try {
        mkdirSync(path.join(tree, "packages/pricing/variables"), {
            recursive: true,
        });
        mkdirSync(path.join(tree, "docs"), { recursive: true });
        writeFileSync(
            path.join(tree, "packages/pricing/rototo-package.toml"),
            "schema_version = 1\n",
        );
        writeFileSync(
            path.join(tree, "packages/pricing/variables/flag.toml"),
            "schema_version = 1\n",
        );
        const found = await native.discoverPackages(tree);
        assert.deepEqual(found, ["packages/pricing"]);

        // A package at the tree root is "." and ends discovery.
        writeFileSync(
            path.join(tree, "rototo-package.toml"),
            "schema_version = 1\n",
        );
        assert.deepEqual(await native.discoverPackages(tree), ["."]);
    } finally {
        rmSync(tree, { recursive: true, force: true });
    }
});

test("semantic model, lint, and inspect report answer for examples/basic", async () => {
    const model = (await native.semanticModel(BASIC)) as {
        version: number;
        variables: unknown[];
        references: unknown[];
    };
    assert.equal(model.version, 5);
    assert.ok(model.variables.length > 0);
    assert.ok(model.references.length > 0);

    const lint = await native.lintPackage(BASIC);
    assert.ok(lint.documents.length > 0);
    const errors = lint.diagnostics.filter(
        (diagnostic) =>
            (diagnostic as { severity?: string }).severity === "error",
    );
    assert.deepEqual(errors, []);

    const report = (await native.inspectReport(BASIC, {
        variables: ["premium_users"],
        context: SAMPLE_CONTEXT,
    })) as { variables: { id: string }[] };
    assert.equal(report.variables.length, 1);
    assert.equal(report.variables[0]!.id, "premium_users");
});

test("traced resolution answers one variable and the whole package", async () => {
    const single = (await native.traceResolution(
        BASIC,
        "premium_users",
        SAMPLE_CONTEXT,
    )) as { resolution: { id: string; value: unknown } };
    assert.equal(single.resolution.id, "premium_users");
    assert.equal(single.resolution.value, true);

    const batch = (await native.traceResolutions(BASIC, FULL_CONTEXT)) as {
        resolution: { id: string };
    }[];
    assert.ok(batch.length > 10);
    assert.ok(batch.some((trace) => trace.resolution.id === "premium_users"));
});

test("applyEdit returns a plan and change records without touching disk", async () => {
    const before = readFileSync(
        path.join(BASIC, "variables/premium_users.toml"),
        "utf8",
    );
    const outcome = await native.applyEdit(BASIC, [
        { op: "set_default", variable: "premium_users", value: true },
    ]);
    assert.equal(outcome.plan.writes.length, 1);
    assert.equal(outcome.plan.writes[0]!.path, "variables/premium_users.toml");
    assert.deepEqual(outcome.records, [
        {
            operation: "set_default",
            address: "variable=premium_users#/resolve/default",
            before: false,
            after: true,
        },
    ]);
    // Pure: the working tree is untouched.
    assert.equal(
        readFileSync(path.join(BASIC, "variables/premium_users.toml"), "utf8"),
        before,
    );

    // The ownership parameter is live at the boundary.
    await assert.rejects(
        native.applyEdit(
            BASIC,
            [
                {
                    op: "set_default",
                    variable: "premium_users",
                    value: true,
                },
            ],
            { inherited: ["variable=premium_users"] },
        ),
        /inherited/,
    );
});

test("diffPackages reports the semantic change between two roots", async () => {
    const after = mkdtempSync(path.join(tmpdir(), "rototo-diff-"));
    try {
        execFileSync("cp", ["-r", `${BASIC}/.`, after]);
        const outcome = await native.applyEdit(after, [
            {
                op: "set_default",
                variable: "payments_risk_threshold",
                value: 0.9,
            },
        ]);
        writeFileSync(
            path.join(after, outcome.plan.writes[0]!.path),
            outcome.plan.writes[0]!.content,
        );
        const diff = (await native.diffPackages(BASIC, after)) as {
            changes: { kind: string }[];
        };
        assert.ok(diff.changes.length > 0);
    } finally {
        rmSync(after, { recursive: true, force: true });
    }
});

test("the pin store stages full-SHA pins and refuses refs", async () => {
    const scratch = mkdtempSync(path.join(tmpdir(), "rototo-pins-"));
    const repo = path.join(scratch, "repo");
    try {
        mkdirSync(repo, { recursive: true });
        const git = (...args: string[]) =>
            execFileSync("git", args, { cwd: repo });
        git("init", "--quiet");
        git("config", "user.email", "rototo@example.com");
        git("config", "user.name", "Rototo Test");
        git("config", "uploadpack.allowReachableSHA1InWant", "true");
        writeFileSync(
            path.join(repo, "rototo-package.toml"),
            "schema_version = 1\n",
        );
        git("add", ".");
        git("commit", "--quiet", "-m", "init");
        const pin = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repo })
            .toString()
            .trim();

        const store = new native._PinStore(path.join(scratch, "pins"));
        await assert.rejects(
            store.stage(repo, "main"),
            /not a full commit SHA/,
        );
        const tree = await store.stage(repo, pin);
        assert.deepEqual(await native.discoverPackages(tree), ["."]);
        // Same pin again is a cache read even with the remote gone.
        rmSync(repo, { recursive: true, force: true });
        assert.equal(await store.stage(repo, pin), tree);
    } finally {
        rmSync(scratch, { recursive: true, force: true });
    }
});
