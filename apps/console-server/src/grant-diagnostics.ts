// Grant diagnostics (design/console-identity-authz.md 6.3): treat grant
// configuration the way rototo treats packages — validate it and report
// incoherence. These are console diagnostics with their own listing, never
// package lint, and they never fail anything.

import type { Store } from "./store.ts";

export type GrantDiagnostic = {
    severity: "warning" | "info";
    message: string;
    // What the diagnostic is about, for the admin UI to link.
    subject: { kind: "grant" | "group" | "principal"; id: string };
};

export function grantDiagnostics(store: Store): GrantDiagnostic[] {
    const diagnostics: GrantDiagnostic[] = [];
    const trees = new Set(store.listSourceTrees().map((tree) => tree.id));
    const groups = new Map(store.listGroups().map((group) => [group.id, group]));
    const principals = new Map(
        store.listPrincipals().map((principal) => [principal.id, principal]),
    );

    for (const grant of store.listGrants()) {
        // A grant whose resource points at nothing: the package moved, or
        // the tree was deregistered. Package paths cannot be verified
        // without staging, but the tree id can.
        const treeRef = grant.resource.match(
            /^(?:source-tree:|package:)([^/:]+)/,
        );
        if (treeRef !== null && !trees.has(treeRef[1] as string)) {
            diagnostics.push({
                severity: "warning",
                message: `grant ${grant.id} (${grant.action} on ${grant.resource}) points at a source tree that is not registered`,
                subject: { kind: "grant", id: grant.id },
            });
        }
        if (grant.granteeKind === "group" && !groups.has(grant.granteeId)) {
            diagnostics.push({
                severity: "warning",
                message: `grant ${grant.id} names group ${grant.granteeId}, which no longer exists`,
                subject: { kind: "grant", id: grant.id },
            });
        }
        if (grant.granteeKind === "principal") {
            const principal = principals.get(grant.granteeId);
            if (principal === undefined) {
                diagnostics.push({
                    severity: "warning",
                    message: `grant ${grant.id} names a principal that does not exist`,
                    subject: { kind: "grant", id: grant.id },
                });
            } else if (principal.status === "disabled") {
                diagnostics.push({
                    severity: "info",
                    message: `grant ${grant.id} is held by ${principal.displayName}, who is disabled; it does nothing until they are re-enabled`,
                    subject: { kind: "principal", id: principal.id },
                });
            }
        }
    }

    // A group with grants but no active members: the grants are dead
    // weight, and any approval role naming the group cannot be satisfied.
    const grantedGroups = new Set(
        store
            .listGrants()
            .filter((grant) => grant.granteeKind === "group")
            .map((grant) => grant.granteeId),
    );
    for (const [id, group] of groups) {
        const activeMembers = store
            .listGroupMembers(id)
            .filter(
                (member) => principals.get(member)?.status === "active",
            );
        if (activeMembers.length === 0 && grantedGroups.has(id)) {
            diagnostics.push({
                severity: "warning",
                message: `group ${group.name} holds grants but has no active members; nothing can act on them, and a surface approval naming role:${group.name} cannot be satisfied`,
                subject: { kind: "group", id },
            });
        }
    }

    return diagnostics;
}
