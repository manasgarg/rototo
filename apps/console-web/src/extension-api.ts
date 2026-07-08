// The extension contract (design/console-surfaces.md "Extensions"). An
// experience is deployment-trusted TypeScript that registers a renderer for
// a surface kind, and its renderer receives exactly two capabilities plus a
// toolkit: read (the surface's bound entities as semantic views, with the
// read side attached), propose (Layer 3 operations against the active
// change set), and the ui kit. Extensions render and propose, nothing else:
// no file access, no git, no tokens, no routes. This module is the ONLY
// thing an extension may import besides react and its own files; the
// contract check in scripts/check-extension-contract.mjs enforces that.

import type { ComponentType, ReactNode } from "react";

export type {
    CommitRecord,
    ContextInventory,
    Control,
    EditOperation,
    Surface,
    SurfaceItem,
    TraceOutcome,
    UpcomingChange,
    VariableAllocationView,
    VariableRuleView,
} from "./lib/api.ts";

import type {
    CommitRecord,
    ContextInventory,
    Control,
    EditOperation,
    Surface,
    SurfaceItem,
    TraceOutcome,
    UpcomingChange,
} from "./lib/api.ts";

// What an experience may read. Everything is scoped to the signed-in
// principal by the server; the client simply cannot see more than the user
// could.
export type ExperienceRead = {
    pin: string;
    packagePath: string;
    // The read side the host already fetched with the surface.
    upcoming: UpcomingChange[];
    history: CommitRecord[];
    pending: {
        id: string;
        title: string;
        state: string;
        prNumber: number | null;
    }[];
    // The package's saved samples and synthesized boundary contexts.
    contexts: () => Promise<ContextInventory>;
    // The lenient batch preview: every variable resolved and traced under
    // one context.
    preview: (context: Record<string, unknown>) => Promise<TraceOutcome[]>;
};

// The design-system toolkit, passed in so extensions look like the console
// without importing its internals.
export type UiKit = {
    Banner: ComponentType<{
        tone: "info" | "warn" | "err";
        children: ReactNode;
    }>;
    Pill: ComponentType<{
        tone: "info" | "ok" | "warn" | "err" | "neutral";
        title?: string;
        children: ReactNode;
    }>;
    // `submit` marks the form's default button (Enter submits); everything
    // else renders type="button" and needs an onClick.
    Button: ComponentType<{
        tone?: "primary" | "secondary" | "ghost";
        disabled?: boolean;
        title?: string;
        submit?: boolean;
        onClick?: () => void;
        children: ReactNode;
    }>;
    Toggle: ComponentType<{
        on: boolean;
        disabled?: boolean;
        onChange: (next: boolean) => void;
    }>;
    Table: ComponentType<{ head: ReactNode[]; children: ReactNode }>;
    Field: ComponentType<{
        label: string;
        hint?: string;
        children: ReactNode;
    }>;
    // The floor's inferred control, so extension cells behave exactly like
    // floor cells (commit-on-blur, one operation per commit).
    ControlInput: ComponentType<{
        control: Control;
        value: unknown;
        disabled: boolean;
        onCommit: (value: unknown) => void;
    }>;
    // Per-item degradation: when a bound entity's shape outgrows what the
    // experience understands, it renders this instead of guessing.
    // Experiences degrade; they never block and never lie.
    AdvancedShape: ComponentType<{
        label: string;
        detail?: string;
        onOpen?: () => void;
    }>;
};

export type ExperienceProps = {
    surface: Surface;
    items: SurfaceItem[];
    // False when no change set is active; propose() is inert then and
    // controls should disable.
    editable: boolean;
    // The evaluation instant the read side used, for deterministic
    // derivations.
    now: string;
    read: ExperienceRead;
    // Operations flow through decide(), the edit engine, governance, lint,
    // and approval exactly like every other edit in the system.
    propose: (operations: EditOperation[], summary: string) => void;
    ui: UiKit;
    // The escalation link per-item degradation points at.
    openWorkbench: () => void;
};

// What an extension exports: the kind it claims and the renderer. The
// deployment's console build lists its extensions in
// src/lib/experiences.ts (build-time composition).
export type ExperienceModule = {
    kind: string;
    Render: ComponentType<ExperienceProps>;
};
