import assert from "node:assert/strict";
import { test } from "node:test";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { RototoError, Workspace } from "../dist/index.js";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const EXAMPLES_BASIC = resolve(ROOT, "examples/basic");

test("workspace exposes TypeScript resolution objects", async () => {
  const workspace = await Workspace.load(EXAMPLES_BASIC);

  const variable = await workspace.resolveVariable("premium-message", {
    user: { tier: "premium" },
  });
  const qualifier = await workspace.resolveQualifier("premium-users", {
    user: { tier: "premium" },
  });

  assert.equal(variable.id, "premium-message");
  assert.equal(variable.valueKey, "premium");
  assert.equal(variable.value, "Welcome back, premium member.");
  assert.equal(qualifier.id, "premium-users");
  assert.equal(qualifier.value, true);
});

test("inspected workspace can lint but not resolve", async () => {
  const workspace = await Workspace.inspect(EXAMPLES_BASIC);
  const lint = await workspace.lint();

  assert.deepEqual(lint.diagnostics, []);
  await assert.rejects(
    () => workspace.resolveVariable("premium-message", {}),
    (error) =>
      error instanceof RototoError &&
      error.message.includes("workspace was loaded without a runtime model"),
  );
});

test("context must be a JSON object", async () => {
  const workspace = await Workspace.load(EXAMPLES_BASIC);

  await assert.rejects(
    () => workspace.resolveVariable("premium-message", ["not", "an", "object"]),
    (error) =>
      error instanceof RototoError &&
      error.message.includes("resolve context must be a JSON object"),
  );
});

test("context validation can be skipped", async () => {
  const workspace = await Workspace.load(EXAMPLES_BASIC);

  const result = await workspace.resolveVariable(
    "premium-message",
    { user: { tier: { bad: "shape" } } },
    { validateContext: false },
  );

  assert.equal(result.valueKey, "control");
});

test("load rejects invalid lint mode", async () => {
  await assert.rejects(
    () => Workspace.load(EXAMPLES_BASIC, { lint: "warn" }),
    (error) =>
      error instanceof RototoError &&
      error.message.includes("lint must be 'deny' or 'skip'"),
  );
});
