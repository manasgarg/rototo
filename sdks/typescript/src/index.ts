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
