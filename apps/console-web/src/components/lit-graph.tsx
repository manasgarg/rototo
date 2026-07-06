// The lit-up reference graph (design/console-system-view.md, ring 1
// execution): the package's variables on their reference edges, each node
// carrying its resolved value for the chosen context. Paths that fired are
// bright, evaluated-but-false paths stay normal, everything past the first
// match dims. With no context chosen, the same graph shows pure structure.

import type { SemanticModel, TraceOutcome } from "@/lib/api";

const NODE_WIDTH = 190;
const NODE_HEIGHT = 46;
const COLUMN_GAP = 90;
const ROW_GAP = 18;
const PADDING = 16;

type GraphNode = {
    id: string;
    depth: number;
    row: number;
    x: number;
    y: number;
};

type EdgeState = "fired" | "evaluated" | "dormant" | "plain";

type GraphEdge = {
    from: string;
    to: string;
    ruleIndex: number | null;
    state: EdgeState;
};

export function LitGraph({
    model,
    outcomes,
    onOpenVariable,
}: {
    model: SemanticModel;
    outcomes: Map<string, TraceOutcome> | null;
    onOpenVariable: (id: string) => void;
}) {
    const { nodes, edges, width, height } = layout(model, outcomes);
    if (nodes.length === 0) {
        return null;
    }
    const byId = new Map(nodes.map((node) => [node.id, node]));

    return (
        <div className="graph-scroll">
            <svg
                className="lit-graph"
                viewBox={`0 0 ${width} ${height}`}
                width={width}
                height={height}
                role="img"
                aria-label="Variable reference graph"
            >
                <defs>
                    <marker
                        id="arrow"
                        viewBox="0 0 8 8"
                        refX="7"
                        refY="4"
                        markerWidth="7"
                        markerHeight="7"
                        orient="auto-start-reverse"
                    >
                        <path d="M 0 0 L 8 4 L 0 8 z" fill="currentColor" />
                    </marker>
                </defs>
                {edges.map((edge, index) => {
                    const from = byId.get(edge.from);
                    const to = byId.get(edge.to);
                    if (from === undefined || to === undefined) {
                        return null;
                    }
                    // Dependencies point left: from the reader's left edge
                    // to the dependency's right edge.
                    const x1 = from.x;
                    const y1 = from.y + NODE_HEIGHT / 2;
                    const x2 = to.x + NODE_WIDTH;
                    const y2 = to.y + NODE_HEIGHT / 2;
                    const bend = Math.max(24, (x1 - x2) / 2);
                    return (
                        <path
                            key={index}
                            className={`graph-edge graph-edge-${edge.state}`}
                            d={`M ${x1} ${y1} C ${x1 - bend} ${y1}, ${x2 + bend} ${y2}, ${x2} ${y2}`}
                            markerEnd="url(#arrow)"
                        >
                            <title>
                                {edge.from}
                                {edge.ruleIndex !== null
                                    ? ` rule ${edge.ruleIndex}`
                                    : ""}{" "}
                                reads {edge.to} ({edge.state})
                            </title>
                        </path>
                    );
                })}
                {nodes.map((node) => {
                    const outcome = outcomes?.get(node.id) ?? null;
                    const state =
                        outcomes === null
                            ? "plain"
                            : outcome?.error !== undefined
                              ? "error"
                              : "resolved";
                    return (
                        <g
                            key={node.id}
                            className={`graph-node graph-node-${state}`}
                            transform={`translate(${node.x}, ${node.y})`}
                            onClick={() => onOpenVariable(node.id)}
                        >
                            <rect
                                width={NODE_WIDTH}
                                height={NODE_HEIGHT}
                                rx={8}
                            />
                            <text className="graph-node-id" x={10} y={19}>
                                {clip(node.id, 24)}
                            </text>
                            <text className="graph-node-value" x={10} y={36}>
                                {outcome === null
                                    ? ""
                                    : outcome.error !== undefined
                                      ? "cannot resolve"
                                      : clip(
                                            JSON.stringify(
                                                outcome.trace?.resolution.value,
                                            ) ?? "",
                                            26,
                                        )}
                            </text>
                            <title>
                                {node.id}
                                {outcome?.error !== undefined
                                    ? `\n${outcome.error}`
                                    : outcome?.trace !== undefined
                                      ? `\n= ${JSON.stringify(outcome.trace.resolution.value)}`
                                      : ""}
                            </title>
                        </g>
                    );
                })}
            </svg>
        </div>
    );
}

