import {
    type JsonObject,
    type JsonValue,
    type RefreshOutcome,
    type RefreshStatusJson,
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
    VariableResolutionJson,
    VariableResolutionSourceJson,
    PackageLintJson,
};

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
        default?: ModelValueField;
        rules: Array<{
            index: number;
            location: ModelLocation;
            qualifier?: ModelField;
            value?: ModelValueField;
        }>;
    };
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
   Tools consume this instead of parsing package files themselves. */
export type PackageSemanticModel = {
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
    ): Promise<boolean> {
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
    ): Promise<boolean> {
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
