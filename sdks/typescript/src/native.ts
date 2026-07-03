import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

type NativePackage = {
    root(): string;
    identity(): PackageIdentityJson;
    lint(): Promise<PackageLintJson>;
    semanticModel(): Promise<JsonValue>;
    resolveVariable(
        id: string,
        context: JsonValue,
        validateContext?: boolean,
        trace?: boolean,
    ): VariableResolutionJson;
    subscribeTraceEvents(): NativeTraceEvents;
};

type NativePackageConstructor = {
    load(
        source: string,
        packageToken?: string,
        lint?: "deny" | "skip",
    ): Promise<NativePackage>;
    inspect(source: string, packageToken?: string): Promise<NativePackage>;
};

type NativeRefreshingPackage = {
    resolveVariable(
        id: string,
        context: JsonValue,
        validateContext?: boolean,
        trace?: boolean,
    ): VariableResolutionJson;
    refreshNow(): Promise<RefreshOutcome>;
    status(): Promise<RefreshStatusJson>;
    identity(): Promise<PackageIdentityJson>;
    snapshot(): Promise<RefreshSnapshotJson>;
    subscribeEvents(): NativeRefreshEvents;
    subscribeTraceEvents(): NativeTraceEvents;
    shutdown(): Promise<void>;
};

type NativeRefreshEvents = {
    recv(): Promise<RefreshEventJson | null>;
};

type NativeTraceEvents = {
    recv(): Promise<JsonValue | null>;
};

type NativeRefreshingPackageConstructor = {
    load(
        source: string,
        periodSeconds?: number,
        packageToken?: string,
        lint?: "deny" | "skip",
    ): Promise<NativeRefreshingPackage>;
};

export type NativeModule = {
    version(): string;
    _Package: NativePackageConstructor;
    _RefreshingPackage: NativeRefreshingPackageConstructor;
};

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue =
    | JsonPrimitive
    | JsonValue[]
    | { [key: string]: JsonValue };
export type JsonObject = { [key: string]: JsonValue };

export type VariableResolutionJson = {
    id: string;
    value: JsonValue;
    source: VariableResolutionSourceJson;
};

export type VariableResolutionSourceJson =
    | { kind: "literal" }
    | { kind: "catalog"; catalog: string; value: string };

export type RefreshOutcome = "unchanged" | "refreshed" | "immutable";

export type RefreshStatusJson = {
    currentFingerprint: JsonValue | null;
    lastSuccess: number | null;
    lastAttempt: number | null;
    consecutiveFailures: number;
    lastError: string | null;
    refreshing: boolean;
    immutable: boolean;
};

export type PackageLayerIdentityJson = {
    source: string;
    fingerprint: JsonValue | null;
    releaseId: string | null;
    immutable: boolean;
};

export type PackageIdentityJson = {
    source: string;
    fingerprint: JsonValue | null;
    releaseId: string | null;
    loadedAt: number;
    immutable: boolean;
    layers: PackageLayerIdentityJson[];
};

export type RefreshEventSummaryJson = {
    eventId: string;
    eventType: string;
    releaseId: string | null;
    completedAt: number;
};

export type RefreshSnapshotJson = {
    identity: PackageIdentityJson;
    lastAttempt: number | null;
    lastSuccess: number | null;
    lastEvent: RefreshEventSummaryJson | null;
    consecutiveFailures: number;
    lastError: string | null;
    refreshing: boolean;
    immutable: boolean;
};

export type SdkIdentityJson = {
    name: string;
    version: string;
    language: string;
};

export type RefreshEventJson = {
    schemaVersion: number;
    eventId: string;
    eventType: string;
    source: string;
    previous: PackageIdentityJson | null;
    current: PackageIdentityJson | null;
    attemptedAt: number;
    completedAt: number;
    durationMs: number;
    outcome: RefreshOutcome | null;
    consecutiveFailures: number;
    error: string | null;
    sdk: SdkIdentityJson;
};

export type PackageLintJson = {
    root: string;
    diagnostics: JsonValue[];
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
        `failed to load rototo native module:\n${errors.join("\n")}`,
    );
}

function nativeCandidates(): string[] {
    const platform = process.platform;
    const arch = process.arch;

    if (platform === "linux") {
        const linuxArch =
            arch === "x64" ? "x64" : arch === "arm64" ? "arm64" : arch;
        const libc = isMusl() ? "musl" : "gnu";
        return [
            `../rototo.linux-${linuxArch}-${libc}.node`,
            `../rototo.linux-${linuxArch}-gnu.node`,
            `../rototo.linux-${linuxArch}.node`,
            "../rototo.node",
        ];
    }

    if (platform === "darwin") {
        const darwinArch =
            arch === "x64" ? "x64" : arch === "arm64" ? "arm64" : arch;
        return [`../rototo.darwin-${darwinArch}.node`, "../rototo.node"];
    }

    if (platform === "win32") {
        const windowsArch = arch === "x64" ? "x64" : arch;
        return [`../rototo.win32-${windowsArch}-msvc.node`, "../rototo.node"];
    }

    return ["../rototo.node"];
}

function isMusl(): boolean {
    if (process.platform !== "linux") {
        return false;
    }
    const report = process.report?.getReport() as
        | { header?: { glibcVersionRuntime?: string } }
        | undefined;
    return !report?.header?.glibcVersionRuntime;
}
