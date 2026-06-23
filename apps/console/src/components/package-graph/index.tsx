import { Link } from "@/lib/link";
import { ReadOnlySource } from "@/components/read-only-source";
import { Search } from "lucide-react";
import { useMemo, useState, type ComponentType } from "react";
import { ColumnsGraph } from "./columns";
import type { GraphNode, PackageGraphData } from "./types";

export type { PackageGraphData } from "./types";

/* The concept registry. A graph concept is a client component taking
   PackageGraphData, the active search query (matching entities highlight,
   everything else stays functional), and an optional onInspect callback —
   add an entry here to try a new visualization; with more than one
   registered, the card grows a switcher so concepts can be compared live. */
const CONCEPTS: Array<{
    id: string;
    label: string;
    Component: ComponentType<{
        data: PackageGraphData;
        query?: string;
        inspectedId?: string | null;
        currentEntityId?: string | null;
        onInspect?: (node: GraphNode | null) => void;
    }>;
}> = [{ id: "columns", label: "Columns", Component: ColumnsGraph }];

const KIND_LABEL: Record<GraphNode["kind"], string> = {
    requestContext: "request context",
    qualifier: "qualifier",
    variable: "variable",
    catalog: "catalog",
    catalogEntry: "value",
};

export function PackageGraph({
    data,
    currentEntityId = null,
    showToolbar = true,
}: {
    data: PackageGraphData;
    currentEntityId?: string | null;
    showToolbar?: boolean;
}) {
    const [conceptId, setConceptId] = useState(CONCEPTS[0].id);
    const [query, setQuery] = useState("");
    const [inspectedId, setInspectedId] = useState<string | null>(null);
    const concept =
        CONCEPTS.find((candidate) => candidate.id === conceptId) ?? CONCEPTS[0];
    const Active = concept.Component;

    const needle = query.trim().toLowerCase();
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
        if (node) {
            setInspectedId(node.id);
        }
    };

    return (
        <div className="graph-frame">
            {showToolbar ? (
                <div className="graph-toolbar">
                    <label className="search-control graph-search">
                        <span className="search-icon">
                            <Search aria-hidden size={15} />
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
                    {CONCEPTS.length > 1 ? (
                        <div
                            className="segmented-control"
                            role="tablist"
                            aria-label="Graph view"
                        >
                            {CONCEPTS.map((candidate) => (
                                <button
                                    aria-selected={candidate.id === concept.id}
                                    className={
                                        candidate.id === concept.id
                                            ? "active"
                                            : ""
                                    }
                                    key={candidate.id}
                                    onClick={() => setConceptId(candidate.id)}
                                    role="tab"
                                    type="button"
                                >
                                    {candidate.label}
                                </button>
                            ))}
                        </div>
                    ) : null}
                </div>
            ) : null}
            {hasEntities ? (
                <div className="graph-visuals">
                    <div className="graph-canvases">
                        {partitions.components.map((component, index) => {
                            const previewNode =
                                inspectedNode &&
                                component.nodes.some(
                                    (node) => node.id === inspectedNode.id,
                                )
                                    ? inspectedNode
                                    : null;
                            return (
                                <section
                                    aria-label={`Connected entity graph ${index + 1}`}
                                    className="graph-canvas"
                                    key={component.nodes
                                        .map((node) => node.id)
                                        .join("|")}
                                >
                                    {partitions.components.length > 1 ? (
                                        <div className="graph-canvas-head">
                                            <span className="label">
                                                Connection {index + 1}
                                            </span>
                                            <span className="hint">
                                                {component.nodes.length}{" "}
                                                {component.nodes.length === 1
                                                    ? "entity"
                                                    : "entities"}
                                            </span>
                                        </div>
                                    ) : null}
                                    <div
                                        className={
                                            previewNode
                                                ? "graph-canvas-body has-preview"
                                                : "graph-canvas-body"
                                        }
                                    >
                                        <div className="graph-scroll">
                                            <Active
                                                currentEntityId={
                                                    currentEntityId
                                                }
                                                data={component}
                                                inspectedId={inspectedId}
                                                onInspect={inspect}
                                                query={needle}
                                            />
                                        </div>
                                        <GraphPreview node={previewNode} />
                                    </div>
                                </section>
                            );
                        })}
                    </div>
                    {partitions.isolated.length > 0 ? (
                        <section
                            aria-label="Unconnected entities"
                            className="graph-isolated"
                        >
                            <div className="graph-isolated-head">
                                <span className="label">
                                    Unconnected entities
                                </span>
                                <span className="hint">
                                    {partitions.isolated.length}{" "}
                                    {partitions.isolated.length === 1
                                        ? "entity"
                                        : "entities"}{" "}
                                    with no links
                                </span>
                            </div>
                            <div
                                className={
                                    inspectedNode &&
                                    partitions.isolated.some(
                                        (node) => node.id === inspectedNode.id,
                                    )
                                        ? "graph-isolated-body has-preview"
                                        : "graph-isolated-body"
                                }
                            >
                                <div className="graph-isolated-list">
                                    {partitions.isolated.map((node) => {
                                        const matches =
                                            needle.length === 0 ||
                                            node.label
                                                .toLowerCase()
                                                .includes(needle);
                                        const className = [
                                            "graph-isolated-node",
                                            inspectedNode?.id === node.id
                                                ? "is-active"
                                                : "",
                                            needle.length > 0 && matches
                                                ? "is-highlighted"
                                                : "",
                                            needle.length > 0 && !matches
                                                ? "is-dimmed"
                                                : "",
                                        ]
                                            .filter(Boolean)
                                            .join(" ");
                                        return (
                                            <Link
                                                className={className}
                                                href={node.href}
                                                key={node.id}
                                                onFocus={() => inspect(node)}
                                                onMouseEnter={() =>
                                                    inspect(node)
                                                }
                                            >
                                                <span
                                                    className="graph-isolated-kind"
                                                    data-kind={node.kind}
                                                >
                                                    {KIND_LABEL[node.kind]}
                                                </span>
                                                <span className="mono">
                                                    {node.label}
                                                </span>
                                                {node.edited ? (
                                                    <span className="graph-edited-mark">
                                                        ~
                                                    </span>
                                                ) : null}
                                            </Link>
                                        );
                                    })}
                                </div>
                                <GraphPreview
                                    node={
                                        inspectedNode &&
                                        partitions.isolated.some(
                                            (node) =>
                                                node.id === inspectedNode.id,
                                        )
                                            ? inspectedNode
                                            : null
                                    }
                                />
                            </div>
                        </section>
                    ) : null}
                </div>
            ) : (
                <div className="graph-empty hint">
                    No graph entities are declared in this package.
                </div>
            )}
        </div>
    );
}

function GraphPreview({ node }: { node: GraphNode | null }) {
    if (!node) {
        return null;
    }
    return (
        <aside className="graph-preview" aria-live="polite">
            <div className="graph-preview-head">
                <div className="graph-preview-title">
                    <span className="label">{KIND_LABEL[node.kind]}</span>
                    <h4 className="mono">{node.label}</h4>
                </div>
                {node.edited ? (
                    <span className="graph-edited-mark">~</span>
                ) : null}
            </div>
            {node.source ? (
                <div className="codewell-frame graph-preview-source">
                    <div className="codehead">
                        <span>{node.path}</span>
                        <span>read-only</span>
                    </div>
                    <ReadOnlySource
                        language={node.language ?? "text"}
                        marks={[]}
                        text={node.source}
                    />
                </div>
            ) : (
                <div className="graph-preview-empty hint">
                    Definition unavailable.
                </div>
            )}
        </aside>
    );
}

function partitionGraph(data: PackageGraphData): {
    components: PackageGraphData[];
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
    const components: PackageGraphData[] = [];
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
            if (!current) {
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
