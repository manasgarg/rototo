// The package reference graph, in the interaction model the pre-rewrite
// console converged on: every declared entity in kind columns following
// resolution order — conditions feed variables, variables select catalogs,
// catalogs contain values. Hovering a node lights up its edges and
// neighbors and previews its definition beside the canvas; search
// highlights matches without hiding the rest; each connected component gets
// its own canvas and entities with no links sit in a strip below. With a
// chosen context the same graph lights up (design/console-system-view.md
// ring 1): variable nodes carry their resolved values, rule edges that
// fired are bright, and paths that never ran dim.

import { useEffect, useMemo, useRef, useState } from "react";

import {
    readPackageFile,
    type SemanticModel,
    type TraceOutcome,
    type VariableModel,
} from "@/lib/api";
import { CodeEditor, codeLanguageForPath } from "@/components/code-editor";
import { resolvedValueText } from "@/lib/format";
import { navigate, type AddressStep } from "@/lib/router";

type GraphNodeKind =
    "condition" | "variable" | "list" | "catalog" | "catalogEntry";

type GraphNode = {
    id: string;
    kind: GraphNodeKind;
    label: string;
    href: string;
    /* The defining file, for the hover preview. */
    path: string;
    /* The id resolution outcomes are keyed by, for variable-kind nodes. */
    variableId?: string;
    /* Entities semantically tied to this one beyond drawn edges, such as a
     variable's selected values or a condition's rule-selected catalog path.
     Hover highlighting includes them. */
    related?: string[];
};

type GraphEdge = {
    from: string;
    to: string;
    kind: "checks" | "requires" | "reads" | "selects" | "contains";
    /* The variable whose resolution walks this edge, for lighting. */
    reader?: string;
    /* Rule indexes on the reader that read this dependency. */
    ruleIndexes?: number[];
    /* True when the edge is walked outside any rule (query, catalog type). */
    unconditional?: boolean;
};

type GraphData = { nodes: GraphNode[]; edges: GraphEdge[] };

const KIND_LABEL: Record<GraphNodeKind, string> = {
    condition: "condition",
    variable: "variable",
    list: "list",
    catalog: "catalog",
    catalogEntry: "value",
};

export function ReferenceGraph({
    model,
    outcomes,
    treeId,
    packagePath,
    pin,
    hrefFor,
}: {
    model: SemanticModel;
    outcomes: Map<string, TraceOutcome> | null;
    treeId: string;
    packagePath: string;
    pin: string;
    hrefFor: (steps: AddressStep[]) => string;
}) {
    const [query, setQuery] = useState("");
    const [inspectedId, setInspectedId] = useState<string | null>(null);
    // Definitions load on first inspection and stay for the pin's lifetime;
    // the parent keys this component by pin so a new commit starts clean.
    const [sources, setSources] = useState<Map<string, string | null>>(
        () => new Map(),
    );
    const pendingPaths = useRef(new Set<string>());

    const needle = query.trim().toLowerCase();
    const data = useMemo(() => packageGraph(model, hrefFor), [model, hrefFor]);
    const partitions = useMemo(() => partitionGraph(data), [data]);
    const nodesById = useMemo(
        () => new Map(data.nodes.map((node) => [node.id, node])),
        [data.nodes],
    );
    const inspectedNode = inspectedId
        ? (nodesById.get(inspectedId) ?? null)
        : null;
    const hasEntities =
        partitions.components.length > 0 || partitions.isolated.length > 0;

    const inspect = (node: GraphNode | null) => {
        if (node === null) {
            return;
        }
        setInspectedId(node.id);
        if (!sources.has(node.path) && !pendingPaths.current.has(node.path)) {
            pendingPaths.current.add(node.path);
            readPackageFile(treeId, packagePath, pin, node.path).then(
                (response) =>
                    setSources((current) =>
                        new Map(current).set(node.path, response.content),
                    ),
                () =>
                    setSources((current) =>
                        new Map(current).set(node.path, null),
                    ),
            );
        }
    };

    // Unconnected entities join the same canvas as one last cluster of
    // edgeless nodes, so the whole package is one picture.
    const clusters =
        partitions.isolated.length > 0
            ? [
                  ...partitions.components,
                  { nodes: partitions.isolated, edges: [] },
              ]
            : partitions.components;

    if (!hasEntities) {
        return (
            <div className="graph-empty hint">
                No graph entities are declared in this package.
            </div>
        );
    }

    return (
        <div className="graph-frame">
            <div className="graph-toolbar">
                <label className="search-control graph-search">
                    <span className="search-icon" aria-hidden>
                        <svg
                            width="15"
                            height="15"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="2"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                        >
                            <circle cx="11" cy="11" r="8" />
                            <path d="m21 21-4.3-4.3" />
                        </svg>
                    </span>
                    <input
                        aria-label="Search graph entities"
                        className="input"
                        onChange={(event) => setQuery(event.target.value)}
                        placeholder="Search entities"
                        type="search"
                        value={query}
                    />
                </label>
            </div>
            <div className="graph-visuals">
                <section aria-label="Entity graph" className="graph-canvas">
                    <div
                        className={
                            inspectedNode
                                ? "graph-canvas-body has-preview"
                                : "graph-canvas-body"
                        }
                    >
                        <div className="graph-scroll">
                            <ColumnsGraph
                                components={clusters}
                                inspectedId={inspectedId}
                                onInspect={inspect}
                                outcomes={outcomes}
                                query={needle}
                            />
                        </div>
                        <GraphPreview
                            node={inspectedNode}
                            source={
                                inspectedNode
                                    ? sources.get(inspectedNode.path)
                                    : undefined
                            }
                            outcome={
                                inspectedNode?.variableId !== undefined
                                    ? outcomes?.get(inspectedNode.variableId)
                                    : null
                            }
                        />
                    </div>
                </section>
            </div>
        </div>
    );
}

