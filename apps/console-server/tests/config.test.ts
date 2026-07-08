// Where the console puts things on disk: the pin cache follows the XDG
// base directories, and the data dir stays an explicit opt-in.

import assert from "node:assert/strict";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

import { resolveConfig } from "../src/config.ts";

test("the cache dir honors XDG_CACHE_HOME", () => {
    const config = resolveConfig({ XDG_CACHE_HOME: "/var/cache/me" });
    assert.equal(
        config.cacheDir,
        path.join("/var/cache/me", "rototo", "console"),
    );
});

test("the cache dir falls back to ~/.cache", () => {
    const config = resolveConfig({});
    assert.equal(
        config.cacheDir,
        path.join(os.homedir(), ".cache", "rototo", "console"),
    );
});

test("a blank XDG_CACHE_HOME means unset, per the spec", () => {
    const config = resolveConfig({ XDG_CACHE_HOME: "  " });
    assert.equal(
        config.cacheDir,
        path.join(os.homedir(), ".cache", "rototo", "console"),
    );
});

test("the data dir stays an explicit opt-in", () => {
    assert.equal(resolveConfig({}).dataDir, null);
    assert.equal(
        resolveConfig({}, { dataDir: "/srv/console" }).dataDir,
        "/srv/console",
    );
});
