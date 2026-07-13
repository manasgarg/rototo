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

// The inverse of resourceString for the scopes the v1 admin surface
// administers: deployment, source-tree, and package. Entity-scoped grants
// exist in the schema but arrive with per-surface approvers; refusing to
// parse them here keeps the admin API honest about what it can validate.
export function parseResource(value: string): Resource | null {
    if (value === "deployment") {
        return { kind: "deployment" };
    }
    const tree = value.match(/^source-tree:([^/:]+)$/);
    if (tree !== null) {
        return { kind: "source-tree", sourceTree: tree[1] as string };
    }
    const pkg = value.match(/^package:([^/:]+)\/(.+)$/);
    if (pkg !== null) {
        return {
            kind: "package",
            sourceTree: pkg[1] as string,
            path: pkg[2] as string,
        };
    }
    return null;
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
    // The undirected extends-adjacency of package paths within a source
    // tree, for the lineage derivation (design/console-identity-authz.md
    // section 6): propose on a package derives view over the packages its
    // composition connects it to. Absent or empty means the derivation is
    // inert, which is also its state under Backend A (repo-wide view).
    packageLinks?: (sourceTree: string) => Promise<Map<string, string[]>>;
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
        context: DecisionContext = {},
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

        // The two-person default: under Backend B the console is the only
        // enforcement point, so a grant never approves a change its holder
        // helped write. `contributors` covers everyone who committed, not
        // just the creator, so shared change sets cannot launder approvals.
        // Backend A is deliberately untouched by this rule: there GitHub's
        // branch protection is the policy, and the console never gets
        // stricter than the authority.
        const contributed =
            action === "approve" &&
            (context.contributors ?? []).includes(principal.id);

        // Backend B: console grants, checked up the resource lineage; held
        // directly or through group membership.
        const grants = this.deps.store.grantsForPrincipal(principal.id);
        const lineage = resourceLineage(resource).map(resourceString);
        for (const grant of grants) {
            if (
                isAction(grant.action) &&
                actionImplies(grant.action, action) &&
                lineage.includes(grant.resource)
            ) {
                if (contributed) {
                    return {
                        allow: false,
                        backend: "grant",
                        reason: "you contributed to this change; a second person must approve it",
                    };
                }
                return {
                    allow: true,
                    backend: "grant",
                    reason: `${grant.action} grant on ${grant.resource}`,
                };
            }
        }

        // The lineage derivation: propose on a package grants view over the
        // packages its composition connects it to, in both directions —
        // upstream to edit with understanding, downstream to judge impact.
        // Derived visibility is view only; nothing else is ever derived.
        if (
            action === "view" &&
            resource.kind === "package" &&
            this.deps.packageLinks !== undefined
        ) {
            const derived = await this.derivedView(grants, resource);
            if (derived !== null) {
                return derived;
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

    // Every allow carries its path, so an auditor can always answer "why
    // can this person see this?".
    private async derivedView(
        grants: { action: string; resource: string }[],
        resource: Extract<Resource, { kind: "package" }>,
    ): Promise<Decision | null> {
        const packagePrefix = `package:${resource.sourceTree}/`;
        const entityPrefix = `entity:${resource.sourceTree}/`;
        const proposedOn = new Set<string>();
        for (const grant of grants) {
            if (
                !isAction(grant.action) ||
                !actionImplies(grant.action, "propose")
            ) {
                continue;
            }
            if (grant.resource.startsWith(packagePrefix)) {
                proposedOn.add(grant.resource.slice(packagePrefix.length));
            } else if (grant.resource.startsWith(entityPrefix)) {
                // The entity's package anchors the closure. The entity
                // segment is an address (`variable=x`); everything before
                // the first address-shaped segment is the package path.
                const rest = grant.resource.slice(entityPrefix.length);
                const cut = rest.search(/\/[a-z-]+=/);
                proposedOn.add(cut === -1 ? rest : rest.slice(0, cut));
            }
        }
        if (proposedOn.size === 0) {
            return null;
        }
        const links = await (
            this.deps.packageLinks as (
                sourceTree: string,
            ) => Promise<Map<string, string[]>>
        )(resource.sourceTree);
        for (const start of proposedOn) {
            const path = connectedPath(links, start, resource.path);
            if (path !== null) {
                return {
                    allow: true,
                    backend: "grant",
                    reason: `view derived from propose on package:${resource.sourceTree}/${start} (connected via ${path.join(" -> ")})`,
                };
            }
        }
        return null;
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

// Breadth-first over the undirected extends graph; returns the connecting
// path (start included) or null. Small graphs — a tree holds tens of
// packages, not thousands.
function connectedPath(
    links: Map<string, string[]>,
    start: string,
    goal: string,
): string[] | null {
    if (start === goal) {
        return [start];
    }
    const previous = new Map<string, string>([[start, start]]);
    const queue = [start];
    while (queue.length > 0) {
        const current = queue.shift() as string;
        for (const next of links.get(current) ?? []) {
            if (previous.has(next)) {
                continue;
            }
            previous.set(next, current);
            if (next === goal) {
                const path = [goal];
                let step = goal;
                while (step !== start) {
                    step = previous.get(step) as string;
                    path.unshift(step);
                }
                return path;
            }
            queue.push(next);
        }
    }
    return null;
}
