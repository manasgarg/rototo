// Status derivation for the flags experience: interpret a bool variable's
// default plus rules plus allocation into a domain state ("dark", "on for
// employees, 40% for everyone else, scheduled to flip Friday"). The
// recognizers are deliberately narrow — simple default, condition-variable
// ring rules, plain context comparisons, one allocation, env.now schedule
// rules — and anything outside them degrades that flag to "advanced shape"
// instead of guessing. Experiences degrade; they never block and never lie.
//
// This module is pure and imports nothing, so the flag-rollout walkthrough
// (a server-side gate test) can feed it the wire shapes the server actually
// produces.

export type FlagRule = {
    index: number;
    when: string | null;
    value: unknown;
};

export type FlagArm = {
    name: string;
    buckets: string | null;
    value: unknown;
};

export type FlagAllocation = {
    layer: string;
    id: string;
    status: string | null;
    unit: string | null;
    totalBuckets: number | null;
    eligibility: string | null;
    arms: FlagArm[];
};

// The slice of a variable surface item the derivation reads. SurfaceItem's
// variable arm satisfies it structurally.
export type FlagShape = {
    id: string;
    variableType: string | null;
    default: unknown;
    method: string | null;
    rules: FlagRule[];
    allocation: FlagAllocation | null;
};

export type RecognizedRule =
    | {
          kind: "condition";
          index: number;
          name: string;
          negated: boolean;
          value: boolean;
      }
    | { kind: "fact"; index: number; text: string; value: boolean }
    | {
          kind: "schedule";
          index: number;
          op: string;
          instant: string;
          value: boolean;
      };

// The rollout dial's data: recognized only for the canonical shape of one
// treatment arm assigning true (optionally one control arm assigning false)
// anchored at bucket 0.
export type RolloutView = {
    layer: string;
    allocation: string;
    status: string | null;
    unit: string | null;
    arm: string;
    controlArm: string | null;
    totalBuckets: number;
    treatmentBuckets: number;
    percent: number;
};

export type FlagStatus =
    | { state: "advanced"; reason: string }
    | {
          state: "dark" | "on" | "partial";
          summary: string;
          clauses: string[];
          rules: RecognizedRule[];
          rollout: RolloutView | null;
      };

export function deriveFlagStatus(flag: FlagShape, now: string): FlagStatus {
    if (flag.variableType !== "bool") {
        return {
            state: "advanced",
            reason: `type ${flag.variableType ?? "unknown"} is not a flag`,
        };
    }
    if (flag.method !== null && flag.method !== "allocation") {
        return {
            state: "advanced",
            reason: `resolve method "${flag.method}" is cleverer than rings and rollouts`,
        };
    }
    if (typeof flag.default !== "boolean") {
        return { state: "advanced", reason: "the default is not a boolean" };
    }

    const rules: RecognizedRule[] = [];
    for (const rule of flag.rules) {
        const recognized = recognizeRule(rule);
        if (recognized === null) {
            return {
                state: "advanced",
                reason: `rule ${rule.index} is more clever than this experience understands`,
            };
        }
        rules.push(recognized);
    }

    let rollout: RolloutView | null = null;
    if (flag.method === "allocation") {
        rollout = recognizeRollout(flag.allocation);
        if (rollout === null) {
            return {
                state: "advanced",
                reason: "the allocation shape is not a single-treatment rollout",
            };
        }
    }

    const clauses: string[] = [];
    let exposure = false;
    for (const rule of rules) {
        if (rule.kind === "condition") {
            clauses.push(
                `${rule.value ? "on" : "off"} for ${rule.negated ? "everyone but " : ""}${rule.name.replaceAll("_", " ")}`,
            );
            exposure = exposure || rule.value;
        } else if (rule.kind === "fact") {
            clauses.push(`${rule.value ? "on" : "off"} when ${rule.text}`);
            exposure = exposure || rule.value;
        } else {
            const future = rule.instant > now;
            if (future) {
                clauses.push(
                    `scheduled ${rule.value ? "on" : "off"} at ${rule.instant}`,
                );
            } else {
                clauses.push(
                    `${rule.value ? "on" : "off"} since ${rule.instant}`,
                );
                exposure = exposure || rule.value;
            }
        }
    }
    if (rollout !== null && rollout.status === "running") {
        clauses.push(
            `${rollout.percent}% of everyone else${rollout.unit === null ? "" : ` (by ${rollout.unit})`}`,
        );
        exposure = exposure || rollout.percent > 0;
    }

    if (flag.default === true) {
        const diverted =
            rules.some((rule) => rule.value === false) ||
            (rollout !== null && rollout.status === "running");
        if (!diverted && clauses.length === 0) {
            return {
                state: "on",
                summary: "launched: on for everyone",
                clauses: [],
                rules,
                rollout,
            };
        }
        clauses.push("otherwise on");
        return {
            state: "on",
            summary: joinClauses(clauses),
            clauses,
            rules,
            rollout,
        };
    }

    if (!exposure) {
        const pending = clauses.filter((clause) =>
            clause.startsWith("scheduled"),
        );
        const summary =
            pending.length > 0
                ? `dark, ${joinClauses(pending)}`
                : "dark: off for everyone";
        return { state: "dark", summary, clauses, rules, rollout };
    }

    clauses.push("otherwise off");
    return {
        state: "partial",
        summary: joinClauses(clauses),
        clauses,
        rules,
        rollout,
    };
}

