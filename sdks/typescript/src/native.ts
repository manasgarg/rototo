import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

type NativeWorkspace = {
    root(): string;
    lint(): Promise<WorkspaceLintJson>;
    semanticModel(): Promise<JsonValue>;
    resolveVariable(
        id: string,
        context: JsonValue,
        validateContext?: boolean,
    ): Promise<VariableResolutionJson>;
    resolveQualifier(
        id: string,
        context: JsonValue,
        validateContext?: boolean,
    ): Promise<QualifierResolutionJson>;
};

type NativeWorkspaceConstructor = {
    load(
        source: string,
        workspaceToken?: string,
        lint?: "deny" | "skip",
    ): Promise<NativeWorkspace>;
    inspect(source: string, workspaceToken?: string): Promise<NativeWorkspace>;
};

type NativeRefreshingWorkspace = {
    resolveVariable(
        id: string,
        context: JsonValue,
        validateContext?: boolean,
    ): Promise<VariableResolutionJson>;
    resolveQualifier(
        id: string,
        context: JsonValue,
        validateContext?: boolean,
    ): Promise<QualifierResolutionJson>;
    refreshNow(): Promise<RefreshOutcome>;
    status(): Promise<RefreshStatusJson>;
    shutdown(): Promise<void>;
};

type NativeRefreshingWorkspaceConstructor = {
    load(
        source: string,
        periodSeconds?: number,
        workspaceToken?: string,
        lint?: "deny" | "skip",
    ): Promise<NativeRefreshingWorkspace>;
};

export type NativeModule = {
    version(): string;
    _Workspace: NativeWorkspaceConstructor;
    _RefreshingWorkspace: NativeRefreshingWorkspaceConstructor;
};

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue =
    | JsonPrimitive
    | JsonValue[]
    | { [key: string]: JsonValue };
export type JsonObject = { [key: string]: JsonValue };

export type VariableResolutionJson = {
    id: string;
    valueKey: string;
    value: JsonValue;
};

export type QualifierResolutionJson = {
    id: string;
    value: boolean;
};

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

export type WorkspaceLintJson = {
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
