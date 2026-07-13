// The in-process LSP bridge (design/console-semantic.md "LSP"): one real
// rototo language server per editing session, hosted inside the native
// module over a duplex transport. The bridge owns JSON-RPC id correlation
// and notification fan-out; unsaved editor text rides as LSP overlays, so
// diagnostics, completion, hover, definition, and references all see what
// the editor sees, not what the staged tree holds.
//
// The wire to the browser speaks package-relative file paths. The bridge
// rewrites them to file:// URIs on the staged root going in and strips the
// root coming out, so the client never learns server filesystem layout.

import { randomBytes } from "node:crypto";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { ApiError } from "./errors.ts";
import { native, type NativeLspSession } from "./native.ts";

const SESSION_TTL_MS = 10 * 60_000;
const REQUEST_TIMEOUT_MS = 10_000;
const MAX_QUEUED_NOTIFICATIONS = 200;

type Waiter = {
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
};

type BridgeSession = {
    id: string;
    // Who opened it; every later call must come from the same subject.
    subjectKey: string;
    root: string;
    rootUri: string;
    session: NativeLspSession;
    nextRequestId: number;
    pending: Map<number, Waiter>;
    notifications: unknown[];
    lastUsedAt: number;
    ended: boolean;
};

export class LspBridge {
    private readonly sessions = new Map<string, BridgeSession>();

    // Opens a session rooted at a staged package and completes the LSP
    // initialize handshake before returning.
    async open(root: string, subjectKey: string): Promise<string> {
        this.sweep();
        const session: BridgeSession = {
            id: `lsp_${randomBytes(8).toString("hex")}`,
            subjectKey,
            root,
            rootUri: pathToFileURL(root).href,
            session: new native.LspSession(),
            nextRequestId: 1,
            pending: new Map(),
            notifications: [],
            lastUsedAt: Date.now(),
            ended: false,
        };
        this.pump(session);
        this.sessions.set(session.id, session);
        try {
            await this.dispatch(session, "initialize", {
                rootUri: session.rootUri,
            });
            await this.sendNotification(session, "initialized", {});
        } catch (error) {
            this.sessions.delete(session.id);
            throw error;
        }
        return session.id;
    }

    async request(
        id: string,
        subjectKey: string,
        method: string,
        params: unknown,
    ): Promise<unknown> {
        const session = this.lookup(id, subjectKey);
        return this.dispatch(session, method, this.outbound(session, params));
    }

    async notify(
        id: string,
        subjectKey: string,
        method: string,
        params: unknown,
    ): Promise<void> {
        const session = this.lookup(id, subjectKey);
        await this.sendNotification(
            session,
            method,
            this.outbound(session, params),
        );
    }

    // Drains queued server-to-client notifications (publishDiagnostics,
    // mostly). The client polls this after edits settle.
    drain(id: string, subjectKey: string): unknown[] {
        const session = this.lookup(id, subjectKey);
        const drained = session.notifications;
        session.notifications = [];
        return drained;
    }

    async close(id: string, subjectKey: string): Promise<void> {
        const session = this.lookup(id, subjectKey);
        this.sessions.delete(id);
        await this.endSession(session);
    }

    private lookup(id: string, subjectKey: string): BridgeSession {
        const session = this.sessions.get(id);
        // An unknown and a foreign session answer identically, so session
        // ids cannot be probed.
        if (session === undefined || session.subjectKey !== subjectKey) {
            throw new ApiError(404, "no such editing session");
        }
        session.lastUsedAt = Date.now();
        return session;
    }

    private async dispatch(
        session: BridgeSession,
        method: string,
        params: unknown,
    ): Promise<unknown> {
        if (session.ended) {
            throw new ApiError(409, "the editing session has ended");
        }
        const requestId = session.nextRequestId++;
        const outcome = new Promise<unknown>((resolve, reject) => {
            session.pending.set(requestId, { resolve, reject });
            setTimeout(() => {
                if (session.pending.delete(requestId)) {
                    reject(
                        new ApiError(
                            504,
                            `the language server timed out on ${method}`,
                        ),
                    );
                }
            }, REQUEST_TIMEOUT_MS).unref();
        });
        await session.session.send(
            JSON.stringify({ jsonrpc: "2.0", id: requestId, method, params }),
        );
        return outcome;
    }

