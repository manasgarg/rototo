// What every route module gets from app.ts: the services plus the two
// per-request resolutions (who is asking, and which credential acts).

import type { ActingCredential } from "./app-credential.ts";
import type { ChangeSets } from "./change-sets.ts";
import type { ServerConfig } from "./config.ts";
import type { DecisionPoint, Subject } from "./decide.ts";
import type { GitOps } from "./git.ts";
import type { LspBridge } from "./lsp-bridge.ts";
import type { PackageStager } from "./packages.ts";
import type { Reconciler } from "./reconciler.ts";
import type { SourceTreeRow, Store } from "./store.ts";

export type ConsoleContext = {
    config: ServerConfig;
    store: Store;
    decision: DecisionPoint;
    git: GitOps;
    stager: PackageStager;
    changeSets: ChangeSets;
    reconciler: Reconciler;
    lsp: LspBridge;
    subjectFor(cookieHeader: string | undefined): Subject | null;
    // The stable actor id for rows and diaries: the principal id, or
    // "local" for local mode's implicit principal.
    subjectId(subject: Subject): string;
    // The subject's own credential only: the stored GitHub token, or the
    // ambient token in local mode. Null when the subject has none.
    actingToken(subject: Subject): Promise<string | null>;
    // The acting credential for operations on a tree (design/console-git-ops.md
    // rule 5): the user's own token when they have one (GitHub enforces),
    // else the App's installation token (the console enforces via decide),
    // else null — read-only.
    actingCredential(
        subject: Subject,
        tree: SourceTreeRow,
    ): Promise<ActingCredential | null>;
};
