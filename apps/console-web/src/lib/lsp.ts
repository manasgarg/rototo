// One language-server file session for an editor: the console-server bridge
// hosts a real rototo LSP per session, and this class owns the client half.
// The buffer rides as an overlay (didOpen once, debounced full-document
// didChange while typing), diagnostics arrive by polling the bridge's
// notification queue, and completion/hover are plain requests. Everything
// fails soft: a dead bridge just means the static lint diagnostics stand.

import {
    closeLspSession,
    lspNotifications,
    lspNotify,
    lspRequest,
    openLspSession,
} from "@/lib/api";

export type LspPosition = { line: number; character: number };
export type LspRange = { start: LspPosition; end: LspPosition };

export type LspDiagnostic = {
    range: LspRange;
    /** LSP severity: 1 error, 2 warning. */
    severity?: number;
    code?: string;
    message: string;
    data?: { help?: string };
};

export type LspCompletionItem = {
    label: string;
    kind?: number;
    detail?: string;
    insertText?: string;
    filterText?: string;
};

export type LspHover = {
    value: string;
    range?: LspRange | null;
};

const CHANGE_DEBOUNCE_MS = 300;
const POLL_INTERVAL_MS = 500;

export class LspFile {
    private readonly file: string;
    private sessionId: string | null = null;
    private version = 1;
    private opened = false;
    private disposed = false;
    private pendingText: string | null = null;
    private debounce: ReturnType<typeof setTimeout> | null = null;
    private pollTimer: ReturnType<typeof setInterval> | null = null;
    private readonly sinks = new Set<(diagnostics: LspDiagnostic[]) => void>();

    constructor(
        treeId: string,
        packagePath: string,
        pin: string,
        file: string,
    ) {
        this.file = file;
        openLspSession(treeId, packagePath, pin).then(
            (response) => {
                if (this.disposed) {
                    void closeLspSession(response.session).catch(() => {});
                    return;
                }
                this.sessionId = response.session;
                if (this.pendingText !== null) {
                    this.flush(this.pendingText);
                    this.pendingText = null;
                }
                this.pollTimer = setInterval(
                    () => this.poll(),
                    POLL_INTERVAL_MS,
                );
            },
            () => {},
        );
    }

    /** Push the full buffer; the first push opens the overlay. */
    update(text: string): void {
        if (this.disposed) {
            return;
        }
        if (this.sessionId === null) {
            this.pendingText = text;
            return;
        }
        if (!this.opened) {
            this.flush(text);
            return;
        }
        if (this.debounce !== null) {
            clearTimeout(this.debounce);
        }
        this.debounce = setTimeout(() => this.flush(text), CHANGE_DEBOUNCE_MS);
    }

    /** Register a diagnostics sink; returns the unsubscribe. */
    onDiagnostics(sink: (diagnostics: LspDiagnostic[]) => void): () => void {
        this.sinks.add(sink);
        return () => {
            this.sinks.delete(sink);
        };
    }

    async completion(position: LspPosition): Promise<LspCompletionItem[]> {
        if (this.sessionId === null || !this.opened) {
            return [];
        }
        const { result } = await lspRequest<
            LspCompletionItem[] | { items?: LspCompletionItem[] } | null
        >(this.sessionId, "textDocument/completion", {
            textDocument: { uri: this.file },
            position,
        });
        if (Array.isArray(result)) {
            return result;
        }
        return result?.items ?? [];
    }

    async hover(position: LspPosition): Promise<LspHover | null> {
        if (this.sessionId === null || !this.opened) {
            return null;
        }
        const { result } = await lspRequest<{
            contents?: { value?: string } | string;
            range?: LspRange;
        } | null>(this.sessionId, "textDocument/hover", {
            textDocument: { uri: this.file },
            position,
        });
        if (result == null) {
            return null;
        }
        const value =
            typeof result.contents === "string"
                ? result.contents
                : (result.contents?.value ?? "");
        return value === "" ? null : { value, range: result.range ?? null };
    }

    dispose(): void {
        this.disposed = true;
        if (this.debounce !== null) {
            clearTimeout(this.debounce);
        }
        if (this.pollTimer !== null) {
            clearInterval(this.pollTimer);
        }
        this.sinks.clear();
        if (this.sessionId !== null) {
            void closeLspSession(this.sessionId).catch(() => {});
            this.sessionId = null;
        }
    }

    private flush(text: string): void {
        if (this.sessionId === null || this.disposed) {
            return;
        }
        if (!this.opened) {
            this.opened = true;
            void lspNotify(this.sessionId, "textDocument/didOpen", {
                textDocument: {
                    uri: this.file,
                    languageId: languageIdFor(this.file),
                    version: this.version,
                    text,
                },
            }).catch(() => {});
            return;
        }
        this.version += 1;
        void lspNotify(this.sessionId, "textDocument/didChange", {
            textDocument: { uri: this.file, version: this.version },
            contentChanges: [{ text }],
        }).catch(() => {});
    }

    private poll(): void {
        if (this.sessionId === null || this.disposed) {
            return;
        }
        lspNotifications(this.sessionId).then(
            (response) => {
                for (const message of response.notifications) {
                    if (
                        message.method !== "textDocument/publishDiagnostics" ||
                        message.params?.uri !== this.file
                    ) {
                        continue;
                    }
                    const params = message.params as {
                        diagnostics?: LspDiagnostic[];
                        version?: number;
                    };
                    // Diagnostics for an older buffer would squiggle the
                    // wrong ranges; the next build reports again.
                    if (
                        params.version !== undefined &&
                        params.version !== this.version
                    ) {
                        continue;
                    }
                    for (const sink of this.sinks) {
                        sink(params.diagnostics ?? []);
                    }
                }
            },
            () => {},
        );
    }
}

function languageIdFor(file: string): string {
    if (file.endsWith(".json")) {
        return "json";
    }
    if (file.endsWith(".lua")) {
        return "lua";
    }
    return "toml";
}
