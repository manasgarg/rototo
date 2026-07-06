// The fire drill (rule 1): delete the database, re-register the trees, and
// rebuild the change-set table from rototo-console/* branches plus the
// Rototo-Change-Set PR markers. What survives is what the spec promises
// survives; the diary is the acknowledged loss.

import assert from "node:assert/strict";
import { after, test } from "node:test";

import { rebuildChangeSets } from "../src/fire-drill.ts";
import { Store } from "../src/store.ts";
import { TokenCrypto } from "../src/token-crypto.ts";
import { gitHarness, json, TEST_KEY } from "./helpers.ts";

const harness = gitHarness();
after(() => harness.cleanup());

const dev = harness.signIn({ login: "dev", token: "dev-token" });
harness.github.grantRepo("dev-token", "acme/config", {
    pull: true,
    push: true,
});
harness.fakeGit.tokenLogins.set("dev-token", "dev");

async function create(title: string): Promise<any> {
    return json(
        await harness.post(
            `/api/source-trees/${harness.tree.id}/change-sets`,
            { title },
            dev.headers,
        ),
    );
}

async function edit(changeSet: any): Promise<void> {
    const head = await harness.fakeGit.getRef(
        "",
        { owner: "acme", name: "config" },
        changeSet.branch,
    );
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
}

test("the change-set table rebuilds from GitHub after total store loss", async () => {
    // Three change sets in the three branch-surviving states: a draft (no
    // PR), a proposal (open PR), and an externally closed PR whose branch
    // is still around.
    const draft = await create("Still drafting");
    await edit(draft);

    const proposed = await create("Waiting for review");
    await edit(proposed);
    const proposedSubmit = await json(
        await harness.post(
            `/api/change-sets/${proposed.id}/submit`,
            {},
            dev.headers,
        ),
    );

    const closed = await create("Rejected in review");
    await edit(closed);
    const closedSubmit = await json(
        await harness.post(
            `/api/change-sets/${closed.id}/submit`,
            {},
            dev.headers,
        ),
    );
    await harness.fakeGit.closePull(
        "",
        { owner: "acme", name: "config" },
        closedSubmit.pull.number,
    );
    void proposedSubmit;

    // The drill: the store is gone. An admin re-registers the tree, the
    // author re-enrolls, and the rebuild walks GitHub.
    const fresh = new Store(null);
    const tree = fresh.insertSourceTree({
        kind: "github",
        owner: "acme",
        name: "config",
        defaultBranch: "main",
        createdBy: null,
    });
    const principal = fresh.createPrincipal("dev");
    fresh.attachIdentity(
        principal.id,
        {
            provider: "github",
            subject: "re-enrolled",
            login: "dev",
            email: null,
            emailVerified: false,
            name: "dev",
            avatarUrl: null,
        },
        TokenCrypto.fromEnvValue(TEST_KEY).encrypt("dev-token"),
    );

    const rebuilt = await rebuildChangeSets({
        store: fresh,
        git: harness.fakeGit,
        tree,
        token: "dev-token",
    });
    assert.equal(rebuilt.length, 3);

    const byId = new Map(rebuilt.map((row) => [row.id, row]));
    assert.equal(byId.get(draft.id)?.state, "draft");
    assert.equal(byId.get(proposed.id)?.state, "proposed");
    assert.equal(byId.get(closed.id)?.state, "abandoned");

    // Titles come back from the PRs where PRs exist; a draft keeps its id.
    assert.equal(byId.get(proposed.id)?.title, "Waiting for review");
    assert.equal(byId.get(closed.id)?.title, "Rejected in review");
    assert.equal(byId.get(draft.id)?.title, draft.id);

    // Authorship best-effort: the PR author's login maps to the
    // re-enrolled principal; the PR-less draft honestly does not know.
    assert.equal(byId.get(proposed.id)?.authorPrincipal, principal.id);
    assert.equal(byId.get(draft.id)?.authorPrincipal, "unknown");

    // Running the drill again is a no-op.
    const again = await rebuildChangeSets({
        store: fresh,
        git: harness.fakeGit,
        tree,
        token: "dev-token",
    });
    assert.deepEqual(again, []);
    fresh.close();
});
