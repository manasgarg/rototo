import {
    type JsonObject,
    type JsonValue,
    type RefreshOutcome,
    type RefreshStatusJson,
    type PackageIdentityJson,
    type PackageLayerIdentityJson,
    type RefreshSnapshotJson,
    type RefreshEventJson,
    type RefreshEventSummaryJson,
    type SdkIdentityJson,
    type VariableResolutionJson,
    type VariableResolutionSourceJson,
    type PackageLintJson,
    native,
} from "./native.js";

export type {
    JsonObject,
    JsonValue,
    RefreshOutcome,
    RefreshStatusJson,
    PackageIdentityJson,
    PackageLayerIdentityJson,
    RefreshSnapshotJson,
    RefreshEventJson,
    RefreshEventSummaryJson,
    SdkIdentityJson,
    VariableResolutionJson,
    VariableResolutionSourceJson,
    PackageLintJson,
};

export type PackageLayerIdentity = PackageLayerIdentityJson;
export type PackageIdentity = PackageIdentityJson;
export type RefreshEventSummary = RefreshEventSummaryJson;
export type RefreshSnapshot = RefreshSnapshotJson;
export type RefreshEvent = RefreshEventJson;
export type SdkIdentity = SdkIdentityJson;

/** One item from a resolution trace stream: a captured trace, or a marker that
 * a lagging consumer dropped `count` traces. */
export type TraceStreamItem =
    | { kind: "trace"; trace: Record<string, unknown> }
    | { kind: "dropped"; count: number };

export type LintMode = "deny" | "skip";

export type LoadOptions = {
    packageToken?: string;
    lint?: LintMode;
};

export type InspectOptions = {
    packageToken?: string;
};

export type ResolveOptions = {
    validateContext?: boolean;
    /** Emit a resolution trace for this call onto the trace stream. Only
     * produces output while something is subscribed via `traceEvents()`. */
    trace?: boolean;
    /** Scope the resolution to one tenant, whose id expressions read as
     * `env.tenant`. */
    tenant?: string;
};

export type RefreshingPackageOptions = LoadOptions & {
    periodSeconds?: number;
};