function GraphPreview({
    node,
    source,
    outcome,
}: {
    node: GraphNode | null;
    /* undefined = loading, null = unavailable. */
    source: string | null | undefined;
    outcome?: TraceOutcome | null;
}) {
    if (node === null) {
        return null;
    }
    return (
        <aside className="graph-preview" aria-live="polite">
            <div className="graph-preview-head">
                <div className="graph-preview-title">
                    <span className="label">{KIND_LABEL[node.kind]}</span>
                    <h4 className="mono">{node.label}</h4>
                </div>
            </div>
            {outcome?.error !== undefined ? (
                <div className="banner banner-warn">{outcome.error}</div>
            ) : null}
            {source === null ? (
                <div className="graph-preview-empty hint">
                    Definition unavailable.
                </div>
            ) : (
                <div className="graph-preview-source">
                    <div className="graph-preview-file">
                        <span className="mono">{node.path}</span>
                        <span>read-only</span>
                    </div>
                    {source === undefined ? (
                        <div className="graph-preview-empty hint">Loading…</div>
                    ) : (
                        <CodeEditor
                            className="graph-preview-code"
                            disabled
                            language={codeLanguageForPath(node.path)}
                            onChange={() => {}}
                            value={source}
                        />
                    )}
                </div>
            )}
        </aside>
    );
}

/* --- deriving the graph from the semantic model --- */

