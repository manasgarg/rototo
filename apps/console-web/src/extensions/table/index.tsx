// The table experience: bound catalogs as rich tables. Schema-driven
// columns and cell widgets (the contract's ControlInput carries the
// reference pickers), effective-dating awareness (current versus scheduled
// rows), priority ordering, and a query-winner preview panel when a bound
// variable selects from the catalog. One archetype configured three ways
// covers pricing, providers, and campaigns; it holds zero architectural
// privilege and proves the contract by existing.

import { useEffect, useState } from "react";

import type {
    ContextInventory,
    EditOperation,
    ExperienceModule,
    ExperienceProps,
    SurfaceItem,
    TraceOutcome,
} from "../../extension-api.ts";
import { planTable, type TablePlan } from "./logic.ts";

function Render(props: ExperienceProps) {
    const { items, editable, now, read, propose, ui, openWorkbench } = props;

    const catalogs = items.filter(
        (item): item is Extract<SurfaceItem, { kind: "catalog" }> =>
            item.kind === "catalog",
    );
    const selectors = items.filter(
        (item): item is Extract<SurfaceItem, { kind: "variable" }> =>
            item.kind === "variable" &&
            catalogs.some((catalog) => catalog.id === item.variableType),
    );
    const extras = items.filter(
        (item) =>
            item.kind !== "catalog" &&
            !(
                item.kind === "variable" &&
                selectors.some((selector) => selector.id === item.id)
            ),
    );

    return (
        <div className="section">
            {catalogs.map((catalog) => (
                <CatalogTable
                    key={catalog.id}
                    catalog={catalog}
                    now={now}
                    editable={editable}
                    propose={propose}
                    ui={ui}
                />
            ))}
            {selectors.length > 0 ? (
                <WinnerPanel
                    selectors={selectors.map((selector) => selector.id)}
                    read={read}
                    ui={ui}
                />
            ) : null}
            {extras.map((item, index) => (
                <ui.AdvancedShape
                    key={index}
                    label={labelOf(item)}
                    detail="The table experience renders catalogs; this binding is something else."
                    onOpen={openWorkbench}
                />
            ))}
        </div>
    );
}

function CatalogTable({
    catalog,
    now,
    editable,
    propose,
    ui,
}: {
    catalog: Extract<SurfaceItem, { kind: "catalog" }>;
    now: string;
    editable: boolean;
    propose: (operations: EditOperation[], summary: string) => void;
    ui: ExperienceProps["ui"];
}) {
    const plan: TablePlan = planTable(catalog.schema, catalog.entries, now);
    const head = [
        "entry",
        ...(plan.effectiveField !== null || plan.untilField !== null
            ? ["timing"]
            : []),
        ...catalog.fields.map((field) => (
            <span className="mono" key={field.field}>
                {field.field}
            </span>
        )),
        ...(catalog.canDelete ? [""] : []),
    ];
    return (
        <div className="surface-item">
            <div className="section-header-text">
                <h3 className="mono">{catalog.id}</h3>
                {catalog.description !== null ? (
                    <p className="hint">{catalog.description}</p>
                ) : null}
            </div>
            <ui.Table head={head}>
                {plan.rows.map((row) => (
                    <tr
                        key={row.key}
                        className={
                            row.timing === "expired" ? "row-muted" : undefined
                        }
                    >
                        <td className="mono">
                            {row.key}
                            {row.priority !== null ? (
                                <span className="hint"> #{row.priority}</span>
                            ) : null}
                        </td>
                        {plan.effectiveField !== null ||
                        plan.untilField !== null ? (
                            <td>
                                <ui.Pill
                                    tone={
                                        row.timing === "current"
                                            ? "ok"
                                            : row.timing === "scheduled"
                                              ? "info"
                                              : "neutral"
                                    }
                                >
                                    {row.timing}
                                </ui.Pill>
                            </td>
                        ) : null}
                        {catalog.fields.map((field) => (
                            <td key={field.field}>
                                <ui.ControlInput
                                    control={field}
                                    value={fieldOf(row.value, field.field)}
                                    disabled={!editable}
                                    onCommit={(value) =>
                                        propose(
                                            [
                                                {
                                                    op: "set_field",
                                                    target: `catalog=${catalog.id}:entry=${row.key}#/${field.field}`,
                                                    value,
                                                },
                                            ],
                                            `Set ${catalog.id}/${row.key} ${field.field}`,
                                        )
                                    }
                                />
                            </td>
                        ))}
                        {catalog.canDelete ? (
                            <td>
                                <ui.Button
                                    tone="ghost"
                                    disabled={!editable}
                                    title="Delete entry"
                                    onClick={() =>
                                        propose(
                                            [
                                                {
                                                    op: "delete",
                                                    target: `catalog=${catalog.id}:entry=${row.key}`,
                                                },
                                            ],
                                            `Delete ${catalog.id}/${row.key}`,
                                        )
                                    }
                                >
                                    ×
                                </ui.Button>
                            </td>
                        ) : null}
                    </tr>
                ))}
            </ui.Table>
            {catalog.canAdd && editable ? (
                <AddRow catalog={catalog.id} propose={propose} ui={ui} />
            ) : null}
        </div>
    );
}

