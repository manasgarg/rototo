// The admin surface (tranche C5): principals, groups, grants, invitations,
// and grant diagnostics. Everything here is explanation and intent; the
// server re-decides administer on every act and appends to authz_audit.

import { useCallback, useEffect, useState } from "react";

import {
    adminCreateGrant,
    adminCreateGroup,
    adminCreateInvitation,
    adminDeleteGroup,
    adminDiagnostics,
    adminGrants,
    adminGroupMember,
    adminGroups,
    adminInvitations,
    adminPrincipals,
    adminRevokeGrant,
    adminRevokeInvitation,
    adminSetPrincipalStatus,
    adminSourceTrees,
    adminUnlinkIdentity,
    adminUpdateGroup,
    deregisterSourceTree,
    registerSourceTree,
    setSourceTreeBranch,
    type AdminDiagnostic,
    type AdminGrant,
    type AdminGroup,
    type AdminInvitation,
    type AdminPrincipal,
    type AdminSourceTree,
    type MeResponse,
} from "@/lib/api";

export function AdminPage({ me }: { me: MeResponse }) {
    const [principals, setPrincipals] = useState<AdminPrincipal[]>([]);
    const [groups, setGroups] = useState<AdminGroup[]>([]);
    const [grants, setGrants] = useState<AdminGrant[]>([]);
    const [invitations, setInvitations] = useState<AdminInvitation[]>([]);
    const [trees, setTrees] = useState<AdminSourceTree[]>([]);
    const [diagnostics, setDiagnostics] = useState<AdminDiagnostic[]>([]);
    const [error, setError] = useState<string | null>(null);
    const [inviteLink, setInviteLink] = useState<string | null>(null);

    const allowed = me.capabilities?.deployment.administer.allow === true;

    const refresh = useCallback(() => {
        if (!allowed) {
            return;
        }
        Promise.all([
            adminPrincipals(),
            adminGroups(),
            adminGrants(),
            adminInvitations(),
            adminSourceTrees(),
            adminDiagnostics(),
        ]).then(
            ([p, g, gr, inv, st, diag]) => {
                setPrincipals(p.principals);
                setGroups(g.groups);
                setGrants(gr.grants);
                setInvitations(inv.invitations);
                setTrees(st.sourceTrees);
                setDiagnostics(diag.diagnostics);
                setError(null);
            },
            (failure: Error) => setError(failure.message),
        );
    }, [allowed]);
    useEffect(() => {
        refresh();
    }, [refresh]);

    if (!allowed) {
        return (
            <div className="card">
                <h1>Administration</h1>
                <p className="hint">
                    {me.capabilities?.deployment.administer.reason ??
                        "administer at deployment scope is required"}
                </p>
            </div>
        );
    }

    const act = (action: Promise<unknown>) => {
        action.then(refresh, (failure: Error) => setError(failure.message));
    };
    const nameOf = (principalId: string): string =>
        principals.find((p) => p.id === principalId)?.displayName ??
        principalId;

    return (
        <div className="section">
            <div className="section-header-text">
                <h1>Administration</h1>
                <p className="hint">
                    Grants are allow-only; the only deny is absence or a
                    disabled principal, so this page reads top to bottom.
                </p>
            </div>
            {error !== null ? (
                <div className="banner banner-err">{error}</div>
            ) : null}
            {diagnostics.map((diagnostic, index) => (
                <div
                    className={`banner ${diagnostic.severity === "warning" ? "banner-warn" : "banner-info"}`}
                    key={index}
                >
                    {diagnostic.message}
                </div>
            ))}

            <div className="card">
                <h2>Source trees</h2>
                <p className="hint">
                    Deregistering hides a tree and blocks new change sets; its
                    merged history stays, and registering the same repository
                    again reactivates it.
                </p>
                <div className="row-list">
                    {trees.map((tree) => (
                        <div className="row row-static" key={tree.id}>
                            <span className="row-text">
                                <span className="row-title mono">
                                    {tree.kind === "github"
                                        ? `${tree.owner}/${tree.name}`
                                        : tree.id}
                                </span>
                                <span className="row-sub mono">
                                    {tree.defaultBranch ??
                                        "default branch unknown"}
                                </span>
                            </span>
                            <span className="row-side">
                                {tree.status === "deregistered" ? (
                                    <span className="pill pill-neutral">
                                        deregistered
                                    </span>
                                ) : (
                                    <>
                                        <BranchEditor
                                            tree={tree}
                                            onSave={(branch) =>
                                                act(
                                                    setSourceTreeBranch(
                                                        tree.id,
                                                        branch,
                                                    ),
                                                )
                                            }
                                        />
                                        <button
                                            className="btn btn-ghost btn-sm"
                                            onClick={() =>
                                                act(
                                                    deregisterSourceTree(
                                                        tree.id,
                                                    ),
                                                )
                                            }
                                        >
                                            Deregister
                                        </button>
                                    </>
                                )}
                            </span>
                        </div>
                    ))}
                </div>
                <RegisterTreeForm
                    onRegister={(input) => act(registerSourceTree(input))}
                />
            </div>

            <div className="card">
                <h2>People</h2>
                <div className="row-list">
                    {principals.map((principal) => (
                        <div className="row row-static" key={principal.id}>
                            <span className="row-text">
                                <span className="row-title">
                                    {principal.displayName}
                                </span>
                                <span className="row-sub">
                                    {principal.identities.map(
                                        (identity, index) => (
                                            <span key={identity.id}>
                                                {index > 0 ? ", " : ""}
                                                {identity.provider}:
                                                {identity.login ??
                                                    identity.email ??
                                                    "?"}
                                                {principal.identities.length >
                                                1 ? (
                                                    <button
                                                        className="btn btn-icon btn-sm btn-remove"
                                                        title="Unlink this identity; the principal keeps signing in with the others"
                                                        onClick={() =>
                                                            act(
                                                                adminUnlinkIdentity(
                                                                    identity.id,
                                                                ),
                                                            )
                                                        }
                                                    >
                                                        ×
                                                    </button>
                                                ) : null}
                                            </span>
                                        ),
                                    )}
                                    {principal.groups.length > 0
                                        ? ` · ${principal.groups.join(", ")}`
                                        : ""}
                                </span>
                            </span>
                            <span className="row-side">
                                {principal.status === "disabled" ? (
                                    <span className="pill pill-err">
                                        disabled
                                    </span>
                                ) : null}
                                <button
                                    className="btn btn-ghost btn-sm"
                                    onClick={() =>
                                        act(
                                            adminSetPrincipalStatus(
                                                principal.id,
                                                principal.status === "active"
                                                    ? "disabled"
                                                    : "active",
                                            ),
                                        )
                                    }
                                >
                                    {principal.status === "active"
                                        ? "Disable"
                                        : "Enable"}
                                </button>
                            </span>
                        </div>
                    ))}
                </div>
            </div>

            <div className="card">
                <h2>Groups</h2>
                <p className="hint">
                    Surface approval roles (role:&lt;name&gt;) name these
                    groups.
                </p>
                {groups.map((group) => (
                    <div className="row row-static" key={group.id}>
                        <span className="row-text">
                            <span className="row-title mono">{group.name}</span>
                            <span className="row-sub">
                                {group.members.length === 0
                                    ? "no members"
                                    : group.members.map(nameOf).join(", ")}
                            </span>
                        </span>
                        <span className="row-side">
                            <MemberEditor
                                group={group}
                                principals={principals}
                                onChange={(principalId, remove) =>
                                    act(
                                        adminGroupMember(
                                            group.id,
                                            principalId,
                                            remove,
                                        ),
                                    )
                                }
                            />
                            <GroupNameEditor
                                group={group}
                                onSave={(name) =>
                                    act(adminUpdateGroup(group.id, { name }))
                                }
                            />
                            <button
                                className="btn btn-ghost btn-sm"
                                title="Refused while grants reference the group"
                                onClick={() => act(adminDeleteGroup(group.id))}
                            >
                                Delete
                            </button>
                        </span>
                    </div>
                ))}
                <NewGroupForm
                    onCreate={(name) => act(adminCreateGroup(name))}
                />
            </div>

            <div className="card">
                <h2>Grants</h2>
                <div className="row-list">
                    {grants.map((grant) => (
                        <div className="row row-static" key={grant.id}>
                            <span className="row-text">
                                <span className="row-title mono">
                                    {grant.action} on {grant.resource}
                                </span>
                                <span className="row-sub">
                                    {grant.granteeKind === "group"
                                        ? `group ${groups.find((g) => g.id === grant.granteeId)?.name ?? grant.granteeId}`
                                        : nameOf(grant.granteeId)}
                                </span>
                            </span>
                            <span className="row-side">
                                <button
                                    className="btn btn-ghost btn-sm"
                                    onClick={() =>
                                        act(adminRevokeGrant(grant.id))
                                    }
                                >
                                    Revoke
                                </button>
                            </span>
                        </div>
                    ))}
                </div>
                <NewGrantForm
                    principals={principals}
                    groups={groups}
                    trees={(me.capabilities?.sourceTrees ?? []).map(
                        (tree) => tree.id,
                    )}
                    onCreate={(input) => act(adminCreateGrant(input))}
                />
            </div>

            <div className="card">
                <h2>Invitations</h2>
                {inviteLink !== null ? (
                    <div className="banner banner-info">
                        Share this link once; it is not stored:{" "}
                        <span className="mono">{inviteLink}</span>
                    </div>
                ) : null}
                <div className="row-list">
                    {invitations.map((invitation) => (
                        <div className="row row-static" key={invitation.id}>
                            <span className="row-text">
                                <span className="row-title">
                                    {invitation.email}
                                </span>
                                <span className="row-sub">
                                    {invitation.redeemedBy !== null
                                        ? `redeemed by ${nameOf(invitation.redeemedBy)}`
                                        : `open until ${invitation.expiresAt.slice(0, 10)}`}
                                </span>
                            </span>
                            {invitation.redeemedBy === null ? (
                                <span className="row-side">
                                    <button
                                        className="btn btn-ghost btn-sm"
                                        onClick={() =>
                                            act(
                                                adminRevokeInvitation(
                                                    invitation.id,
                                                ),
                                            )
                                        }
                                    >
                                        Revoke
                                    </button>
                                </span>
                            ) : null}
                        </div>
                    ))}
                </div>
                <NewInvitationForm
                    groups={groups}
                    trees={(me.capabilities?.sourceTrees ?? []).map(
                        (tree) => tree.id,
                    )}
                    onCreate={(input) => {
                        adminCreateInvitation(input).then(
                            (response) => {
                                setInviteLink(response.link);
                                refresh();
                            },
                            (failure: Error) => setError(failure.message),
                        );
                    }}
                />
            </div>
        </div>
    );
}