function packageGraph(
    model: SemanticModel,
    hrefFor: (steps: AddressStep[]) => string,
): GraphData {
    // Variables another variable's rule condition reads are conditions even
    // when their own shape does not follow the convention.
    const conditionTargets = new Set<string>();
    for (const reference of model.references) {
        if (
            reference.via.kind === "ruleCondition" &&
            reference.from.kind === "variable" &&
            reference.to.kind === "variable" &&
            typeof reference.to.id === "string"
        ) {
            conditionTargets.add(reference.to.id);
        }
    }

    const nodes: GraphNode[] = [];
    const byId = new Map<string, GraphNode>();
    const pushNode = (node: GraphNode) => {
        if (!byId.has(node.id)) {
            byId.set(node.id, node);
            nodes.push(node);
        }
    };

    for (const variable of model.variables) {
        pushNode({
            id: `variable:${variable.id}`,
            kind: isCondition(variable, conditionTargets)
                ? "condition"
                : "variable",
            label: variable.id,
            variableId: variable.id,
            href: hrefFor([{ class: "variable", id: variable.id }]),
            path: variable.location.path,
        });
    }
    for (const list of model.lists) {
        pushNode({
            id: `list:${list.id}`,
            kind: "list",
            label: list.id,
            href: hrefFor([{ class: "list", id: list.id }]),
            path: `lists/${list.id}.toml`,
        });
    }
    for (const catalog of model.catalogs) {
        pushNode({
            id: `catalog:${catalog.id}`,
            kind: "catalog",
            label: catalog.id,
            href: hrefFor([{ class: "catalog", id: catalog.id }]),
            path: catalog.path,
        });
    }
    for (const entry of model.catalogEntries) {
        pushNode({
            id: `entry:${entry.catalog}:${entry.key}`,
            kind: "catalogEntry",
            label: entry.key,
            href: hrefFor([
                { class: "catalog", id: entry.catalog },
                { class: "entry", id: entry.key },
            ]),
            path: `data/catalogs/${entry.catalog}/${entry.key}.toml`,
        });
    }

    const edges: GraphEdge[] = [];
    const edgeByPair = new Map<string, GraphEdge>();
    const relatedByNode = new Map<string, Set<string>>();
    const pushEdge = (edge: GraphEdge) => {
        if (
            !byId.has(edge.from) ||
            !byId.has(edge.to) ||
            edge.from === edge.to
        ) {
            return;
        }
        const key = `${edge.from}->${edge.to}`;
        const existing = edgeByPair.get(key);
        if (existing === undefined) {
            edgeByPair.set(key, edge);
            edges.push(edge);
            return;
        }
        // One drawn edge per pair; lighting merges every rule that reads it.
        if (edge.ruleIndexes !== undefined) {
            existing.ruleIndexes = [
                ...(existing.ruleIndexes ?? []),
                ...edge.ruleIndexes,
            ];
        }
        existing.unconditional = existing.unconditional || edge.unconditional;
    };
    const pushRelated = (from: string, to: string) => {
        if (!byId.has(from) || !byId.has(to) || from === to) {
            return;
        }
        const related = relatedByNode.get(from) ?? new Set<string>();
        related.add(to);
        relatedByNode.set(from, related);
    };

    // A rule's condition relates to the catalog path that rule selects, so
    // hovering the condition reaches the value it would pick.
    const ruleConditionsByRule = new Map<string, string[]>();
    const ruleEntryByRule = new Map<
        string,
        { catalog: string; entry: string }
    >();

    for (const reference of model.references) {
        if (
            reference.from.kind !== "variable" ||
            typeof reference.from.id !== "string"
        ) {
            continue;
        }
        const readerId = reference.from.id;
        const readerNode = `variable:${readerId}`;
        const via = reference.via;
        const to = reference.to;
        if (to.kind === "variable" && typeof to.id === "string") {
            if (via.kind !== "ruleCondition" && via.kind !== "query") {
                continue;
            }
            const dependency = `variable:${to.id}`;
            const dependencyKind = byId.get(dependency)?.kind;
            pushEdge({
                from: dependency,
                to: readerNode,
                kind:
                    dependencyKind === "condition"
                        ? byId.get(readerNode)?.kind === "condition"
                            ? "requires"
                            : "checks"
                        : "reads",
                reader: readerId,
                ruleIndexes:
                    via.kind === "ruleCondition" &&
                    typeof via.index === "number"
                        ? [via.index]
                        : undefined,
                unconditional: via.kind === "query" ? true : undefined,
            });
            if (
                via.kind === "ruleCondition" &&
                typeof via.index === "number" &&
                dependencyKind === "condition"
            ) {
                const rule = `${readerId}:${via.index}`;
                const conditions = ruleConditionsByRule.get(rule) ?? [];
                conditions.push(dependency);
                ruleConditionsByRule.set(rule, conditions);
            }
        } else if (to.kind === "list" && typeof to.id === "string") {
            pushEdge({
                from: readerNode,
                to: `list:${to.id}`,
                kind: via.kind === "variableList" ? "selects" : "reads",
                reader: readerId,
                ruleIndexes:
                    via.kind === "ruleCondition" &&
                    typeof via.index === "number"
                        ? [via.index]
                        : undefined,
                unconditional:
                    via.kind === "variableList" || via.kind === "query"
                        ? true
                        : undefined,
            });
        } else if (to.kind === "catalog" && typeof to.id === "string") {
            pushEdge({
                from: readerNode,
                to: `catalog:${to.id}`,
                kind: "selects",
                reader: readerId,
                unconditional: true,
            });
        } else if (
            to.kind === "catalogEntry" &&
            typeof to["catalog"] === "string" &&
            typeof to["key"] === "string" &&
            (via.kind === "ruleValue" || via.kind === "resolveDefault")
        ) {
            // Selected values are not drawn as edges (the path goes through
            // the catalog) but hover highlighting should reach them.
            const entryNode = `entry:${to["catalog"]}:${to["key"]}`;
            pushRelated(readerNode, entryNode);
            if (via.kind === "ruleValue" && typeof via.index === "number") {
                ruleEntryByRule.set(`${readerId}:${via.index}`, {
                    catalog: to["catalog"],
                    entry: to["key"],
                });
            }
        }
    }

    for (const [rule, entry] of ruleEntryByRule) {
        for (const condition of ruleConditionsByRule.get(rule) ?? []) {
            pushRelated(condition, `catalog:${entry.catalog}`);
            pushRelated(condition, `entry:${entry.catalog}:${entry.entry}`);
        }
    }

    for (const entry of model.catalogEntries) {
        pushEdge({
            from: `catalog:${entry.catalog}`,
            to: `entry:${entry.catalog}:${entry.key}`,
            kind: "contains",
        });
    }

    for (const node of nodes) {
        const related = relatedByNode.get(node.id);
        if (related !== undefined && related.size > 0) {
            node.related = Array.from(related);
        }
    }

    return { nodes, edges };
}

