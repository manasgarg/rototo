/* Wire types for the rototo console API. The Rust server is the source of
   truth (src/console/); these mirror its serde camelCase output. */

/** Server deployment mode resolved once at console startup. */
export type ConsoleDeployment = "local" | "hosted";
/** Server write policy resolved once at console startup. */
export type ConsoleWritePolicy = "disabled" | "pull-request" | "direct-push";
/** Explanation of where the active GitHub token came from. */
export type GitHubTokenSource =
    | "flag"
    | "environment"
    | "device-flow"
    | "gh-cli"
    | "oauth-session";

/** Principal identity returned by the server without exposing credentials. */
export type ActorIdentity =
    | {
          kind: "gitConfig";
          name: string | null;
          email: string | null;
      }
    | {
          kind: "gitHub";
          id: string;
          login: string;
          name: string | null;
          avatarUrl: string | null;
      };

/** `/api/me` payload for auth state and process capabilities. */
export type MeResponse = {
    deployment: ConsoleDeployment;
    writePolicy: ConsoleWritePolicy;
    deviceFlow: boolean;
    tokenSource: GitHubTokenSource | null;
    authError: string | null;
    user: SessionUser | null;
};

/** Signed-in console user projection; the session token remains server-side. */
export type SessionUser = {
    principalId: string;
    identity: ActorIdentity;
    displayName: string;
    avatarUrl: string | null;
    hasGithubToken: boolean;
};

/** Normalized workspace source class used for UI capability messaging. */
export type WorkspaceSourceKind =
    | "localPath"
    | "fileUrl"
    | "gitFile"
    | "gitHubArchive"
    | "gitHubGit"
    | "httpsArchive"
    | "genericGitRemote";

/** Read capability calculated for the current user and workspace response. */
export type WorkspaceCapability =
    | { status: "allowed" }
    | { status: "missingCredential"; reason: string };

/** Write capability calculated by the server from policy, source, and token. */
export type WorkspaceWriteCapability =
    | { kind: "disabled"; reason: string }
    | { kind: "pullRequest"; backend: "gitHubApi" | "localGit" }
    | { kind: "directPush"; backend: "gitHubApi" | "localGit" };

/** Combined read/write capability summary for one workspace response. */
export type WorkspaceCapabilities = {
    read: WorkspaceCapability;
    write: WorkspaceWriteCapability;
};

/** Registered repository row persisted by the console store. */
export type RepoRecord = {
    id: string;
    principalId: string;
    owner: string;
    name: string;
    defaultRef: string;
    createdAt: string;
    updatedAt: string;
    lastDiscoveredAt: string | null;
};

/** Repository row plus active discovered workspaces, rebuilt for responses. */
export type RepoWithWorkspaces = RepoRecord & {
    workspaces: WorkspaceRecord[];
};

/** Persisted workspace discovery row inside a registered repository. */
export type WorkspaceRecord = {
    id: string;
    slug: string;
    repoId: string;
    owner: string;
    name: string;
    path: string;
    ref: string;
    source: string;
    discoveredAt: string;
};

/** Repository branch selected by one console user. */
export type BranchRecord = {
    id: string;
    repoId: string;
    principalId: string;
    branch: string;
    baseRef: string;
    baseCommit: string | null;
    prUrl: string | null;
    prNumber: number | null;
    prState: string | null;
    prMergedAt: string | null;
    prSyncedAt: string | null;
    lastSelectedWorkspacePath: string | null;
    lastSeenCommit: string | null;
    status: "active" | "recent" | "archived";
    createdAt: string;
    lastOpenedAt: string;
    lastEditedAt: string | null;
    archivedAt: string | null;
};

/** Changed file derived from comparing a branch with its base ref. */
export type BranchChangeRecord = {
    id: string;
    filePath: string;
};

/* Lint diagnostics arrive as the Rust LintDiagnostic serde output; the
   console reads them loosely, same as the admin app did. */
/** Diagnostic target from Rust lint output, kept loose for compatibility. */
export type SemanticTarget = {
    entity?: Record<string, unknown>;
    field?: Record<string, unknown>;
};

/** Browser-facing copy of a Rust lint diagnostic. */
export type LintDiagnostic = {
    rule?: { id?: string } | string;
    severity?: string;
    stage?: string;
    target?: SemanticTarget;
    message?: string;
    help?: string;
    location?: {
        path?: string;
        range?: {
            // 0-based, LSP-style positions
            start?: { line?: number; character?: number; column?: number };
            end?: { line?: number; character?: number; column?: number };
        };
    };
};

/** Successful lint payload for one staged workspace root. */
export type WorkspaceLintView = {
    root: string;
    diagnostics: LintDiagnostic[];
};

/* Lint payloads degrade to this shape when staging or lint itself failed. */
export type WorkspaceLintLoad =
    | WorkspaceLintView
    | { root: string; diagnostics: LintDiagnostic[]; error: string };

