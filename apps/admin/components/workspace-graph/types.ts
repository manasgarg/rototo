/* The graph data contract. The server builds this once from the semantic
   model; rendering concepts consume it. Keep this stable so new graph
   concepts are plug-and-play — a concept is just a client component taking
   WorkspaceGraphData. */

export type GraphNodeKind =
  | "qualifier"
  | "variable"
  | "resource"
  | "resourceObject"
  | "schema"
  | "linter";

export type GraphNode = {
  /* Stable unique id (the entity target key). */
  id: string;
  kind: GraphNodeKind;
  label: string;
  href: string;
  /* The entity's source text, for hover previews. May be truncated. */
  source?: string;
  language?: "json" | "lua" | "toml" | "text";
  /* Entities semantically tied to this one beyond drawn edges (a variable's
     selected objects); hover highlighting includes them and the drawn edges
     that connect into them. */
  related?: string[];
};

export type GraphEdgeKind = "checks" | "selects" | "contains" | "validates" | "requires";

export type GraphEdge = {
  from: string;
  to: string;
  kind: GraphEdgeKind;
};

export type WorkspaceGraphData = {
  nodes: GraphNode[];
  edges: GraphEdge[];
};
