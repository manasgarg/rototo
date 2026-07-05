// The console store: bookkeeping only, rebuildable by design
// (design/console-git-ops.md rule 1). Git keeps the content; these tables
// keep coordination state. This is the new console's own store, starting at
// schema version 1; like the old console, a version mismatch asks for a
// fresh data directory instead of migrating, which is acceptable while the
// store holds nothing you would cry about losing.

import { DatabaseSync } from "node:sqlite";
import { mkdirSync } from "node:fs";
import { randomBytes } from "node:crypto";
import path from "node:path";

const SCHEMA_VERSION = 1;

export type PrincipalRow = {
    id: string;
    kind: "human";
    displayName: string;
    status: "active" | "disabled";
    createdAt: string;
    updatedAt: string;
};

export type IdentityRow = {
    id: string;
    principalId: string;
    provider: "github";
    subject: string;
    login: string | null;
    email: string | null;
    emailVerified: boolean;
    name: string | null;
    avatarUrl: string | null;
    credentialCiphertext: string | null;
    createdAt: string;
    lastSeenAt: string;
};

export type SessionRow = {
    id: string;
    principalId: string;
    createdAt: string;
    expiresAt: string;
};

export type SourceTreeRow = {
    id: string;
    kind: "github" | "local";
    owner: string | null;
    name: string | null;
    defaultBranch: string | null;
    createdBy: string | null;
    createdAt: string;
};

export type GrantRow = {
    id: string;
    granteeKind: "principal" | "group";
    granteeId: string;
    action: string;
    resource: string;
    createdBy: string | null;
    createdAt: string;
};

export type IdentitySnapshot = {
    provider: "github";
    subject: string;
    login: string | null;
    email: string | null;
    emailVerified: boolean;
    name: string | null;
    avatarUrl: string | null;
};

export class Store {
    private readonly db: DatabaseSync;

    constructor(dataDir: string | null) {
        if (dataDir === null) {
            this.db = new DatabaseSync(":memory:");
        } else {
            mkdirSync(dataDir, { recursive: true });
            this.db = new DatabaseSync(path.join(dataDir, "console.sqlite"));
        }
        this.db.exec("PRAGMA journal_mode = WAL");
        this.db.exec("PRAGMA foreign_keys = ON");
        this.migrate();
    }

    close(): void {
        this.db.close();
    }

