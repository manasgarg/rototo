// The decision point in isolation: default deny, the disabled rule, the
// action ladder, grant inheritance down the resource lineage, and the
// Backend A permission mapping.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
    DecisionPoint,
    resourceLineage,
    resourceString,
} from "../src/decide.ts";
import { Store } from "../src/store.ts";
import { TokenCrypto } from "../src/token-crypto.ts";
import { FakeGitHub, TEST_KEY } from "./helpers.ts";

function decisionPoint(store: Store, github: FakeGitHub): DecisionPoint {
    return new DecisionPoint({
        authMode: "team",
        store,
        github,
        tokenCrypto: () => TokenCrypto.fromEnvValue(TEST_KEY),
    });
}

test("local mode short-circuits to allow", async () => {
    const store = new Store(null);
    const decide = new DecisionPoint({
        authMode: "local",
        store,
        github: new FakeGitHub(),
        tokenCrypto: () => TokenCrypto.fromEnvValue(TEST_KEY),
    });
    const decision = await decide.decide({ kind: "local" }, "administer", {
        kind: "deployment",
    });
    assert.equal(decision.allow, true);
    assert.equal(decision.backend, "local");
});

test("default deny: no grant, no GitHub identity", async () => {
    const store = new Store(null);
    const principal = store.createPrincipal("Priya");
    const decide = decisionPoint(store, new FakeGitHub());
    const decision = await decide.decide(
        { kind: "principal", id: principal.id },
        "view",
        { kind: "deployment" },
    );
    assert.equal(decision.allow, false);
    assert.equal(decision.backend, null);
    assert.match(decision.reason, /default deny/);
});

test("a disabled principal always gets deny, grants notwithstanding", async () => {
    const store = new Store(null);
    const principal = store.createPrincipal("Mallory");
    store.insertGrant({
        granteeKind: "principal",
        granteeId: principal.id,
        action: "administer",
        resource: "deployment",
        createdBy: null,
    });
    store.setPrincipalStatus(principal.id, "disabled");
    const decide = decisionPoint(store, new FakeGitHub());
    const decision = await decide.decide(
        { kind: "principal", id: principal.id },
        "view",
        { kind: "deployment" },
    );
    assert.equal(decision.allow, false);
    assert.equal(decision.reason, "principal is disabled");
});

test("grants inherit down the resource lineage and imply lower verbs", async () => {
    const store = new Store(null);
    const principal = store.createPrincipal("Ada");
    const tree = store.insertSourceTree({
        kind: "github",
        owner: "acme",
        name: "config",
        defaultBranch: "main",
        createdBy: null,
    });
    store.insertGrant({
        granteeKind: "principal",
        granteeId: principal.id,
        action: "approve",
        resource: resourceString({ kind: "source-tree", sourceTree: tree.id }),
        createdBy: null,
    });
    const decide = decisionPoint(store, new FakeGitHub());
    const subject = { kind: "principal" as const, id: principal.id };
    const pkg = {
        kind: "package" as const,
        sourceTree: tree.id,
        path: "packages/pricing",
    };

    // approve on the tree implies view and propose on a package inside it.
    for (const action of ["view", "propose", "approve"] as const) {
        const decision = await decide.decide(subject, action, pkg);
        assert.equal(decision.allow, true, `${action} should inherit`);
        assert.equal(decision.backend, "grant");
    }
    // ...but not administer.
    const administer = await decide.decide(subject, "administer", pkg);
    assert.equal(administer.allow, false);
});

test("Backend A maps GitHub permissions onto the verb ladder", async () => {
    const cases: [
        Partial<{
            pull: boolean;
            push: boolean;
            maintain: boolean;
            admin: boolean;
        }>,
        Record<"view" | "propose" | "approve" | "administer", boolean>,
    ][] = [
        [
            { pull: true },
            { view: true, propose: false, approve: false, administer: false },
        ],
        [
            { pull: true, push: true },
            { view: true, propose: true, approve: false, administer: false },
        ],
        [
            { pull: true, push: true, maintain: true },
            { view: true, propose: true, approve: true, administer: false },
        ],
        [
            { pull: true, push: true, admin: true },
            { view: true, propose: true, approve: true, administer: true },
        ],
        [
            {},
            { view: false, propose: false, approve: false, administer: false },
        ],
    ];

    for (const [permissions, expected] of cases) {
        const store = new Store(null);
        const github = new FakeGitHub();
        const principal = store.createPrincipal("Dev");
        const crypto = TokenCrypto.fromEnvValue(TEST_KEY);
        store.attachIdentity(
            principal.id,
            {
                provider: "github",
                subject: "42",
                login: "dev",
                email: null,
                emailVerified: false,
                name: null,
                avatarUrl: null,
            },
            crypto.encrypt("user-token"),
        );
        const tree = store.insertSourceTree({
            kind: "github",
            owner: "acme",
            name: "config",
            defaultBranch: "main",
            createdBy: null,
        });
        github.grantRepo("user-token", "acme/config", permissions);
        const decide = decisionPoint(store, github);
        for (const action of [
            "view",
            "propose",
            "approve",
            "administer",
        ] as const) {
            const decision = await decide.decide(
                { kind: "principal", id: principal.id },
                action,
                { kind: "source-tree", sourceTree: tree.id },
            );
            assert.equal(
                decision.allow,
                expected[action],
                `${JSON.stringify(permissions)} -> ${action}`,
            );
            if (decision.allow) {
                assert.equal(decision.backend, "github");
            }
        }
    }
});

