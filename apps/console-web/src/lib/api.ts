// Wire types for the TypeScript console server (apps/console-server). The
// server is the source of truth for these shapes.

export type Decision = {
    allow: boolean;
    backend: "local" | "grant" | "github" | null;
    reason: string;
};

export type CapabilitySummary = {
    view: Decision;
    propose: Decision;
    approve: Decision;
    administer: Decision;
};

export type SourceTreeSummary = {
    id: string;
    kind: "github" | "local";
    owner: string | null;
    name: string | null;
    defaultBranch: string | null;
    resource: string;
    capabilities: CapabilitySummary;
};

export type MeResponse = {
    authMode: "local" | "team";
    principal: {
        id: string;
        kind: string;
        displayName: string;
        status: string;
    } | null;
    identities: unknown[];
    enrollment: "enrolled" | null;
    githubCredentialSource?: string | null;
    signIn?: { github: boolean };
    capabilities?: {
        deployment: CapabilitySummary;
        sourceTrees: SourceTreeSummary[];
    };
};

// --- the semantic model, as far as the workbench renders it ---

export type ModelLocation = { path: string; range?: unknown };
export type ModelField = { value?: string; location: ModelLocation };
export type ModelValueField = { value?: unknown; location: ModelLocation };

export type RuleModel = {
    index: number;
    location: ModelLocation;
    when?: ModelField;
    value?: ModelValueField;
};

export type VariableModel = {
    id: string;
    location: ModelLocation;
    description?: string;
    declaration: { kind: string; value?: string; location: ModelLocation };
    resolve?: {
        default?: ModelValueField;
        rules: RuleModel[];
    };
};

export type ModelEntityRef = Record<string, unknown> & {
    kind: string;
    id?: string;
};

export type ReferenceModel = {
    from: ModelEntityRef;
    to: ModelEntityRef;
    via: { kind: string; index?: number };
    location: ModelLocation;
    declaration?: ModelLocation;
};

export type SemanticModel = {
    version: number;
    extends: { source: string }[];
    variables: VariableModel[];
    catalogs: { id: string; path: string }[];
    catalogEntries: { catalog: string; key: string }[];
    enums: { id: string }[];
    evaluationContexts: { id: string; path: string }[];
    layers: { id: string }[];
    references: ReferenceModel[];
};

export type LintDiagnostic = {
    severity: string;
    rule?: string;
    message: string;
    location?: { path?: string };
};

export type PackageLint = {
    documents: unknown[];
    diagnostics: LintDiagnostic[];
};

export type PackageListing = {
    ref: string;
    pin: string;
    packages: { path: string }[];
};

export type PackageDetail = {
    pin: string;
    path: string;
    model: SemanticModel;
    lint: PackageLint;
};

export type ChangeRecord = {
    operation: string;
    address: string;
    before?: unknown;
    after?: unknown;
};

export type EditResponse = {
    pin: string;
    records: ChangeRecord[];
    lint: PackageLint;
};

export type ChangeSet = {
    id: string;
    sourceTreeId: string;
    title: string;
    authorPrincipal: string;
    actingMode: "user" | "app";
    baseRef: string;
    baseShaAtCreation: string | null;
    state: "draft" | "proposed" | "merged" | "abandoned";
    prNumber: number | null;
    prUrl: string | null;
    headSha: string | null;
    behindBase: boolean;
    conflicted: boolean;
    observedVia: string | null;
    lastReconciledAt: string | null;
    createdAt: string;
    updatedAt: string;
    branch: string;
};

export type ChangeSetEvent = {
    id: number;
    changeSetId: string;
    at: string;
    actor: string | null;
    event: string;
    detail: string | null;
};

export type ChangeSetDetail = {
    changeSet: ChangeSet;
    events: ChangeSetEvent[];
    collaborators: { principalId: string; addedBy: string; addedAt: string }[];
};

// One operation on the structured edit path; the server-side engine defines
// the vocabulary and validates the shape.
export type EditOperation = Record<string, unknown> & { op: string };

// --- the read side (tranche C3) ---

export type RuleTrace = {
    index: number;
    condition: string;
    value: unknown;
    matched: boolean;
};

