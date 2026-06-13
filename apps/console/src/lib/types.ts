/* Wire types for the rototo console API. The Rust server is the source of
   truth (src/console/); these mirror its serde camelCase output. */

export type ConsoleMode = "local" | "team" | "read-only";

export type MeResponse = {
  mode: ConsoleMode;
  deviceFlow: boolean;
  tokenSource: "flag" | "environment" | "device-flow" | "gh-cli" | null;
  authError: string | null;
  user: SessionUser | null;
};

export type SessionUser = {
  githubUserId: string;
  githubLogin: string;
  githubName: string | null;
  githubAvatarUrl: string | null;
};

export type RepoRecord = {
  id: string;
  githubUserId: string;
  owner: string;
  name: string;
  defaultRef: string;
  createdAt: string;
  updatedAt: string;
  lastDiscoveredAt: string | null;
};

export type RepoWithWorkspaces = RepoRecord & {
  workspaces: WorkspaceRecord[];
};

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

export type DraftSessionRecord = {
  id: string;
  workspaceId: string;
  githubUserId: string;
  branch: string;
  baseRef: string;
  status: "open" | "published";
  prUrl: string | null;
  prNumber: number | null;
  prState: string | null;
  prMergedAt: string | null;
  prSyncedAt: string | null;
  createdAt: string;
  updatedAt: string;
  publishedAt: string | null;
};

export type DraftChangeRecord = {
  id: string;
  draftId: string;
  filePath: string;
  variableId: string;
  valueKey: string;
  beforeJson: string;
  afterJson: string;
  updatedAt: string;
};

export type DraftEventRecord = {
  id: string;
  draftId: string;
  kind: string;
  summary: string;
  detailJson: string | null;
  createdAt: string;
};

/* Lint diagnostics arrive as the Rust LintDiagnostic serde output; the
   console reads them loosely, same as the admin app did. */
export type SemanticTarget = {
  entity?: Record<string, unknown>;
  field?: Record<string, unknown>;
};

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

export type WorkspaceLintView = {
  root: string;
  diagnostics: LintDiagnostic[];
};

/* Lint payloads degrade to this shape when staging or lint itself failed. */
export type WorkspaceLintLoad =
  | WorkspaceLintView
  | { root: string; diagnostics: LintDiagnostic[]; error: string };

/* The semantic model: serde of rototo's WorkspaceSemanticModel. */
export type ModelLocation = {
  path: string;
  range?: unknown;
};

export type ModelField = {
  value?: string;
  location: ModelLocation;
};

export type PredicateModel = {
  index: number;
  location: ModelLocation;
  attribute?: ModelField;
  op?: ModelField;
  value?: unknown;
};

export type QualifierModel = {
  id: string;
  location: ModelLocation;
  description?: string;
  predicates: PredicateModel[];
};

export type DeclarationModel = {
  kind: string;
  value?: string;
  location: ModelLocation;
};

export type ValueModel = {
  key: string;
  location: ModelLocation;
  value: unknown;
};

export type RuleModel = {
  index: number;
  location: ModelLocation;
  qualifier?: ModelField;
  value?: ModelField;
};

export type ResolveModel = {
  location: ModelLocation;
  default?: ModelField;
  rules: RuleModel[];
};

export type VariableModel = {
  id: string;
  location: ModelLocation;
  description?: string;
  declaration: DeclarationModel;
  values: ValueModel[];
  resolve?: ResolveModel;
};

export type CatalogModel = {
  id: string;
  location: ModelLocation;
  description?: string;
  schema?: ModelField;
};

export type CatalogEntryModel = {
  catalog: string;
  key: string;
  location: ModelLocation;
  value: unknown;
};

export type SchemaModel = {
  path: string;
  location: ModelLocation;
  json?: unknown;
};

export type LinterModel = {
  path: string;
  location: ModelLocation;
  rules: Array<{ id: string; title: string; help: string }>;
};

