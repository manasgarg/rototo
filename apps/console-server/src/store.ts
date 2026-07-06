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

const SCHEMA_VERSION = 3;

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
    provider: "github" | "oidc";
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
    provider: "github" | "oidc";
    subject: string;
    login: string | null;
    email: string | null;
    emailVerified: boolean;
    name: string | null;
    avatarUrl: string | null;
};

export type GroupRow = {
    id: string;
    name: string;
    description: string | null;
    createdAt: string;
};

export type InvitationRow = {
    id: string;
    email: string;
    providerRestriction: string | null;
    initialGroups: string[];
    initialGrants: { action: string; resource: string }[];
    tokenHash: string;
    expiresAt: string;
    redeemedBy: string | null;
    createdBy: string | null;
    createdAt: string;
};

export type ChangeSetApprovalRow = {
    changeSetId: string;
    principalId: string;
    approvedAt: string;
};

export type ChangeSetState = "draft" | "proposed" | "merged" | "abandoned";

// One row per proposed change: one branch, at most one PR
// (design/console-git-ops.md). Handlers write the intent columns; the
// reconciler alone writes the observed ones (prNumber through observedVia).
export type ChangeSetRow = {
    id: string;
    sourceTreeId: string;
    title: string;
    authorPrincipal: string;
    actingMode: "user" | "app";
    baseRef: string;
    baseShaAtCreation: string | null;
    state: ChangeSetState;
    prNumber: number | null;
    prUrl: string | null;
    headSha: string | null;
    behindBase: boolean;
    conflicted: boolean;
    observedVia: string | null;
    lastReconciledAt: string | null;
    createdAt: string;
    updatedAt: string;
};

// The observed facts, written only by the reconciler.
export type ChangeSetObserved = {
    state?: ChangeSetState;
    prNumber?: number | null;
    prUrl?: string | null;
    headSha: string | null;
    behindBase: boolean;
    conflicted: boolean;
    observedVia: string;
};

export type ChangeSetCollaboratorRow = {
    changeSetId: string;
    principalId: string;
    addedBy: string;
    addedAt: string;
};