// The default branch is the one updatable fact; owner and name are the
// tree's identity, so a renamed repository is a new registration.
function BranchEditor({
    tree,
    onSave,
}: {
    tree: AdminSourceTree;
    onSave: (branch: string) => void;
}) {
    const [editing, setEditing] = useState(false);
    const [branch, setBranch] = useState(tree.defaultBranch ?? "");
    if (!editing) {
        return (
            <button
                className="btn btn-ghost btn-sm"
                onClick={() => {
                    setBranch(tree.defaultBranch ?? "");
                    setEditing(true);
                }}
            >
                Edit branch
            </button>
        );
    }
    return (
        <span className="inline-form">
            <input
                autoFocus
                className="input mono"
                placeholder="main"
                value={branch}
                onChange={(event) => setBranch(event.target.value)}
            />
            <button
                className="btn btn-primary btn-sm"
                disabled={branch.trim() === ""}
                onClick={() => {
                    setEditing(false);
                    onSave(branch.trim());
                }}
            >
                Save
            </button>
            <button
                className="btn btn-ghost btn-sm"
                onClick={() => setEditing(false)}
            >
                Cancel
            </button>
        </span>
    );
}

function RegisterTreeForm({
    onRegister,
}: {
    onRegister: (input: {
        owner: string;
        name: string;
        defaultBranch?: string;
    }) => void;
}) {
    const [owner, setOwner] = useState("");
    const [name, setName] = useState("");
    const [branch, setBranch] = useState("");
    return (
        <div className="inline-form">
            <input
                className="input mono"
                placeholder="owner"
                value={owner}
                onChange={(event) => setOwner(event.target.value)}
            />
            <input
                className="input mono"
                placeholder="repository"
                value={name}
                onChange={(event) => setName(event.target.value)}
            />
            <input
                className="input mono"
                placeholder="branch (from GitHub if blank)"
                value={branch}
                onChange={(event) => setBranch(event.target.value)}
            />
            <button
                className="btn btn-secondary btn-sm"
                disabled={owner.trim() === "" || name.trim() === ""}
                onClick={() => {
                    onRegister({
                        owner: owner.trim(),
                        name: name.trim(),
                        ...(branch.trim() === ""
                            ? {}
                            : { defaultBranch: branch.trim() }),
                    });
                    setOwner("");
                    setName("");
                    setBranch("");
                }}
            >
                Register
            </button>
        </div>
    );
}

