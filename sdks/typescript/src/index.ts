import {
  type JsonObject,
  type JsonValue,
  type QualifierResolutionJson,
  type RefreshOutcome,
  type RefreshStatusJson,
  type VariableResolutionJson,
  type WorkspaceLintJson,
  native,
} from "./native.js";

export type {
  JsonObject,
  JsonValue,
  QualifierResolutionJson,
  RefreshOutcome,
  RefreshStatusJson,
  VariableResolutionJson,
  WorkspaceLintJson,
};

export type LintMode = "deny" | "skip";

export type LoadOptions = {
  workspaceToken?: string;
  lint?: LintMode;
};

export type InspectOptions = {
  workspaceToken?: string;
};

export type ResolveOptions = {
  validateContext?: boolean;
};

export type RefreshingWorkspaceOptions = LoadOptions & {
  periodSeconds?: number;
};

export type VariableResolution = {
  id: string;
  valueKey: string;
  value: JsonValue;
};

export type QualifierResolution = {
  id: string;
  value: boolean;
};

export type RefreshStatus = {
  currentFingerprint: JsonValue | null;
  lastSuccess: number | null;
  lastAttempt: number | null;
  consecutiveFailures: number;
  lastError: string | null;
  refreshing: boolean;
  immutable: boolean;
};

export type WorkspaceLint = {
  root: string;
  diagnostics: JsonValue[];
};

export type ModelPosition = { line: number; character: number };

export type ModelRange = { start: ModelPosition; end: ModelPosition };

export type ModelLocation = { path: string; range?: ModelRange };

/* A scalar field as rototo parsed it: `value` is present only when the field
   had the expected shape; the location always points at the field. */
export type ModelField = { value?: string; location: ModelLocation };

export type QualifierModel = {
  id: string;
  location: ModelLocation;
  description?: string;
  predicates: Array<{
    index: number;
    location: ModelLocation;
    attribute?: ModelField;
    op?: ModelField;
    value?: JsonValue;
  }>;
};

export type VariableModel = {
  id: string;
  location: ModelLocation;
  description?: string;
  declaration: {
    kind: "primitive" | "catalog" | "schema" | "missing" | "conflict" | "invalid";
    value?: string;
    location: ModelLocation;
  };
  values: Array<{ key: string; location: ModelLocation; value: JsonValue }>;
  valuesSection?: ModelLocation;
  resolve?: {
    location: ModelLocation;
    default?: ModelField;
    rules: Array<{
      index: number;
      location: ModelLocation;
      qualifier?: ModelField;
      value?: ModelField;
    }>;
  };
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
  value: JsonValue;
};

export type SchemaModel = {
  path: string;
  location: ModelLocation;
  json?: JsonValue;
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
  /* Where in the source entity the reference sits, for semantic display. */
  via: ModelReferenceVia;
};

/* The serializable projection of rototo's semantic and reference indexes.
   Tools consume this instead of parsing workspace files themselves. */
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

export class RototoError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "RototoError";
  }
}

export const VERSION = native.version();
export const __version__ = VERSION;

export function version(): string {
  return VERSION;
}

export class Workspace {
  private constructor(private readonly inner: NativeWorkspaceHandle) {}

  static async load(source: string, options: LoadOptions = {}): Promise<Workspace> {
    try {
      const inner = await native._Workspace.load(
        String(source),
        options.workspaceToken,
        options.lint ?? "deny",
      );
      return new Workspace(inner);
    } catch (error) {
      throw toRototoError(error);
    }
  }

  static async inspect(
    source: string,
    options: InspectOptions = {},
  ): Promise<Workspace> {
    try {
      const inner = await native._Workspace.inspect(
        String(source),
        options.workspaceToken,
      );
      return new Workspace(inner);
    } catch (error) {
      throw toRototoError(error);
    }
  }

  get root(): string {
    return this.inner.root();
  }

  async lint(): Promise<WorkspaceLint> {
    try {
      return await this.inner.lint();
    } catch (error) {
      throw toRototoError(error);
    }
  }

  async semanticModel(): Promise<WorkspaceSemanticModel> {
    try {
      return (await this.inner.semanticModel()) as WorkspaceSemanticModel;
    } catch (error) {
      throw toRototoError(error);
    }
  }

  async resolveVariable(
    id: string,
    context: JsonObject,
    options: ResolveOptions = {},
  ): Promise<VariableResolution> {
    try {
      return await this.inner.resolveVariable(
        id,
        context,
        options.validateContext ?? true,
      );
    } catch (error) {
      throw toRototoError(error);
    }
  }

  async resolveQualifier(
    id: string,
    context: JsonObject,
    options: ResolveOptions = {},
  ): Promise<QualifierResolution> {
    try {
      return await this.inner.resolveQualifier(
        id,
        context,
        options.validateContext ?? true,
      );
    } catch (error) {
      throw toRototoError(error);
    }
  }
}

export class RefreshingWorkspace {
  private constructor(private readonly inner: NativeRefreshingWorkspaceHandle) {}

  static async load(
    source: string,
    options: RefreshingWorkspaceOptions = {},
  ): Promise<RefreshingWorkspace> {
    try {
      const inner = await native._RefreshingWorkspace.load(
        String(source),
        options.periodSeconds,
        options.workspaceToken,
        options.lint ?? "deny",
      );
      return new RefreshingWorkspace(inner);
    } catch (error) {
      throw toRototoError(error);
    }
  }

  async resolveVariable(
    id: string,
    context: JsonObject,
    options: ResolveOptions = {},
  ): Promise<VariableResolution> {
    try {
      return await this.inner.resolveVariable(
        id,
        context,
        options.validateContext ?? true,
      );
    } catch (error) {
      throw toRototoError(error);
    }
  }

  async resolveQualifier(
    id: string,
    context: JsonObject,
    options: ResolveOptions = {},
  ): Promise<QualifierResolution> {
    try {
      return await this.inner.resolveQualifier(
        id,
        context,
        options.validateContext ?? true,
      );
    } catch (error) {
      throw toRototoError(error);
    }
  }

  async refreshNow(): Promise<RefreshOutcome> {
    try {
      return await this.inner.refreshNow();
    } catch (error) {
      throw toRototoError(error);
    }
  }

  async status(): Promise<RefreshStatus> {
    try {
      return await this.inner.status();
    } catch (error) {
      throw toRototoError(error);
    }
  }

  async shutdown(): Promise<void> {
    try {
      await this.inner.shutdown();
    } catch (error) {
      throw toRototoError(error);
    }
  }
}

type NativeWorkspaceHandle = Awaited<ReturnType<typeof native._Workspace.load>>;
type NativeRefreshingWorkspaceHandle = Awaited<
  ReturnType<typeof native._RefreshingWorkspace.load>
>;

function toRototoError(error: unknown): RototoError {
  if (error instanceof RototoError) {
    return error;
  }
  const message = error instanceof Error ? error.message : String(error);
  return new RototoError(message, error instanceof Error ? { cause: error } : undefined);
}
