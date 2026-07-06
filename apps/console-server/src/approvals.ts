// Approval policy (design/console-git-ops.md "Getting it approved and
// merged" + design/console-surfaces.md `approval`). A change set touching
// several surfaces must satisfy every touched surface's requirement:
// `role:<name>` demands an approval from an active member of that console
// group, `none` demands nothing (kill switches, failover — speed is the
// point), and files no surface covers fall under the deployment default of
// one second-person approval. Contributors never count toward any
// requirement — helping write a change disqualifies approving it.

import { branchFor, repoId } from "./change-sets.ts";
import type { GitOps } from "./git.ts";
import { native } from "./native.ts";
import type { PackageStager } from "./packages.ts";
import type { ChangeSetRow, SourceTreeRow, Store } from "./store.ts";
import { bindingPaths, readSurfaces, type ModelView } from "./surfaces.ts";

// Everyone who committed to the change set: its creator plus any
// collaborator with a committed diary entry. Derived from the console's
// own records; commits pushed to the branch with raw git attribute
// themselves through GitHub instead.
export function contributorsOf(store: Store, changeSet: ChangeSetRow): string[] {
    const contributors = new Set<string>([changeSet.authorPrincipal]);
    for (const event of store.listChangeSetEvents(changeSet.id)) {
        if (event.event === "committed" && event.actor !== null) {
            contributors.add(event.actor);
        }
    }
    return [...contributors];
}

export type ApprovalRequirement =
    // A surface named a console role; an active member of the group must
    // approve.
    | { kind: "role"; role: string; surfaces: string[] }
    // The deployment default: one approval from someone who did not
    // contribute.
    | { kind: "second-person" };

export type ApprovalPolicy = {
    requirements: ApprovalRequirement[];
    // Surfaces that declared approval = "none"; informational.
    autoApproved: string[];
};

// What this change set must satisfy, from the surfaces its files touch on
// the branch head. Files outside every surface's bindings keep the
// deployment default in force.
export async function approvalPolicy(
    deps: { git: GitOps; stager: PackageStager },
    tree: SourceTreeRow,
    changeSet: ChangeSetRow,
    token: string,
): Promise<ApprovalPolicy> {
    const repo = repoId(tree);
    const branch = branchFor(changeSet.id);
    const head = await deps.git.getRef(token, repo, branch);
    if (head === null) {
        return { requirements: [{ kind: "second-person" }], autoApproved: [] };
    }
    const comparison = await deps.git.compare(
        token,
        repo,
        changeSet.baseRef,
        branch,
    );
    const treeRoot = await deps.stager.stageTree(tree, head, token);
    const packages = await native.discoverPackages(treeRoot);

    const roleRequirements = new Map<string, Set<string>>();
    const autoApproved: string[] = [];
    let uncovered = comparison.files.length === 0 ? true : false;

    for (const packagePath of packages) {
        const prefix = packagePath === "." ? "" : `${packagePath}/`;
        const touched = comparison.files
            .filter((file) => file.startsWith(prefix))
            .map((file) => file.slice(prefix.length));
        if (touched.length === 0) {
            continue;
        }
        const packageRoot = deps.stager.packageRoot(treeRoot, packagePath);
        const model = (await native
            .semanticModel(packageRoot)
            .catch(() => null)) as ModelView | null;
        if (model === null) {
            // A package that cannot even model gets the default policy.
            uncovered = true;
            continue;
        }
        const surfaces = readSurfaces(model);
        const covered = new Set<string>();
        for (const surface of surfaces) {
            const paths = bindingPaths(surface, model);
            const touches = touched.filter((file) =>
                paths.some(
                    (bindingPath) =>
                        file === bindingPath ||
                        file.startsWith(`${bindingPath}/`),
                ),
            );
            if (touches.length === 0) {
                continue;
            }
            for (const file of touches) {
                covered.add(file);
            }
            const approval = surface.approval;
            if (approval === "none") {
                autoApproved.push(surface.id);
            } else if (approval !== null && approval.startsWith("role:")) {
                const role = approval.slice(5);
                const set = roleRequirements.get(role) ?? new Set<string>();
                set.add(surface.id);
                roleRequirements.set(role, set);
            } else {
                // A surface with no declared approval keeps the default.
                uncovered = true;
            }
        }
        if (touched.some((file) => !covered.has(file))) {
            uncovered = true;
        }
    }

    // Files in no package at all (workflows, READMEs) keep the default too.
    const inSomePackage = (file: string): boolean =>
        packages.some((packagePath) =>
            packagePath === "."
                ? true
                : file.startsWith(`${packagePath}/`),
        );
    if (comparison.files.some((file) => !inSomePackage(file))) {
        uncovered = true;
    }

    const requirements: ApprovalRequirement[] = [
        ...[...roleRequirements.entries()].map(
            ([role, surfaces]): ApprovalRequirement => ({
                kind: "role",
                role,
                surfaces: [...surfaces].sort(),
            }),
        ),
    ];
    if (uncovered) {
        requirements.push({ kind: "second-person" });
    }
    return { requirements, autoApproved };
}

export type PolicyStatus = {
    satisfied: boolean;
    // Human-readable, one line per outstanding requirement.
    missing: string[];
};

// Approvals on record versus the requirements, contributors excluded.
export function policyStatus(
    store: Store,
    changeSet: ChangeSetRow,
    policy: ApprovalPolicy,
): PolicyStatus {
    const contributors = new Set(contributorsOf(store, changeSet));
    const approvals = store
        .listApprovals(changeSet.id)
        .map((row) => row.principalId)
        .filter((principal) => !contributors.has(principal))
        .filter(
            (principal) =>
                store.getPrincipal(principal)?.status === "active",
        );
    const missing: string[] = [];
    for (const requirement of policy.requirements) {
        if (requirement.kind === "second-person") {
            if (approvals.length === 0) {
                missing.push("an approval from someone who did not contribute");
            }
            continue;
        }
        const group = store.getGroupByName(requirement.role);
        const members = new Set(
            group === null ? [] : store.listGroupMembers(group.id),
        );
        if (!approvals.some((principal) => members.has(principal))) {
            missing.push(
                `an approval from role:${requirement.role} (surface${requirement.surfaces.length === 1 ? "" : "s"} ${requirement.surfaces.join(", ")})`,
            );
        }
    }
    return { satisfied: missing.length === 0, missing };
}
