"use client";

import { useState, type ComponentType } from "react";
import { ColumnsGraph } from "./columns";
import type { WorkspaceGraphData } from "./types";

export type { WorkspaceGraphData } from "./types";

/* The concept registry. A graph concept is a client component taking
   WorkspaceGraphData — add an entry here to try a new visualization; with
   more than one registered, the card grows a switcher so concepts can be
   compared live. */
const CONCEPTS: Array<{
  id: string;
  label: string;
  Component: ComponentType<{ data: WorkspaceGraphData }>;
}> = [{ id: "columns", label: "Columns", Component: ColumnsGraph }];

export function WorkspaceGraph({ data }: { data: WorkspaceGraphData }) {
  const [conceptId, setConceptId] = useState(CONCEPTS[0].id);
  const concept = CONCEPTS.find((candidate) => candidate.id === conceptId) ?? CONCEPTS[0];
  const Active = concept.Component;
  return (
    <div className="graph-frame">
      {CONCEPTS.length > 1 ? (
        <div className="segmented-control graph-switcher" role="tablist" aria-label="Graph view">
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
      <Active data={data} />
    </div>
  );
}