// A named runtime condition by convention: a bool variable that defaults to
// false and whose rules only ever switch it on, or one another variable's
// rule condition already leans on.
function isCondition(
    variable: VariableModel,
    conditionTargets: Set<string>,
): boolean {
    if (
        variable.declaration.kind !== "primitive" ||
        variable.declaration.value !== "bool"
    ) {
        return false;
    }
    if (conditionTargets.has(variable.id)) {
        return true;
    }
    const resolve = variable.resolve;
    return (
        resolve !== undefined &&
        resolve.default?.value === false &&
        resolve.rules.length > 0 &&
        resolve.rules.every((rule) => rule.value?.value === true)
    );
}

function partitionGraph(data: GraphData): {
    components: GraphData[];
    isolated: GraphNode[];
} {
    const nodesById = new Map(data.nodes.map((node) => [node.id, node]));
    const indexById = new Map(
        data.nodes.map((node, index) => [node.id, index] as const),
    );
    const adjacency = new Map<string, Set<string>>();

    for (const node of data.nodes) {
        adjacency.set(node.id, new Set());
    }

    const connect = (left: string, right: string) => {
        if (left === right || !nodesById.has(left) || !nodesById.has(right)) {
            return;
        }
        adjacency.get(left)?.add(right);
        adjacency.get(right)?.add(left);
    };

    for (const edge of data.edges) {
        connect(edge.from, edge.to);
    }
    for (const node of data.nodes) {
        for (const related of node.related ?? []) {
            connect(node.id, related);
        }
    }

    const visited = new Set<string>();
    const components: GraphData[] = [];
    const isolated: GraphNode[] = [];

    for (const node of data.nodes) {
        if (visited.has(node.id)) {
            continue;
        }
        const ids: string[] = [];
        const stack = [node.id];
        visited.add(node.id);

        while (stack.length > 0) {
            const current = stack.pop();
            if (current === undefined) {
                continue;
            }
            ids.push(current);
            for (const next of adjacency.get(current) ?? []) {
                if (!visited.has(next)) {
                    visited.add(next);
                    stack.push(next);
                }
            }
        }

        const nodeIds = new Set(ids);
        const orderedNodes = ids
            .map((id) => nodesById.get(id))
            .filter((candidate): candidate is GraphNode => Boolean(candidate))
            .sort(
                (left, right) =>
                    (indexById.get(left.id) ?? 0) -
                    (indexById.get(right.id) ?? 0),
            );

        if (orderedNodes.length === 1 && adjacency.get(node.id)?.size === 0) {
            isolated.push(orderedNodes[0]);
            continue;
        }

        components.push({
            nodes: orderedNodes.map((componentNode) => ({
                ...componentNode,
                related: componentNode.related?.filter((related) =>
                    nodeIds.has(related),
                ),
            })),
            edges: data.edges.filter(
                (edge) => nodeIds.has(edge.from) && nodeIds.has(edge.to),
            ),
        });
    }

    return { components, isolated };
}

