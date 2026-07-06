// What every route module gets from app.ts: the services plus the two
// per-request resolutions (who is asking, and which credential acts).

import type { ChangeSets } from "./change-sets.ts";
import type { ServerConfig } from "./config.ts";
import type { DecisionPoint, Subject } from "./decide.ts";
import type { GitOps } from "./git.ts";
import type { PackageStager } from "./packages.ts";
import type { Reconciler } from "./reconciler.ts";
import type { Store } from "./store.ts";

export type ConsoleContext = {
    config: ServerConfig;
    store: Store;
    decision: DecisionPoint;
    git: GitOps;
    stager: PackageStager;
    changeSets: ChangeSets;
    reconciler: Reconciler;
    subjectFor(cookieHeader: string | undefined): Subject | null;
    // The stable actor id for rows and diaries: the principal id, or
    // "local" for local mode's implicit principal.
    subjectId(subject: Subject): string;
    // The acting credential (design/console-git-ops.md rule 5). C2 is user
    // tokens only: the subject's own GitHub credential, or the ambient
    // token in local mode. Null when the subject has none.
    actingToken(subject: Subject): Promise<string | null>;
};
