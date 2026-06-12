import { createHash } from "node:crypto";
import { Workspace as RototoWorkspace, type WorkspaceSemanticModel } from "rototo";
import type { WorkspaceRecord } from "./db";

/* One semantic model computation per staged handle; the staged cache already
   bounds the handle's lifetime. */
const semanticModelCache = new WeakMap<RototoWorkspace, Promise<WorkspaceSemanticModel>>();

/* The model shape this admin build consumes. A lower version from the native
   module means the dev server is holding a stale binary. */
const EXPECTED_SEMANTIC_MODEL_VERSION = 2;

export function semanticModelFor(
  inspected: RototoWorkspace,
): Promise<WorkspaceSemanticModel> {
  let model = semanticModelCache.get(inspected);
  if (!model) {
    model = inspected.semanticModel().then((loaded) => {
      if ((loaded.version ?? 0) < EXPECTED_SEMANTIC_MODEL_VERSION) {
        throw new Error(
          `the loaded rototo native module produces semantic model v${loaded.version}, ` +
            `but this admin build needs v${EXPECTED_SEMANTIC_MODEL_VERSION} — ` +
            "restart the admin server so Node reloads the rebuilt module",
        );
      }
      return loaded;
    });
    semanticModelCache.set(inspected, model);
    model.catch(() => semanticModelCache.delete(inspected));
  }
  return model;
}

/* Staging a workspace source downloads and extracts the GitHub tarball, so
   doing it per render makes every screen wait on GitHub (and burns API
   quota). Staged handles serve stale-while-revalidate: after the fresh
   window, the cached handle is returned immediately and a background restage
   replaces it. Saves invalidate their source so draft screens see fresh
   content immediately. */
const STAGE_FRESH_MS = 30_000;

type StagedEntry = {
  staged: Promise<RototoWorkspace>;
  stagedAt: number;
  revalidating: boolean;
};

const stagedCache: Map<string, StagedEntry> = ((
  globalThis as Record<string, unknown>
).__rototoStagedCache ??= new Map()) as Map<string, StagedEntry>;

function cachedStage(
  kind: string,
  workspace: WorkspaceRecord,
  githubToken: string,
  stage: () => Promise<RototoWorkspace>,
): Promise<RototoWorkspace> {
  const tokenKey = createHash("sha256").update(githubToken).digest("hex").slice(0, 12);
  const key = `${kind}:${tokenKey}:${workspace.source}`;
  const entry = stagedCache.get(key);
  if (entry) {
    if (Date.now() - entry.stagedAt >= STAGE_FRESH_MS && !entry.revalidating) {
      entry.revalidating = true;
      void stage().then(
        (fresh) => {
          // An invalidation while restaging deletes the entry; do not put a
          // possibly pre-commit snapshot back.
          if (stagedCache.get(key) === entry) {
            stagedCache.set(key, {
              staged: Promise.resolve(fresh),
              stagedAt: Date.now(),
              revalidating: false,
            });
          }
        },
        () => {
          entry.revalidating = false;
        },
      );
    }
    return entry.staged;
  }
  const created: StagedEntry = { staged: stage(), stagedAt: Date.now(), revalidating: false };
  stagedCache.set(key, created);
  created.staged.catch(() => {
    if (stagedCache.get(key) === created) {
      stagedCache.delete(key);
    }
  });
  return created.staged;
}

export function invalidateStagedWorkspaces(source: string): void {
  for (const key of stagedCache.keys()) {
    if (key.endsWith(`:${source}`)) {
      stagedCache.delete(key);
    }
  }
}

export type SemanticTarget = {
  entity?: Record<string, unknown>;
  field?: Record<string, unknown>;
};

export type LintDiagnostic = {
  rule?: { id?: string } | string;
  severity?: string;
  stage?: string;
  target?: SemanticTarget;
  message?: string;
  help?: string;
  location?: {
    path?: string;
    range?: {
      // 0-based, LSP-style positions
      start?: { line?: number; character?: number; column?: number };
      end?: { line?: number; character?: number; column?: number };
    };
  };
};

export type WorkspaceLintView = {
  root: string;
  diagnostics: LintDiagnostic[];
};

export async function lintWorkspace(
  workspace: WorkspaceRecord,
  githubToken: string,
): Promise<WorkspaceLintView> {
  const inspected = await inspectWorkspace(workspace, githubToken);
  return lintInspectedWorkspace(inspected);
}

export async function inspectWorkspace(
  workspace: WorkspaceRecord,
  githubToken: string,
): Promise<RototoWorkspace> {
  return cachedStage("inspect", workspace, githubToken, () =>
    RototoWorkspace.inspect(workspace.source, {
      workspaceToken: githubToken,
    }),
  );
}

/* A runtime-capable handle for resolution previews. The runtime model is
   only compiled under lint mode "deny", so previews exist exactly when the
   workspace lints clean — the same workspaces applications can load. */
/* The runtime workspace compiles its model in memory, but keep the staged
   inspect handle alive anyway so its checkout cannot be reclaimed under a
   runtime that still references the path. */
const runtimeKeepAlive = new WeakMap<object, object>();

export async function loadWorkspaceRuntime(
  workspace: WorkspaceRecord,
  githubToken: string,
): Promise<RototoWorkspace> {
  return cachedStage("load", workspace, githubToken, async () => {
    // Reuse the staged inspect checkout instead of downloading the source a
    // second time; load from the local root applies the same lint-deny gate.
    const inspected = await inspectWorkspace(workspace, githubToken);
    const runtime = await RototoWorkspace.load(inspected.root, {});
    runtimeKeepAlive.set(runtime, inspected);
    return runtime;
  });
}

export async function lintInspectedWorkspace(
  inspected: RototoWorkspace,
): Promise<WorkspaceLintView> {
  const lint = await inspected.lint();
  return {
    root: lint.root,
    diagnostics: lint.diagnostics as LintDiagnostic[],
  };
}