/* The semantic model: serde of rototo's WorkspaceSemanticModel. */
/** Source location emitted by the Rust semantic model. */
export type ModelLocation = {
    path: string;
    range?: unknown;
};

/** Value plus location for a scalar field in the semantic model. */
export type ModelField = {
    value?: string;
    location: ModelLocation;
};

/** Qualifier predicate as described by the semantic model. */
export type PredicateModel = {
    index: number;
    location: ModelLocation;
    attribute?: ModelField;
    op?: ModelField;
    value?: unknown;
};

/** Qualifier declaration from the semantic model. */
export type QualifierModel = {
    id: string;
    location: ModelLocation;
    description?: string;
    predicates: PredicateModel[];
};

/** Variable declaration kind and optional referenced schema/catalog/type. */
export type DeclarationModel = {
    kind: string;
    value?: string;
    location: ModelLocation;
};

/** Named value declared by a variable or value file. */
export type ValueModel = {
    key: string;
    location: ModelLocation;
    value: unknown;
};

/** Resolve rule in declaration order. */
export type RuleModel = {
    index: number;
    location: ModelLocation;
    qualifier?: ModelField;
    value?: ModelField;
};

/** Variable resolution block with default and ordered rules. */
export type ResolveModel = {
    location: ModelLocation;
    default?: ModelField;
    rules: RuleModel[];
};

/** Variable declaration from the semantic model. */
export type VariableModel = {
    id: string;
    location: ModelLocation;
    description?: string;
    declaration: DeclarationModel;
    values: ValueModel[];
    resolve?: ResolveModel;
};

/** Catalog declaration from the semantic model. */
export type CatalogModel = {
    id: string;
    location: ModelLocation;
    description?: string;
    schema?: ModelField;
};

/** Catalog entry value from the semantic model. */
export type CatalogEntryModel = {
    catalog: string;
    key: string;
    location: ModelLocation;
    value: unknown;
};

/** JSON Schema file from the semantic model. */
export type SchemaModel = {
    path: string;
    location: ModelLocation;
    json?: unknown;
};

/** Custom Lua linter file and the rules it declares. */
export type LinterModel = {
    path: string;
    location: ModelLocation;
    rules: Array<{ id: string; title: string; help: string }>;
};

/** Node identity used by semantic references. */
export type ModelEntityRef =
    | { kind: "qualifier"; id: string }
    | { kind: "variable"; id: string }
    | { kind: "catalog"; id: string }
    | { kind: "catalogEntry"; catalog: string; key: string }
    | { kind: "schema"; path: string }
    | { kind: "value"; variable: string; key: string }
    | { kind: "contextAttribute"; name: string };

/** Edge reason used by semantic references. */
export type ModelReferenceVia =
    | { kind: "predicateQualifier"; index: number }
    | { kind: "predicateContextAttribute"; index: number }
    | { kind: "variableCatalog" }
    | { kind: "catalogSchema" }
    | { kind: "resolveDefault" }
    | { kind: "ruleQualifier"; index: number }
    | { kind: "ruleValue"; index: number };

/** Directed semantic reference from one workspace entity to another. */
export type ReferenceModel = {
    from: ModelEntityRef;
    to: ModelEntityRef;
    location: ModelLocation;
    via: ModelReferenceVia;
};

/** Full semantic graph for a staged workspace, generated by Rust. */
export type WorkspaceSemanticModel = {
    version: number;
    qualifiers: QualifierModel[];
    variables: VariableModel[];
    catalogs: CatalogModel[];
    catalogEntries: CatalogEntryModel[];
    schemas: SchemaModel[];
    linters: LinterModel[];
    references: ReferenceModel[];
};

/* Workspace inventory, computed server-side from the semantic model. */
/** Variable inventory row derived from the semantic model for navigation. */
export type VariableInventoryItem = {
    id: string;
    path: string;
    description: string | null;
    declaration: string;
    defaultValueKey: string | null;
    ruleCount: number;
    qualifierReferences: string[];
    ruleValueKeys: string[];
    catalogReference: string | null;
    schemaReference: string | null;
};

/** Qualifier inventory row derived from the semantic model. */
export type QualifierInventoryItem = {
    id: string;
    path: string;
    description: string | null;
    predicateCount: number;
    qualifierReferences: string[];
};

/** Catalog inventory row with schema and entry-count summary. */
export type CatalogInventoryItem = {
    id: string;
    path: string;
    description: string | null;
    schema: string | null;
    schemaReference: string | null;
    entryCount: number;
};

/** Catalog entry inventory row linked to its editable source path. */
export type CatalogEntryInventoryItem = {
    catalogId: string;
    key: string;
    id: string;
    path: string;
};

/** Standalone schema inventory row for editor navigation. */
export type SchemaInventoryItem = {
    id: string;
    path: string;
    title: string | null;
};