// Group names are labels, not addresses, so rename is safe; surface
// approval roles reference the name, which is why it stays snake_case.
function GroupNameEditor({
    group,
    onSave,
}: {
    group: AdminGroup;
    onSave: (name: string) => void;
}) {
    const [editing, setEditing] = useState(false);
    const [name, setName] = useState(group.name);
    if (!editing) {
        return (
            <button
                className="btn btn-ghost btn-sm"
                onClick={() => {
                    setName(group.name);
                    setEditing(true);
                }}
            >
                Rename
            </button>
        );
    }
    return (
        <span className="inline-form">
            <input
                autoFocus
                className="input mono"
                value={name}
                onChange={(event) => setName(event.target.value)}
            />
            <button
                className="btn btn-primary btn-sm"
                disabled={name.trim() === ""}
                onClick={() => {
                    setEditing(false);
                    onSave(name.trim());
                }}
            >
                Save
            </button>
            <button
                className="btn btn-ghost btn-sm"
                onClick={() => setEditing(false)}
            >
                Cancel
            </button>
        </span>
    );
}

function MemberEditor({
    group,
    principals,
    onChange,
}: {
    group: AdminGroup;
    principals: AdminPrincipal[];
    onChange: (principalId: string, remove: boolean) => void;
}) {
    return (
        <select
            className="input"
            value=""
            onChange={(event) => {
                if (event.target.value !== "") {
                    onChange(
                        event.target.value,
                        group.members.includes(event.target.value),
                    );
                }
            }}
        >
            <option value="">add / remove…</option>
            {principals.map((principal) => (
                <option key={principal.id} value={principal.id}>
                    {group.members.includes(principal.id) ? "− " : "+ "}
                    {principal.displayName}
                </option>
            ))}
        </select>
    );
}

