// The flags derivation, unit-tested on the wire shapes the server produces.
// The lifecycle these cases walk (dark → 10% → 50% → launched) is the same
// one the flag-rollout walkthrough drives end to end through the server.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
    advanceRingOperation,
    bucketWidth,
    deriveFlagStatus,
    dialOperations,
    nextRing,
    scheduleFlipOperation,
    type FlagShape,
} from "./logic.ts";

const NOW = "2026-07-06T12:00:00Z";

function rolloutFlag(overrides: Partial<FlagShape> = {}): FlagShape {
    return {
        id: "new_editor",
        variableType: "bool",
        default: false,
        method: "allocation",
        rules: [],
        allocation: {
            layer: "rollout",
            id: "new_editor",
            status: "draft",
            unit: "context.user.id",
            totalBuckets: 1000,
            eligibility: null,
            arms: [
                { name: "treatment", buckets: "0-99", value: true },
                { name: "control", buckets: "100-999", value: false },
            ],
        },
        ...overrides,
    };
}

test("a drafted rollout is dark", () => {
    const status = deriveFlagStatus(rolloutFlag(), NOW);
    assert.equal(status.state, "dark");
    assert.match(status.state === "dark" ? status.summary : "", /dark/);
});

test("a running rollout states its percentage", () => {
    const flag = rolloutFlag();
    flag.allocation!.status = "running";
    const status = deriveFlagStatus(flag, NOW);
    assert.equal(status.state, "partial");
    if (status.state !== "partial") {
        return;
    }
    assert.match(status.summary, /10% of everyone else/);
    assert.equal(status.rollout?.percent, 10);
});

test("widening the treatment arm moves the percentage", () => {
    const flag = rolloutFlag();
    flag.allocation!.status = "running";
    flag.allocation!.arms = [
        { name: "treatment", buckets: "0-499", value: true },
        { name: "control", buckets: "500-999", value: false },
    ];
    const status = deriveFlagStatus(flag, NOW);
    assert.equal(status.state, "partial");
    if (status.state !== "partial") {
        return;
    }
    assert.equal(status.rollout?.percent, 50);
});

test("default true with a concluded allocation is launched", () => {
    const flag = rolloutFlag({ default: true });
    flag.allocation!.status = "concluded";
    const status = deriveFlagStatus(flag, NOW);
    assert.equal(status.state, "on");
    if (status.state !== "on") {
        return;
    }
    assert.match(status.summary, /launched/);
});

test("ring rules read as named conditions", () => {
    const flag: FlagShape = {
        id: "checkout_redesign",
        variableType: "bool",
        default: false,
        method: null,
        rules: [
            { index: 0, when: 'variables["employees"]', value: true },
            {
                index: 1,
                when: 'env.now >= timestamp("2026-07-10T00:00:00Z")',
                value: true,
            },
        ],
        allocation: null,
    };
    const status = deriveFlagStatus(flag, NOW);
    assert.equal(status.state, "partial");
    if (status.state !== "partial") {
        return;
    }
    assert.match(status.summary, /on for employees/);
    assert.match(status.summary, /scheduled on at 2026-07-10/);
});

test("anything the recognizers do not understand degrades, never guesses", () => {
    const clever = deriveFlagStatus(
        {
            id: "clever",
            variableType: "bool",
            default: false,
            method: null,
            rules: [
                {
                    index: 0,
                    when: "size(context.user.teams.filter(t, t.beta)) > 0",
                    value: true,
                },
            ],
            allocation: null,
        },
        NOW,
    );
    assert.equal(clever.state, "advanced");

    const notAFlag = deriveFlagStatus(
        {
            id: "plan",
            variableType: "string",
            default: "starter",
            method: null,
            rules: [],
            allocation: null,
        },
        NOW,
    );
    assert.equal(notAFlag.state, "advanced");
});

test("the dial emits set_arm_buckets for treatment and control together", () => {
    const flag = rolloutFlag();
    flag.allocation!.status = "running";
    const status = deriveFlagStatus(flag, NOW);
    assert.equal(status.state, "partial");
    if (status.state !== "partial" || status.rollout === null) {
        return;
    }
    const operations = dialOperations(status.rollout, 500);
    assert.deepEqual(operations, [
        {
            op: "set_arm_buckets",
            layer: "rollout",
            allocation: "new_editor",
            arm: "treatment",
            buckets: "0-499",
        },
        {
            op: "set_arm_buckets",
            layer: "rollout",
            allocation: "new_editor",
            arm: "control",
            buckets: "500-999",
        },
    ]);
});

test("ring advancement adds the next unclaimed ring after the last ring rule", () => {
    const flag: FlagShape = {
        id: "checkout_redesign",
        variableType: "bool",
        default: false,
        method: null,
        rules: [{ index: 0, when: 'variables["employees"]', value: true }],
        allocation: null,
    };
    const status = deriveFlagStatus(flag, NOW);
    assert.notEqual(status.state, "advanced");
    if (status.state === "advanced") {
        return;
    }
    const next = nextRing(["employees", "beta_users"], status.rules);
    assert.deepEqual(next, { ring: "beta_users", position: 1 });
    assert.deepEqual(
        advanceRingOperation("checkout_redesign", "beta_users", 1),
        {
            op: "add_rule",
            variable: "checkout_redesign",
            position: 1,
            when: 'variables["beta_users"]',
            value: true,
        },
    );
    assert.equal(nextRing(["employees"], status.rules), null);
});

test("schedule flips and bucket widths parse the shapes rototo writes", () => {
    assert.equal(bucketWidth("0-499"), 500);
    assert.equal(bucketWidth("42"), 1);
    assert.equal(bucketWidth("9-3"), null);
    assert.deepEqual(
        scheduleFlipOperation("new_editor", "2026-07-10T00:00:00Z", true),
        {
            op: "add_rule",
            variable: "new_editor",
            position: 0,
            when: 'env.now >= timestamp("2026-07-10T00:00:00Z")',
            value: true,
        },
    );
});