export type VariableResolution = {
    id: string;
    value: JsonValue;
    source: VariableResolutionSourceJson;
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

export type PackageLint = {
    root: string;
    diagnostics: JsonValue[];
};

export type ModelPosition = { line: number; character: number };

export type ModelRange = { start: ModelPosition; end: ModelPosition };

export type ModelLocation = { path: string; range?: ModelRange };

/* A scalar field as rototo parsed it: `value` is present only when the field
   had the expected shape; the location always points at the field. */
export type ModelField = { value?: string; location: ModelLocation };
export type ModelValueField = { value?: JsonValue; location: ModelLocation };

export type VariableModel = {
    id: string;
    location: ModelLocation;
    description?: string;
    declaration: {
        kind:
            | "primitive"
            | "catalog"
            | "schema"
            | "missing"
            | "conflict"
            | "invalid";
        value?: string;
        location: ModelLocation;
    };
    values: Array<{ key: string; location: ModelLocation; value: JsonValue }>;
    valuesSection?: ModelLocation;
    resolve?: {
        location: ModelLocation;
        method?: ModelField;
        default?: ModelValueField;
        rules: Array<{
            index: number;
            location: ModelLocation;
            when?: ModelField;
            value?: ModelValueField;
        }>;
        query?: QueryModel;
        allocation?: ModelField;
        assigns: Array<{
            location: ModelLocation;
            arm?: string;
            value?: JsonValue;
        }>;
    };
};

/* The `method = "query"` parameters on `[resolve]`. */
export type QueryModel = {
    from?: ModelField;
    filter?: ModelField;
    sort?: ModelField;
    order?: ModelField;
    limit?: ModelField;
};

export type LayerModel = {
    id: string;
    location: ModelLocation;
    description?: string;
    unit?: ModelField;
    buckets?: number;
    allocations: Array<{
        location: ModelLocation;
        id?: string;
        status?: string;
        eligibility?: ModelField;
        arms: Array<{
            location: ModelLocation;
            name?: string;
            buckets?: string;
        }>;
    }>;
};

export type CatalogModel = {
    id: string;
    path: string;
    location: ModelLocation;
    description?: string;
    json?: JsonValue;
};

export type CatalogEntryModel = {
    catalog: string;
    key: string;
    location: ModelLocation;
    value: JsonValue;
};

export type EvaluationContextModel = {
    id: string;
    path: string;
    location: ModelLocation;
    title?: string;
    description?: string;
    json?: JsonValue;
};

export type EvaluationContextSampleModel = {
    evaluationContext: string;
    key: string;
    path: string;
    location: ModelLocation;
    value?: JsonValue;
};

export type VariableEvaluationContextModel = {
    variable: string;
    evaluationContexts: string[];
};

export type LinterModel = {
    path: string;
    location: ModelLocation;
    rules: Array<{ id: string; title: string; help: string }>;
};

export type ModelEntityRef =
    | { kind: "variable"; id: string }
    | { kind: "allocation"; id: string }
    | { kind: "catalog"; id: string }
    | { kind: "catalogEntry"; catalog: string; key: string }
    | { kind: "evaluationContext"; id: string }
    | {
          kind: "evaluationContextSample";
          evaluationContext: string;
          key: string;
      }
    | { kind: "value"; variable: string; key: string }
    | { kind: "contextAttribute"; name: string };

export type ModelReferenceVia =
    | { kind: "variableCatalog" }
    | { kind: "resolveDefault" }
    | { kind: "ruleCondition"; index: number }
    | { kind: "ruleValue"; index: number }
    | { kind: "query" }
    | { kind: "allocation" };

export type ReferenceModel = {
    from: ModelEntityRef;
    to: ModelEntityRef;
    location: ModelLocation;
    /* Where in the source entity the reference sits, for semantic display. */
    via: ModelReferenceVia;
};

/* The serializable projection of rototo's semantic and reference indexes.
   Tools consume this instead of parsing package files themselves. */
export type PackageSemanticModel = {
    version: number;
    variables: VariableModel[];
    layers: LayerModel[];
    catalogs: CatalogModel[];
    catalogEntries: CatalogEntryModel[];
    evaluationContexts: EvaluationContextModel[];
    evaluationContextSamples: EvaluationContextSampleModel[];
    linters: LinterModel[];
    references: ReferenceModel[];
    variableEvaluationContexts: VariableEvaluationContextModel[];
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

export class Package {
    private constructor(private readonly inner: NativePackageHandle) {}

    static async load(
        source: string,
        options: LoadOptions = {},
    ): Promise<Package> {
        try {
            const inner = await native._Package.load(
                String(source),
                options.packageToken,
                options.lint ?? "deny",
            );
            return new Package(inner);
        } catch (error) {
            throw toRototoError(error);
        }
    }

    static async inspect(
        source: string,
        options: InspectOptions = {},
    ): Promise<Package> {
        try {
            const inner = await native._Package.inspect(
                String(source),
                options.packageToken,
            );
            return new Package(inner);
        } catch (error) {
            throw toRototoError(error);
        }
    }

    get root(): string {
        return this.inner.root();
    }

    identity(): PackageIdentity {
        return this.inner.identity();
    }

    async lint(): Promise<PackageLint> {
        try {
            return await this.inner.lint();
        } catch (error) {
            throw toRototoError(error);
        }
    }

    async semanticModel(): Promise<PackageSemanticModel> {
        try {
            return (await this.inner.semanticModel()) as PackageSemanticModel;
        } catch (error) {
            throw toRototoError(error);
        }
    }

    resolveVariable(
        id: string,
        context: JsonObject,
        options: ResolveOptions = {},
    ): VariableResolution {
        try {
            return this.inner.resolveVariable(
                id,
                context,
                options.validateContext ?? true,
                options.trace ?? false,
                options.tenant,
            );
        } catch (error) {
            throw toRototoError(error);
        }
    }

    /* Yield resolution trace stream items as they occur. Tracing is computed
       only while this iterator is consumed; with no subscriber a `[[trace]]`
       policy costs nothing. */
    async *traceEvents(): AsyncGenerator<TraceStreamItem, void, void> {
        const events = this.inner.subscribeTraceEvents();
        for (;;) {
            let item: TraceStreamItem | null;
            try {
                item =
                    (await events.recv()) as unknown as TraceStreamItem | null;
            } catch (error) {
                throw toRototoError(error);
            }
            if (item === null) {
                return;
            }
            yield item;
        }
    }
}

export class RefreshingPackage {
    private constructor(
        private readonly inner: NativeRefreshingPackageHandle,
    ) {}

    static async load(
        source: string,
        options: RefreshingPackageOptions = {},
    ): Promise<RefreshingPackage> {
        try {
            const inner = await native._RefreshingPackage.load(
                String(source),
                options.periodSeconds,
                options.packageToken,
                options.lint ?? "deny",
            );
            return new RefreshingPackage(inner);
        } catch (error) {
            throw toRototoError(error);
        }
    }

    resolveVariable(
        id: string,
        context: JsonObject,
        options: ResolveOptions = {},
    ): VariableResolution {
        try {
            return this.inner.resolveVariable(
                id,
                context,
                options.validateContext ?? true,
                options.trace ?? false,
                options.tenant,
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

    async identity(): Promise<PackageIdentity> {
        try {
            return await this.inner.identity();
        } catch (error) {
            throw toRototoError(error);
        }
    }

    async snapshot(): Promise<RefreshSnapshot> {
        try {
            return await this.inner.snapshot();
        } catch (error) {
            throw toRototoError(error);
        }
    }

    /* Yield refresh events as they occur. The stream ends when the package is
       shut down. A lagging consumer skips dropped events rather than erroring;
       recover ground truth from `snapshot()`. */
    async *refreshEvents(): AsyncGenerator<RefreshEvent, void, void> {
        const events = this.inner.subscribeEvents();
        for (;;) {
            let event: RefreshEvent | null;
            try {
                event = await events.recv();
            } catch (error) {
                throw toRototoError(error);
            }
            if (event === null) {
                return;
            }
            yield event;
        }
    }

    /* Yield resolution trace stream items as they occur. */
    async *traceEvents(): AsyncGenerator<TraceStreamItem, void, void> {
        const events = this.inner.subscribeTraceEvents();
        for (;;) {
            let item: TraceStreamItem | null;
            try {
                item =
                    (await events.recv()) as unknown as TraceStreamItem | null;
            } catch (error) {
                throw toRototoError(error);
            }
            if (item === null) {
                return;
            }
            yield item;
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

type NativePackageHandle = Awaited<ReturnType<typeof native._Package.load>>;
type NativeRefreshingPackageHandle = Awaited<
    ReturnType<typeof native._RefreshingPackage.load>
>;

function toRototoError(error: unknown): RototoError {
    if (error instanceof RototoError) {
        return error;
    }
    const message = error instanceof Error ? error.message : String(error);
    return new RototoError(
        message,
        error instanceof Error ? { cause: error } : undefined,
    );
}
