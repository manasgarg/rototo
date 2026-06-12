import { mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { decryptToken, encryptToken } from "./token-encryption";

const DEFAULT_SESSION_TTL_MS = 1000 * 60 * 60 * 24 * 14;
const OAUTH_STATE_TTL_MS = 1000 * 60 * 10;
const SESSION_TOKEN_BYTES = 32;

export type SessionUser = {
  sessionHash: string;
  githubUserId: string;
  githubLogin: string;
  githubName: string | null;
  githubAvatarUrl: string | null;
  githubToken: string;
};

export type RepoRecord = {
  id: string;
  githubUserId: string;
  owner: string;
  name: string;
  defaultRef: string;
  createdAt: string;
  updatedAt: string;
  lastDiscoveredAt: string | null;
};

export type WorkspaceRecord = {
  id: string;
  /* Derived, human-readable URL handle (repo name + workspace path). Stable
     across re-discovery, unlike the row id. */
  slug: string;
  repoId: string;
  owner: string;
  name: string;
  path: string;
  ref: string;
  source: string;
  discoveredAt: string;
};

export type RepoWithWorkspaces = RepoRecord & {
  workspaces: WorkspaceRecord[];
};

export type DraftSessionRecord = {
  id: string;
  workspaceId: string;
  githubUserId: string;
  branch: string;
  baseRef: string;
  status: "open" | "published";
  prUrl: string | null;
  prNumber: number | null;
  prState: string | null;
  prMergedAt: string | null;
  prSyncedAt: string | null;
  createdAt: string;
  updatedAt: string;
  publishedAt: string | null;
};

export type DraftChangeRecord = {
  id: string;
  draftId: string;
  filePath: string;
  variableId: string;
  valueKey: string;
  beforeJson: string;
  afterJson: string;
  updatedAt: string;
};

export type DraftEventRecord = {
  id: string;
  draftId: string;
  kind: string;
  summary: string;
  detailJson: string | null;
  createdAt: string;
};

let database: DatabaseSync | null = null;

export function db(): DatabaseSync {
  if (database) {
    return database;
  }

  const dir = resolve(process.cwd(), process.env.ROTOTO_ADMIN_DATA_DIR ?? ".rototo-admin");
  mkdirSync(dir, { recursive: true });
  database = new DatabaseSync(join(dir, "admin.db"));
  database.exec(`
    PRAGMA journal_mode = WAL;
  `);
  ensureSessionsTable(database);
  database.exec(`

    CREATE TABLE IF NOT EXISTS oauth_states (
      state TEXT PRIMARY KEY,
      created_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS repos (
      id TEXT PRIMARY KEY,
      github_user_id TEXT NOT NULL,
      owner TEXT NOT NULL,
      name TEXT NOT NULL,
      default_ref TEXT NOT NULL,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      last_discovered_at TEXT,
      UNIQUE(github_user_id, owner, name)
    );

    CREATE TABLE IF NOT EXISTS workspaces (
      id TEXT PRIMARY KEY,
      repo_id TEXT NOT NULL,
      owner TEXT NOT NULL,
      name TEXT NOT NULL,
      path TEXT NOT NULL,
      ref TEXT NOT NULL,
      source TEXT NOT NULL,
      discovered_at TEXT NOT NULL,
      UNIQUE(repo_id, path, ref),
      FOREIGN KEY(repo_id) REFERENCES repos(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS draft_sessions (
      id TEXT PRIMARY KEY,
      workspace_id TEXT NOT NULL,
      github_user_id TEXT NOT NULL,
      branch TEXT NOT NULL,
      base_ref TEXT NOT NULL,
      status TEXT NOT NULL,
      pr_url TEXT,
      pr_number INTEGER,
      pr_state TEXT,
      pr_merged_at TEXT,
      pr_synced_at TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      published_at TEXT,
      FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS draft_changes (
      id TEXT PRIMARY KEY,
      draft_id TEXT NOT NULL,
      file_path TEXT NOT NULL,
      variable_id TEXT NOT NULL,
      value_key TEXT NOT NULL,
      before_json TEXT NOT NULL,
      after_json TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      UNIQUE(draft_id, variable_id, value_key),
      FOREIGN KEY(draft_id) REFERENCES draft_sessions(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS draft_events (
      id TEXT PRIMARY KEY,
      draft_id TEXT NOT NULL,
      kind TEXT NOT NULL,
      summary TEXT NOT NULL,
      detail_json TEXT,
      created_at TEXT NOT NULL,
      FOREIGN KEY(draft_id) REFERENCES draft_sessions(id) ON DELETE CASCADE
    );
  `);
  ensureDraftSessionColumns(database);
  migrateWorkspaceSources(database);
  return database;
}

export function createSession(input: {
  githubUserId: string;
  githubLogin: string;
  githubName: string | null;
  githubAvatarUrl: string | null;
  githubToken: string;
}): string {
  const sessionToken = newSessionToken();
  const now = new Date();
  const expiresAt = new Date(now.getTime() + DEFAULT_SESSION_TTL_MS);
  db()
    .prepare(
      `INSERT INTO sessions (
        id, github_user_id, github_login, github_name, github_avatar_url,
        github_token_ciphertext, created_at, expires_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    )
    .run(
      sessionTokenHash(sessionToken),
      input.githubUserId,
      input.githubLogin,
      input.githubName,
      input.githubAvatarUrl,
      encryptToken(input.githubToken),
      now.toISOString(),
      expiresAt.toISOString(),
    );
  return sessionToken;
}

export function getSession(sessionToken: string | undefined): SessionUser | null {
  if (!sessionToken) {
    return null;
  }
  const row = db()
    .prepare(
      `SELECT id, github_user_id, github_login, github_name, github_avatar_url,
        github_token_ciphertext, expires_at
       FROM sessions
       WHERE id = ?`,
    )
    .get(sessionTokenHash(sessionToken)) as SessionRow | undefined;
  if (!row) {
    return null;
  }
  if (Date.parse(row.expires_at) <= Date.now()) {
    deleteSession(sessionToken);
    return null;
  }
  let githubToken: string;
  try {
    githubToken = decryptToken(row.github_token_ciphertext);
  } catch {
    return null;
  }
  return {
    sessionHash: row.id,
    githubUserId: row.github_user_id,
    githubLogin: row.github_login,
    githubName: row.github_name,
    githubAvatarUrl: row.github_avatar_url,
    githubToken,
  };
}

export function deleteSession(sessionToken: string): void {
  db().prepare("DELETE FROM sessions WHERE id = ?").run(sessionTokenHash(sessionToken));
}

export function createOAuthState(state: string): void {
  db()
    .prepare("INSERT OR REPLACE INTO oauth_states (state, created_at) VALUES (?, ?)")
    .run(state, new Date().toISOString());
}

export function consumeOAuthState(state: string): boolean {
  const row = db()
    .prepare("SELECT state, created_at FROM oauth_states WHERE state = ?")
    .get(state) as OAuthStateRow | undefined;
  db().prepare("DELETE FROM oauth_states WHERE state = ?").run(state);
  if (!row) {
    return false;
  }
  return Date.parse(row.created_at) + OAUTH_STATE_TTL_MS > Date.now();
}

export function upsertRepoWithWorkspaces(input: {
  githubUserId: string;
  owner: string;
  name: string;
  defaultRef: string;
  workspaces: Array<{ path: string; ref: string; source: string }>;
}): RepoWithWorkspaces {
  const now = new Date().toISOString();
  const existing = db()
    .prepare("SELECT * FROM repos WHERE github_user_id = ? AND owner = ? AND name = ?")
    .get(input.githubUserId, input.owner, input.name) as RepoRow | undefined;
  const repoId = existing?.id ?? randomUUID();
  if (existing) {
    db()
      .prepare(
        `UPDATE repos
         SET default_ref = ?, updated_at = ?, last_discovered_at = ?
         WHERE id = ?`,
      )
      .run(input.defaultRef, now, now, repoId);
  } else {
    db()
      .prepare(
        `INSERT INTO repos (
          id, github_user_id, owner, name, default_ref,
          created_at, updated_at, last_discovered_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        repoId,
        input.githubUserId,
        input.owner,
        input.name,
        input.defaultRef,
        now,
        now,
        now,
      );
  }

  db().prepare("DELETE FROM workspaces WHERE repo_id = ?").run(repoId);
  const insertWorkspace = db().prepare(
    `INSERT INTO workspaces (
      id, repo_id, owner, name, path, ref, source, discovered_at
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
  );
  for (const workspace of input.workspaces) {
    insertWorkspace.run(
      randomUUID(),
      repoId,
      input.owner,
      input.name,
      workspace.path,
      workspace.ref,
      workspace.source,
      now,
    );
  }

  const repo = getRepoByIdForUser(repoId, input.githubUserId);
  if (!repo) {
    throw new Error("repo registration failed");
  }
  return repo;
}

export function listReposForUser(githubUserId: string): RepoWithWorkspaces[] {
  const repos = db()
    .prepare(
      `SELECT * FROM repos
       WHERE github_user_id = ?
       ORDER BY updated_at DESC, owner ASC, name ASC`,
    )
    .all(githubUserId) as RepoRow[];
  return repos.map(repoWithWorkspaces);
}

export function getRepoByIdForUser(
  repoId: string,
  githubUserId: string,
): RepoWithWorkspaces | null {
  const repo = db()
    .prepare("SELECT * FROM repos WHERE id = ? AND github_user_id = ?")
    .get(repoId, githubUserId) as RepoRow | undefined;
  return repo ? repoWithWorkspaces(repo) : null;
}

export function deleteRepoForUser(repoId: string, githubUserId: string): boolean {
  const repo = getRepoByIdForUser(repoId, githubUserId);
  if (!repo) {
    return false;
  }
  db()
    .prepare(
      `DELETE FROM draft_changes WHERE draft_id IN (
         SELECT d.id FROM draft_sessions d
         INNER JOIN workspaces w ON w.id = d.workspace_id
         WHERE w.repo_id = ?
       )`,
    )
    .run(repoId);
  db()
    .prepare(
      `DELETE FROM draft_events WHERE draft_id IN (
         SELECT d.id FROM draft_sessions d
         INNER JOIN workspaces w ON w.id = d.workspace_id
         WHERE w.repo_id = ?
       )`,
    )
    .run(repoId);
  db()
    .prepare(
      `DELETE FROM draft_sessions WHERE workspace_id IN (
         SELECT id FROM workspaces WHERE repo_id = ?
       )`,
    )
    .run(repoId);
  db().prepare("DELETE FROM workspaces WHERE repo_id = ?").run(repoId);
  db().prepare("DELETE FROM repos WHERE id = ?").run(repoId);
  return true;
}

export function listWorkspacesForUser(githubUserId: string): WorkspaceRecord[] {
  const rows = db()
    .prepare(
      `SELECT w.*
       FROM workspaces w
       INNER JOIN repos r ON r.id = w.repo_id
       WHERE r.github_user_id = ?
       ORDER BY w.owner ASC, w.name ASC, w.path ASC`,
    )
    .all(githubUserId) as WorkspaceRow[];
  return rows.map(workspaceFromRow);
}

/* Accepts the row id or the derived slug, so friendly URLs and older id
   URLs both resolve. */
export function getWorkspaceForUser(
  workspaceHandle: string,
  githubUserId: string,
): WorkspaceRecord | null {
  const row = db()
    .prepare(
      `SELECT w.*
       FROM workspaces w
       INNER JOIN repos r ON r.id = w.repo_id
       WHERE w.id = ? AND r.github_user_id = ?`,
    )
    .get(workspaceHandle, githubUserId) as WorkspaceRow | undefined;
  if (row) {
    return workspaceFromRow(row);
  }
  return (
    listWorkspacesForUser(githubUserId).find(
      (workspace) => workspace.slug === workspaceHandle,
    ) ?? null
  );
}

export function createDraftSession(input: {
  workspaceId: string;
  githubUserId: string;
  branch: string;
  baseRef: string;
}): DraftSessionRecord {
  const now = new Date().toISOString();
  const id = randomUUID();
  db()
    .prepare(
      `INSERT INTO draft_sessions (
        id, workspace_id, github_user_id, branch, base_ref, status,
        created_at, updated_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    )
    .run(
      id,
      input.workspaceId,
      input.githubUserId,
      input.branch,
      input.baseRef,
      "open",
      now,
      now,
    );
  const draft = getDraftSessionForUser(id, input.workspaceId, input.githubUserId);
  if (!draft) {
    throw new Error("draft session creation failed");
  }
  recordDraftEvent({
    draftId: draft.id,
    kind: "draft.created",
    summary: `Created draft branch ${draft.branch}`,
    detail: { branch: draft.branch, baseRef: draft.baseRef },
  });
  return draft;
}

export function listDraftSessionsForWorkspace(
  workspaceId: string,
  githubUserId: string,
): DraftSessionRecord[] {
  const rows = db()
    .prepare(
      `SELECT *
       FROM draft_sessions
       WHERE workspace_id = ? AND github_user_id = ?
       ORDER BY updated_at DESC`,
    )
    .all(workspaceId, githubUserId) as DraftSessionRow[];
  return rows.map(draftSessionFromRow);
}

export function getDraftSessionForUser(
  draftId: string,
  workspaceId: string,
  githubUserId: string,
): DraftSessionRecord | null {
  const row = db()
    .prepare(
      `SELECT *
       FROM draft_sessions
       WHERE id = ? AND workspace_id = ? AND github_user_id = ?`,
    )
    .get(draftId, workspaceId, githubUserId) as DraftSessionRow | undefined;
  return row ? draftSessionFromRow(row) : null;
}

export function recordDraftChange(input: {
  draftId: string;
  filePath: string;
  variableId: string;
  valueKey: string;
  before: unknown;
  after: unknown;
}): DraftChangeRecord | null {
  const now = new Date().toISOString();
  const existing = db()
    .prepare(
      `SELECT *
       FROM draft_changes
       WHERE draft_id = ? AND variable_id = ? AND value_key = ?`,
    )
    .get(input.draftId, input.variableId, input.valueKey) as DraftChangeRow | undefined;
  const before = existing ? parseJson(existing.before_json) : input.before;
  if (jsonEqual(before, input.after)) {
    if (existing) {
      db()
        .prepare(
          `DELETE FROM draft_changes
           WHERE draft_id = ? AND variable_id = ? AND value_key = ?`,
        )
        .run(input.draftId, input.variableId, input.valueKey);
      db()
        .prepare("UPDATE draft_sessions SET updated_at = ? WHERE id = ?")
        .run(now, input.draftId);
      recordDraftEvent({
        draftId: input.draftId,
        kind: "change.reverted",
        summary: `Reverted ${input.variableId} ${input.valueKey}`,
        detail: {
          filePath: input.filePath,
          variableId: input.variableId,
          valueKey: input.valueKey,
        },
      });
    }
    return null;
  }

  if (existing) {
    db()
      .prepare(
        `UPDATE draft_changes
         SET file_path = ?, after_json = ?, updated_at = ?
         WHERE draft_id = ? AND variable_id = ? AND value_key = ?`,
      )
      .run(
        input.filePath,
        JSON.stringify(input.after),
        now,
        input.draftId,
        input.variableId,
        input.valueKey,
      );
  } else {
    db()
      .prepare(
        `INSERT INTO draft_changes (
          id, draft_id, file_path, variable_id, value_key, before_json, after_json, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        randomUUID(),
        input.draftId,
        input.filePath,
        input.variableId,
        input.valueKey,
        JSON.stringify(input.before),
        JSON.stringify(input.after),
        now,
      );
  }
  const change = db()
    .prepare(
      `SELECT *
       FROM draft_changes
       WHERE draft_id = ? AND variable_id = ? AND value_key = ?`,
    )
    .get(input.draftId, input.variableId, input.valueKey) as DraftChangeRow | undefined;
  if (!change) {
    throw new Error("draft change recording failed");
  }
  db()
    .prepare("UPDATE draft_sessions SET updated_at = ? WHERE id = ?")
    .run(now, input.draftId);
  recordDraftEvent({
    draftId: input.draftId,
    kind: existing ? "change.updated" : "change.created",
    summary: `${existing ? "Updated" : "Changed"} ${input.variableId} ${input.valueKey}`,
    detail: {
      filePath: input.filePath,
      variableId: input.variableId,
      valueKey: input.valueKey,
    },
  });
  return draftChangeFromRow(change);
}

export function listDraftChanges(draftId: string): DraftChangeRecord[] {
  const rows = db()
    .prepare(
      `SELECT *
       FROM draft_changes
       WHERE draft_id = ?
       ORDER BY updated_at ASC, variable_id ASC`,
    )
    .all(draftId) as DraftChangeRow[];
  return rows.filter(isNetDraftChange).map(draftChangeFromRow);
}

export function markDraftPublished(input: {
  draftId: string;
  prNumber: number;
  prState: string;
  prUrl: string;
}): void {
  const now = new Date().toISOString();
  db()
    .prepare(
      `UPDATE draft_sessions
       SET status = ?, pr_url = ?, pr_number = ?, pr_state = ?, pr_synced_at = ?,
           updated_at = ?, published_at = ?
       WHERE id = ?`,
    )
    .run("published", input.prUrl, input.prNumber, input.prState, now, now, now, input.draftId);
  recordDraftEvent({
    draftId: input.draftId,
    kind: "pr.created",
    summary: `Created pull request #${input.prNumber}`,
    detail: { prUrl: input.prUrl, prNumber: input.prNumber, prState: input.prState },
  });
}

export function updateDraftBranch(input: {
  draftId: string;
  branch: string;
  previousBranch: string;
}): DraftSessionRecord {
  const now = new Date().toISOString();
  db()
    .prepare("UPDATE draft_sessions SET branch = ?, updated_at = ? WHERE id = ?")
    .run(input.branch, now, input.draftId);
  recordDraftEvent({
    draftId: input.draftId,
    kind: "draft.branch_renamed",
    summary: `Renamed draft branch to ${input.branch}`,
    detail: { previousBranch: input.previousBranch, branch: input.branch },
  });
  const row = db()
    .prepare("SELECT * FROM draft_sessions WHERE id = ?")
    .get(input.draftId) as DraftSessionRow | undefined;
  if (!row) {
    throw new Error("draft session update failed");
  }
  return draftSessionFromRow(row);
}

export function updateDraftPullRequestState(input: {
  draftId: string;
  prNumber: number;
  prState: string;
  prUrl: string;
  prMergedAt: string | null;
}): DraftSessionRecord {
  const now = new Date().toISOString();
  const existing = db()
    .prepare("SELECT * FROM draft_sessions WHERE id = ?")
    .get(input.draftId) as DraftSessionRow | undefined;
  const changed =
    !existing ||
    existing.pr_number !== input.prNumber ||
    existing.pr_state !== input.prState ||
    existing.pr_merged_at !== input.prMergedAt;
  const updatedAt = changed ? now : (existing?.updated_at ?? now);

  // A pull request closed without merging ends the publish attempt, not the
  // draft: reopen it so the branch can be edited and published again. The
  // closed pull request stays on GitHub and in the draft's activity.
  const reopened =
    input.prState === "closed" &&
    input.prMergedAt === null &&
    existing?.status === "published";
  if (reopened) {
    db()
      .prepare(
        `UPDATE draft_sessions
         SET status = 'open', pr_number = NULL, pr_state = NULL, pr_url = NULL,
             pr_merged_at = NULL, pr_synced_at = ?, published_at = NULL, updated_at = ?
         WHERE id = ?`,
      )
      .run(now, now, input.draftId);
    recordDraftEvent({
      draftId: input.draftId,
      kind: "pr.closed",
      summary: `Pull request #${input.prNumber} was closed without merging — draft reopened`,
      detail: {
        prNumber: input.prNumber,
        prUrl: input.prUrl,
      },
    });
  } else {
    db()
      .prepare(
        `UPDATE draft_sessions
         SET pr_number = ?, pr_state = ?, pr_url = ?, pr_merged_at = ?, pr_synced_at = ?, updated_at = ?
         WHERE id = ?`,
      )
      .run(
        input.prNumber,
        input.prState,
        input.prUrl,
        input.prMergedAt,
        now,
        updatedAt,
        input.draftId,
      );
    if (changed) {
      recordDraftEvent({
        draftId: input.draftId,
        kind: "pr.synced",
        summary: `Synced pull request #${input.prNumber}: ${input.prState}`,
        detail: {
          prNumber: input.prNumber,
          prState: input.prState,
          prUrl: input.prUrl,
          prMergedAt: input.prMergedAt,
        },
      });
    }
  }
  const row = db()
    .prepare("SELECT * FROM draft_sessions WHERE id = ?")
    .get(input.draftId) as DraftSessionRow | undefined;
  if (!row) {
    throw new Error("draft pull request state update failed");
  }
  return draftSessionFromRow(row);
}

export function recordDraftEvent(input: {
  draftId: string;
  kind: string;
  summary: string;
  detail?: unknown;
}): DraftEventRecord {
  const id = randomUUID();
  const now = new Date().toISOString();
  db()
    .prepare(
      `INSERT INTO draft_events (
        id, draft_id, kind, summary, detail_json, created_at
      ) VALUES (?, ?, ?, ?, ?, ?)`,
    )
    .run(
      id,
      input.draftId,
      input.kind,
      input.summary,
      input.detail === undefined ? null : JSON.stringify(input.detail),
      now,
    );
  return {
    id,
    draftId: input.draftId,
    kind: input.kind,
    summary: input.summary,
    detailJson: input.detail === undefined ? null : JSON.stringify(input.detail),
    createdAt: now,
  };
}

export function listDraftEvents(draftId: string): DraftEventRecord[] {
  const rows = db()
    .prepare(
      `SELECT *
       FROM draft_events
       WHERE draft_id = ?
       ORDER BY created_at ASC, id ASC`,
    )
    .all(draftId) as DraftEventRow[];
  return rows.map(draftEventFromRow);
}

function repoWithWorkspaces(row: RepoRow): RepoWithWorkspaces {
  const repo = repoFromRow(row);
  const rows = db()
    .prepare(
      `SELECT * FROM workspaces
       WHERE repo_id = ?
       ORDER BY path ASC`,
    )
    .all(repo.id) as WorkspaceRow[];
  const workspaces = rows.map(workspaceFromRow);
  return { ...repo, workspaces };
}

function repoFromRow(row: RepoRow): RepoRecord {
  return {
    id: row.id,
    githubUserId: row.github_user_id,
    owner: row.owner,
    name: row.name,
    defaultRef: row.default_ref,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
    lastDiscoveredAt: row.last_discovered_at,
  };
}

function workspaceFromRow(row: WorkspaceRow): WorkspaceRecord {
  return {
    id: row.id,
    slug: workspaceSlug(row.name, row.path),
    repoId: row.repo_id,
    owner: row.owner,
    name: row.name,
    path: row.path,
    ref: row.ref,
    source: normalizeWorkspaceSource(row.source),
    discoveredAt: row.discovered_at,
  };
}

function workspaceSlug(name: string, path: string): string {
  const base = path === "." ? name : `${name}-${path}`;
  return base
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function draftSessionFromRow(row: DraftSessionRow): DraftSessionRecord {
  return {
    id: row.id,
    workspaceId: row.workspace_id,
    githubUserId: row.github_user_id,
    branch: row.branch,
    baseRef: row.base_ref,
    status: row.status === "published" ? "published" : "open",
    prUrl: row.pr_url,
    prNumber: row.pr_number,
    prState: row.pr_state,
    prMergedAt: row.pr_merged_at,
    prSyncedAt: row.pr_synced_at,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
    publishedAt: row.published_at,
  };
}

function draftEventFromRow(row: DraftEventRow): DraftEventRecord {
  return {
    id: row.id,
    draftId: row.draft_id,
    kind: row.kind,
    summary: row.summary,
    detailJson: row.detail_json,
    createdAt: row.created_at,
  };
}

function draftChangeFromRow(row: DraftChangeRow): DraftChangeRecord {
  return {
    id: row.id,
    draftId: row.draft_id,
    filePath: row.file_path,
    variableId: row.variable_id,
    valueKey: row.value_key,
    beforeJson: row.before_json,
    afterJson: row.after_json,
    updatedAt: row.updated_at,
  };
}

function isNetDraftChange(row: DraftChangeRow): boolean {
  return !jsonEqual(parseJson(row.before_json), parseJson(row.after_json));
}

function parseJson(value: string): unknown {
  return JSON.parse(value);
}

function jsonEqual(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function migrateWorkspaceSources(database: DatabaseSync): void {
  database
    .prepare("UPDATE workspaces SET source = replace(source, '/zipball/', '/tarball/')")
    .run();
}

function ensureDraftSessionColumns(database: DatabaseSync): void {
  const columns = new Set(
    (database.prepare("PRAGMA table_info(draft_sessions)").all() as TableInfoRow[]).map(
      (row) => row.name,
    ),
  );
  const additions = [
    ["pr_number", "INTEGER"],
    ["pr_state", "TEXT"],
    ["pr_merged_at", "TEXT"],
    ["pr_synced_at", "TEXT"],
  ] as const;
  for (const [name, type] of additions) {
    if (!columns.has(name)) {
      database.exec(`ALTER TABLE draft_sessions ADD COLUMN ${name} ${type}`);
    }
  }
}

function normalizeWorkspaceSource(source: string): string {
  return source.replace("/zipball/", "/tarball/");
}

function ensureSessionsTable(database: DatabaseSync): void {
  const existing = database
    .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'sessions'")
    .get();
  if (existing) {
    const columns = new Set(
      (database.prepare("PRAGMA table_info(sessions)").all() as TableInfoRow[]).map(
        (row) => row.name,
      ),
    );
    if (!columns.has("github_token_ciphertext") || !columns.has("session_hash_version")) {
      database.exec("DROP TABLE sessions");
    }
  }

  database.exec(`
    CREATE TABLE IF NOT EXISTS sessions (
      id TEXT PRIMARY KEY,
      github_user_id TEXT NOT NULL,
      github_login TEXT NOT NULL,
      github_name TEXT,
      github_avatar_url TEXT,
      github_token_ciphertext TEXT NOT NULL,
      session_hash_version INTEGER NOT NULL DEFAULT 1,
      created_at TEXT NOT NULL,
      expires_at TEXT NOT NULL
    );
  `);
}

function newSessionToken(): string {
  return randomBytes(SESSION_TOKEN_BYTES).toString("base64url");
}

function sessionTokenHash(sessionToken: string): string {
  return createHash("sha256").update(sessionToken, "utf8").digest("hex");
}

type SessionRow = {
  id: string;
  github_user_id: string;
  github_login: string;
  github_name: string | null;
  github_avatar_url: string | null;
  github_token_ciphertext: string;
  expires_at: string;
};

type TableInfoRow = {
  name: string;
};

type OAuthStateRow = {
  state: string;
  created_at: string;
};

type RepoRow = {
  id: string;
  github_user_id: string;
  owner: string;
  name: string;
  default_ref: string;
  created_at: string;
  updated_at: string;
  last_discovered_at: string | null;
};

type WorkspaceRow = {
  id: string;
  repo_id: string;
  owner: string;
  name: string;
  path: string;
  ref: string;
  source: string;
  discovered_at: string;
};

type DraftSessionRow = {
  id: string;
  workspace_id: string;
  github_user_id: string;
  branch: string;
  base_ref: string;
  status: string;
  pr_url: string | null;
  pr_number: number | null;
  pr_state: string | null;
  pr_merged_at: string | null;
  pr_synced_at: string | null;
  created_at: string;
  updated_at: string;
  published_at: string | null;
};

type DraftChangeRow = {
  id: string;
  draft_id: string;
  file_path: string;
  variable_id: string;
  value_key: string;
  before_json: string;
  after_json: string;
  updated_at: string;
};

type DraftEventRow = {
  id: string;
  draft_id: string;
  kind: string;
  summary: string;
  detail_json: string | null;
  created_at: string;
};
