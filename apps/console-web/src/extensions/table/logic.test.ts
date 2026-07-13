// Row planning for the table experience: effective dating and priority
// ordering derived from schema plus entries alone.

import assert from "node:assert/strict";
import { test } from "node:test";

import { planTable } from "./logic.ts";

const NOW = "2026-07-06T12:00:00Z";

test("date fields classify rows as current, scheduled, or expired", () => {
    const schema = {
        type: "object",
        properties: {
            amount: { type: "number" },
            effective_from: { type: "string", format: "date-time" },
            effective_until: { type: "string", format: "date-time" },
        },
    };
    const plan = planTable(
        schema,
        [
            {
                key: "current_price",
                value: { amount: 49, effective_from: "2026-01-01T00:00:00Z" },
            },
            {
                key: "october_increase",
                value: { amount: 59, effective_from: "2026-10-01T00:00:00Z" },
            },
            {
                key: "old_promo",
                value: {
                    amount: 29,
                    effective_from: "2025-01-01T00:00:00Z",
                    effective_until: "2025-06-01T00:00:00Z",
                },
            },
        ],
        NOW,
    );
    assert.equal(plan.effectiveField, "effective_from");
    const timing = Object.fromEntries(
        plan.rows.map((row) => [row.key, row.timing]),
    );
    assert.equal(timing.current_price, "current");
    assert.equal(timing.october_increase, "scheduled");
    assert.equal(timing.old_promo, "expired");
});

test("priority orders rows highest first; no dating means no timing", () => {
    const plan = planTable(
        { type: "object", properties: { priority: { type: "integer" } } },
        [
            { key: "fallback", value: { priority: 1 } },
            { key: "campaign", value: { priority: 10 } },
            { key: "unprioritized", value: {} },
        ],
        NOW,
    );
    assert.deepEqual(
        plan.rows.map((row) => row.key),
        ["campaign", "fallback", "unprioritized"],
    );
    assert.equal(plan.rows[0]?.timing, null);
});

test("entries carrying date fields get the awareness even when the schema leaves them open", () => {
    const plan = planTable(
        { type: "object" },
        [
            {
                key: "scheduled_banner",
                value: { effective_from: "2027-01-01T00:00:00Z" },
            },
        ],
        NOW,
    );
    assert.equal(plan.effectiveField, "effective_from");
    assert.equal(plan.rows[0]?.timing, "scheduled");
});
