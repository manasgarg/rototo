// Typed loader for the internal console bindings (rototo-console-native).
// The binary is built from ../Cargo.toml by `napi build --platform`; unlike
// the public SDK there are no prebuilt fallbacks, because this module ships
// inside the console server, never as a standalone package.

import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue =
    JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };
export type JsonObject = { [key: string]: JsonValue };

export type InspectRequest = {
    variables?: "all" | string[] | null;
    catalogs?: "all" | string[] | null;
    lintRules?: "all" | string[] | null;
    lintAuthorities?: "all" | string[] | null;
    linters?: "all" | string[] | null;
    context?: JsonObject;
};

export type EditPlanJson = {
    writes: { path: string; content: string }[];
    deletes: string[];
};

export type ChangeRecordJson = {
    operation: string;
    address: string;
    before?: JsonValue;
    after?: JsonValue;
};

export type EditOutcomeJson = {
    plan: EditPlanJson;
    records: ChangeRecordJson[];
};

export type PackageLintJson = {
    documents: JsonValue[];
    diagnostics: JsonValue[];
};

export type NativePinStore = {
    stage(remote: string, pin: string, token?: string): Promise<string>;
};

type NativePinStoreConstructor = new (
    root: string,
    maxBytes?: number,
) => NativePinStore;

/// One in-process LSP session. `send` writes a JSON-RPC message (as a
/// serialized string); `receive` yields the next server message, or null
/// when the session ends. Drive receive from a single pump loop.
export type NativeLspSession = {
    send(message: string): Promise<void>;
    receive(): Promise<string | null>;
};

type NativeLspSessionConstructor = new () => NativeLspSession;

export type TraceOutcomeJson = {
    id: string;
    trace?: JsonValue;
    error?: string;
};

export type UpcomingChangeJson = {
    variable: string;
    site:
        | { kind: "rule"; index: number }
        | { kind: "queryFilter" }
        | { kind: "querySort" };
    boundary: string;
    comparison: string;
    expression: string;
    location: { path: string; range?: JsonValue };
};

export type ResolveFixtureJson = {
    target: { kind: "variable"; id: string };
    caseId: string;
    title: string;
    because: string | null;
    context: JsonObject;
    expect: JsonValue;
};

export type NativeModule = {
    version(): string;
    buildProfile(): "release" | "debug";
    _PinStore: NativePinStoreConstructor;
    discoverPackages(root: string): Promise<string[]>;
    semanticModel(root: string): Promise<JsonValue>;
    lintPackage(root: string): Promise<PackageLintJson>;
    inspectReport(root: string, request?: InspectRequest): Promise<JsonValue>;
    diffPackages(
        beforeRoot: string,
        afterRoot: string,
        context?: JsonObject,
    ): Promise<JsonValue>;
    applyEdit(
        root: string,
        operations: JsonValue[],
        options?: { inherited?: string[] },
    ): Promise<EditOutcomeJson>;
    traceResolutions(root: string, context: JsonObject): Promise<JsonValue[]>;
    traceResolution(
        root: string,
        variable: string,
        context: JsonObject,
    ): Promise<JsonValue>;
    traceResolutionOutcomes(
        root: string,
        context: JsonObject,
    ): Promise<TraceOutcomeJson[]>;
    upcomingChanges(root: string, now: string): Promise<UpcomingChangeJson[]>;
    resolveFixtures(
        root: string,
        variables?: string[],
    ): Promise<ResolveFixtureJson[]>;
    LspSession: NativeLspSessionConstructor;
};

export const native: NativeModule = loadNative();

function loadNative(): NativeModule {
    const errors: string[] = [];
    for (const candidate of nativeCandidates()) {
        try {
            return require(candidate) as NativeModule;
        } catch (error) {
            const message =
                error instanceof Error ? error.message : String(error);
            errors.push(`${candidate}: ${message}`);
        }
    }
    throw new Error(
        `failed to load the rototo-console-native module; run 'npm run build:native' in apps/console-server first:\n${errors.join("\n")}`,
    );
}

function nativeCandidates(): string[] {
    const platform = process.platform;
    const arch = process.arch;
    if (platform === "linux") {
        const libc = isMusl() ? "musl" : "gnu";
        return [
            `../rototo-console-native.linux-${arch}-${libc}.node`,
            `../rototo-console-native.linux-${arch}-gnu.node`,
        ];
    }
    if (platform === "darwin") {
        return [`../rototo-console-native.darwin-${arch}.node`];
    }
    return [`../rototo-console-native.${platform}-${arch}.node`];
}

function isMusl(): boolean {
    if (process.platform !== "linux") {
        return false;
    }
    const report = process.report?.getReport() as
        { header?: { glibcVersionRuntime?: string } } | undefined;
    return !report?.header?.glibcVersionRuntime;
}