/* --- the layered-columns canvas --- */

const COLUMNS: Array<{ kind: GraphNodeKind; title: string }> = [
    { kind: "condition", title: "conditions" },
    { kind: "variable", title: "variables" },
    { kind: "list", title: "lists" },
    { kind: "catalog", title: "catalogs" },
    { kind: "catalogEntry", title: "values" },
];

const KIND_COLOR: Record<GraphNodeKind, string> = {
    condition: "var(--cyan-600)",
    variable: "var(--sea-600)",
    list: "var(--info-500)",
    catalog: "var(--ok-700)",
    catalogEntry: "var(--ink-1)",
};

const ROW_GAP = 8;
const CLUSTER_GAP = 36;
const HEADER_HEIGHT = 34;
const NODE_HEIGHT = 22;
const VALUE_NODE_HEIGHT = 40;
const PADDING = 6;
const MAX_COL_WIDTH = 220;
const MIN_COL_WIDTH = 110;
const CHAR_WIDTH = 6.6;

type EdgeState = "plain" | "fired" | "evaluated" | "dormant";

function ColumnsGraph({
    components,
    inspectedId,
    onInspect,
    outcomes,
    query = "",
}: {
    /* Connected components, drawn as clusters one after another. */
    components: GraphData[];
    inspectedId?: string | null;
    onInspect?: (node: GraphNode | null) => void;
    outcomes: Map<string, TraceOutcome> | null;
    query?: string;
}) {
    const containerRef = useRef<HTMLDivElement>(null);
    const merged = useMemo(
        () => ({
            nodes: components.flatMap((component) => component.nodes),
            edges: components.flatMap((component) => component.edges),
        }),
        [components],
    );
    const [active, setActive] = useState<string | null>(null);
    const [containerWidth, setContainerWidth] = useState(980);

    // Fit the columns to the container so the canvas never scrolls
    // horizontally; only the geometry adapts, the type size stays fixed.
    useEffect(() => {
        const element = containerRef.current;
        if (!element) {
            return;
        }
        const observer = new ResizeObserver((entries) => {
            const width = entries[0]?.contentRect.width;
            if (width) {
                setContainerWidth(width);
            }
        });
        observer.observe(element);
        return () => observer.disconnect();
    }, []);

    // Variable nodes grow a value line once a context is chosen.
    const withValues = outcomes !== null;

    const layout = useMemo(() => {
        const nodeHeight = (kind: GraphNodeKind): number =>
            withValues && (kind === "condition" || kind === "variable")
                ? VALUE_NODE_HEIGHT
                : NODE_HEIGHT;
        // Columns are shared across every cluster so the whole canvas reads
        // as one aligned diagram in resolution order.
        const columns = COLUMNS.filter((column) =>
            merged.nodes.some((node) => node.kind === column.kind),
        );
        const count = Math.max(columns.length, 1);
        const gap = Math.max(28, Math.min(56, containerWidth * 0.04));
        const colWidth = Math.max(
            MIN_COL_WIDTH,
            Math.min(
                MAX_COL_WIDTH,
                (containerWidth - PADDING * 2 - (count - 1) * gap) / count,
            ),
        );
        const positions = new Map<
            string,
            { x: number; y: number; column: number; height: number }
        >();
        // Clusters stack one after another, separated by a hairline.
        const dividers: number[] = [];
        let clusterTop = HEADER_HEIGHT;
        components.forEach((component, clusterIndex) => {
            if (clusterIndex > 0) {
                dividers.push(clusterTop - CLUSTER_GAP / 2);
            }
            let clusterBottom = clusterTop;
            columns.forEach((column, columnIndex) => {
                const height = nodeHeight(column.kind);
                const nodes = component.nodes.filter(
                    (node) => node.kind === column.kind,
                );
                nodes.forEach((node, rowIndex) => {
                    positions.set(node.id, {
                        x: PADDING + columnIndex * (colWidth + gap),
                        y: clusterTop + rowIndex * (height + ROW_GAP),
                        column: columnIndex,
                        height,
                    });
                });
                clusterBottom = Math.max(
                    clusterBottom,
                    clusterTop + nodes.length * (height + ROW_GAP) - ROW_GAP,
                );
            });
            clusterTop = clusterBottom + CLUSTER_GAP;
        });
        return {
            columns,
            positions,
            dividers,
            colWidth,
            gap,
            maxLabelChars: Math.max(
                8,
                Math.floor((colWidth - 30) / CHAR_WIDTH),
            ),
            width: PADDING * 2 + count * colWidth + (count - 1) * gap,
            height: clusterTop - CLUSTER_GAP + 8,
        };
    }, [containerWidth, components, merged, withValues]);

    const neighbors = useMemo(() => {
        const map = new Map<string, Set<string>>();
        const connect = (a: string, b: string) => {
            (map.get(a) ?? map.set(a, new Set()).get(a))?.add(b);
            (map.get(b) ?? map.set(b, new Set()).get(b))?.add(a);
        };
        for (const edge of merged.edges) {
            connect(edge.from, edge.to);
        }
        for (const node of merged.nodes) {
            for (const related of node.related ?? []) {
                connect(node.id, related);
            }
        }
        return map;
    }, [merged]);

    // Everything lit by the hovered node: itself, its edge neighbors, and
    // its related entities that complete a selected catalog path.
    const litNodes = useMemo(() => {
        if (!active) {
            return null;
        }
        return new Set([active, ...(neighbors.get(active) ?? [])]);
    }, [active, neighbors]);

    // Search highlights matches; everything else stays on the canvas, faded
    // but fully functional.
    const matches = useMemo(() => {
        if (!query) {
            return null;
        }
        return new Set(
            merged.nodes
                .filter((node) => node.label.toLowerCase().includes(query))
                .map((node) => node.id),
        );
    }, [merged, query]);

    const isDimmed = (id: string) =>
        (active !== null &&
            active !== id &&
            !(neighbors.get(active)?.has(id) ?? false)) ||
        (matches !== null && !matches.has(id));

    const handleHover = (id: string | null) => {
        setActive(id);
        if (id) {
            const node = merged.nodes.find((candidate) => candidate.id === id);
            if (node) {
                onInspect?.(node);
            }
        }
    };

    return (
        <div ref={containerRef} style={{ position: "relative" }}>
            <svg
                role="img"
                aria-label="Package reference graph"
                height={layout.height}
                style={{ display: "block", maxWidth: "100%" }}
                viewBox={`0 0 ${layout.width} ${layout.height}`}
                width={layout.width}
            >
                {merged.edges.map((edge, index) => {
                    const from = layout.positions.get(edge.from);
                    const to = layout.positions.get(edge.to);
                    if (!from || !to) {
                        return null;
                    }
                    const state = edgeState(edge, outcomes);
                    const lit =
                        litNodes !== null &&
                        litNodes.has(edge.from) &&
                        litNodes.has(edge.to);
                    const offSearch =
                        matches !== null &&
                        !matches.has(edge.from) &&
                        !matches.has(edge.to);
                    const y1 = from.y + from.height / 2;
                    const y2 = to.y + to.height / 2;
                    let path: string;
                    if (from.column === to.column) {
                        // Same-column references loop through the gap on the
                        // right so they never leave the viewport.
                        const x = from.x + layout.colWidth;
                        const bend = Math.min(
                            layout.gap - 6,
                            22 + Math.abs(y2 - y1) / 6,
                        );
                        path = `M ${x} ${y1} C ${x + bend} ${y1}, ${x + bend} ${y2}, ${x} ${y2}`;
                    } else {
                        const leftToRight = from.column < to.column;
                        const x1 = leftToRight
                            ? from.x + layout.colWidth
                            : from.x;
                        const x2 = leftToRight ? to.x : to.x + layout.colWidth;
                        const bend = Math.max(24, Math.abs(x2 - x1) / 2);
                        path = `M ${x1} ${y1} C ${x1 + (leftToRight ? bend : -bend)} ${y1}, ${
                            x2 + (leftToRight ? -bend : bend)
                        } ${y2}, ${x2} ${y2}`;
                    }
                    // Hover owns the only stroke change. The chosen context
                    // speaks through node values and the dimming of paths
                    // that never ran; a persistent stroke difference reads
                    // as a stuck highlight, whatever its color.
                    return (
                        <path
                            d={path}
                            fill="none"
                            key={index}
                            stroke={lit ? "var(--sea-500)" : "var(--line-2)"}
                            strokeWidth={lit ? 1.8 : 1}
                            opacity={
                                (active !== null && !lit) || offSearch
                                    ? 0.25
                                    : state === "dormant" && !lit
                                      ? 0.3
                                      : 1
                            }
                        >
                            <title>{edgeTitle(edge, state)}</title>
                        </path>
                    );
                })}
                {layout.columns.map((column, columnIndex) => (
                    <text
                        fill="var(--ink-2)"
                        fontSize={11}
                        key={column.kind}
                        letterSpacing="0.1em"
                        x={
                            PADDING +
                            columnIndex * (layout.colWidth + layout.gap)
                        }
                        y={14}
                    >
                        {column.title.toUpperCase()}
                    </text>
                ))}
                {layout.dividers.map((y) => (
                    <line
                        key={`divider-${y}`}
                        stroke="var(--line-1)"
                        strokeDasharray="3 5"
                        x1={PADDING}
                        x2={layout.width - PADDING}
                        y1={y}
                        y2={y}
                    />
                ))}
                {merged.nodes.map((node) => {
                    const position = layout.positions.get(node.id);
                    if (!position) {
                        return null;
                    }
                    return (
                        <GraphNodeBox
                            active={inspectedId === node.id}
                            dimmed={isDimmed(node.id)}
                            height={position.height}
                            highlighted={matches?.has(node.id) ?? false}
                            key={node.id}
                            maxLabelChars={layout.maxLabelChars}
                            node={node}
                            onHover={handleHover}
                            outcome={
                                outcomes !== null &&
                                node.variableId !== undefined
                                    ? (outcomes.get(node.variableId) ?? null)
                                    : null
                            }
                            showValue={
                                outcomes !== null &&
                                node.variableId !== undefined
                            }
                            width={layout.colWidth}
                            x={position.x}
                            y={position.y}
                        />
                    );
                })}
            </svg>
        </div>
    );
}

