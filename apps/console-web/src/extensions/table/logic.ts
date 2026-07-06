// Row planning for the table experience: effective-dating awareness
// (current versus scheduled versus expired rows when entries carry date
// fields) and priority ordering, derived from the catalog schema and the
// entry values alone. Pure and import-free so it can be unit-tested with
// wire shapes directly.

export type TableEntry = { key: string; value: unknown };

export type RowPlan = {
    key: string;
    value: unknown;
    // "current" | "scheduled" | "expired"; null when the catalog carries no
    // effective dating.
    timing: string | null;
    priority: number | null;
};

export type TablePlan = {
    effectiveField: string | null;
    untilField: string | null;
    priorityField: string | null;
    rows: RowPlan[];
};

const EFFECTIVE_NAMES = ["effective_from", "effective_at", "starts_at"];
const UNTIL_NAMES = ["effective_until", "ends_at", "expires_at"];
const PRIORITY_NAMES = ["priority", "rank"];

export function planTable(
    schema: unknown,
    entries: TableEntry[],
    now: string,
): TablePlan {
    const properties = schemaProperties(schema);
    const effectiveField = findField(properties, entries, EFFECTIVE_NAMES);
    const untilField = findField(properties, entries, UNTIL_NAMES);
    const priorityField = findField(properties, entries, PRIORITY_NAMES);

    const rows: RowPlan[] = entries.map((entry) => {
        const from = dateField(entry.value, effectiveField);
        const until = dateField(entry.value, untilField);
        let timing: string | null = null;
        if (effectiveField !== null || untilField !== null) {
            if (from !== null && from > now) {
                timing = "scheduled";
            } else if (until !== null && until <= now) {
                timing = "expired";
            } else {
                timing = "current";
            }
        }
        const priorityRaw =
            priorityField === null
                ? undefined
                : fieldOf(entry.value, priorityField);
        return {
            key: entry.key,
            value: entry.value,
            timing,
            priority: typeof priorityRaw === "number" ? priorityRaw : null,
        };
    });

    // Priority ordering: the highest priority first (that is the row a
    // priority-sorted query would pick), stable by key otherwise.
    rows.sort((left, right) => {
        if (left.priority !== null || right.priority !== null) {
            const gap =
                (right.priority ?? -Infinity) - (left.priority ?? -Infinity);
            if (gap !== 0) {
                return gap;
            }
        }
        return left.key.localeCompare(right.key);
    });

    return { effectiveField, untilField, priorityField, rows };
}

function findField(
    properties: Set<string>,
    entries: TableEntry[],
    names: string[],
): string | null {
    for (const name of names) {
        if (properties.has(name)) {
            return name;
        }
    }
    // Schemas that leave dating open still get the awareness when the
    // entries actually carry the field.
    for (const name of names) {
        if (entries.some((entry) => fieldOf(entry.value, name) !== undefined)) {
            return name;
        }
    }
    return null;
}

function schemaProperties(schema: unknown): Set<string> {
    if (
        typeof schema !== "object" ||
        schema === null ||
        Array.isArray(schema)
    ) {
        return new Set();
    }
    const properties = (schema as Record<string, unknown>).properties;
    if (
        typeof properties !== "object" ||
        properties === null ||
        Array.isArray(properties)
    ) {
        return new Set();
    }
    return new Set(Object.keys(properties));
}

function fieldOf(value: unknown, field: string | null): unknown {
    if (
        field === null ||
        typeof value !== "object" ||
        value === null ||
        Array.isArray(value)
    ) {
        return undefined;
    }
    return (value as Record<string, unknown>)[field];
}

function dateField(value: unknown, field: string | null): string | null {
    const raw = fieldOf(value, field);
    return typeof raw === "string" && raw !== "" ? raw : null;
}
