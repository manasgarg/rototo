// The URL grammar, round-tripped: every page the console can show has one
// spelling, and parse(format(x)) gives x back. The package tail is the
// addressing grammar (design/addressing.md), so namespaced ids, nested
// entities, and subtree selections must all survive the trip.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
    changeSetUrl,
    changesUrl,
    formatAddress,
    isCollective,
    packageUrl,
    parseAddress,
    parseHash,
    treeUrl,
    type PackageView,
    type ViewState,
} from "./router.ts";

test("top-level pages parse", () => {
    assert.deepEqual(parseHash("/").route, { page: "home" });
    assert.deepEqual(parseHash("").route, { page: "home" });
    assert.deepEqual(parseHash("/admin").route, { page: "admin" });
    assert.deepEqual(parseHash("/not-enrolled").route, {
        page: "not-enrolled",
    });
    assert.deepEqual(parseHash("/nonsense").route, { page: "home" });
});

test("tree pages parse and format", () => {
    assert.deepEqual(parseHash(treeUrl("st_7")).route, {
        page: "tree",
        treeId: "st_7",
    });
    assert.deepEqual(parseHash(changesUrl("st_7")).route, {
        page: "changes",
        treeId: "st_7",
    });
    assert.deepEqual(parseHash(changeSetUrl("st_7", "cs_42")).route, {
        page: "change-set",
        treeId: "st_7",
        changeSetId: "cs_42",
    });
    // An unknown tree-level noun lands on the tree home, not a dead page.
    assert.deepEqual(parseHash("/trees/st_7/bogus").route, {
        page: "tree",
        treeId: "st_7",
    });
});

function roundTrip(
    packagePath: string,
    view: PackageView,
    state?: ViewState,
): void {
    const url = packageUrl("st_7", packagePath, view, state);
    const parsed = parseHash(url);
    assert.deepEqual(
        parsed.route,
        { page: "package", treeId: "st_7", packagePath, view },
        `route of ${url}`,
    );
    if (state !== undefined) {
        assert.deepEqual(parsed.state, state, `state of ${url}`);
    }
}

test("package overview round-trips, root and nested", () => {
    roundTrip("examples/billing", { kind: "overview" });
    roundTrip(".", { kind: "overview" });
    assert.equal(
        packageUrl("st_7", ".", { kind: "overview" }),
        "/trees/st_7/-",
    );
    assert.equal(
        packageUrl("st_7", "examples/billing", { kind: "overview" }),
        "/trees/st_7/examples/billing/-",
    );
});

test("entity addresses round-trip", () => {
    roundTrip("examples/billing", {
        kind: "address",
        steps: [{ class: "variable", id: "active_plan" }],
    });
    roundTrip(".", {
        kind: "address",
        steps: [{ class: "variable", id: "payments/max_tokens" }],
    });
    roundTrip("examples/billing", {
        kind: "address",
        steps: [
            { class: "catalog", id: "acme/banner" },
            { class: "entry", id: "promo/summer" },
        ],
    });
    roundTrip("examples/billing", {
        kind: "address",
        steps: [
            { class: "evaluation-context", id: "request" },
            { class: "sample", id: "premium_enterprise" },
        ],
    });
    roundTrip("examples/billing", {
        kind: "address",
        steps: [{ class: "list", id: "currencies" }],
    });
    roundTrip("examples/billing", {
        kind: "address",
        steps: [{ class: "manifest", id: "" }],
    });
});

test("collectives and namespace subtrees round-trip", () => {
    roundTrip("examples/billing", {
        kind: "address",
        steps: [{ class: "variable", id: "" }],
    });
    // The trailing slash of a subtree selection survives segment splitting.
    roundTrip("examples/billing", {
        kind: "address",
        steps: [{ class: "variable", id: "payments/" }],
    });
    roundTrip("examples/billing", {
        kind: "address",
        steps: [
            { class: "catalog", id: "plans" },
            { class: "entry", id: "" },
        ],
    });
});

test("page-noun tails round-trip", () => {
    roundTrip("examples/billing", { kind: "surfaces", surfaceId: null });
    roundTrip("examples/billing", { kind: "surfaces", surfaceId: "pricing" });
    roundTrip("examples/billing", {
        kind: "files",
        file: "variables/active_plan.toml",
    });
    roundTrip("examples/billing", { kind: "history" });
    roundTrip("examples/billing", { kind: "diagnostics" });
    // There is no file-browsing screen: a bare files tail is the overview.
    assert.deepEqual(parseHash("/trees/st_7/examples/billing/-/files").route, {
        page: "package",
        treeId: "st_7",
        packagePath: "examples/billing",
        view: { kind: "overview" },
    });
});

test("view state rides the query and round-trips", () => {
    roundTrip(
        "examples/billing",
        { kind: "address", steps: [{ class: "variable", id: "active_plan" }] },
        {
            changeSetId: "cs_42",
            pin: "0123456789abcdef0123456789abcdef01234567",
            context: "sample:premium",
        },
    );
    roundTrip(
        ".",
        { kind: "overview" },
        { changeSetId: null, pin: null, context: "synthetic:active_plan · r0" },
    );
    // No state means no query string at all.
    assert.ok(!packageUrl("st_7", ".", { kind: "overview" }).includes("?"));
});

test("address parsing is lexical", () => {
    assert.deepEqual(parseAddress("variable=payments/rules"), [
        { class: "variable", id: "payments/rules" },
    ]);
    assert.deepEqual(parseAddress("catalog=a/b:entry=c/d"), [
        { class: "catalog", id: "a/b" },
        { class: "entry", id: "c/d" },
    ]);
    // A pointer suffix cannot ride in a fragment; it is dropped, not fatal.
    assert.deepEqual(parseAddress("variable=x#/resolve/default"), [
        { class: "variable", id: "x" },
    ]);
    assert.equal(parseAddress(""), null);
    assert.equal(parseAddress("no-binder"), null);
    assert.equal(parseAddress("=orphan"), null);
    assert.equal(
        formatAddress([
            { class: "catalog", id: "plans" },
            { class: "entry", id: "pro" },
        ]),
        "catalog=plans:entry=pro",
    );
});

test("collectives are what the grammar says they are", () => {
    assert.ok(isCollective({ class: "variable", id: "" }));
    assert.ok(isCollective({ class: "variable", id: "payments/" }));
    assert.ok(!isCollective({ class: "variable", id: "payments/max_tokens" }));
});

test("an unparseable tail lands on the overview", () => {
    assert.deepEqual(parseHash("/trees/st_7/-/garbage").route, {
        page: "package",
        treeId: "st_7",
        packagePath: ".",
        view: { kind: "overview" },
    });
});