// Rules evaluate in order until the first match: the matching rule's reads
// fired, earlier rules' reads were evaluated, later ones never ran. Without
// a chosen context every edge is plain structure.
function edgeState(
    edge: GraphEdge,
    outcomes: Map<string, TraceOutcome> | null,
): EdgeState {
    if (outcomes === null || edge.reader === undefined) {
        return "plain";
    }
    const outcome = outcomes.get(edge.reader);
    if (outcome?.trace === undefined) {
        return "dormant";
    }
    if (edge.unconditional) {
        return "evaluated";
    }
    const matched = outcome.trace.rules.find((rule) => rule.matched);
    let state: EdgeState = "dormant";
    for (const index of edge.ruleIndexes ?? []) {
        if (matched === undefined || index < matched.index) {
            state = state === "fired" ? state : "evaluated";
        } else if (index === matched.index) {
            state = "fired";
        }
    }
    return state;
}

function edgeTitle(edge: GraphEdge, state: EdgeState): string {
    const from = edge.from.slice(edge.from.indexOf(":") + 1);
    const to = edge.to.slice(edge.to.indexOf(":") + 1);
    const sentence =
        edge.kind === "selects"
            ? `${from} selects from ${to}`
            : edge.kind === "contains"
              ? `${from} contains ${to}`
              : `${to} ${edge.kind} ${from}`;
    return state === "plain" ? sentence : `${sentence} (${state})`;
}

