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

test("the data dir defaults to XDG_DATA_HOME and persists", () => {
    const config = resolveConfig({ XDG_DATA_HOME: "/var/data/me" });
    assert.equal(
        config.dataDir,
        path.join("/var/data/me", "rototo", "console"),
    );
});

test("the data dir falls back to ~/.local/share", () => {
    const config = resolveConfig({});
    assert.equal(
        config.dataDir,
        path.join(os.homedir(), ".local", "share", "rototo", "console"),
    );
});

test("an explicit --data-dir wins and pulls the cache alongside", () => {
    const config = resolveConfig(
        { XDG_DATA_HOME: "/var/data/me", XDG_CACHE_HOME: "/var/cache/me" },
        { dataDir: "/srv/console" },
    );
    assert.equal(config.dataDir, "/srv/console");
    assert.equal(config.cacheDir, "/srv/console");
});