    private async sendNotification(
        session: BridgeSession,
        method: string,
        params: unknown,
    ): Promise<void> {
        if (session.ended) {
            throw new ApiError(409, "the editing session has ended");
        }
        await session.session.send(
            JSON.stringify({ jsonrpc: "2.0", method, params }),
        );
    }

    // One reader loop per session: responses settle their waiters,
    // everything else queues for the next drain.
    private pump(session: BridgeSession): void {
        void (async () => {
            for (;;) {
                const raw = await session.session.receive();
                if (raw === null) {
                    break;
                }
                const message = JSON.parse(raw) as {
                    id?: number;
                    result?: unknown;
                    error?: { message?: string };
                    method?: string;
                };
                if (message.id !== undefined && message.method === undefined) {
                    const waiter = session.pending.get(message.id);
                    session.pending.delete(message.id);
                    if (waiter === undefined) {
                        continue;
                    }
                    if (message.error !== undefined) {
                        waiter.reject(
                            new ApiError(
                                400,
                                message.error.message ??
                                    "the language server refused the request",
                            ),
                        );
                    } else {
                        waiter.resolve(this.inbound(session, message.result));
                    }
                    continue;
                }
                session.notifications.push(this.inbound(session, message));
                if (session.notifications.length > MAX_QUEUED_NOTIFICATIONS) {
                    session.notifications.splice(
                        0,
                        session.notifications.length - MAX_QUEUED_NOTIFICATIONS,
                    );
                }
            }
        })().finally(() => {
            session.ended = true;
            for (const waiter of session.pending.values()) {
                waiter.reject(new ApiError(409, "the editing session ended"));
            }
            session.pending.clear();
        });
    }

    private async endSession(session: BridgeSession): Promise<void> {
        if (session.ended) {
            return;
        }
        try {
            await this.dispatch(session, "shutdown", null);
            await this.sendNotification(session, "exit", null);
        } catch {
            // The pump observing EOF is what actually ends the session.
        }
    }

    private sweep(): void {
        const cutoff = Date.now() - SESSION_TTL_MS;
        for (const [id, session] of this.sessions) {
            if (session.lastUsedAt < cutoff) {
                this.sessions.delete(id);
                void this.endSession(session);
            }
        }
    }

    // Client -> server: relative paths in any "uri" field become file://
    // URIs under the session root; escapes are refused.
    private outbound(session: BridgeSession, value: unknown): unknown {
        return rewriteUris(value, (uri) => {
            if (uri.startsWith("file://")) {
                throw new ApiError(
                    400,
                    "send package-relative paths, not URIs",
                );
            }
            const absolute = path.resolve(session.root, uri);
            if (
                absolute !== session.root &&
                !absolute.startsWith(session.root + path.sep)
            ) {
                throw new ApiError(400, `path escapes the package: ${uri}`);
            }
            return pathToFileURL(absolute).href;
        });
    }

    // Server -> client: URIs under the root fold back to relative paths.
    private inbound(session: BridgeSession, value: unknown): unknown {
        const prefix = `${session.rootUri}/`;
        return rewriteUris(value, (uri) =>
            uri.startsWith(prefix) ? uri.slice(prefix.length) : uri,
        );
    }
}

function rewriteUris(
    value: unknown,
    rewrite: (uri: string) => string,
): unknown {
    if (Array.isArray(value)) {
        return value.map((entry) => rewriteUris(entry, rewrite));
    }
    if (value !== null && typeof value === "object") {
        const result: Record<string, unknown> = {};
        for (const [key, entry] of Object.entries(value)) {
            result[key] =
                key === "uri" && typeof entry === "string"
                    ? rewrite(entry)
                    : rewriteUris(entry, rewrite);
        }
        return result;
    }
    return value;
}