export type ResolutionTrace = {
    resolution: { id: string; value: unknown };
    default_value: unknown;
    rules: RuleTrace[];
    provenance?: string;
    allocation?: {
        layer: string;
        allocation: string;
        enrolled: boolean;
        bucket?: number;
        arm?: string;
    };
};

// One variable in the lenient batch: the trace, or the error that stopped
// it (a rule reading a context key the chosen context does not carry).
export type TraceOutcome = {
    id: string;
    trace?: ResolutionTrace;
    error?: string;
};

export type SampleContext = {
    evaluationContext: string;
    key: string;
    context: Record<string, unknown> | null;
};

// A synthesized boundary context from the fixtures machinery: one behavior
// case of one variable, with the expected outcome.
export type SynthesizedContext = {
    target: { kind: "variable"; id: string };
    caseId: string;
    title: string;
    because: string | null;
    context: Record<string, unknown>;
    expect: {
        kind: string;
        value: unknown;
        matched: { kind: string; index?: number; condition?: string };
    };
};

export type ContextInventory = {
    pin: string;
    path: string;
    samples: SampleContext[];
    synthesized: SynthesizedContext[];
};

export type UpcomingChange = {
    variable: string;
    site:
        | { kind: "rule"; index: number }
        | { kind: "queryFilter" }
        | { kind: "querySort" };
    boundary: string;
    comparison: string;
    expression: string;
    location: { path: string };
};

export type CommitRecord = {
    sha: string;
    message: string;
    authorName: string | null;
    date: string;
};

export type CompositionEdge = {
    from: string;
    source: string;
    to: string | null;
};

// --- surfaces (tranche C4): the domain lens at floor fidelity ---

export type SurfaceDiagnostic = {
    severity: "error" | "warning" | "info";
    message: string;
};

export type SurfaceBinding = {
    target: string;
    editableFields: string[] | null;
    canAdd: boolean;
    canDelete: boolean;
};

export type Surface = {
    id: string;
    kind: string | null;
    title: string;
    description: string | null;
    audience: string[];
    approval: string | null;
    caution: string | null;
    config: Record<string, unknown> | null;
    bindings: SurfaceBinding[];
    diagnostics: SurfaceDiagnostic[];
};

export type Control =
    | { control: "toggle" }
    | { control: "select"; options: unknown[] }
    | { control: "number" }
    | { control: "text" }
    | { control: "json" };

export type FieldControl = Control & { field: string };

export type SurfaceItem =
    | {
          kind: "variable";
          id: string;
          variableType: string | null;
          description: string | null;
          control: Control;
          default: unknown;
          ruleCount: number;
      }
    | {
          kind: "catalog";
          id: string;
          description: string | null;
          schema: unknown;
          entries: { key: string; value: unknown }[];
          editableFields: string[] | null;
          canAdd: boolean;
          canDelete: boolean;
          fields: FieldControl[];
      }
    | {
          kind: "entry";
          catalog: string;
          key: string;
          value: unknown;
          editableFields: string[] | null;
          fields: FieldControl[];
      }
    | { kind: "missing"; target: string };

export type SurfaceSuggestion = {
    id: string;
    kind: string;
    title: string;
    reason: string;
    operations: EditOperation[];
};

export type SurfaceList = {
    pin: string;
    path: string;
    surfaces: Surface[];
    diagnostics: SurfaceDiagnostic[];
    suggestions: SurfaceSuggestion[];
};

export type SurfaceDetail = {
    pin: string;
    path: string;
    surface: Surface;
    items: SurfaceItem[];
    now: string;
    upcoming: UpcomingChange[];
    history: CommitRecord[];
    pending: {
        id: string;
        title: string;
        state: string;
        prNumber: number | null;
    }[];
};

// --- the three-delta review (tranche C4) ---

export type SemanticChange = {
    kind: string;
    target: {
        entity: Record<string, unknown> & { kind: string };
        field?: Record<string, unknown>;
    };
    before?: unknown;
    after?: unknown;
};

export type OutcomeImpact = {
    variable: string;
    before?: { id: string; value: unknown };
    before_error?: string;
    after?: { id: string; value: unknown };
    after_error?: string;
};

export type ContextImpact = {
    context: string;
    impacts: OutcomeImpact[];
    compared: number;
};

export type ReviewContext = {
    label: string;
    source: "sample" | "synthetic";
    context: Record<string, unknown>;
};

