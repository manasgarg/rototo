import { spawn, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import type { DraftSessionRecord, WorkspaceRecord } from "./db";
import { inspectWorkspace } from "./rototo";
import { draftLintTarget } from "./workspace-edit";

/* Bridges the admin editor to the real `rototo lsp` language server. The
   admin app never reimplements lint, completion, or hover semantics: a draft
   editing session stages the draft branch, spawns `rototo lsp` against the
   staged checkout, and forwards the editor's unsaved text as LSP document
   overlays. HTTP calls are serialized per session so the single JSON-RPC
   stream stays ordered. */

export type LspPositionWire = { line: number; character: number };

export type LspRangeWire = { start: LspPositionWire; end: LspPositionWire };

export type LspDiagnosticWire = {
  message: string;
  severity: "error" | "warning";
  rule: string | null;
  help: string | null;
  range: LspRangeWire;
};

export type LspCompletionWire = {
  label: string;
  kind: number;
  detail: string | null;
};

export type LspHoverWire = {
  value: string;
  range: LspRangeWire | null;
};

type PendingRequest = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: NodeJS.Timeout;
};

type LspSession = {
  key: string;
  child: ChildProcess;
  /* Keeps the staged checkout's temp directory alive for the session. */
  staged: Awaited<ReturnType<typeof inspectWorkspace>>;
  root: string;
  nextRequestId: number;
  pending: Map<number, PendingRequest>;
  diagnosticsByUri: Map<string, unknown[]>;
  openDocuments: Map<string, { version: number; text: string }>;
  queue: Promise<unknown>;
  idleTimer: NodeJS.Timeout | null;
  stderrTail: string;
  closed: boolean;
};

const REQUEST_TIMEOUT_MS = 30_000;
const IDLE_SESSION_MS = 10 * 60_000;

/* Survives Next.js dev-mode module reloads. */
const sessions: Map<string, Promise<LspSession>> = ((
  globalThis as Record<string, unknown>
).__rototoLspSessions ??= new Map()) as Map<string, Promise<LspSession>>;

export async function lspUpdate(input: {
  workspace: WorkspaceRecord;
  draft: DraftSessionRecord;
  githubToken: string;
  userId: string;
  path: string;
  text: string;
}): Promise<{ diagnostics: LspDiagnosticWire[] }> {
  const session = await sessionFor(input);
  return enqueue(session, async () => {
    const uri = await syncDocument(session, input.workspace, input.path, input.text);
    // documentSymbol acts as a barrier: the server publishes diagnostics for
    // the didChange before it answers the next request on the same stream.
    await request(session, "textDocument/documentSymbol", { textDocument: { uri } });
    const raw = (session.diagnosticsByUri.get(uri) ?? []) as Array<Record<string, unknown>>;
    return { diagnostics: raw.map(simplifyDiagnostic) };
  });
}

export async function lspCompletion(input: {
  workspace: WorkspaceRecord;
  draft: DraftSessionRecord;
  githubToken: string;
  userId: string;
  path: string;
  text: string;
  position: LspPositionWire;
}): Promise<{ items: LspCompletionWire[] }> {
  const session = await sessionFor(input);
  return enqueue(session, async () => {
    const uri = await syncDocument(session, input.workspace, input.path, input.text);
    const result = await request(session, "textDocument/completion", {
      textDocument: { uri },
      position: input.position,
    });
    const items = Array.isArray(result) ? (result as Array<Record<string, unknown>>) : [];
    return {
      items: items.map((item) => ({
        label: String(item.label ?? ""),
        kind: typeof item.kind === "number" ? item.kind : 0,
        detail: typeof item.detail === "string" ? item.detail : null,
      })),
    };
  });
}

export async function lspHover(input: {
  workspace: WorkspaceRecord;
  draft: DraftSessionRecord;
  githubToken: string;
  userId: string;
  path: string;
  text: string;
  position: LspPositionWire;
}): Promise<{ hover: LspHoverWire | null }> {
  const session = await sessionFor(input);
  return enqueue(session, async () => {
    const uri = await syncDocument(session, input.workspace, input.path, input.text);
    const result = (await request(session, "textDocument/hover", {
      textDocument: { uri },
      position: input.position,
    })) as Record<string, unknown> | null;
    const value =
      result && typeof result === "object"
        ? ((result.contents as Record<string, unknown> | undefined)?.value as string | undefined)
        : undefined;
    if (!value) {
      return { hover: null };
    }
    return {
      hover: {
        value,
        range: (result?.range as LspRangeWire | undefined) ?? null,
      },
    };
  });
}

