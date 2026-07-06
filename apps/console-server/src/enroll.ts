// Enrollment (design/console-identity-authz.md 3.4): completing
// authentication must not grant access. This module owns the one question
// both callbacks ask — does this verified identity get a principal? — and
// applies the deployment policy: invite-only (the hosted default),
// domain-allowlist, or open. Invitations carry initial groups and grants;
// the bootstrap admin list mints the first administer grant.

import type { ServerConfig } from "./config.ts";
import type { IdentitySnapshot, InvitationRow, Store } from "./store.ts";

export type EnrollmentOutcome =
    | { kind: "signed-in"; principalId: string }
    | { kind: "enrolled"; principalId: string }
    | { kind: "not-enrolled"; reason: string };

export function enrollOrSignIn(
    store: Store,
    config: ServerConfig,
    input: {
        snapshot: IdentitySnapshot;
        credentialCiphertext: string | null;
        // A redeem link's token hash; the token itself is the authorization.
        inviteTokenHash: string | null;
    },
): EnrollmentOutcome {
    const { snapshot } = input;

    // An identity already attached to a principal signs in; the snapshot
    // and credential refresh as a side effect.
    const existing = store.getIdentity(snapshot.provider, snapshot.subject);
    if (existing !== null) {
        store.refreshIdentity(
            existing.id,
            snapshot,
            input.credentialCiphertext,
        );
        bootstrapAdmin(store, config.admins, existing.principalId, snapshot);
        return { kind: "signed-in", principalId: existing.principalId };
    }

    // Not attached: the enrollment policy decides whether a principal is
    // created. Order matters — an explicit invitation always works, the
    // bootstrap list always works, then the policy's own gates.
    const invitation =
        (input.inviteTokenHash !== null
            ? store.invitationByTokenHash(input.inviteTokenHash)
            : null) ??
        (snapshot.email !== null && snapshot.emailVerified
            ? store.openInvitationForEmail(snapshot.email, snapshot.provider)
            : null);

    const bootstrap = matchesAdmins(config.admins, snapshot);
    const domainEnrolls =
        config.enrollment === "domain-allowlist" &&
        snapshot.email !== null &&
        snapshot.emailVerified &&
        config.enrollmentDomains.some((domain) =>
            (snapshot.email as string)
                .toLowerCase()
                .endsWith(`@${domain.toLowerCase()}`),
        );

    if (
        invitation === null &&
        !bootstrap &&
        !domainEnrolls &&
        config.enrollment !== "open"
    ) {
        return {
            kind: "not-enrolled",
            reason: "this identity is not enrolled; ask an administrator for an invitation",
        };
    }

    const principal = store.createPrincipal(
        snapshot.name ?? snapshot.login ?? snapshot.email ?? "unnamed",
    );
    store.attachIdentity(principal.id, snapshot, input.credentialCiphertext);
    store.appendAudit(
        null,
        "principal.enroll",
        JSON.stringify({
            principal: principal.id,
            provider: snapshot.provider,
            via:
                invitation !== null
                    ? `invitation ${invitation.id}`
                    : bootstrap
                      ? "admins bootstrap"
                      : domainEnrolls
                        ? "domain allowlist"
                        : "open enrollment",
        }),
    );
    if (invitation !== null) {
        applyInvitation(store, invitation, principal.id);
    }
    bootstrapAdmin(store, config.admins, principal.id, snapshot);
    return { kind: "enrolled", principalId: principal.id };
}

function applyInvitation(
    store: Store,
    invitation: InvitationRow,
    principalId: string,
): void {
    store.markInvitationRedeemed(invitation.id, principalId);
    for (const groupId of invitation.initialGroups) {
        if (store.getGroup(groupId) !== null) {
            store.addGroupMember(groupId, principalId);
        }
    }
    for (const grant of invitation.initialGrants) {
        store.insertGrant({
            granteeKind: "principal",
            granteeId: principalId,
            action: grant.action,
            resource: grant.resource,
            createdBy: invitation.createdBy,
        });
    }
    store.appendAudit(
        null,
        "invitation.redeem",
        JSON.stringify({ invitation: invitation.id, principal: principalId }),
    );
}

function matchesAdmins(admins: string[], snapshot: IdentitySnapshot): boolean {
    if (snapshot.provider === "github" && snapshot.login !== null) {
        return admins.includes(`github:${snapshot.login}`);
    }
    if (snapshot.provider === "oidc" && snapshot.email !== null) {
        return admins.some(
            (entry) =>
                entry.toLowerCase() ===
                `oidc:${(snapshot.email as string).toLowerCase()}`,
        );
    }
    return false;
}

// A fresh deployment reads ROTOTO_CONSOLE_ADMINS (`github:<login>` or
// `oidc:<email>`). The first sign-in matching an entry mints a durable
// deployment-scope administer grant; after that the env var is ignored for
// that entry, since logins and emails are mutable and the grant row is
// keyed to the stable principal.
export function bootstrapAdmin(
    store: Store,
    admins: string[],
    principalId: string,
    snapshot: IdentitySnapshot,
): void {
    if (!matchesAdmins(admins, snapshot)) {
        return;
    }
    const existing = store
        .grantsForPrincipal(principalId)
        .some(
            (grant) =>
                grant.action === "administer" &&
                grant.resource === "deployment",
        );
    if (existing) {
        return;
    }
    const grant = store.insertGrant({
        granteeKind: "principal",
        granteeId: principalId,
        action: "administer",
        resource: "deployment",
        createdBy: null,
    });
    store.appendAudit(
        null,
        "grant.create",
        JSON.stringify({
            grant: grant.id,
            grantee: principalId,
            action: "administer",
            resource: "deployment",
            via: "ROTOTO_CONSOLE_ADMINS bootstrap",
        }),
    );
}
