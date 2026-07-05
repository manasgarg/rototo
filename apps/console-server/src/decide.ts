// The decision point (design/console-identity-authz.md section 4): one
// internal function answers every permission question, shaped like an
// AuthZEN request — decide(subject, action, resource, context) — without
// taking on the wire protocol or a policy engine.
//
// Rules that hold regardless of backend: default deny; a disabled principal
// always gets deny; local mode short-circuits to allow; the server calls
// decide during every mutation, and anything sent to the browser earlier is
// explanation, never authority.

import type { GitHubFacts } from "./github.ts";
import type { Store } from "./store.ts";
import type { TokenCrypto } from "./token-crypto.ts";

export const ACTIONS = ["view", "propose", "approve", "administer"] as const;
export type Action = (typeof ACTIONS)[number];

// The four verbs are strictly ordered; a grant at a level implies the
// levels below it on the same resource.
const ACTION_RANK: Record<Action, number> = {
    view: 0,
    propose: 1,
    approve: 2,
    administer: 3,
};

export function actionImplies(granted: Action, requested: Action): boolean {
    return ACTION_RANK[granted] >= ACTION_RANK[requested];
}

// One node of the administrative resource tree:
//   deployment
//     source-tree:<id>
//       package:<source-tree-id>/<path>
//         entity:<source-tree-id>/<path>/<kind>/<id>
export type Resource =
    | { kind: "deployment" }
    | { kind: "source-tree"; sourceTree: string }
    | { kind: "package"; sourceTree: string; path: string }
    | {
          kind: "entity";
          sourceTree: string;
          path: string;
          entity: string;
      };

export function resourceString(resource: Resource): string {
    switch (resource.kind) {
        case "deployment":
            return "deployment";
        case "source-tree":
            return `source-tree:${resource.sourceTree}`;
        case "package":
            return `package:${resource.sourceTree}/${resource.path}`;
        case "entity":
            return `entity:${resource.sourceTree}/${resource.path}/${resource.entity}`;
    }
}

// The resource and its ancestors, nearest first; grants attach at any level
// and inherit downward.
export function resourceLineage(resource: Resource): Resource[] {
    switch (resource.kind) {
        case "deployment":
            return [resource];
        case "source-tree":
            return [resource, { kind: "deployment" }];
        case "package":
            return [
                resource,
                { kind: "source-tree", sourceTree: resource.sourceTree },
                { kind: "deployment" },
            ];
        case "entity":
            return [
                resource,
                {
                    kind: "package",
                    sourceTree: resource.sourceTree,
                    path: resource.path,
                },
                { kind: "source-tree", sourceTree: resource.sourceTree },
                { kind: "deployment" },
            ];
    }
}

export type Subject =
    // The single implicit principal of local mode: workstation trust.
    { kind: "local" } | { kind: "principal"; id: string };

export type DecisionContext = {
    // Principals who committed to the change under review; evaluated by
    // two-person policies from Layer 2 on. Carried in the seam from day one.
    contributors?: string[];
};

export type Decision = {
    allow: boolean;
    // Which backend produced the answer; null on default deny.
    backend: "local" | "grant" | "github" | null;
    // One sentence a human can read in an audit log.
    reason: string;
};

type DecisionDeps = {
    authMode: "local" | "team";
    store: Store;
    github: GitHubFacts;
    tokenCrypto: () => TokenCrypto;
};

export class DecisionPoint {
    private readonly deps: DecisionDeps;

    constructor(deps: DecisionDeps) {
        this.deps = deps;
    }

    async decide(
        subject: Subject,
        action: Action,
        resource: Resource,
        _context: DecisionContext = {},
    ): Promise<Decision> {
        if (this.deps.authMode === "local" || subject.kind === "local") {
            return {
                allow: true,
                backend: "local",
                reason: "local mode trusts the workstation",
            };
        }

        const principal = this.deps.store.getPrincipal(subject.id);
        if (principal === null) {
            return {
                allow: false,
                backend: null,
                reason: "no such principal",
            };
        }
        if (principal.status === "disabled") {
            return {
                allow: false,
                backend: null,
                reason: "principal is disabled",
            };
        }

        // Backend B: console grants, checked up the resource lineage.
        const grants = this.deps.store.grantsForPrincipal(principal.id);
        const lineage = resourceLineage(resource).map(resourceString);
        for (const grant of grants) {
            if (
                isAction(grant.action) &&
                actionImplies(grant.action, action) &&
                lineage.includes(grant.resource)
            ) {
                return {
                    allow: true,
                    backend: "grant",
                    reason: `${grant.action} grant on ${grant.resource}`,
                };
            }
        }

        // Backend A: GitHub-derived, advisory. Only source-tree-scoped
        // resources map onto a repository; deployment scope is grants-only.
        if (resource.kind !== "deployment") {
            const decision = await this.decideFromGitHub(
                principal.id,
                action,
                resource,
            );
            if (decision !== null) {
                return decision;
            }
        }

        return {
            allow: false,
            backend: null,
            reason: "no grant or GitHub permission matched (default deny)",
        };
    }

    private async decideFromGitHub(
        principalId: string,
        action: Action,
        resource: Exclude<Resource, { kind: "deployment" }>,
    ): Promise<Decision | null> {
        const tree = this.deps.store.getSourceTree(resource.sourceTree);
        if (
            tree === null ||
            tree.kind !== "github" ||
            tree.owner === null ||
            tree.name === null
        ) {
            return null;
        }
        const github = this.deps.store
            .identitiesForPrincipal(principalId)
            .find(
                (identity) =>
                    identity.provider === "github" &&
                    identity.credentialCiphertext !== null,
            );
        if (github === undefined) {
            return null;
        }
        let token: string;
        try {
            token = this.deps
                .tokenCrypto()
                .decrypt(github.credentialCiphertext as string);
        } catch {
            return null;
        }
        const facts = await this.deps.github.repoFacts(
            token,
            tree.owner,
            tree.name,
        );
        if (facts === null) {
            return {
                allow: false,
                backend: "github",
                reason: `GitHub does not show ${tree.owner}/${tree.name} to this identity`,
            };
        }
        const repo = `${tree.owner}/${tree.name}`;
        const granted = grantedByPermissions(facts.permissions);
        if (granted !== null && actionImplies(granted.action, action)) {
            return {
                allow: true,
                backend: "github",
                reason: `repo ${granted.permission} permission on ${repo}`,
            };
        }
        return {
            allow: false,
            backend: "github",
            reason: `no GitHub permission on ${repo} grants ${action}`,
        };
    }
}

// The Backend A mapping table: pull -> view, push -> propose,
// maintain/admin -> approve (the pre-proposal prediction), admin ->
// administer. Expressed as the strongest action the permissions grant, with
// the ladder covering everything below it.
function grantedByPermissions(permissions: {
    pull: boolean;
    push: boolean;
    maintain: boolean;
    admin: boolean;
}): { action: Action; permission: string } | null {
    if (permissions.admin) {
        return { action: "administer", permission: "admin" };
    }
    if (permissions.maintain) {
        return { action: "approve", permission: "maintain" };
    }
    if (permissions.push) {
        return { action: "propose", permission: "push" };
    }
    if (permissions.pull) {
        return { action: "view", permission: "pull" };
    }
    return null;
}

function isAction(value: string): value is Action {
    return (ACTIONS as readonly string[]).includes(value);
}