function NewGroupForm({ onCreate }: { onCreate: (name: string) => void }) {
    const [name, setName] = useState("");
    return (
        <div className="inline-form">
            <input
                className="input mono"
                placeholder="group_name"
                value={name}
                onChange={(event) => setName(event.target.value)}
            />
            <button
                className="btn btn-secondary btn-sm"
                disabled={name.trim() === ""}
                onClick={() => {
                    onCreate(name.trim());
                    setName("");
                }}
            >
                New group
            </button>
        </div>
    );
}

function NewGrantForm({
    principals,
    groups,
    trees,
    onCreate,
}: {
    principals: AdminPrincipal[];
    groups: AdminGroup[];
    trees: string[];
    onCreate: (input: {
        granteeKind: "principal" | "group";
        granteeId: string;
        action: string;
        resource: string;
    }) => void;
}) {
    const [grantee, setGrantee] = useState("");
    const [action, setAction] = useState("view");
    const [resource, setResource] = useState("deployment");
    return (
        <div className="inline-form">
            <select
                className="input"
                value={grantee}
                onChange={(event) => setGrantee(event.target.value)}
            >
                <option value="">grantee…</option>
                {principals.map((principal) => (
                    <option
                        key={principal.id}
                        value={`principal:${principal.id}`}
                    >
                        {principal.displayName}
                    </option>
                ))}
                {groups.map((group) => (
                    <option key={group.id} value={`group:${group.id}`}>
                        group {group.name}
                    </option>
                ))}
            </select>
            <select
                className="input"
                value={action}
                onChange={(event) => setAction(event.target.value)}
            >
                {["view", "propose", "approve", "administer"].map((verb) => (
                    <option key={verb} value={verb}>
                        {verb}
                    </option>
                ))}
            </select>
            <select
                className="input"
                value={resource}
                onChange={(event) => setResource(event.target.value)}
            >
                <option value="deployment">deployment</option>
                {trees.map((tree) => (
                    <option key={tree} value={`source-tree:${tree}`}>
                        source-tree:{tree}
                    </option>
                ))}
            </select>
            <button
                className="btn btn-secondary btn-sm"
                disabled={grantee === ""}
                onClick={() => {
                    const [kind, id] = grantee.split(":", 2) as [
                        "principal" | "group",
                        string,
                    ];
                    onCreate({
                        granteeKind: kind,
                        granteeId: id,
                        action,
                        resource,
                    });
                }}
            >
                Grant
            </button>
        </div>
    );
}

function NewInvitationForm({
    groups,
    trees,
    onCreate,
}: {
    groups: AdminGroup[];
    trees: string[];
    onCreate: (input: {
        email: string;
        initialGroups?: string[];
        initialGrants?: { action: string; resource: string }[];
    }) => void;
}) {
    const [email, setEmail] = useState("");
    const [group, setGroup] = useState("");
    const [proposeOn, setProposeOn] = useState("");
    return (
        <div className="inline-form">
            <input
                className="input"
                placeholder="person@company.com"
                value={email}
                onChange={(event) => setEmail(event.target.value)}
            />
            <select
                className="input"
                value={group}
                onChange={(event) => setGroup(event.target.value)}
            >
                <option value="">no group</option>
                {groups.map((entry) => (
                    <option key={entry.id} value={entry.id}>
                        {entry.name}
                    </option>
                ))}
            </select>
            <select
                className="input"
                value={proposeOn}
                onChange={(event) => setProposeOn(event.target.value)}
            >
                <option value="">no grant</option>
                {trees.map((tree) => (
                    <option key={tree} value={`source-tree:${tree}`}>
                        propose on {tree}
                    </option>
                ))}
            </select>
            <button
                className="btn btn-primary btn-sm"
                disabled={!email.includes("@")}
                onClick={() => {
                    onCreate({
                        email: email.trim(),
                        ...(group === "" ? {} : { initialGroups: [group] }),
                        ...(proposeOn === ""
                            ? {}
                            : {
                                  initialGrants: [
                                      {
                                          action: "propose",
                                          resource: proposeOn,
                                      },
                                  ],
                              }),
                    });
                    setEmail("");
                }}
            >
                Invite
            </button>
        </div>
    );
}