function GraphNodeBox({
    active,
    dimmed,
    height,
    highlighted,
    maxLabelChars,
    node,
    onHover,
    outcome,
    showValue,
    width,
    x,
    y,
}: {
    active: boolean;
    dimmed: boolean;
    height: number;
    highlighted: boolean;
    maxLabelChars: number;
    node: GraphNode;
    onHover: (id: string | null) => void;
    outcome: TraceOutcome | null;
    showValue: boolean;
    width: number;
    x: number;
    y: number;
}) {
    const label =
        node.label.length > maxLabelChars
            ? `${node.label.slice(0, Math.max(1, maxLabelChars - 1))}…`
            : node.label;
    const error = outcome?.error !== undefined;
    const value = !showValue
        ? null
        : outcome === null
          ? ""
          : error
            ? "cannot resolve"
            : clip(
                  outcome.trace !== undefined
                      ? resolvedValueText(outcome.trace)
                      : "",
                  maxLabelChars,
              );
    const open = () => {
        navigate(node.href.startsWith("#") ? node.href.slice(1) : node.href);
    };
    return (
        <g
            aria-label={`${KIND_LABEL[node.kind]} ${node.label}`}
            onClick={open}
            onFocus={() => onHover(node.id)}
            onBlur={() => onHover(null)}
            onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    open();
                }
            }}
            onMouseEnter={() => onHover(node.id)}
            onMouseLeave={() => onHover(null)}
            opacity={dimmed ? 0.3 : 1}
            role="link"
            style={{ cursor: "pointer" }}
            tabIndex={0}
        >
            <rect
                fill={
                    active
                        ? "var(--ink-0)"
                        : highlighted
                          ? "var(--sea-50)"
                          : "var(--paper-1)"
                }
                height={height}
                rx={5}
                stroke={
                    active
                        ? "var(--ink-0)"
                        : highlighted
                          ? "var(--sea-400)"
                          : error
                            ? "var(--warn-500)"
                            : "var(--line-2)"
                }
                strokeDasharray={error && !active ? "4 3" : undefined}
                strokeWidth={active || highlighted || error ? 1.4 : 1}
                width={width}
                x={x}
                y={y}
            />
            <circle
                cx={x + 11}
                cy={showValue ? y + 12 : y + height / 2}
                fill={active ? "var(--paper-0)" : KIND_COLOR[node.kind]}
                r={3}
            />
            <text
                fill={active ? "var(--paper-0)" : "var(--ink-0)"}
                fontFamily="var(--font-mono), ui-monospace, monospace"
                fontSize={11.5}
                x={x + 21}
                y={showValue ? y + 16 : y + height / 2 + 4}
            >
                {label}
            </text>
            {value !== null && value !== "" ? (
                <text
                    fill={
                        active
                            ? "var(--paper-0)"
                            : error
                              ? "var(--warn-700)"
                              : "var(--sea-700)"
                    }
                    fontFamily="var(--font-mono), ui-monospace, monospace"
                    fontSize={10.5}
                    x={x + 21}
                    y={y + 31}
                >
                    {value}
                </text>
            ) : null}
            <title>
                {node.label}
                {outcome?.error !== undefined
                    ? `\n${outcome.error}`
                    : outcome?.trace !== undefined
                      ? `\n= ${resolvedValueText(outcome.trace)}`
                      : ""}
            </title>
        </g>
    );
}

function clip(text: string, length: number): string {
    return text.length > length ? `${text.slice(0, length)}…` : text;
}
