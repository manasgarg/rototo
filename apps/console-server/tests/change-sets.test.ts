// Change sets end to end against the fake git-data GitHub: one edit plan is
// one commit, the compare-and-swap retries a moved head, the expected-pin
// staleness check rebases disjoint changes and rejects overlapping ones,
// and the raw-text escape hatch lints after the fact.

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

async function createChangeSet(title: string): Promise<any> {
    const response = await harness.post(
        `/api/source-trees/${harness.tree.id}/change-sets`,
        { title },
        dev.headers,
    );
    assert.equal(response.status, 201, await response.clone().text());
    return json(response);
}

test("a change set is one branch created at the base pin", async () => {
    const changeSet = await createChangeSet("First");
    assert.equal(changeSet.state, "draft");
    assert.equal(changeSet.baseRef, "main");
    assert.equal(changeSet.baseShaAtCreation, harness.basePin);
    assert.equal(
        await harness.fakeGit.getRef("", REPO, changeSet.branch),
        harness.basePin,
    );
    const detail = await json(
        await harness.get(`/api/change-sets/${changeSet.id}`, dev.headers),
    );
    assert.deepEqual(
        detail.events.map((event: any) => event.event),
        ["created"],
    );
});

test("one save with several operations lands as one commit", async () => {
    const changeSet = await createChangeSet("Two defaults");
    const before = harness.fakeGit.commitCount(changeSet.branch);
    const response = await harness.post(
        `/api/change-sets/${changeSet.id}/edits`,
        {
            packagePath: harness.packagePath,
            expectedPin: harness.basePin,
            operations: [
                { op: "set_default", variable: "premium_users", value: true },
                {
                    op: "set_default",
                    variable: "payments_risk_threshold",
                    value: 0.9,
                },
            ],
        },
        dev.headers,
    );
    assert.equal(response.status, 200, await response.clone().text());
    const result = await json(response);

    // One logical change, one commit, both files in it.
    assert.equal(harness.fakeGit.commitCount(changeSet.branch), before + 1);
    assert.deepEqual(harness.fakeGit.changedFiles(result.pin).sort(), [
        "packages/basic/variables/payments_risk_threshold.toml",
        "packages/basic/variables/premium_users.toml",
    ]);
    assert.equal(result.records.length, 2);
    assert.equal(
        result.records[0].address,
        "variable=premium_users#/resolve/default",
    );
    assert.match(
        harness.fakeGit.readFileAt(
            result.pin,
            "packages/basic/variables/premium_users.toml",
        ),
        /default = true/,
    );
    // The post-edit stage lints clean.
    const errors = result.lint.diagnostics.filter(
        (diagnostic: any) => diagnostic.severity === "error",
    );
    assert.deepEqual(errors, []);
});

test("a head that moves during the write is retried, not lost", async () => {
    const changeSet = await createChangeSet("Racy save");
    harness.fakeGit.failNextRefUpdates = 1;
    const response = await harness.post(
        `/api/change-sets/${changeSet.id}/edits`,
        {
            packagePath: harness.packagePath,
            expectedPin: harness.basePin,
            operations: [
                { op: "set_default", variable: "premium_users", value: true },
            ],
        },
        dev.headers,
    );
    assert.equal(response.status, 200, await response.clone().text());
    const result = await json(response);
    assert.equal(
        await harness.fakeGit.getRef("", REPO, changeSet.branch),
        result.pin,
    );
});

test("intervening disjoint commits rebase the plan automatically", async () => {
    const changeSet = await createChangeSet("Disjoint");
    // Someone else lands a commit the client has not seen, touching a file
    // the plan does not.
    const external = harness.fakeGit.commitDirect(
        changeSet.branch,
        "external note",
        [{ path: "docs/note.md", content: "external edit\n" }],
    );
    const response = await harness.post(
        `/api/change-sets/${changeSet.id}/edits`,
        {
            packagePath: harness.packagePath,
            // The pin the client computed against, now stale.
            expectedPin: harness.basePin,
            operations: [
                { op: "set_default", variable: "premium_users", value: true },
            ],
        },
        dev.headers,
    );
    assert.equal(response.status, 200, await response.clone().text());
    const result = await json(response);
    // Both survive: the external commit is the parent of ours.
    assert.match(
        harness.fakeGit.readFileAt(result.pin, "docs/note.md"),
        /external edit/,
    );
    assert.match(
        harness.fakeGit.readFileAt(
            result.pin,
            "packages/basic/variables/premium_users.toml",
        ),
        /default = true/,
    );
    assert.notEqual(external, result.pin);
});

