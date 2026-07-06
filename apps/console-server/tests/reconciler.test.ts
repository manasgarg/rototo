// The reconciler: write down what we want, watch what actually happens.
// External merges, closes, and branch deletions land in our rows as
// observed facts; behind-base and conflicted are facts, never states; and
// seeing the same fact twice changes nothing.

import assert from "node:assert/strict";
import { after, test } from "node:test";

import { gitHarness, json } from "./helpers.ts";

const harness = gitHarness();
after(() => harness.cleanup());

const dev = harness.signIn({ login: "dev", token: "dev-token" });
harness.github.grantRepo("dev-token", "acme/config", {
    pull: true,
    push: true,
});
const REPO = { owner: "acme", name: "config" };

async function draftWithEdit(title: string): Promise<any> {
    const changeSet = await json(
        await harness.post(
            `/api/source-trees/${harness.tree.id}/change-sets`,
            { title },
            dev.headers,
        ),
    );
    const head = await harness.fakeGit.getRef("", REPO, changeSet.branch);
    await harness.post(
        `/api/change-sets/${changeSet.id}/edits`,
        {
            packagePath: harness.packagePath,
            expectedPin: head,
            operations: [
                { op: "set_default", variable: "premium_users", value: true },
            ],
        },
        dev.headers,
    );
    return changeSet;
}

async function submitted(title: string): Promise<any> {
    const changeSet = await draftWithEdit(title);
    const response = await json(
        await harness.post(
            `/api/change-sets/${changeSet.id}/submit`,
            {},
            dev.headers,
        ),
    );
    return response.changeSet;
}

test("an external merge is observed as merged, via the background pass", async () => {
    const changeSet = await submitted("Merged on GitHub");
    harness.fakeGit.externalMerge(changeSet.prNumber);

    // The loop the interval drives, using the author's own credential.
    await harness.app.reconciler.reconcileAll();

    const detail = await json(
        await harness.get(`/api/change-sets/${changeSet.id}`, dev.headers),
    );
    assert.equal(detail.changeSet.state, "merged");
    assert.equal(detail.changeSet.observedVia, "reconciler");
    assert.ok(
        detail.events.some((event: any) => event.event === "merged"),
        "the diary records the observed merge",
    );
    // The squash landed on main.
    const mainPin = await harness.fakeGit.getRef("", REPO, "main");
    assert.match(
        harness.fakeGit.readFileAt(
            mainPin as string,
            "packages/basic/variables/premium_users.toml",
        ),
        /default = true/,
    );
});

test("an externally closed PR is observed as abandoned", async () => {
    const changeSet = await submitted("Closed on GitHub");
    await harness.fakeGit.closePull("", REPO, changeSet.prNumber);
    const detail = await json(
        await harness.post(
            `/api/change-sets/${changeSet.id}/reconcile`,
            {},
            dev.headers,
        ),
    );
    assert.equal(detail.changeSet.state, "abandoned");
    assert.equal(detail.changeSet.observedVia, "reconciler");
});

test("an externally deleted branch abandons a draft", async () => {
    const changeSet = await draftWithEdit("Branch swept away");
    await harness.fakeGit.deleteRef("", REPO, changeSet.branch);
    const detail = await json(
        await harness.post(
            `/api/change-sets/${changeSet.id}/reconcile`,
            {},
            dev.headers,
        ),
    );
    assert.equal(detail.changeSet.state, "abandoned");
});

test("behind-base and conflicted are observed facts, not states", async () => {
    const changeSet = await submitted("Falls behind");
    // Base moves after the branch: behind, still proposed.
    harness.fakeGit.commitDirect("main", "unrelated mainline work", [
        { path: "docs/mainline.md", content: "moves on\n" },
    ]);
    let detail = await json(
        await harness.post(
            `/api/change-sets/${changeSet.id}/reconcile`,
            {},
            dev.headers,
        ),
    );
    assert.equal(detail.changeSet.state, "proposed");
    assert.equal(detail.changeSet.behindBase, true);
    assert.equal(detail.changeSet.conflicted, false);

    harness.fakeGit.markConflicted(changeSet.prNumber);
    detail = await json(
        await harness.post(
            `/api/change-sets/${changeSet.id}/reconcile`,
            {},
            dev.headers,
        ),
    );
    assert.equal(detail.changeSet.state, "proposed");
    assert.equal(detail.changeSet.conflicted, true);
});

test("reconciling the same fact twice changes nothing", async () => {
    const changeSet = await submitted("Idempotent");
    harness.fakeGit.externalMerge(changeSet.prNumber);
    await harness.post(
        `/api/change-sets/${changeSet.id}/reconcile`,
        {},
        dev.headers,
    );
    await harness.post(
        `/api/change-sets/${changeSet.id}/reconcile`,
        {},
        dev.headers,
    );
    await harness.app.reconciler.reconcileAll();
    const detail = await json(
        await harness.get(`/api/change-sets/${changeSet.id}`, dev.headers),
    );
    assert.equal(detail.changeSet.state, "merged");
    assert.equal(
        detail.events.filter((event: any) => event.event === "merged").length,
        1,
        "one observed transition, one diary entry",
    );
});