export type ModelEntityRef =
  | { kind: "qualifier"; id: string }
  | { kind: "variable"; id: string }
  | { kind: "catalog"; id: string }
  | { kind: "catalogEntry"; catalog: string; key: string }
  | { kind: "schema"; path: string }
  | { kind: "value"; variable: string; key: string }
  | { kind: "contextAttribute"; name: string };

export type ModelReferenceVia =
  | { kind: "predicateQualifier"; index: number }
  | { kind: "predicateContextAttribute"; index: number }
  | { kind: "variableCatalog" }
  | { kind: "catalogSchema" }
  | { kind: "resolveDefault" }
  | { kind: "ruleQualifier"; index: number }
  | { kind: "ruleValue"; index: number };

export type ReferenceModel = {
  from: ModelEntityRef;
  to: ModelEntityRef;
  location: ModelLocation;
  via: ModelReferenceVia;
};

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

export type QualifierInventoryItem = {
  id: string;
  path: string;
  description: string | null;
  predicateCount: number;
  qualifierReferences: string[];
};

export type CatalogInventoryItem = {
  id: string;
  path: string;
  description: string | null;
  schema: string | null;
  schemaReference: string | null;
  entryCount: number;
};

export type CatalogEntryInventoryItem = {
  catalogId: string;
  key: string;
  id: string;
  path: string;
};

export type SchemaInventoryItem = {
  id: string;
  path: string;
  title: string | null;
};

export type LinterInventoryItem = {
  id: string;
  title: string | null;
  path: string | null;
  kind: "rule" | "script";
};

export type ContextInventory = {
  schemaPath: string | null;
  exampleCount: number;
  examples: string[];
};

export type WorkspaceInventory = {
  variables: VariableInventoryItem[];
  qualifiers: QualifierInventoryItem[];
  catalogs: CatalogInventoryItem[];
  catalogEntries: CatalogEntryInventoryItem[];
  schemas: SchemaInventoryItem[];
  linters: LinterInventoryItem[];
  context: ContextInventory;
};

export type WorkspaceDefinition = {
  path: string;
  text: string;
  language: "json" | "lua" | "toml" | "text";
};

/* Resolution previews against saved request contexts, computed server-side
   by the real runtime. */
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

export type QualifierContextEvaluation = {
  name: string;
  path: string;
  evaluation: QualifierEvaluation | null;
  error?: string;
};

export type EditContextPreview = {
  name: string;
  qualifierTruth: Record<string, boolean>;
};

/* Draft editing: each editable entity arrives with its draft-branch text. */
export type EditableEntity = {
  section: "variables" | "qualifiers" | "catalogs" | "schemas" | "context" | "linters";
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
export type ConsoleData = {
  repos: RepoWithWorkspaces[];
  workspaces: WorkspaceRecord[];
  drafts: Array<{ draft: DraftSessionRecord; workspace: WorkspaceRecord }>;
};

export type WorkspaceSummary = {
  variables: number;
  qualifiers: number;
  catalogs: number;
  schemas: number;
  error: string | null;
};

export type WorkspaceData = {
  workspace: WorkspaceRecord;
  drafts: DraftSessionRecord[];
  inventory: WorkspaceInventory;
  inventoryError: string | null;
  lint: WorkspaceLintLoad;
  model: WorkspaceSemanticModel | null;
};

export type WorkspaceEntityData = {
  definition: WorkspaceDefinition | null;
  definitionError: string | null;
  contextResolutions: SavedContextResolution[];
  qualifierEvaluations: QualifierContextEvaluation[];
};

export type DraftData = {
  workspace: WorkspaceRecord;
  draft: DraftSessionRecord;
  prSyncError: string | null;
  changes: DraftChangeRecord[];
  events: DraftEventRecord[];
  lint: WorkspaceLintLoad;
  model: WorkspaceSemanticModel | null;
  entities: EditableEntity[];
  editLoadError: string | null;
  editedPaths: string[];
};

export type DraftEntityData = {
  baseText: string | null;
  contextPreviews: EditContextPreview[];
};