function joinClauses(clauses: string[]): string {
    return clauses.join(", ");
}

function recognizeRule(rule: FlagRule): RecognizedRule | null {
    if (typeof rule.value !== "boolean" || rule.when === null) {
        return null;
    }
    const when = rule.when.trim();

    const condition = when.match(
        /^(!?)\s*variables(?:\.([a-z0-9_]+)|\["([a-z0-9_/]+)"\])$/,
    );
    if (condition !== null) {
        return {
            kind: "condition",
            index: rule.index,
            name: (condition[2] ?? condition[3]) as string,
            negated: condition[1] === "!",
            value: rule.value,
        };
    }

    const schedule = when.match(
        /^env\.now\s*(>=|>|<=|<)\s*(?:timestamp\()?"([^"]+)"\)?$/,
    );
    if (schedule !== null) {
        return {
            kind: "schedule",
            index: rule.index,
            op: schedule[1] as string,
            instant: schedule[2] as string,
            value: rule.value,
        };
    }

    const fact = when.match(
        /^context\.[a-z0-9_.]+\s*(==|!=|<=|>=|<|>|in)\s*.+$/,
    );
    if (fact !== null) {
        return {
            kind: "fact",
            index: rule.index,
            text: when,
            value: rule.value,
        };
    }

    return null;
}

// A bucket range like "0-499" (single bucket "42" is "42-42").
export function bucketWidth(range: string | null): number | null {
    if (range === null) {
        return null;
    }
    const match = range.trim().match(/^(\d+)\s*-\s*(\d+)$/);
    if (match !== null) {
        const from = Number(match[1]);
        const to = Number(match[2]);
        return to >= from ? to - from + 1 : null;
    }
    if (/^\d+$/.test(range.trim())) {
        return 1;
    }
    return null;
}

function recognizeRollout(
    allocation: FlagAllocation | null,
): RolloutView | null {
    if (allocation === null || allocation.totalBuckets === null) {
        return null;
    }
    const treatments = allocation.arms.filter((arm) => arm.value === true);
    const controls = allocation.arms.filter((arm) => arm.value === false);
    if (
        treatments.length !== 1 ||
        controls.length > 1 ||
        treatments.length + controls.length !== allocation.arms.length
    ) {
        return null;
    }
    const treatment = treatments[0] as FlagArm;
    const width = bucketWidth(treatment.buckets);
    if (width === null) {
        return null;
    }
    // The dial only understands treatment anchored at bucket 0; anything
    // fancier is a real experiment, not a rollout.
    if (!/^0(\s*-|$)/.test((treatment.buckets ?? "").trim())) {
        return null;
    }
    return {
        layer: allocation.layer,
        allocation: allocation.id,
        status: allocation.status,
        unit: allocation.unit,
        arm: treatment.name,
        controlArm: (controls[0]?.name as string | undefined) ?? null,
        totalBuckets: allocation.totalBuckets,
        treatmentBuckets: width,
        percent: Math.round((width / allocation.totalBuckets) * 100),
    };
}

// The operations one dial move emits: grow (or shrink) the treatment arm,
// and keep the control arm covering the rest when one exists. One propose,
// one commit.
export function dialOperations(
    rollout: RolloutView,
    nextTreatmentBuckets: number,
): { op: string; [key: string]: unknown }[] {
    const total = rollout.totalBuckets;
    const capped = Math.max(
        1,
        Math.min(
            nextTreatmentBuckets,
            rollout.controlArm === null ? total : total - 1,
        ),
    );
    const operations: { op: string; [key: string]: unknown }[] = [
        {
            op: "set_arm_buckets",
            layer: rollout.layer,
            allocation: rollout.allocation,
            arm: rollout.arm,
            buckets: `0-${capped - 1}`,
        },
    ];
    if (rollout.controlArm !== null) {
        operations.push({
            op: "set_arm_buckets",
            layer: rollout.layer,
            allocation: rollout.allocation,
            arm: rollout.controlArm,
            buckets: `${capped}-${total - 1}`,
        });
    }
    return operations;
}

// Ring advancement: the surface's [config] names the rollout order of
// condition variables (config.rings). The next ring is the first one no
// recognized rule grants yet; advancing adds one rule after the last ring
// rule (or at the front), value true.
export function nextRing(
    rings: string[],
    rules: RecognizedRule[],
): { ring: string; position: number } | null {
    const granted = new Set(
        rules
            .filter(
                (rule) =>
                    rule.kind === "condition" &&
                    rule.value === true &&
                    !rule.negated,
            )
            .map((rule) => (rule as { name: string }).name),
    );
    const next = rings.find((ring) => !granted.has(ring));
    if (next === undefined) {
        return null;
    }
    const lastRingRule = rules
        .filter((rule) => rule.kind === "condition" && rule.value === true)
        .map((rule) => rule.index)
        .reduce((max, index) => Math.max(max, index), -1);
    return { ring: next, position: lastRingRule + 1 };
}

export function advanceRingOperation(
    variable: string,
    ring: string,
    position: number,
): { op: string; [key: string]: unknown } {
    return {
        op: "add_rule",
        variable,
        position,
        when: `variables["${ring}"]`,
        value: true,
    };
}

export function scheduleFlipOperation(
    variable: string,
    instant: string,
    to: boolean,
): { op: string; [key: string]: unknown } {
    return {
        op: "add_rule",
        variable,
        position: 0,
        when: `env.now >= timestamp("${instant}")`,
        value: to,
    };
}