/* The staged checkout goes stale once a save commits to the draft branch;
   drop the session so the next request restages. */
export function dropLspSessionsForDraft(draftId: string): void {
  for (const key of sessions.keys()) {
    if (key.endsWith(`:${draftId}`)) {
      void destroySession(key);
    }
  }
}

async function sessionFor(input: {
  workspace: WorkspaceRecord;
  draft: DraftSessionRecord;
  githubToken: string;
  userId: string;
}): Promise<LspSession> {
  const key = `${input.userId}:${input.draft.id}`;
  let pendingSession = sessions.get(key);
  if (!pendingSession) {
    pendingSession = createSession(key, input.workspace, input.draft, input.githubToken);
    sessions.set(key, pendingSession);
    pendingSession.catch(() => sessions.delete(key));
  }
  const session = await pendingSession;
  if (session.closed) {
    sessions.delete(key);
    return sessionFor(input);
  }
  touchSession(session);
  return session;
}

async function createSession(
  key: string,
  workspace: WorkspaceRecord,
  draft: DraftSessionRecord,
  githubToken: string,
): Promise<LspSession> {
  const staged = await inspectWorkspace(draftLintTarget(workspace, draft), githubToken);
  const child = spawn(rototoBinary(), ["lsp"], {
    stdio: ["pipe", "pipe", "pipe"],
  });
  const session: LspSession = {
    key,
    child,
    staged,
    root: staged.root,
    nextRequestId: 1,
    pending: new Map(),
    diagnosticsByUri: new Map(),
    openDocuments: new Map(),
    queue: Promise.resolve(),
    idleTimer: null,
    stderrTail: "",
    closed: false,
  };

  let buffer: Buffer = Buffer.alloc(0);
  child.stdout?.on("data", (chunk: Buffer) => {
    buffer = Buffer.concat([buffer, chunk]);
    for (;;) {
      const frame = readFrame(buffer);
      buffer = frame.buffer;
      if (frame.message === undefined) {
        break;
      }
      handleServerMessage(session, frame.message as Record<string, unknown>);
    }
  });
  child.stderr?.on("data", (chunk: Buffer) => {
    session.stderrTail = (session.stderrTail + chunk.toString("utf8")).slice(-2000);
  });
  const fail = (reason: string) => {
    session.closed = true;
    for (const pending of session.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(new Error(reason));
    }
    session.pending.clear();
    sessions.delete(key);
  };
  child.on("error", (error) => fail(`rototo lsp failed to start: ${error.message}`));
  child.on("exit", (code) =>
    fail(
      `rototo lsp exited${code === null ? "" : ` with code ${code}`}${
        session.stderrTail ? `: ${session.stderrTail.trim()}` : ""
      }`,
    ),
  );

  await request(session, "initialize", {
    rootUri: fileUri(session.root, null),
    capabilities: {},
  });
  notify(session, "initialized", {});
  touchSession(session);
  return session;
}

async function destroySession(key: string): Promise<void> {
  const pendingSession = sessions.get(key);
  sessions.delete(key);
  if (!pendingSession) {
    return;
  }
  try {
    const session = await pendingSession;
    session.closed = true;
    if (session.idleTimer) {
      clearTimeout(session.idleTimer);
    }
    session.child.kill();
  } catch {
    // creation already failed; nothing to stop
  }
}

function touchSession(session: LspSession): void {
  if (session.idleTimer) {
    clearTimeout(session.idleTimer);
  }
  session.idleTimer = setTimeout(() => {
    void destroySession(session.key);
  }, IDLE_SESSION_MS);
  session.idleTimer.unref?.();
}

function enqueue<T>(session: LspSession, work: () => Promise<T>): Promise<T> {
  const run = session.queue.then(work, work);
  session.queue = run.then(
    () => undefined,
    () => undefined,
  );
  return run;
}

async function syncDocument(
  session: LspSession,
  workspace: WorkspaceRecord,
  repoPath: string,
  text: string,
): Promise<string> {
  const relative = workspaceRelativePath(workspace.path, repoPath);
  const uri = fileUri(session.root, relative);
  const open = session.openDocuments.get(uri);
  if (!open) {
    session.openDocuments.set(uri, { version: 1, text });
    notify(session, "textDocument/didOpen", {
      textDocument: { uri, languageId: languageId(relative), version: 1, text },
    });
  } else if (open.text !== text) {
    open.version += 1;
    open.text = text;
    notify(session, "textDocument/didChange", {
      textDocument: { uri, version: open.version },
      contentChanges: [{ text }],
    });
  }
  return uri;
}