export type ReviewDenominator = {
    samples: number;
    synthesized: number;
    variables: {
        id: string;
        sampleCount: number;
        defaultCovered: boolean;
        rules: { index: number; covered: boolean }[];
    }[];
};

export type PackageReview = {
    path: string;
    changes: SemanticChange[];
    contexts: ReviewContext[];
    contextImpacts: ContextImpact[];
    impactError: string | null;
    denominator: ReviewDenominator;
    lint: { introduced: LintDiagnostic[]; resolved: LintDiagnostic[] };
    surfaces: {
        id: string;
        title: string;
        approval: string | null;
        caution: string | null;
    }[];
};

export type ChangeSetReview = {
    basePin: string;
    headPin: string;
    files: string[];
    packages: PackageReview[];
};

export class ApiError extends Error {
    readonly status: number;
    readonly paths: string[] | undefined;

    constructor(status: number, message: string, paths?: string[]) {
        super(message);
        this.status = status;
        this.paths = paths;
    }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await fetch(path, init);
    if (!response.ok) {
        let message = `the console server answered ${response.status}`;
        let paths: string[] | undefined;
        try {
            const body = (await response.json()) as {
                error?: { message?: string; paths?: string[] };
            };
            message = body.error?.message ?? message;
            paths = body.error?.paths;
        } catch {
            // Keep the status-only message.
        }
        throw new ApiError(response.status, message, paths);
    }
    return (await response.json()) as T;
}

export function apiGet<T>(path: string): Promise<T> {
    return request<T>(path);
}

export function apiPost<T>(path: string, body: unknown): Promise<T> {
    return request<T>(path, {
        method: "POST",
        headers: {
            "content-type": "application/json",
            // The mutation guard: CSRF cannot forge this header.
            "x-rototo-console": "1",
        },
        body: JSON.stringify(body),
    });
}

export function fetchMe(): Promise<MeResponse> {
    return apiGet<MeResponse>("/api/me");
}

export function listPackages(
    treeId: string,
    ref?: string,
): Promise<PackageListing> {
    const query = ref === undefined ? "" : `?ref=${encodeURIComponent(ref)}`;
    return apiGet<PackageListing>(
        `/api/source-trees/${treeId}/packages${query}`,
    );
}

export function readPackage(
    treeId: string,
    packagePath: string,
    pin: string,
): Promise<PackageDetail> {
    return apiGet<PackageDetail>(
        `/api/source-trees/${treeId}/package?path=${encodeURIComponent(packagePath)}&pin=${pin}`,
    );
}

export function listPackageFiles(
    treeId: string,
    packagePath: string,
    pin: string,
): Promise<{ pin: string; files: string[] }> {
    return apiGet(
        `/api/source-trees/${treeId}/package-files?path=${encodeURIComponent(packagePath)}&pin=${pin}`,
    );
}

export function readPackageFile(
    treeId: string,
    packagePath: string,
    pin: string,
    file: string,
): Promise<{ pin: string; file: string; content: string }> {
    return apiGet(
        `/api/source-trees/${treeId}/file?path=${encodeURIComponent(packagePath)}&pin=${pin}&file=${encodeURIComponent(file)}`,
    );
}

export function listChangeSets(
    treeId: string,
): Promise<{ changeSets: ChangeSet[] }> {
    return apiGet(`/api/source-trees/${treeId}/change-sets`);
}

export function createChangeSet(
    treeId: string,
    title: string,
): Promise<ChangeSet> {
    return apiPost(`/api/source-trees/${treeId}/change-sets`, { title });
}

export function readChangeSet(id: string): Promise<ChangeSetDetail> {
    return apiGet(`/api/change-sets/${id}`);
}

export function saveEdit(
    changeSetId: string,
    edit: {
        packagePath: string;
        expectedPin: string;
        summary?: string;
        operations?: EditOperation[];
        files?: { path: string; content: string }[];
        deletes?: string[];
    },
): Promise<EditResponse> {
    return apiPost(`/api/change-sets/${changeSetId}/edits`, edit);
}

export function submitChangeSet(
    id: string,
    body?: string,
): Promise<{ changeSet: ChangeSet; pull: { number: number; url: string } }> {
    return apiPost(`/api/change-sets/${id}/submit`, { body });
}