function layout(
    model: SemanticModel,
    outcomes: Map<string, TraceOutcome> | null,
): {
    nodes: GraphNode[];
    edges: GraphEdge[];
    width: number;
    height: number;
} {
    const variableIds = model.variables.map((variable) => variable.id);
    const known = new Set(variableIds);

    // Variable-to-variable references, with the rule index that reads them.
    const references: { from: string; to: string; ruleIndex: number | null }[] =
        [];
    for (const reference of model.references) {
        if (
            reference.from.kind !== "variable" ||
            reference.to.kind !== "variable"
        ) {
            continue;
        }
        const from = reference.from.id as string;
        const to = reference.to.id as string;
        if (!known.has(from) || !known.has(to) || from === to) {
            continue;
        }
        const via = reference.via as { kind: string; index?: number };
        references.push({
            from,
            to,
            ruleIndex:
                via.kind === "ruleCondition" || via.kind === "ruleValue"
                    ? (via.index ?? null)
                    : null,
        });
    }

    // Depth = how far a variable sits above its dependencies; dependencies
    // draw left of their readers. Reference cycles are lint errors, but the
    // visited guard keeps a broken package from hanging the view.
    const dependencies = new Map<string, string[]>();
    for (const reference of references) {
        const list = dependencies.get(reference.from) ?? [];
        list.push(reference.to);
        dependencies.set(reference.from, list);
    }
    const depths = new Map<string, number>();
    const measure = (id: string, trail: Set<string>): number => {
        const cached = depths.get(id);
        if (cached !== undefined) {
            return cached;
        }
        if (trail.has(id)) {
            return 0;
        }
        trail.add(id);
        const below = (dependencies.get(id) ?? []).map((dep) =>
            measure(dep, trail),
        );
        trail.delete(id);
        const depth = below.length === 0 ? 0 : Math.max(...below) + 1;
        depths.set(id, depth);
        return depth;
    };
    for (const id of variableIds) {
        measure(id, new Set());
    }

    const columns = new Map<number, string[]>();
    for (const id of variableIds) {
        const depth = depths.get(id) ?? 0;
        const column = columns.get(depth) ?? [];
        column.push(id);
        columns.set(depth, column);
    }

    const nodes: GraphNode[] = [];
    let maxRows = 0;
    for (const [depth, ids] of columns) {
        maxRows = Math.max(maxRows, ids.length);
        ids.forEach((id, row) => {
            nodes.push({
                id,
                depth,
                row,
                x: PADDING + depth * (NODE_WIDTH + COLUMN_GAP),
                y: PADDING + row * (NODE_HEIGHT + ROW_GAP),
            });
        });
    }
    const columnCount = columns.size;

    const edges: GraphEdge[] = references.map((reference) => ({
        ...reference,
        state: edgeState(reference, outcomes),
    }));

    return {
        nodes,
        edges,
        width:
            PADDING * 2 +
            columnCount * NODE_WIDTH +
            (columnCount - 1) * COLUMN_GAP,
        height: PADDING * 2 + maxRows * NODE_HEIGHT + (maxRows - 1) * ROW_GAP,
    };
}

// Rules evaluate in order until the first match: the matching rule's
// references fired, earlier rules' references were evaluated, later ones
// never ran. Without a chosen context every edge is plain structure.
function edgeState(
    reference: { from: string; ruleIndex: number | null },
    outcomes: Map<string, TraceOutcome> | null,
): EdgeState {
    if (outcomes === null) {
        return "plain";
    }
    const outcome = outcomes.get(reference.from);
    if (outcome?.trace === undefined) {
        return "dormant";
    }
    if (reference.ruleIndex === null) {
        return "evaluated";
    }
    const matched = outcome.trace.rules.find((rule) => rule.matched);
    if (matched === undefined) {
        return "evaluated";
    }
    if (reference.ruleIndex === matched.index) {
        return "fired";
    }
    return reference.ruleIndex < matched.index ? "evaluated" : "dormant";
}

function clip(text: string, length: number): string {
    return text.length > length ? `${text.slice(0, length)}…` : text;
}
