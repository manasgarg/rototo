"use client";

import { useRouter } from "next/navigation";
import { useMemo, useState } from "react";
import type { GraphNode, GraphNodeKind, WorkspaceGraphData } from "./types";

/* Concept: layered columns. Entities group into columns by kind in
   resolution order — qualifiers feed variables, variables select objects,
   objects and variables validate against schemas — with linters alongside.
   Hovering a node lights up its edges and neighbors and previews its source;
   clicking opens the entity. */

const COLUMNS: Array<{ kind: GraphNodeKind; title: string }> = [
  { kind: "qualifier", title: "qualifiers" },
  { kind: "variable", title: "variables" },
  { kind: "resource", title: "resources" },
  { kind: "resourceObject", title: "objects" },
  { kind: "schema", title: "schemas" },
  { kind: "linter", title: "linters" },
];

const KIND_COLOR: Record<GraphNodeKind, string> = {
  qualifier: "var(--cyan-600)",
  variable: "var(--sea-600)",
  resource: "var(--ok-700)",
  resourceObject: "var(--ink-1)",
  schema: "var(--info-700)",
  linter: "var(--warn-700)",
};

const COL_WIDTH = 200;
const COL_GAP = 56;
const ROW_HEIGHT = 30;
const HEADER_HEIGHT = 34;
const NODE_HEIGHT = 22;
const PADDING = 6;

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
  const [active, setActive] = useState<string | null>(null);

  const layout = useMemo(() => {
    const columns = COLUMNS.map((column) => ({
      ...column,
      nodes: data.nodes.filter((node) => node.kind === column.kind),
    })).filter((column) => column.nodes.length > 0);
    const positions = new Map<string, { x: number; y: number; column: number }>();
    columns.forEach((column, columnIndex) => {
      column.nodes.forEach((node, rowIndex) => {
        positions.set(node.id, {
          x: PADDING + columnIndex * (COL_WIDTH + COL_GAP),
          y: HEADER_HEIGHT + rowIndex * ROW_HEIGHT,
          column: columnIndex,
        });
      });
    });
    const rows = Math.max(...columns.map((column) => column.nodes.length), 1);
    return {
      columns,
      positions,
      width: PADDING * 2 + columns.length * COL_WIDTH + (columns.length - 1) * COL_GAP,
      height: HEADER_HEIGHT + rows * ROW_HEIGHT + 8,
    };
  }, [data]);

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
  // related entities (a variable's selected objects).
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
    (active !== null && active !== id && !(neighbors.get(active)?.has(id) ?? false)) ||
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
    <div style={{ position: "relative" }}>
      <svg
        role="img"
        aria-label="Workspace entity graph"
        height={layout.height}
        style={{ display: "block" }}
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
            litNodes !== null && litNodes.has(edge.from) && litNodes.has(edge.to);
          const offSearch =
            matches !== null && !matches.has(edge.from) && !matches.has(edge.to);
          const y1 = from.y + NODE_HEIGHT / 2;
          const y2 = to.y + NODE_HEIGHT / 2;
          let path: string;
          if (from.column === to.column) {
            // Same-column references loop through the gap on the right so
            // they never leave the viewport.
            const x = from.x + COL_WIDTH;
            const bend = Math.min(COL_GAP - 6, 22 + Math.abs(y2 - y1) / 6);
            path = `M ${x} ${y1} C ${x + bend} ${y1}, ${x + bend} ${y2}, ${x} ${y2}`;
          } else {
            const leftToRight = from.column < to.column;
            const x1 = leftToRight ? from.x + COL_WIDTH : from.x;
            const x2 = leftToRight ? to.x : to.x + COL_WIDTH;
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
              opacity={(active !== null && !lit) || offSearch ? 0.25 : 1}
            />
          );
        })}
        {layout.columns.map((column, columnIndex) => (
          <text
            fill="var(--ink-2)"
            fontSize={11}
            key={column.kind}
            letterSpacing="0.1em"
            x={PADDING + columnIndex * (COL_WIDTH + COL_GAP)}
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
                node={node}
                onHover={handleHover}
                onOpen={() => router.push(node.href)}
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
  node,
  onHover,
  onOpen,
  x,
  y,
}: {
  dimmed: boolean;
  highlighted: boolean;
  node: GraphNode;
  onHover: (id: string | null) => void;
  onOpen: () => void;
  x: number;
  y: number;
}) {
  const label = node.label.length > 24 ? `${node.label.slice(0, 23)}…` : node.label;
  return (
    <g
      onClick={onOpen}
      onMouseEnter={() => onHover(node.id)}
      onMouseLeave={() => onHover(null)}
      opacity={dimmed ? 0.3 : 1}
      style={{ cursor: "pointer" }}
    >
      <rect
        fill={highlighted ? "var(--sea-50)" : "var(--paper-1)"}
        height={NODE_HEIGHT}
        rx={5}
        stroke={highlighted ? "var(--sea-400)" : "var(--line-2)"}
        strokeWidth={highlighted ? 1.4 : 1}
        width={COL_WIDTH}
        x={x}
        y={y}
      />
      <circle cx={x + 11} cy={y + NODE_HEIGHT / 2} fill={KIND_COLOR[node.kind]} r={3} />
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