function request(session: LspSession, method: string, params: unknown): Promise<unknown> {
  if (session.closed) {
    return Promise.reject(new Error("rototo lsp session is closed"));
  }
  const id = session.nextRequestId;
  session.nextRequestId += 1;
  const promise = new Promise<unknown>((resolvePromise, rejectPromise) => {
    const timer = setTimeout(() => {
      session.pending.delete(id);
      rejectPromise(new Error(`rototo lsp ${method} timed out`));
    }, REQUEST_TIMEOUT_MS);
    timer.unref?.();
    session.pending.set(id, { resolve: resolvePromise, reject: rejectPromise, timer });
  });
  writeMessage(session, { jsonrpc: "2.0", id, method, params });
  return promise;
}

function notify(session: LspSession, method: string, params: unknown): void {
  writeMessage(session, { jsonrpc: "2.0", method, params });
}

function writeMessage(session: LspSession, message: Record<string, unknown>): void {
  const body = JSON.stringify(message);
  session.child.stdin?.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
}

function handleServerMessage(session: LspSession, message: Record<string, unknown>): void {
  const id = message.id;
  if (typeof id === "number" && session.pending.has(id)) {
    const pending = session.pending.get(id) as PendingRequest;
    session.pending.delete(id);
    clearTimeout(pending.timer);
    const error = message.error as { message?: string } | undefined;
    if (error) {
      pending.reject(new Error(error.message ?? "rototo lsp request failed"));
    } else {
      pending.resolve(message.result);
    }
    return;
  }
  if (message.method === "textDocument/publishDiagnostics") {
    const params = message.params as
      | { uri?: string; diagnostics?: unknown[] }
      | undefined;
    if (params?.uri) {
      session.diagnosticsByUri.set(params.uri, params.diagnostics ?? []);
    }
  }
}

function readFrame(buffer: Buffer): { message: unknown; buffer: Buffer } {
  const headerEnd = buffer.indexOf("\r\n\r\n");
  if (headerEnd === -1) {
    return { message: undefined, buffer };
  }
  const header = buffer.subarray(0, headerEnd).toString("utf8");
  const lengthMatch = header.match(/Content-Length:\s*(\d+)/i);
  if (!lengthMatch) {
    // Unparseable frame: drop the header and resync.
    return { message: undefined, buffer: buffer.subarray(headerEnd + 4) };
  }
  const length = Number.parseInt(lengthMatch[1], 10);
  const bodyStart = headerEnd + 4;
  if (buffer.length < bodyStart + length) {
    return { message: undefined, buffer };
  }
  const body = buffer.subarray(bodyStart, bodyStart + length).toString("utf8");
  const rest = buffer.subarray(bodyStart + length);
  try {
    return { message: JSON.parse(body), buffer: rest };
  } catch {
    return { message: undefined, buffer: rest };
  }
}

function simplifyDiagnostic(raw: Record<string, unknown>): LspDiagnosticWire {
  const data = raw.data as { rule?: string; help?: string } | undefined;
  return {
    message: String(raw.message ?? ""),
    severity: raw.severity === 1 ? "error" : "warning",
    rule: data?.rule ?? (typeof raw.code === "string" ? raw.code : null),
    help: data?.help ?? null,
    range: (raw.range as LspRangeWire | undefined) ?? {
      start: { line: 0, character: 0 },
      end: { line: 0, character: 0 },
    },
  };
}

function workspaceRelativePath(workspacePath: string, repoPath: string): string {
  if (workspacePath === "." || !repoPath.startsWith(`${workspacePath}/`)) {
    return repoPath;
  }
  return repoPath.slice(workspacePath.length + 1);
}

function fileUri(root: string, relative: string | null): string {
  const path = relative ? `${root}/${relative}` : root;
  return `file://${path.split("/").map(encodeURIComponent).join("/")}`;
}

function languageId(path: string): string {
  if (path.endsWith(".json")) {
    return "json";
  }
  if (path.endsWith(".lua")) {
    return "lua";
  }
  return "toml";
}

function rototoBinary(): string {
  if (process.env.ROTOTO_BIN) {
    return process.env.ROTOTO_BIN;
  }
  for (const candidate of [
    resolve(process.cwd(), "../../target/release/rototo"),
    resolve(process.cwd(), "../../target/debug/rototo"),
  ]) {
    if (existsSync(candidate)) {
      return candidate;
    }
  }
  return "rototo";
}
