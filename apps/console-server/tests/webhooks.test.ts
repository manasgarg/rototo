// Webhooks are nudges, never truth: the signature is verified, the payload
// is reduced to "reconcile sooner", and nothing is copied out of it.

import assert from "node:assert/strict";
import { createHmac } from "node:crypto";
import { after, test } from "node:test";

import { gitHarness, json, mutationHeaders } from "./helpers.ts";

const harness = gitHarness();
after(() => harness.cleanup());

const dev = harness.signIn({ login: "dev", token: "dev-token" });
harness.github.grantRepo("dev-token", "acme/config", {
    pull: true,
    push: true,
});

test("a verified webhook nudges the reconciler; a forged one is refused", async () => {
    // Rebuild the app with a webhook secret; the harness's default has none.
    const secret = "hook-secret";
    (harness.config as { webhookSecret: string | null }).webhookSecret = secret;

    const created = await json(
        await harness.post(
            `/api/source-trees/${harness.tree.id}/change-sets`,
            { title: "Webhook nudge fixture" },
            dev.headers,
        ),
    );
    await harness.post(
        `/api/change-sets/${created.id}/edits`,
        {
            packagePath: harness.packagePath,
            expectedPin: created.baseShaAtCreation,
            operations: [
                { op: "set_default", variable: "premium_users", value: true },
            ],
        },
        dev.headers,
    );
    const submitted = await json(
        await harness.post(
            `/api/change-sets/${created.id}/submit`,
            {},
            dev.headers,
        ),
    );

    // Merge externally; the webhook is how GitHub would tell us sooner.
    harness.fakeGit.externalMerge(submitted.pull.number);

    const payload = JSON.stringify({
        action: "closed",
        pull_request: { head: { ref: created.branch } },
    });
    const forged = await harness.app.fetch(
        new Request("http://console.test/api/webhooks/github", {
            method: "POST",
            headers: {
                "x-github-event": "pull_request",
                "x-hub-signature-256": "sha256=deadbeef",
            },
            body: payload,
        }),
    );
    assert.equal(forged.status, 401);

    const signature = `sha256=${createHmac("sha256", secret)
        .update(payload)
        .digest("hex")}`;
    // Note: no x-rototo-console header — the endpoint authenticates by
    // signature, exempt from the mutation guard.
    const verified = await harness.app.fetch(
        new Request("http://console.test/api/webhooks/github", {
            method: "POST",
            headers: {
                "x-github-event": "pull_request",
                "x-hub-signature-256": signature,
            },
            body: payload,
        }),
    );
    assert.equal(verified.status, 200);
    const outcome = await json(verified);
    assert.equal(outcome.nudged, true);

    // The nudge made the reconciler observe the external merge already.
    const observed = await json(
        await harness.get(`/api/change-sets/${created.id}`, dev.headers),
    );
    assert.equal(observed.changeSet.state, "merged");
});

test("console-initiated merge with a user token lands the change", async () => {
    const created = await json(
        await harness.post(
            `/api/source-trees/${harness.tree.id}/change-sets`,
            { title: "Console merge fixture" },
            dev.headers,
        ),
    );
    await harness.post(
        `/api/change-sets/${created.id}/edits`,
        {
            packagePath: harness.packagePath,
            expectedPin: created.baseShaAtCreation,
            operations: [
                { op: "set_default", variable: "internal_staff", value: true },
            ],
        },
        dev.headers,
    );
    await harness.post(
        `/api/change-sets/${created.id}/submit`,
        {},
        dev.headers,
    );

    // A user token merges through the console: GitHub is the enforcement
    // (the fake accepts, like an unprotected branch would), and the state
    // lands as merged with the diary naming the actor.
    const merged = await json(
        await harness.post(
            `/api/change-sets/${created.id}/merge`,
            {},
            dev.headers,
        ),
    );
    assert.equal(merged.merged, true);
    const observed = await json(
        await harness.get(`/api/change-sets/${created.id}`, dev.headers),
    );
    assert.equal(observed.changeSet.state, "merged");
    assert.ok(observed.events.some((event: any) => event.event === "merged"));
});
