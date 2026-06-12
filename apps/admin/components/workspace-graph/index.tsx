"use client";

import Link from "next/link";
import { Search } from "lucide-react";
import { useMemo, useState, type ComponentType } from "react";
import { ReadOnlySource } from "../read-only-source";
import { ColumnsGraph } from "./columns";
import type { GraphNode, WorkspaceGraphData } from "./types";

export type { WorkspaceGraphData } from "./types";

/* The concept registry. A graph concept is a client component taking
   WorkspaceGraphData (plus an optional onInspect callback for the docked
   source preview) — add an entry here to try a new visualization; with more
   than one registered, the card grows a switcher so concepts can be compared
   live. */
const CONCEPTS: Array<{
  id: string;
  label: string;
  Component: ComponentType<{
    data: WorkspaceGraphData;
    onInspect?: (node: GraphNode | null) => void;
  }>;
}> = [{ id: "columns", label: "Columns", Component: ColumnsGraph }];

export function WorkspaceGraph({ data }: { data: WorkspaceGraphData }) {
  const [conceptId, setConceptId] = useState(CONCEPTS[0].id);
  const [query, setQuery] = useState("");
  const [inspected, setInspected] = useState<GraphNode | null>(null);
  const concept = CONCEPTS.find((candidate) => candidate.id === conceptId) ?? CONCEPTS[0];
  const Active = concept.Component;

  // Filtering happens above the concepts so every visualization gets it:
  // only matching entities stay, with the edges that connect them.
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) {
      return data;
    }
    const nodes = data.nodes.filter((node) => node.label.toLowerCase().includes(needle));
    const visible = new Set(nodes.map((node) => node.id));
    return {
      nodes,
      edges: data.edges.filter((edge) => visible.has(edge.from) && visible.has(edge.to)),
    };
  }, [data, query]);

  return (
    <div className="graph-frame">
      <div className="graph-toolbar">
        <label className="search-control graph-search">
          <span className="search-icon">
            <Search aria-hidden size={15} />
          </span>
          <input
            aria-label="Filter graph entities"
            className="input"
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Filter entities"
            type="search"
            value={query}
          />
        </label>
        {CONCEPTS.length > 1 ? (
          <div className="segmented-control" role="tablist" aria-label="Graph view">
            {CONCEPTS.map((candidate) => (
              <button
                aria-selected={candidate.id === concept.id}
                className={candidate.id === concept.id ? "active" : ""}
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
      {filtered.nodes.length === 0 ? (
        <p className="muted" style={{ fontSize: 13 }}>
          No entities match that filter.
        </p>
      ) : (
        <div className="graph-body">
          <div className="graph-scroll">
            <Active data={filtered} onInspect={setInspected} />
          </div>
          <aside className="graph-inspector">
            {inspected ? (
              <>
                <div className="graph-inspector-head">
                  <span className="label">{inspected.kind}</span>
                  <span className="mono graph-inspector-name">{inspected.label}</span>
                  <Link className="graph-inspector-open" href={inspected.href}>
                    open →
                  </Link>
                </div>
                {inspected.source ? (
                  <div className="graph-inspector-source">
                    <ReadOnlySource
                      language={inspected.language ?? "text"}
                      marks={[]}
                      text={inspected.source}
                    />
                  </div>
                ) : (
                  <p className="muted" style={{ fontSize: 13 }}>
                    No source preview for this entity.
                  </p>
                )}
              </>
            ) : (
              <p className="muted graph-inspector-empty">
                Hover an entity to preview its source here without leaving the graph.
              </p>
            )}
          </aside>
        </div>
      )}
    </div>
  );
}