    private migrate(): void {
        this.db.exec(
            `CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )`,
        );
        const row = this.db
            .prepare("SELECT value FROM meta WHERE key = 'schema_version'")
            .get() as { value: string } | undefined;
        if (row !== undefined && Number(row.value) !== SCHEMA_VERSION) {
            throw new Error(
                `console store has schema version ${row.value}, this build expects ${SCHEMA_VERSION}; ` +
                    "the store is rebuildable, so point the console at a fresh data directory",
            );
        }
        this.db.exec(`
            CREATE TABLE IF NOT EXISTS principals (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL CHECK (kind IN ('human')),
                display_name TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS identities (
                id TEXT PRIMARY KEY,
                principal_id TEXT NOT NULL REFERENCES principals(id),
                provider TEXT NOT NULL,
                subject TEXT NOT NULL,
                login TEXT,
                email TEXT,
                email_verified INTEGER NOT NULL DEFAULT 0,
                name TEXT,
                avatar_url TEXT,
                credential_ciphertext TEXT,
                created_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                UNIQUE (provider, subject)
            );
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                principal_id TEXT NOT NULL REFERENCES principals(id),
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS source_trees (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL CHECK (kind IN ('github', 'local')),
                owner TEXT,
                name TEXT,
                default_branch TEXT,
                created_by TEXT,
                created_at TEXT NOT NULL,
                UNIQUE (kind, owner, name)
            );
            CREATE TABLE IF NOT EXISTS grants (
                id TEXT PRIMARY KEY,
                grantee_kind TEXT NOT NULL CHECK (grantee_kind IN ('principal', 'group')),
                grantee_id TEXT NOT NULL,
                action TEXT NOT NULL,
                resource TEXT NOT NULL,
                created_by TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS authz_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                at TEXT NOT NULL,
                actor TEXT,
                event TEXT NOT NULL,
                detail TEXT
            );
        `);
        this.db
            .prepare(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?)",
            )
            .run(String(SCHEMA_VERSION));
    }

    // Principals and identities. Identity rows are keyed (provider, subject)
    // and never by email or login; credentials live on the identity link,
    // not the session (design/console-identity-authz.md 3.5).

    createPrincipal(displayName: string): PrincipalRow {
        const now = isoNow();
        const row: PrincipalRow = {
            id: `p_${randomId()}`,
            kind: "human",
            displayName,
            status: "active",
            createdAt: now,
            updatedAt: now,
        };
        this.db
            .prepare(
                `INSERT INTO principals (id, kind, display_name, status, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?)`,
            )
            .run(row.id, row.kind, row.displayName, row.status, now, now);
        return row;
    }

    // Disabling a principal invalidates all its sessions and fails every
    // authorization decision, regardless of grants.
    setPrincipalStatus(id: string, status: "active" | "disabled"): void {
        this.db
            .prepare(
                "UPDATE principals SET status = ?, updated_at = ? WHERE id = ?",
            )
            .run(status, isoNow(), id);
        if (status === "disabled") {
            this.db
                .prepare("DELETE FROM sessions WHERE principal_id = ?")
                .run(id);
        }
    }

    getPrincipal(id: string): PrincipalRow | null {
        const row = this.db
            .prepare("SELECT * FROM principals WHERE id = ?")
            .get(id) as Record<string, unknown> | undefined;
        return row === undefined ? null : principalFromRow(row);
    }

    getIdentity(provider: string, subject: string): IdentityRow | null {
        const row = this.db
            .prepare(
                "SELECT * FROM identities WHERE provider = ? AND subject = ?",
            )
            .get(provider, subject) as Record<string, unknown> | undefined;
        return row === undefined ? null : identityFromRow(row);
    }

    identitiesForPrincipal(principalId: string): IdentityRow[] {
        const rows = this.db
            .prepare(
                "SELECT * FROM identities WHERE principal_id = ? ORDER BY created_at",
            )
            .all(principalId) as Record<string, unknown>[];
        return rows.map(identityFromRow);
    }

    attachIdentity(
        principalId: string,
        snapshot: IdentitySnapshot,
        credentialCiphertext: string | null,
    ): IdentityRow {
        const now = isoNow();
        const row: IdentityRow = {
            id: `i_${randomId()}`,
            principalId,
            provider: snapshot.provider,
            subject: snapshot.subject,
            login: snapshot.login,
            email: snapshot.email,
            emailVerified: snapshot.emailVerified,
            name: snapshot.name,
            avatarUrl: snapshot.avatarUrl,
            credentialCiphertext,
            createdAt: now,
            lastSeenAt: now,
        };
        this.db
            .prepare(
                `INSERT INTO identities (
                    id, principal_id, provider, subject, login, email,
                    email_verified, name, avatar_url, credential_ciphertext,
                    created_at, last_seen_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
            )
            .run(
                row.id,
                row.principalId,
                row.provider,
                row.subject,
                row.login,
                row.email,
                row.emailVerified ? 1 : 0,
                row.name,
                row.avatarUrl,
                row.credentialCiphertext,
                now,
                now,
            );
        return row;
    }

    // Refreshes the display snapshot and credential whenever an identity
    // completes a sign-in or link flow.
    refreshIdentity(
        id: string,
        snapshot: IdentitySnapshot,
        credentialCiphertext: string | null,
    ): void {
        this.db
            .prepare(
                `UPDATE identities SET login = ?, email = ?, email_verified = ?,
                    name = ?, avatar_url = ?,
                    credential_ciphertext = COALESCE(?, credential_ciphertext),
                    last_seen_at = ?
                 WHERE id = ?`,
            )
            .run(
                snapshot.login,
                snapshot.email,
                snapshot.emailVerified ? 1 : 0,
                snapshot.name,
                snapshot.avatarUrl,
                credentialCiphertext,
                isoNow(),
                id,
            );
    }

    // Sessions store a hash of the opaque cookie token, never the token.

    createSession(tokenHash: string, principalId: string, ttlMs: number): void {
        const now = Date.now();
        this.db
            .prepare(
                `INSERT INTO sessions (id, principal_id, created_at, expires_at)
                 VALUES (?, ?, ?, ?)`,
            )
            .run(
                tokenHash,
                principalId,
                new Date(now).toISOString(),
                new Date(now + ttlMs).toISOString(),
            );
    }

    getSession(tokenHash: string): SessionRow | null {
        const row = this.db
            .prepare("SELECT * FROM sessions WHERE id = ?")
            .get(tokenHash) as Record<string, unknown> | undefined;
        if (row === undefined) {
            return null;
        }
        const session: SessionRow = {
            id: row.id as string,
            principalId: row.principal_id as string,
            createdAt: row.created_at as string,
            expiresAt: row.expires_at as string,
        };
        if (Date.parse(session.expiresAt) <= Date.now()) {
            this.deleteSession(tokenHash);
            return null;
        }
        return session;
    }

    deleteSession(tokenHash: string): void {
        this.db.prepare("DELETE FROM sessions WHERE id = ?").run(tokenHash);
    }

    // Source trees: registration is a deployment-level act in hosted mode.

    insertSourceTree(
        tree: Omit<SourceTreeRow, "id" | "createdAt">,
    ): SourceTreeRow {
        const row: SourceTreeRow = {
            ...tree,
            id: `st_${randomId()}`,
            createdAt: isoNow(),
        };
        this.db
            .prepare(
                `INSERT INTO source_trees (id, kind, owner, name, default_branch, created_by, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)`,
            )
            .run(
                row.id,
                row.kind,
                row.owner,
                row.name,
                row.defaultBranch,
                row.createdBy,
                row.createdAt,
            );
        return row;
    }

    getSourceTree(id: string): SourceTreeRow | null {
        const row = this.db
            .prepare("SELECT * FROM source_trees WHERE id = ?")
            .get(id) as Record<string, unknown> | undefined;
        return row === undefined ? null : sourceTreeFromRow(row);
    }

    listSourceTrees(): SourceTreeRow[] {
        const rows = this.db
            .prepare("SELECT * FROM source_trees ORDER BY created_at")
            .all() as Record<string, unknown>[];
        return rows.map(sourceTreeFromRow);
    }

    // Grants: allow-only (grantee, action, resource) triples. In C1 only the
    // ROTOTO_CONSOLE_ADMINS bootstrap writes them; administration UI is
    // Phase B.

    insertGrant(grant: Omit<GrantRow, "id" | "createdAt">): GrantRow {
        const row: GrantRow = {
            ...grant,
            id: `g_${randomId()}`,
            createdAt: isoNow(),
        };
        this.db
            .prepare(
                `INSERT INTO grants (id, grantee_kind, grantee_id, action, resource, created_by, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)`,
            )
            .run(
                row.id,
                row.granteeKind,
                row.granteeId,
                row.action,
                row.resource,
                row.createdBy,
                row.createdAt,
            );
        return row;
    }

    grantsForPrincipal(principalId: string): GrantRow[] {
        const rows = this.db
            .prepare(
                "SELECT * FROM grants WHERE grantee_kind = 'principal' AND grantee_id = ?",
            )
            .all(principalId) as Record<string, unknown>[];
        return rows.map((row) => ({
            id: row.id as string,
            granteeKind: row.grantee_kind as "principal" | "group",
            granteeId: row.grantee_id as string,
            action: row.action as string,
            resource: row.resource as string,
            createdBy: row.created_by as string | null,
            createdAt: row.created_at as string,
        }));
    }

    appendAudit(actor: string | null, event: string, detail: string): void {
        this.db
            .prepare(
                "INSERT INTO authz_audit (at, actor, event, detail) VALUES (?, ?, ?, ?)",
            )
            .run(isoNow(), actor, event, detail);
    }
}

function principalFromRow(row: Record<string, unknown>): PrincipalRow {
    return {
        id: row.id as string,
        kind: row.kind as "human",
        displayName: row.display_name as string,
        status: row.status as "active" | "disabled",
        createdAt: row.created_at as string,
        updatedAt: row.updated_at as string,
    };
}

function identityFromRow(row: Record<string, unknown>): IdentityRow {
    return {
        id: row.id as string,
        principalId: row.principal_id as string,
        provider: row.provider as "github",
        subject: row.subject as string,
        login: row.login as string | null,
        email: row.email as string | null,
        emailVerified: Boolean(row.email_verified),
        name: row.name as string | null,
        avatarUrl: row.avatar_url as string | null,
        credentialCiphertext: row.credential_ciphertext as string | null,
        createdAt: row.created_at as string,
        lastSeenAt: row.last_seen_at as string,
    };
}

function sourceTreeFromRow(row: Record<string, unknown>): SourceTreeRow {
    return {
        id: row.id as string,
        kind: row.kind as "github" | "local",
        owner: row.owner as string | null,
        name: row.name as string | null,
        defaultBranch: row.default_branch as string | null,
        createdBy: row.created_by as string | null,
        createdAt: row.created_at as string,
    };
}

function randomId(): string {
    return randomBytes(8).toString("hex");
}

function isoNow(): string {
    return new Date().toISOString();
}
