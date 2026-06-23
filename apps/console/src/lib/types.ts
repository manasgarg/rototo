/* Wire types for the rototo console API. The Rust server is the source of
   truth (src/console/); these mirror its serde camelCase output. */

/** Server deployment mode resolved once at console startup. */
export type ConsoleDeployment = "local" | "hosted";
/** Server write policy resolved once at console startup. */
export type ConsoleWritePolicy = "disabled" | "pull-request" | "direct-push";
/** Console state persistence mode resolved at startup. */
export type ConsoleStateMode = "ephemeral" | "persistent";
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
    | { kind: "pullRequest"; backend: "gitHubApi" }
    | { kind: "directPush"; backend: "gitHubApi" | "localWorkingTree" };

/** Combined read/write capability summary for one workspace response. */
export type WorkspaceCapabilities = {
    read: WorkspaceCapability;
    write: WorkspaceWriteCapability;
};

/** Registered source tree row persisted by the console store. */
export type SourceTreeKind = "gitHub" | "gitRemote" | "localFolder" | "archive";

/** Capability flags calculated from the registered source tree kind. */
export type SourceTreeCapabilities = {
    canRefresh: boolean;
    canDiscoverWorkspaces: boolean;
    canLoadWorkspaces: boolean;
    canBranch: boolean;
    canEdit: boolean;
    canOpenPullRequest: boolean;
};

export type SourceTreeRecord = {
    id: string;
    principalId: string;
    kind: SourceTreeKind;
    source: string;
    displayName: string;
    defaultRevision: string;
    capabilities: SourceTreeCapabilities;
    createdAt: string;
    updatedAt: string;
    lastDiscoveredAt: string | null;
};

/** Source tree row plus active discovered workspaces, rebuilt for responses. */
export type SourceTreeWithWorkspaces = SourceTreeRecord & {
    workspaces: WorkspaceRecord[];
};

/** Console source-management and persistence state. */
export type ConsoleState = {
    mode: ConsoleStateMode;
    fixedWorkspace: boolean;
    canManageSourceTrees: boolean;
};

/** Persisted workspace discovery row inside a registered source tree. */
export type WorkspaceRecord = {
    id: string;
    slug: string;
    sourceTreeId: string;
    sourceTreeLabel: string;
    displayPath: string;
    path: string;
    revision: string;
    source: string;
    discoveredAt: string;
};

