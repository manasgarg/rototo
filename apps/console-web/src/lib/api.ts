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

export type SemanticModel = {
    version: number;
    variables: VariableModel[];
    catalogs: { id: string; path: string }[];
    catalogEntries: { catalog: string; key: string }[];
    enums: { id: string }[];
    evaluationContexts: { id: string; path: string }[];
    layers: { id: string }[];
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
