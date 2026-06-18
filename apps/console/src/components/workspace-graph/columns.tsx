import { useRouter } from "@/lib/navigation";
import { useEffect, useMemo, useRef, useState } from "react";
import type { GraphNode, GraphNodeKind, WorkspaceGraphData } from "./types";

/* Concept: layered columns. Entities group into columns by kind in
   resolution order — qualifiers feed variables, variables select catalogs,
   and catalogs contain values. Hovering a node lights up its edges and
   neighbors and previews its source; clicking opens the entity. */

const COLUMNS: Array<{ kind: GraphNodeKind; title: string }> = [
    { kind: "qualifier", title: "qualifiers" },
    { kind: "variable", title: "variables" },
    { kind: "catalog", title: "catalogs" },
    { kind: "catalogEntry", title: "values" },
];

const KIND_COLOR: Record<GraphNodeKind, string> = {
    qualifier: "var(--cyan-600)",
    variable: "var(--sea-600)",
    catalog: "var(--ok-700)",
    catalogEntry: "var(--ink-1)",
};

const ROW_HEIGHT = 30;
const HEADER_HEIGHT = 34;
const NODE_HEIGHT = 22;
const PADDING = 6;
const MAX_COL_WIDTH = 220;
const MIN_COL_WIDTH = 110;
const CHAR_WIDTH = 6.6;

