"use client";

import { useRouter } from "next/navigation";
import { useMemo, useState } from "react";
import type { GraphNode, GraphNodeKind, WorkspaceGraphData } from "./types";

/* Concept: layered columns. Entities group into columns by kind in
   resolution order — qualifiers feed variables, variables select objects,
   objects and variables validate against schemas — with linters alongside.
   Hovering a node lights up its edges and neighbors; clicking opens the
   entity. */

const COLUMNS: Array<{ kind: GraphNodeKind; title: string }> = [
  { kind: "qualifier", title: "qualifiers" },
  { kind: "variable", title: "variables" },
  { kind: "resourceObject", title: "objects" },
  { kind: "schema", title: "schemas" },
  { kind: "linter", title: "linters" },
];

const KIND_COLOR: Record<GraphNodeKind, string> = {
  qualifier: "var(--cyan-600)",
  variable: "var(--sea-600)",
  resourceObject: "var(--ink-1)",
  schema: "var(--info-700)",
  linter: "var(--warn-700)",
};

const COL_WIDTH = 200;
const COL_GAP = 56;
const ROW_HEIGHT = 30;
const HEADER_HEIGHT = 34;
const NODE_HEIGHT = 22;

export function ColumnsGraph({ data }: { data: WorkspaceGraphData }) {
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
          x: columnIndex * (COL_WIDTH + COL_GAP),
          y: HEADER_HEIGHT + rowIndex * ROW_HEIGHT,
          column: columnIndex,
        });
      });
    });
    const rows = Math.max(...columns.map((column) => column.nodes.length), 1);
    return {
      columns,
      positions,
      width: columns.length * COL_WIDTH + (columns.length - 1) * COL_GAP,
      height: HEADER_HEIGHT + rows * ROW_HEIGHT + 8,
    };
  }, [data]);

  const neighbors = useMemo(() => {
    const map = new Map<string, Set<string>>();
    for (const edge of data.edges) {
      (map.get(edge.from) ?? map.set(edge.from, new Set()).get(edge.from))?.add(edge.to);
      (map.get(edge.to) ?? map.set(edge.to, new Set()).get(edge.to))?.add(edge.from);
    }
    return map;
  }, [data]);

  const isDimmed = (id: string) =>
    active !== null && active !== id && !(neighbors.get(active)?.has(id) ?? false);

  return (
    <svg
      role="img"
      aria-label="Workspace entity graph"
      style={{ width: "100%", height: "auto", display: "block" }}
      viewBox={`0 0 ${layout.width} ${layout.height}`}
    >
      {data.edges.map((edge, index) => {
        const from = layout.positions.get(edge.from);
        const to = layout.positions.get(edge.to);
        if (!from || !to) {
          return null;
        }
        const lit = active === edge.from || active === edge.to;
        const leftToRight = from.column <= to.column;
        const x1 = leftToRight ? from.x + COL_WIDTH : from.x;
        const x2 = leftToRight ? to.x : to.x + COL_WIDTH;
        const y1 = from.y + NODE_HEIGHT / 2;
        const y2 = to.y + NODE_HEIGHT / 2;
        const bend = Math.max(24, Math.abs(x2 - x1) / 2);
        return (
          <path
            d={`M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`}
            fill="none"
            key={index}
            stroke={lit ? "var(--sea-500)" : "var(--line-2)"}
            strokeWidth={lit ? 1.8 : 1}
            opacity={active !== null && !lit ? 0.25 : 1}
          />
        );
      })}
      {layout.columns.map((column, columnIndex) => (
        <text
          className="graph-column-title"
          fill="var(--ink-2)"
          fontSize={11}
          key={column.kind}
          letterSpacing="0.1em"
          x={columnIndex * (COL_WIDTH + COL_GAP)}
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
              key={node.id}
              node={node}
              onActivate={setActive}
              onOpen={() => router.push(node.href)}
              x={position.x}
              y={position.y}
            />
          );
        }),
      )}
    </svg>
  );
}

function GraphNodeBox({
  dimmed,
  node,
  onActivate,
  onOpen,
  x,
  y,
}: {
  dimmed: boolean;
  node: GraphNode;
  onActivate: (id: string | null) => void;
  onOpen: () => void;
  x: number;
  y: number;
}) {
  const label = node.label.length > 24 ? `${node.label.slice(0, 23)}…` : node.label;
  return (
    <g
      onClick={onOpen}
      onMouseEnter={() => onActivate(node.id)}
      onMouseLeave={() => onActivate(null)}
      opacity={dimmed ? 0.3 : 1}
      style={{ cursor: "pointer" }}
    >
      <title>{node.label}</title>
      <rect
        fill="var(--paper-1)"
        height={NODE_HEIGHT}
        rx={5}
        stroke="var(--line-2)"
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
