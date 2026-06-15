/* The graph data contract. The server builds this once from the semantic
   model; rendering concepts consume it. Keep this stable so new graph
   concepts are plug-and-play — a concept is just a client component taking
   WorkspaceGraphData. */

/** Workspace entity kind that can appear as a graph node. */
export type GraphNodeKind =
    | "qualifier"
    | "variable"
    | "catalog"
    | "catalogEntry";

/** Derived graph node for one semantic workspace entity. */
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
     selected entries); hover highlighting includes them and the drawn edges
     that connect into them. */
    related?: string[];
    /* Marked when the entity differs from the base ref in the current draft. */
    edited?: boolean;
};

/** Relationship kind between two semantic graph nodes. */
export type GraphEdgeKind = "checks" | "selects" | "contains" | "requires";

/** Directed graph edge derived from semantic references or containment. */
export type GraphEdge = {
    from: string;
    to: string;
    kind: GraphEdgeKind;
};

/** Complete graph payload rendered for one staged workspace or draft. */
export type WorkspaceGraphData = {
    nodes: GraphNode[];
    edges: GraphEdge[];
};