export function ColumnsGraph({
    data,
    onInspect,
    query = "",
}: {
    data: WorkspaceGraphData;
    onInspect?: (node: GraphNode | null) => void;
    query?: string;
}) {
    const router = useRouter();
    const containerRef = useRef<HTMLDivElement>(null);
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

    const layout = useMemo(() => {
        const columns = COLUMNS.map((column) => ({
            ...column,
            nodes: data.nodes.filter((node) => node.kind === column.kind),
        })).filter((column) => column.nodes.length > 0);
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
            { x: number; y: number; column: number }
        >();
        columns.forEach((column, columnIndex) => {
            column.nodes.forEach((node, rowIndex) => {
                positions.set(node.id, {
                    x: PADDING + columnIndex * (colWidth + gap),
                    y: HEADER_HEIGHT + rowIndex * ROW_HEIGHT,
                    column: columnIndex,
                });
            });
        });
        const rows = Math.max(
            ...columns.map((column) => column.nodes.length),
            1,
        );
        return {
            columns,
            positions,
            colWidth,
            gap,
            maxLabelChars: Math.max(
                8,
                Math.floor((colWidth - 30) / CHAR_WIDTH),
            ),
            width: PADDING * 2 + count * colWidth + (count - 1) * gap,
            height: HEADER_HEIGHT + rows * ROW_HEIGHT + 8,
        };
    }, [containerWidth, data]);

    const neighbors = useMemo(() => {
        const map = new Map<string, Set<string>>();
        const connect = (a: string, b: string) => {
            (map.get(a) ?? map.set(a, new Set()).get(a))?.add(b);
            (map.get(b) ?? map.set(b, new Set()).get(b))?.add(a);
        };
        for (const edge of data.edges) {
            connect(edge.from, edge.to);
        }
        for (const node of data.nodes) {
            for (const related of node.related ?? []) {
                connect(node.id, related);
            }
        }
        return map;
    }, [data]);

    // Everything lit by the hovered node: itself, its edge neighbors, and its
    // related entities that complete a selected catalog path.
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
            data.nodes
                .filter((node) => node.label.toLowerCase().includes(query))
                .map((node) => node.id),
        );
    }, [data, query]);

    const isDimmed = (id: string) =>
        (active !== null &&
            active !== id &&
            !(neighbors.get(active)?.has(id) ?? false)) ||
        (matches !== null && !matches.has(id));

    const handleHover = (id: string | null) => {
        setActive(id);
        if (id) {
            const node = data.nodes.find((candidate) => candidate.id === id);
            if (node) {
                onInspect?.(node);
            }
        }
    };

    return (
        <div ref={containerRef} style={{ position: "relative" }}>
            <svg
                role="img"
                aria-label="Workspace entity graph"
                height={layout.height}
                style={{ display: "block", maxWidth: "100%" }}
                viewBox={`0 0 ${layout.width} ${layout.height}`}
                width={layout.width}
            >
                {data.edges.map((edge, index) => {
                    const from = layout.positions.get(edge.from);
                    const to = layout.positions.get(edge.to);
                    if (!from || !to) {
                        return null;
                    }
                    const lit =
                        litNodes !== null &&
                        litNodes.has(edge.from) &&
                        litNodes.has(edge.to);
                    const offSearch =
                        matches !== null &&
                        !matches.has(edge.from) &&
                        !matches.has(edge.to);
                    const y1 = from.y + NODE_HEIGHT / 2;
                    const y2 = to.y + NODE_HEIGHT / 2;
                    let path: string;
                    if (from.column === to.column) {
                        // Same-column references loop through the gap on the right so
                        // they never leave the viewport.
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
                                    : 1
                            }
                        />
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
                {layout.columns.flatMap((column) =>
                    column.nodes.map((node) => {
                        const position = layout.positions.get(node.id);
                        if (!position) {
                            return null;
                        }
                        return (
                            <GraphNodeBox
                                dimmed={isDimmed(node.id)}
                                highlighted={matches?.has(node.id) ?? false}
                                key={node.id}
                                maxLabelChars={layout.maxLabelChars}
                                node={node}
                                onHover={handleHover}
                                onOpen={() => router.push(node.href)}
                                width={layout.colWidth}
                                x={position.x}
                                y={position.y}
                            />
                        );
                    }),
                )}
            </svg>
        </div>
    );
}

function GraphNodeBox({
    dimmed,
    highlighted,
    maxLabelChars,
    node,
    onHover,
    onOpen,
    width,
    x,
    y,
}: {
    dimmed: boolean;
    highlighted: boolean;
    maxLabelChars: number;
    node: GraphNode;
    onHover: (id: string | null) => void;
    onOpen: () => void;
    width: number;
    x: number;
    y: number;
}) {
    const label =
        node.label.length > maxLabelChars
            ? `${node.label.slice(0, Math.max(1, maxLabelChars - 1))}…`
            : node.label;
    return (
        <g
            onClick={onOpen}
            onMouseEnter={() => onHover(node.id)}
            onMouseLeave={() => onHover(null)}
            opacity={dimmed ? 0.3 : 1}
            style={{ cursor: "pointer" }}
        >
            <rect
                fill={
                    highlighted
                        ? "var(--sea-50)"
                        : node.edited
                          ? "var(--warn-bg)"
                          : "var(--paper-1)"
                }
                height={NODE_HEIGHT}
                rx={5}
                stroke={
                    highlighted
                        ? "var(--sea-400)"
                        : node.edited
                          ? "var(--warn-500)"
                          : "var(--line-2)"
                }
                strokeWidth={highlighted || node.edited ? 1.4 : 1}
                width={width}
                x={x}
                y={y}
            />
            <circle
                cx={x + 11}
                cy={y + NODE_HEIGHT / 2}
                fill={KIND_COLOR[node.kind]}
                r={3}
            />
            {node.edited ? (
                <text
                    fill="var(--warn-700)"
                    fontFamily="var(--font-mono), ui-monospace, monospace"
                    fontSize={12}
                    fontWeight={700}
                    textAnchor="end"
                    x={x + width - 8}
                    y={y + NODE_HEIGHT / 2 + 4}
                >
                    ~
                </text>
            ) : null}
            <text
                fill="var(--ink-0)"
                fontFamily="var(--font-mono), ui-monospace, monospace"
                fontSize={11.5}
                x={x + 21}
                y={y + NODE_HEIGHT / 2 + 4}
            >
                {label}
            </text>
        </g>
    );
}