export type ChangeSetEventRow = {
    id: number;
    changeSetId: string;
    at: string;
    actor: string | null;
    event: string;
    detail: string | null;
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
            CREATE TABLE IF NOT EXISTS change_sets (
                id TEXT PRIMARY KEY,
                source_tree_id TEXT NOT NULL REFERENCES source_trees(id),
                title TEXT NOT NULL,
                author_principal TEXT NOT NULL,
                acting_mode TEXT NOT NULL CHECK (acting_mode IN ('user', 'app')),
                base_ref TEXT NOT NULL,
                base_sha_at_creation TEXT,
                state TEXT NOT NULL
                    CHECK (state IN ('draft', 'proposed', 'merged', 'abandoned')),
                pr_number INTEGER,
                pr_url TEXT,
                head_sha TEXT,
                behind_base INTEGER NOT NULL DEFAULT 0,
                conflicted INTEGER NOT NULL DEFAULT 0,
                observed_via TEXT,
                last_reconciled_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS change_set_collaborators (
                change_set_id TEXT NOT NULL REFERENCES change_sets(id),
                principal_id TEXT NOT NULL,
                added_by TEXT NOT NULL,
                added_at TEXT NOT NULL,
                UNIQUE (change_set_id, principal_id)
            );
            CREATE TABLE IF NOT EXISTS change_set_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                change_set_id TEXT NOT NULL REFERENCES change_sets(id),
                at TEXT NOT NULL,
                actor TEXT,
                event TEXT NOT NULL,
                detail TEXT
            );
            CREATE TABLE IF NOT EXISTS groups (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                description TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS group_members (
                group_id TEXT NOT NULL REFERENCES groups(id),
                principal_id TEXT NOT NULL REFERENCES principals(id),
                UNIQUE (group_id, principal_id)
            );
            CREATE TABLE IF NOT EXISTS invitations (
                id TEXT PRIMARY KEY,
                email TEXT NOT NULL,
                provider_restriction TEXT,
                initial_groups TEXT NOT NULL DEFAULT '[]',
                initial_grants TEXT NOT NULL DEFAULT '[]',
                token_hash TEXT NOT NULL UNIQUE,
                expires_at TEXT NOT NULL,
                redeemed_by TEXT,
                created_by TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS change_set_approvals (
                change_set_id TEXT NOT NULL REFERENCES change_sets(id),
                principal_id TEXT NOT NULL,
                approved_at TEXT NOT NULL,
                UNIQUE (change_set_id, principal_id)
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

    // Best-effort lookup for the fire drill's authorship rebuild. Logins
    // are mutable display data, never authorization keys; (provider,
    // subject) stays the identity key everywhere else.
    identityByLogin(provider: string, login: string): IdentityRow | null {
        const row = this.db
            .prepare(
                "SELECT * FROM identities WHERE provider = ? AND login = ? ORDER BY last_seen_at DESC LIMIT 1",
            )
            .get(provider, login) as Record<string, unknown> | undefined;
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

    // Every grant that reaches the principal: held directly, or through a
    // group membership. decide() evaluates this union.
    grantsForPrincipal(principalId: string): GrantRow[] {
        const rows = this.db
            .prepare(
                `SELECT * FROM grants
                 WHERE (grantee_kind = 'principal' AND grantee_id = ?)
                    OR (grantee_kind = 'group' AND grantee_id IN (
                        SELECT group_id FROM group_members WHERE principal_id = ?
                    ))`,
            )
            .all(principalId, principalId) as Record<string, unknown>[];
        return rows.map(grantFromRow);
    }

    listGrants(): GrantRow[] {
        const rows = this.db
            .prepare("SELECT * FROM grants ORDER BY created_at")
            .all() as Record<string, unknown>[];
        return rows.map(grantFromRow);
    }

    getGrant(id: string): GrantRow | null {
        const row = this.db
            .prepare("SELECT * FROM grants WHERE id = ?")
            .get(id) as Record<string, unknown> | undefined;
        return row === undefined ? null : grantFromRow(row);
    }

    deleteGrant(id: string): void {
        this.db.prepare("DELETE FROM grants WHERE id = ?").run(id);
    }

    appendAudit(actor: string | null, event: string, detail: string): void {
        this.db
            .prepare(
                "INSERT INTO authz_audit (at, actor, event, detail) VALUES (?, ?, ?, ?)",
            )
            .run(isoNow(), actor, event, detail);
    }

    listAudit(): {
        at: string;
        actor: string | null;
        event: string;
        detail: string | null;
    }[] {
        const rows = this.db
            .prepare(
                "SELECT at, actor, event, detail FROM authz_audit ORDER BY id",
            )
            .all() as Record<string, unknown>[];
        return rows.map((row) => ({
            at: row.at as string,
            actor: row.actor as string | null,
            event: row.event as string,
            detail: row.detail as string | null,
        }));
    }

    listPrincipals(): PrincipalRow[] {
        const rows = this.db
            .prepare("SELECT * FROM principals ORDER BY created_at")
            .all() as Record<string, unknown>[];
        return rows.map(principalFromRow);
    }

    // Groups: console-managed sets of principals, existing to make grants
    // administrable and nothing more. Surface approval roles (role:<id>)
    // name groups by name.

    createGroup(name: string, description: string | null): GroupRow {
        const row: GroupRow = {
            id: `grp_${randomId()}`,
            name,
            description,
            createdAt: isoNow(),
        };
        this.db
            .prepare(
                "INSERT INTO groups (id, name, description, created_at) VALUES (?, ?, ?, ?)",
            )
            .run(row.id, row.name, row.description, row.createdAt);
        return row;
    }

    getGroup(id: string): GroupRow | null {
        const row = this.db
            .prepare("SELECT * FROM groups WHERE id = ?")
            .get(id) as Record<string, unknown> | undefined;
        return row === undefined ? null : groupFromRow(row);
    }

    getGroupByName(name: string): GroupRow | null {
        const row = this.db
            .prepare("SELECT * FROM groups WHERE name = ?")
            .get(name) as Record<string, unknown> | undefined;
        return row === undefined ? null : groupFromRow(row);
    }

    listGroups(): GroupRow[] {
        const rows = this.db
            .prepare("SELECT * FROM groups ORDER BY name")
            .all() as Record<string, unknown>[];
        return rows.map(groupFromRow);
    }

    addGroupMember(groupId: string, principalId: string): void {
        this.db
            .prepare(
                "INSERT OR IGNORE INTO group_members (group_id, principal_id) VALUES (?, ?)",
            )
            .run(groupId, principalId);
    }

    removeGroupMember(groupId: string, principalId: string): void {
        this.db
            .prepare(
                "DELETE FROM group_members WHERE group_id = ? AND principal_id = ?",
            )
            .run(groupId, principalId);
    }

    listGroupMembers(groupId: string): string[] {
        const rows = this.db
            .prepare(
                "SELECT principal_id FROM group_members WHERE group_id = ?",
            )
            .all(groupId) as Record<string, unknown>[];
        return rows.map((row) => row.principal_id as string);
    }

    groupsForPrincipal(principalId: string): GroupRow[] {
        const rows = this.db
            .prepare(
                `SELECT g.* FROM groups g
                 JOIN group_members m ON m.group_id = g.id
                 WHERE m.principal_id = ? ORDER BY g.name`,
            )
            .all(principalId) as Record<string, unknown>[];
        return rows.map(groupFromRow);
    }

    // Invitations: single-use, expiring, matched by email (and optionally
    // provider) at sign-in. The token is delivered out of band; only its
    // hash is stored.

    createInvitation(input: {
        email: string;
        providerRestriction: string | null;
        initialGroups: string[];
        initialGrants: { action: string; resource: string }[];
        tokenHash: string;
        expiresAt: string;
        createdBy: string | null;
    }): InvitationRow {
        const row: InvitationRow = {
            id: `inv_${randomId()}`,
            email: input.email,
            providerRestriction: input.providerRestriction,
            initialGroups: input.initialGroups,
            initialGrants: input.initialGrants,
            tokenHash: input.tokenHash,
            expiresAt: input.expiresAt,
            redeemedBy: null,
            createdBy: input.createdBy,
            createdAt: isoNow(),
        };
        this.db
            .prepare(
                `INSERT INTO invitations (
                    id, email, provider_restriction, initial_groups,
                    initial_grants, token_hash, expires_at, created_by, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
            )
            .run(
                row.id,
                row.email,
                row.providerRestriction,
                JSON.stringify(row.initialGroups),
                JSON.stringify(row.initialGrants),
                row.tokenHash,
                row.expiresAt,
                row.createdBy,
                row.createdAt,
            );
        return row;
    }

    listInvitations(): InvitationRow[] {
        const rows = this.db
            .prepare("SELECT * FROM invitations ORDER BY created_at")
            .all() as Record<string, unknown>[];
        return rows.map(invitationFromRow);
    }

    // The open invitation a redeem link names; the token itself is the
    // authorization, so no email check happens on this path.
    invitationByTokenHash(tokenHash: string): InvitationRow | null {
        const row = this.db
            .prepare(
                "SELECT * FROM invitations WHERE token_hash = ? AND redeemed_by IS NULL",
            )
            .get(tokenHash) as Record<string, unknown> | undefined;
        if (row === undefined) {
            return null;
        }
        const invitation = invitationFromRow(row);
        return Date.parse(invitation.expiresAt) <= Date.now()
            ? null
            : invitation;
    }

    // The open (unredeemed, unexpired) invitation matching a verified email
    // at sign-in, optionally restricted to the provider used.
    openInvitationForEmail(
        email: string,
        provider: string,
    ): InvitationRow | null {
        const rows = this.db
            .prepare(
                "SELECT * FROM invitations WHERE redeemed_by IS NULL AND lower(email) = lower(?) ORDER BY created_at",
            )
            .all(email) as Record<string, unknown>[];
        for (const raw of rows) {
            const row = invitationFromRow(raw);
            if (Date.parse(row.expiresAt) <= Date.now()) {
                continue;
            }
            if (
                row.providerRestriction !== null &&
                row.providerRestriction !== provider
            ) {
                continue;
            }
            return row;
        }
        return null;
    }

    markInvitationRedeemed(id: string, principalId: string): void {
        this.db
            .prepare(
                "UPDATE invitations SET redeemed_by = ? WHERE id = ? AND redeemed_by IS NULL",
            )
            .run(principalId, id);
    }

    // Approvals: who approved which change set. The PR comment keeps the
    // copy the fire drill can rebuild from.

    addApproval(changeSetId: string, principalId: string): void {
        this.db
            .prepare(
                `INSERT OR IGNORE INTO change_set_approvals
                    (change_set_id, principal_id, approved_at)
                 VALUES (?, ?, ?)`,
            )
            .run(changeSetId, principalId, isoNow());
    }

    listApprovals(changeSetId: string): ChangeSetApprovalRow[] {
        const rows = this.db
            .prepare(
                "SELECT * FROM change_set_approvals WHERE change_set_id = ? ORDER BY approved_at",
            )
            .all(changeSetId) as Record<string, unknown>[];
        return rows.map((row) => ({
            changeSetId: row.change_set_id as string,
            principalId: row.principal_id as string,
            approvedAt: row.approved_at as string,
        }));
    }

    // Change sets. Intent columns (title, state on submit/abandon) are
    // written by request handlers; the observed columns are written only
    // through updateChangeSetObserved, which the reconciler owns.

    insertChangeSet(input: {
        id: string;
        sourceTreeId: string;
        title: string;
        authorPrincipal: string;
        actingMode: "user" | "app";
        baseRef: string;
        baseShaAtCreation: string | null;
        state: ChangeSetState;
    }): ChangeSetRow {
        const now = isoNow();
        this.db
            .prepare(
                `INSERT INTO change_sets (
                    id, source_tree_id, title, author_principal, acting_mode,
                    base_ref, base_sha_at_creation, state, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
            )
            .run(
                input.id,
                input.sourceTreeId,
                input.title,
                input.authorPrincipal,
                input.actingMode,
                input.baseRef,
                input.baseShaAtCreation,
                input.state,
                now,
                now,
            );
        return this.getChangeSet(input.id) as ChangeSetRow;
    }

    getChangeSet(id: string): ChangeSetRow | null {
        const row = this.db
            .prepare("SELECT * FROM change_sets WHERE id = ?")
            .get(id) as Record<string, unknown> | undefined;
        return row === undefined ? null : changeSetFromRow(row);
    }

    listChangeSets(sourceTreeId: string): ChangeSetRow[] {
        const rows = this.db
            .prepare(
                "SELECT * FROM change_sets WHERE source_tree_id = ? ORDER BY created_at DESC",
            )
            .all(sourceTreeId) as Record<string, unknown>[];
        return rows.map(changeSetFromRow);
    }

    // The reconciler's work list: everything still able to change on GitHub.
    listOpenChangeSets(): ChangeSetRow[] {
        const rows = this.db
            .prepare(
                "SELECT * FROM change_sets WHERE state IN ('draft', 'proposed') ORDER BY created_at",
            )
            .all() as Record<string, unknown>[];
        return rows.map(changeSetFromRow);
    }

    // Handler-side transitions: submit (draft -> proposed) and abandon.
    setChangeSetState(id: string, state: ChangeSetState): void {
        this.db
            .prepare(
                "UPDATE change_sets SET state = ?, updated_at = ? WHERE id = ?",
            )
            .run(state, isoNow(), id);
    }

    // The reconciler's single writer for observed facts (rule 4: write down
    // what we want, watch what actually happens).
    updateChangeSetObserved(id: string, observed: ChangeSetObserved): void {
        const now = isoNow();
        const current = this.getChangeSet(id);
        if (current === null) {
            return;
        }
        this.db
            .prepare(
                `UPDATE change_sets SET
                    state = ?, pr_number = ?, pr_url = ?, head_sha = ?,
                    behind_base = ?, conflicted = ?, observed_via = ?,
                    last_reconciled_at = ?, updated_at = ?
                 WHERE id = ?`,
            )
            .run(
                observed.state ?? current.state,
                observed.prNumber === undefined
                    ? current.prNumber
                    : observed.prNumber,
                observed.prUrl === undefined ? current.prUrl : observed.prUrl,
                observed.headSha,
                observed.behindBase ? 1 : 0,
                observed.conflicted ? 1 : 0,
                observed.observedVia,
                now,
                now,
                id,
            );
    }

    addChangeSetCollaborator(
        changeSetId: string,
        principalId: string,
        addedBy: string,
    ): void {
        this.db
            .prepare(
                `INSERT OR IGNORE INTO change_set_collaborators
                    (change_set_id, principal_id, added_by, added_at)
                 VALUES (?, ?, ?, ?)`,
            )
            .run(changeSetId, principalId, addedBy, isoNow());
    }

    listChangeSetCollaborators(
        changeSetId: string,
    ): ChangeSetCollaboratorRow[] {
        const rows = this.db
            .prepare(
                "SELECT * FROM change_set_collaborators WHERE change_set_id = ? ORDER BY added_at",
            )
            .all(changeSetId) as Record<string, unknown>[];
        return rows.map((row) => ({
            changeSetId: row.change_set_id as string,
            principalId: row.principal_id as string,
            addedBy: row.added_by as string,
            addedAt: row.added_at as string,
        }));
    }

    // The append-only diary: Layer 2's audit trail.
    appendChangeSetEvent(
        changeSetId: string,
        actor: string | null,
        event: string,
        detail: string | null,
    ): void {
        this.db
            .prepare(
                `INSERT INTO change_set_events (change_set_id, at, actor, event, detail)
                 VALUES (?, ?, ?, ?, ?)`,
            )
            .run(changeSetId, isoNow(), actor, event, detail);
    }

    listChangeSetEvents(changeSetId: string): ChangeSetEventRow[] {
        const rows = this.db
            .prepare(
                "SELECT * FROM change_set_events WHERE change_set_id = ? ORDER BY id",
            )
            .all(changeSetId) as Record<string, unknown>[];
        return rows.map((row) => ({
            id: row.id as number,
            changeSetId: row.change_set_id as string,
            at: row.at as string,
            actor: row.actor as string | null,
            event: row.event as string,
            detail: row.detail as string | null,
        }));
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

function grantFromRow(row: Record<string, unknown>): GrantRow {
    return {
        id: row.id as string,
        granteeKind: row.grantee_kind as "principal" | "group",
        granteeId: row.grantee_id as string,
        action: row.action as string,
        resource: row.resource as string,
        createdBy: row.created_by as string | null,
        createdAt: row.created_at as string,
    };
}

function groupFromRow(row: Record<string, unknown>): GroupRow {
    return {
        id: row.id as string,
        name: row.name as string,
        description: row.description as string | null,
        createdAt: row.created_at as string,
    };
}

function invitationFromRow(row: Record<string, unknown>): InvitationRow {
    return {
        id: row.id as string,
        email: row.email as string,
        providerRestriction: row.provider_restriction as string | null,
        initialGroups: JSON.parse(row.initial_groups as string) as string[],
        initialGrants: JSON.parse(row.initial_grants as string) as {
            action: string;
            resource: string;
        }[],
        tokenHash: row.token_hash as string,
        expiresAt: row.expires_at as string,
        redeemedBy: row.redeemed_by as string | null,
        createdBy: row.created_by as string | null,
        createdAt: row.created_at as string,
    };
}

function identityFromRow(row: Record<string, unknown>): IdentityRow {
    return {
        id: row.id as string,
        principalId: row.principal_id as string,
        provider: row.provider as "github" | "oidc",
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

function changeSetFromRow(row: Record<string, unknown>): ChangeSetRow {
    return {
        id: row.id as string,
        sourceTreeId: row.source_tree_id as string,
        title: row.title as string,
        authorPrincipal: row.author_principal as string,
        actingMode: row.acting_mode as "user" | "app",
        baseRef: row.base_ref as string,
        baseShaAtCreation: row.base_sha_at_creation as string | null,
        state: row.state as ChangeSetState,
        prNumber: row.pr_number as number | null,
        prUrl: row.pr_url as string | null,
        headSha: row.head_sha as string | null,
        behindBase: Boolean(row.behind_base),
        conflicted: Boolean(row.conflicted),
        observedVia: row.observed_via as string | null,
        lastReconciledAt: row.last_reconciled_at as string | null,
        createdAt: row.created_at as string,
        updatedAt: row.updated_at as string,
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
