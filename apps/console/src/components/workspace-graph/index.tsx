import { Search } from "lucide-react";
import { useState, type ComponentType } from "react";
import { ColumnsGraph } from "./columns";
import type { GraphNode, WorkspaceGraphData } from "./types";

export type { WorkspaceGraphData } from "./types";

/* The concept registry. A graph concept is a client component taking
   WorkspaceGraphData, the active search query (matching entities highlight,
   everything else stays functional), and an optional onInspect callback —
   add an entry here to try a new visualization; with more than one
   registered, the card grows a switcher so concepts can be compared live. */
const CONCEPTS: Array<{
  id: string;
  label: string;
  Component: ComponentType<{
    data: WorkspaceGraphData;
    query?: string;
    onInspect?: (node: GraphNode | null) => void;
  }>;
}> = [{ id: "columns", label: "Columns", Component: ColumnsGraph }];

export function WorkspaceGraph({ data }: { data: WorkspaceGraphData }) {
  const [conceptId, setConceptId] = useState(CONCEPTS[0].id);
  const [query, setQuery] = useState("");
  const concept = CONCEPTS.find((candidate) => candidate.id === conceptId) ?? CONCEPTS[0];
  const Active = concept.Component;

  const needle = query.trim().toLowerCase();

  return (
    <div className="graph-frame">
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
      <div className="graph-scroll">
        <Active data={data} query={needle} />
      </div>
    </div>
  );
}