function AddRow({
    catalog,
    propose,
    ui,
}: {
    catalog: string;
    propose: (operations: EditOperation[], summary: string) => void;
    ui: ExperienceProps["ui"];
}) {
    const [open, setOpen] = useState(false);
    const [key, setKey] = useState("");
    const [fieldsText, setFieldsText] = useState("{}");
    if (!open) {
        return (
            <div className="action-row">
                <ui.Button tone="secondary" onClick={() => setOpen(true)}>
                    Add entry
                </ui.Button>
            </div>
        );
    }
    return (
        <form
            className="inline-form"
            onSubmit={(event) => {
                event.preventDefault();
                let parsed: unknown;
                try {
                    parsed = JSON.parse(fieldsText);
                } catch {
                    return;
                }
                setOpen(false);
                propose(
                    [
                        {
                            op: "create_entry",
                            catalog,
                            key: key.trim(),
                            fields: parsed,
                        },
                    ],
                    `Add ${catalog}/${key.trim()}`,
                );
            }}
        >
            <input
                className="input mono"
                placeholder="entry_id"
                value={key}
                onChange={(event) => setKey(event.target.value)}
            />
            <input
                className="input mono"
                placeholder="fields as JSON"
                value={fieldsText}
                onChange={(event) => setFieldsText(event.target.value)}
            />
            <ui.Button tone="primary" submit disabled={key.trim() === ""}>
                Create
            </ui.Button>
            <ui.Button tone="ghost" onClick={() => setOpen(false)}>
                Cancel
            </ui.Button>
        </form>
    );
}

// "Which entry wins for this visitor": the bound selector variables
// resolved under a chosen sample context, through the contract's preview.
function WinnerPanel({
    selectors,
    read,
    ui,
}: {
    selectors: string[];
    read: ExperienceProps["read"];
    ui: ExperienceProps["ui"];
}) {
    const [inventory, setInventory] = useState<ContextInventory | null>(null);
    const [picked, setPicked] = useState<number>(0);
    const [outcomes, setOutcomes] = useState<TraceOutcome[] | null>(null);

    useEffect(() => {
        let stale = false;
        read.contexts().then(
            (response) => {
                if (!stale) {
                    setInventory(response);
                }
            },
            () => undefined,
        );
        return () => {
            stale = true;
        };
    }, [read]);

    const samples = (inventory?.samples ?? []).filter(
        (sample) => sample.context !== null,
    );
    useEffect(() => {
        const sample = samples[picked];
        if (sample?.context == null) {
            return;
        }
        let stale = false;
        setOutcomes(null);
        read.preview(sample.context).then(
            (response) => {
                if (!stale) {
                    setOutcomes(response);
                }
            },
            () => undefined,
        );
        return () => {
            stale = true;
        };
        // samples derive from inventory; picked and inventory are the deps.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [read, inventory, picked]);

    if (samples.length === 0) {
        return null;
    }
    return (
        <div className="card">
            <div className="card-head">
                <div className="card-head-text">
                    <h3>Who wins</h3>
                    <p className="hint">
                        The bound selector under a saved sample context.
                    </p>
                </div>
                <select
                    className="input"
                    value={picked}
                    onChange={(event) => setPicked(Number(event.target.value))}
                >
                    {samples.map((sample, index) => (
                        <option key={index} value={index}>
                            {sample.evaluationContext}/{sample.key}
                        </option>
                    ))}
                </select>
            </div>
            {selectors.map((id) => {
                const outcome = outcomes?.find((entry) => entry.id === id);
                return (
                    <div className="field-row surface-item" key={id}>
                        <span className="label mono">{id}</span>
                        {outcomes === null ? (
                            <span className="hint">resolving…</span>
                        ) : outcome?.trace !== undefined ? (
                            <span className="mono">
                                {shortValue(outcome.trace.resolution.value)}
                            </span>
                        ) : (
                            <ui.Pill tone="warn">
                                {outcome?.error ?? "no outcome"}
                            </ui.Pill>
                        )}
                    </div>
                );
            })}
        </div>
    );
}

function shortValue(value: unknown): string {
    const text = JSON.stringify(value);
    return text.length > 120 ? `${text.slice(0, 117)}…` : text;
}

function fieldOf(value: unknown, field: string): unknown {
    if (typeof value !== "object" || value === null || Array.isArray(value)) {
        return undefined;
    }
    return (value as Record<string, unknown>)[field];
}

function labelOf(item: SurfaceItem): string {
    switch (item.kind) {
        case "variable":
        case "catalog":
        case "layer":
            return item.id;
        case "entry":
            return `${item.catalog}/${item.key}`;
        case "missing":
            return item.target;
    }
}

const table: ExperienceModule = { kind: "table", Render };
export default table;