/** Custom lint rule or script inventory row for editor navigation. */
export type LinterInventoryItem = {
    id: string;
    title: string | null;
    path: string | null;
    kind: "rule" | "script";
};

/** Context schema/example summary discovered for preview inputs. */
export type ContextInventory = {
    schemaPath: string | null;
    exampleCount: number;
    examples: string[];
};

/** Server-built navigation inventory for one staged workspace. */
export type WorkspaceInventory = {
    variables: VariableInventoryItem[];
    qualifiers: QualifierInventoryItem[];
    catalogs: CatalogInventoryItem[];
    catalogEntries: CatalogEntryInventoryItem[];
    schemas: SchemaInventoryItem[];
    linters: LinterInventoryItem[];
    context: ContextInventory;
};

/** Source text for one workspace file opened by the console. */
export type WorkspaceDefinition = {
    path: string;
    text: string;
    language: "json" | "lua" | "toml" | "text";
};

/* Resolution previews against saved request contexts, computed server-side
   by the real runtime. */
/** Runtime qualifier evaluation annotated with predicate-level detail. */
export type QualifierEvaluation = {
    id: string;
    matched: boolean | null;
    predicates: Array<{
        index: number;
        attribute: string | null;
        op: string | null;
        valueLiteral: string | null;
        contextValue: string | null;
        nested: QualifierEvaluation | null;
    }>;
};

/** Variable preview result for one saved request context. */
export type SavedContextResolution = {
    name: string;
    path: string;
    ok: boolean;
    valueKey?: string;
    steps?: Array<{
        index: number;
        qualifier: string;
        matched: boolean;
        evaluation: QualifierEvaluation;
    }>;
    usedDefault?: boolean;
    error?: string;
};

/** Qualifier preview result for one saved request context. */
export type QualifierContextEvaluation = {
    name: string;
    path: string;
    evaluation: QualifierEvaluation | null;
    error?: string;
};

/** Branch edit preview truth table for one saved request context. */
export type EditContextPreview = {
    name: string;
    qualifierTruth: Record<string, boolean>;
};

/* Branch editing: each editable entity arrives with its branch text. */
/** Branch-branch entity text and metadata used by the editor screens. */
export type EditableEntity = {
    section:
        | "variables"
        | "qualifiers"
        | "catalogs"
        | "schemas"
        | "context"
        | "linters";
    id: string;
    kind: string;
    path: string;
    description: string | null;
    badge: string | null;
    text: string;
    language: WorkspaceDefinition["language"];
    catalogId?: string | null;
    entryKey?: string | null;
};

/* Screen payloads. */
/** App shell payload: repos, workspaces, and active branch rows. */
export type ConsoleData = {
    repos: RepoWithWorkspaces[];
    workspaces: WorkspaceRecord[];
    branches: Array<{ branch: BranchRecord; workspace: WorkspaceRecord }>;
};

/** Count/error summary for one staged workspace. */
export type WorkspaceSummary = {
    variables: number;
    qualifiers: number;
    catalogs: number;
    schemas: number;
    error: string | null;
};

/** Workspace summary paired with stable workspace navigation ids. */
export type WorkspaceSummaryEntry = WorkspaceSummary & {
    workspaceId: string;
    workspaceSlug: string;
};

/** Workspace summary list payload, preserving server ordering. */
export type WorkspaceSummariesData = {
    summaries: WorkspaceSummaryEntry[];
};

/** Full workspace screen payload for a persisted workspace record. */
export type WorkspaceData = {
    workspace: WorkspaceRecord;
    branches: BranchRecord[];
    inventory: WorkspaceInventory;
    inventoryError: string | null;
    lint: WorkspaceLintLoad;
    model: WorkspaceSemanticModel | null;
    sourceKind: WorkspaceSourceKind;
    capabilities: WorkspaceCapabilities;
};

/** Source text and previews for one workspace inspect entity. */
export type WorkspaceEntityData = {
    definition: WorkspaceDefinition | null;
    definitionError: string | null;
    contextResolutions: SavedContextResolution[];
    qualifierEvaluations: QualifierContextEvaluation[];
};

/** Full branch screen payload, including branch state and editable entities. */
export type BranchData = {
    workspace: WorkspaceRecord;
    branch: BranchRecord;
    prSyncError: string | null;
    changes: BranchChangeRecord[];
    lint: WorkspaceLintLoad;
    model: WorkspaceSemanticModel | null;
    entities: EditableEntity[];
    editLoadError: string | null;
    editedPaths: string[];
    sourceKind: WorkspaceSourceKind;
    capabilities: WorkspaceCapabilities;
};

/** Extra compare/preview payload for one branch entity editor. */
export type BranchEntityData = {
    baseText: string | null;
    contextPreviews: EditContextPreview[];
};