export function abandonChangeSet(
    id: string,
): Promise<{ changeSet: ChangeSet }> {
    return apiPost(`/api/change-sets/${id}/abandon`, {});
}

export function reconcileChangeSet(
    id: string,
): Promise<{ changeSet: ChangeSet }> {
    return apiPost(`/api/change-sets/${id}/reconcile`, {});
}

// --- read-side calls (tranche C3) ---

export function fetchContexts(
    treeId: string,
    packagePath: string,
    pin: string,
): Promise<ContextInventory> {
    return apiGet(
        `/api/source-trees/${treeId}/contexts?path=${encodeURIComponent(packagePath)}&pin=${pin}`,
    );
}

export function runPreview(
    treeId: string,
    packagePath: string,
    pin: string,
    context: Record<string, unknown>,
): Promise<{ pin: string; outcomes: TraceOutcome[] }> {
    return apiPost(
        `/api/source-trees/${treeId}/preview?path=${encodeURIComponent(packagePath)}&pin=${pin}`,
        { context },
    );
}

export function fetchUpcoming(
    treeId: string,
    packagePath: string,
    pin: string,
): Promise<{ now: string; changes: UpcomingChange[] }> {
    return apiGet(
        `/api/source-trees/${treeId}/upcoming?path=${encodeURIComponent(packagePath)}&pin=${pin}`,
    );
}

export function fetchHistory(
    treeId: string,
    packagePath: string,
    until?: string,
): Promise<{ ref: string; commits: CommitRecord[] }> {
    const bound =
        until === undefined ? "" : `&until=${encodeURIComponent(until)}`;
    return apiGet(
        `/api/source-trees/${treeId}/history?path=${encodeURIComponent(packagePath)}${bound}`,
    );
}

export function fetchComposition(
    treeId: string,
    ref?: string,
): Promise<{
    ref: string;
    pin: string;
    nodes: { path: string }[];
    edges: CompositionEdge[];
}> {
    const query = ref === undefined ? "" : `?ref=${encodeURIComponent(ref)}`;
    return apiGet(`/api/source-trees/${treeId}/composition${query}`);
}

// --- surfaces and review calls (tranche C4) ---

export function fetchSurfaces(
    treeId: string,
    packagePath: string,
    pin: string,
): Promise<SurfaceList> {
    return apiGet(
        `/api/source-trees/${treeId}/surfaces?path=${encodeURIComponent(packagePath)}&pin=${pin}`,
    );
}

export function fetchSurface(
    treeId: string,
    packagePath: string,
    pin: string,
    id: string,
): Promise<SurfaceDetail> {
    return apiGet(
        `/api/source-trees/${treeId}/surface?path=${encodeURIComponent(packagePath)}&pin=${pin}&id=${encodeURIComponent(id)}`,
    );
}

export function fetchReview(
    changeSetId: string,
): Promise<{ changeSet: ChangeSet; review: ChangeSetReview }> {
    return apiGet(`/api/change-sets/${changeSetId}/review`);
}

// --- the LSP bridge: live diagnostics for the raw-text editor ---

export function openLspSession(
    treeId: string,
    packagePath: string,
    pin: string,
): Promise<{ session: string }> {
    return apiPost(`/api/source-trees/${treeId}/lsp-sessions`, {
        path: packagePath,
        pin,
    });
}

export function lspNotify(
    session: string,
    method: string,
    params: unknown,
): Promise<{ ok: boolean }> {
    return apiPost(`/api/lsp-sessions/${session}/notify`, { method, params });
}

export function lspRequest<T>(
    session: string,
    method: string,
    params: unknown,
): Promise<{ result: T }> {
    return apiPost(`/api/lsp-sessions/${session}/request`, { method, params });
}

export function lspNotifications(
    session: string,
): Promise<{ notifications: LspServerMessage[] }> {
    return apiGet(`/api/lsp-sessions/${session}/notifications`);
}

export function closeLspSession(session: string): Promise<{ ok: boolean }> {
    return apiPost(`/api/lsp-sessions/${session}/close`, {});
}

export type LspServerMessage = {
    method: string;
    params?: {
        uri?: string;
        diagnostics?: {
            message: string;
            severity?: number;
            code?: string;
            range?: unknown;
        }[];
    };
};