test("intervening overlapping commits reject with changed-under-you", async () => {
    const changeSet = await createChangeSet("Overlap");
    harness.fakeGit.commitDirect(changeSet.branch, "external variable edit", [
        {
            path: "packages/basic/variables/premium_users.toml",
            content: "# rewritten externally\n",
        },
    ]);
    const response = await harness.post(
        `/api/change-sets/${changeSet.id}/edits`,
        {
            packagePath: harness.packagePath,
            expectedPin: harness.basePin,
            operations: [
                { op: "set_default", variable: "premium_users", value: true },
            ],
        },
        dev.headers,
    );
    assert.equal(response.status, 409);
    const body = await json(response);
    assert.match(body.error.message, /changed under you/);
    assert.deepEqual(body.error.paths, [
        "packages/basic/variables/premium_users.toml",
    ]);
});

test("the raw-text path ships whole files and lint judges them after", async () => {
    const changeSet = await createChangeSet("Raw text");
    // A file that parses but drops required fields: the engine is not in
    // the loop, so only post-edit lint can complain.
    const response = await harness.post(
        `/api/change-sets/${changeSet.id}/edits`,
        {
            packagePath: harness.packagePath,
            expectedPin: harness.basePin,
            files: [
                {
                    path: "variables/premium_users.toml",
                    content: "schema_version = 1\n",
                },
            ],
        },
        dev.headers,
    );
    assert.equal(response.status, 200, await response.clone().text());
    const result = await json(response);
    assert.deepEqual(result.records, []);
    const errors = result.lint.diagnostics.filter(
        (diagnostic: any) => diagnostic.severity === "error",
    );
    assert.ok(errors.length > 0, "raw-text lint should flag the gutted file");
});

test("only the author or a collaborator edits; only the author submits", async () => {
    const changeSet = await createChangeSet("Shared work");
    const collaborator = harness.signIn({
        login: "colleague",
        token: "colleague-token",
    });
    harness.github.grantRepo("colleague-token", "acme/config", {
        pull: true,
        push: true,
    });
    const edit = {
        packagePath: harness.packagePath,
        expectedPin: harness.basePin,
        operations: [
            { op: "set_default", variable: "premium_users", value: true },
        ],
    };

    const refused = await harness.post(
        `/api/change-sets/${changeSet.id}/edits`,
        edit,
        collaborator.headers,
    );
    assert.equal(refused.status, 403);

    const shared = await harness.post(
        `/api/change-sets/${changeSet.id}/collaborators`,
        { principalId: collaborator.principalId },
        dev.headers,
    );
    assert.equal(shared.status, 200);

    const allowed = await harness.post(
        `/api/change-sets/${changeSet.id}/edits`,
        edit,
        collaborator.headers,
    );
    assert.equal(allowed.status, 200, await allowed.clone().text());

    // Collaborators edit; the author alone submits, shares, abandons.
    const submit = await harness.post(
        `/api/change-sets/${changeSet.id}/submit`,
        {},
        collaborator.headers,
    );
    assert.equal(submit.status, 403);
});

test("submit opens the marked PR; abandon closes it and deletes the branch", async () => {
    const changeSet = await createChangeSet("Submit and abandon");
    await harness.post(
        `/api/change-sets/${changeSet.id}/edits`,
        {
            packagePath: harness.packagePath,
            expectedPin: harness.basePin,
            operations: [
                { op: "set_default", variable: "premium_users", value: true },
            ],
        },
        dev.headers,
    );
    const submitted = await json(
        await harness.post(
            `/api/change-sets/${changeSet.id}/submit`,
            { body: "Please review." },
            dev.headers,
        ),
    );
    assert.equal(submitted.changeSet.state, "proposed");
    assert.equal(submitted.changeSet.prNumber, submitted.pull.number);
    const pull = await harness.fakeGit.getPull("", REPO, submitted.pull.number);
    assert.ok(pull !== null);
    assert.match(pull.body, new RegExp(`Rototo-Change-Set: ${changeSet.id}`));
    assert.match(pull.body, /Please review\./);

    const abandoned = await json(
        await harness.post(
            `/api/change-sets/${changeSet.id}/abandon`,
            {},
            dev.headers,
        ),
    );
    assert.equal(abandoned.changeSet.state, "abandoned");
    assert.equal(
        await harness.fakeGit.getRef("", REPO, changeSet.branch),
        null,
    );
    const closed = await harness.fakeGit.getPull("", REPO, pull.number);
    assert.equal(closed?.state, "closed");
    assert.equal(closed?.merged, false);
});

test("creating a change set needs propose on the tree", async () => {
    const reader = harness.signIn({ login: "reader", token: "reader-token" });
    harness.github.grantRepo("reader-token", "acme/config", { pull: true });
    const response = await harness.post(
        `/api/source-trees/${harness.tree.id}/change-sets`,
        { title: "Read-only ambition" },
        reader.headers,
    );
    assert.equal(response.status, 403);
});
