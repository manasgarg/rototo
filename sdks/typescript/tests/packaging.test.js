import assert from "node:assert/strict";
import { test } from "node:test";

import {
  RefreshingWorkspace,
  RototoError,
  VERSION,
  Workspace,
  __version__,
  version,
} from "../dist/index.js";

test("public API exports expected names", () => {
  assert.match(__version__, /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/);
  assert.equal(VERSION, __version__);
  assert.equal(version(), __version__);
  assert.equal(typeof RototoError, "function");
  assert.equal(typeof Workspace.load, "function");
  assert.equal(typeof RefreshingWorkspace.load, "function");
});