test("an invisible repository is an honest deny, not an error", async () => {
    const store = new Store(null);
    const github = new FakeGitHub();
    const principal = store.createPrincipal("Outsider");
    const crypto = TokenCrypto.fromEnvValue(TEST_KEY);
    store.attachIdentity(
        principal.id,
        {
            provider: "github",
            subject: "7",
            login: "outsider",
            email: null,
            emailVerified: false,
            name: null,
            avatarUrl: null,
        },
        crypto.encrypt("outsider-token"),
    );
    const tree = store.insertSourceTree({
        kind: "github",
        owner: "acme",
        name: "secrets",
        defaultBranch: "main",
        createdBy: null,
    });
    const decide = decisionPoint(store, github);
    const decision = await decide.decide(
        { kind: "principal", id: principal.id },
        "view",
        { kind: "source-tree", sourceTree: tree.id },
    );
    assert.equal(decision.allow, false);
    assert.equal(decision.backend, "github");
    assert.match(decision.reason, /does not show/);
});

test("resource lineage walks entity -> package -> tree -> deployment", () => {
    const lineage = resourceLineage({
        kind: "entity",
        sourceTree: "st_1",
        path: "pkg",
        entity: "variable/checkout",
    }).map(resourceString);
    assert.deepEqual(lineage, [
        "entity:st_1/pkg/variable/checkout",
        "package:st_1/pkg",
        "source-tree:st_1",
        "deployment",
    ]);
});

test("grants held through group membership reach the principal", async () => {
    const store = new Store(null);
    const principal = store.createPrincipal("Priya");
    const group = store.createGroup("pricing_admins", null);
    store.addGroupMember(group.id, principal.id);
    store.insertGrant({
        granteeKind: "group",
        granteeId: group.id,
        action: "approve",
        resource: "source-tree:st_1",
        createdBy: null,
    });
    const decide = decisionPoint(store, new FakeGitHub());
    const decision = await decide.decide(
        { kind: "principal", id: principal.id },
        "approve",
        { kind: "package", sourceTree: "st_1", path: "packages/basic" },
    );
    assert.equal(decision.allow, true);
    assert.equal(decision.backend, "grant");

    // Removing the membership removes the reach.
    store.removeGroupMember(group.id, principal.id);
    const after = await decide.decide(
        { kind: "principal", id: principal.id },
        "approve",
        { kind: "package", sourceTree: "st_1", path: "packages/basic" },
    );
    assert.equal(after.allow, false);
});

test("the two-person rule: a contributor's grant cannot approve their own change", async () => {
    const store = new Store(null);
    const author = store.createPrincipal("Priya");
    const approver = store.createPrincipal("Alex");
    for (const grantee of [author, approver]) {
        store.insertGrant({
            granteeKind: "principal",
            granteeId: grantee.id,
            action: "approve",
            resource: "source-tree:st_1",
            createdBy: null,
        });
    }
    const decide = decisionPoint(store, new FakeGitHub());
    const resource = {
        kind: "source-tree",
        sourceTree: "st_1",
    } as const;
    const contributors = [author.id];

    const self = await decide.decide(
        { kind: "principal", id: author.id },
        "approve",
        resource,
        { contributors },
    );
    assert.equal(self.allow, false);
    assert.match(self.reason, /second person/);

    // The same grant still lets the author propose (the rule binds approve
    // alone), and a non-contributor approves normally.
    const propose = await decide.decide(
        { kind: "principal", id: author.id },
        "propose",
        resource,
        { contributors },
    );
    assert.equal(propose.allow, true);
    const second = await decide.decide(
        { kind: "principal", id: approver.id },
        "approve",
        resource,
        { contributors },
    );
    assert.equal(second.allow, true);
});