/** Source tree branch selected by one console user. */
export type BranchRecord = {
    id: string;
    sourceTreeId: string;
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

/** JSON value plus location for resolve values in the semantic model. */
export type ModelValueField = {
    value?: unknown;
    location: ModelLocation;
};

/** Legacy qualifier predicate shape retained only for older payloads. */
export type PredicateModel = {
    index: number;
    location: ModelLocation;
    attribute?: ModelField;
    op?: ModelField;
    not?: boolean;
    value?: unknown;
};

/** Qualifier declaration from the semantic model. */
export type QualifierModel = {
    id: string;
    location: ModelLocation;
    description?: string;
    when?: ModelField;
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
    when?: ModelField;
    query?: ModelField;
    value?: ModelValueField;
};

/** Variable resolution block with default and ordered rules. */
export type ResolveModel = {
    location: ModelLocation;
    default?: ModelValueField;
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
    path: string;
    location: ModelLocation;
    description?: string;
    json?: unknown;
};

/** Catalog value value from the semantic model. */
export type CatalogEntryModel = {
    catalog: string;
    key: string;
    location: ModelLocation;
    value: unknown;
};

/** Request context schema from the semantic model. */
export type RequestContextModel = {
    id: string;
    path: string;
    location: ModelLocation;
    title?: string;
    description?: string;
    json?: unknown;
};

/** Saved request context sample from the semantic model. */
export type RequestContextEntryModel = {
    requestContext: string;
    key: string;
    path: string;
    location: ModelLocation;
    value?: unknown;
};

export type QualifierRequestContextModel = {
    qualifier: string;
    requestContexts: string[];
};

export type VariableRequestContextModel = {
    variable: string;
    requestContexts: string[];
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
    | { kind: "requestContext"; id: string }
    | { kind: "requestContextEntry"; requestContext: string; key: string }
    | { kind: "value"; variable: string; key: string }
    | { kind: "contextAttribute"; name: string };

/** Edge reason used by semantic references. */
export type ModelReferenceVia =
    | { kind: "qualifierWhen" }
    | { kind: "qualifierWhenContextAttribute" }
    | { kind: "variableCatalog" }
    | { kind: "resolveDefault" }
    | { kind: "ruleCondition"; index: number }
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
    requestContexts: RequestContextModel[];
    requestContextEntries: RequestContextEntryModel[];
    linters: LinterModel[];
    references: ReferenceModel[];
    qualifierRequestContexts: QualifierRequestContextModel[];
    variableRequestContexts: VariableRequestContextModel[];
};

/* Workspace inventory, computed server-side from the semantic model. */
/** Variable inventory row derived from the semantic model for navigation. */
export type VariableInventoryItem = {
    id: string;
    path: string;
    description: string | null;
    declaration: string;
    defaultValue: string | null;
    ruleCount: number;
    qualifierReferences: string[];
    ruleValues: string[];
    catalogReference: string | null;
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
    entryCount: number;
};

/** Catalog value inventory row linked to its editable source path. */
export type CatalogEntryInventoryItem = {
    catalogId: string;
    key: string;
    id: string;
    path: string;
};

export type RequestContextInventoryItem = {
    id: string;
    path: string;
    title: string | null;
    description: string | null;
    entryCount: number;
};

export type RequestContextEntryInventoryItem = {
    requestContextId: string;
    key: string;
    id: string;
    path: string;
};

/** Custom lint rule or script inventory row for editor navigation. */
export type LinterInventoryItem = {
    id: string;
    title: string | null;
    path: string | null;
    kind: "rule" | "script";
};

/** Request context schema/sample summary discovered for preview inputs. */
export type ContextInventory = {
    requestContexts: RequestContextInventoryItem[];
    entries: RequestContextEntryInventoryItem[];
    exampleCount: number;
    examples: string[];
};

/** Server-built navigation inventory for one staged workspace. */
export type WorkspaceInventory = {
    variables: VariableInventoryItem[];
    qualifiers: QualifierInventoryItem[];
    catalogs: CatalogInventoryItem[];
    catalogEntries: CatalogEntryInventoryItem[];
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
/** Runtime qualifier evaluation annotated with the condition expression. */
export type QualifierEvaluation = {
    id: string;
    matched: boolean | null;
    when: string | null;
    predicates: Array<{
        index: number;
        attribute: string | null;
        op: string | null;
        not: boolean;
        valueLiteral: string | null;
        contextValue: string | null;
        nested: QualifierEvaluation | null;
    }>;
};

/** Variable preview result for one saved request context. */
export type SavedContextResolution = {
    name: string;
    requestContext: string;
    path: string;
    ok: boolean;
    value?: unknown;
    source?: VariableResolutionSource;
    steps?: Array<{
        index: number;
        qualifier: string;
        matched: boolean;
        evaluation: QualifierEvaluation;
    }>;
    usedDefault?: boolean;
    error?: string;
};

export type VariableResolutionSource =
    | { kind: "literal" }
    | { kind: "catalog"; catalog: string; value: string };

/** Qualifier preview result for one saved request context. */
export type QualifierContextEvaluation = {
    name: string;
    requestContext: string;
    path: string;
    evaluation: QualifierEvaluation | null;
    error?: string;
};

/** Branch edit preview truth table for one saved request context. */
export type EditContextPreview = {
    name: string;
    requestContext: string;
    qualifierTruth: Record<string, boolean>;
};

/* Branch editing: each editable entity arrives with its branch text. */
/** Branch-branch entity text and metadata used by the editor screens. */
export type EditableEntity = {
    section: "variables" | "qualifiers" | "catalogs" | "context" | "linters";
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
/** App shell payload: source trees, workspaces, and active branch rows. */
export type ConsoleData = {
    state: ConsoleState;
    sourceTrees: SourceTreeWithWorkspaces[];
    workspaces: WorkspaceRecord[];
    branches: Array<{ branch: BranchRecord; workspace: WorkspaceRecord }>;
};

/** Count/error summary for one staged workspace. */
export type WorkspaceSummary = {
    variables: number;
    qualifiers: number;
    catalogs: number;
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
    definitions: WorkspaceDefinition[];
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
