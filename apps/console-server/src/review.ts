// The three-delta review (design/console-system-view.md, "Editing: the
// three facets, differenced"): what changed (the semantic diff), what it
// does (resolution impact, with its denominator always stated), and whether
// it is healthy (the lint delta). An approver reading those three panels
// knows those three things in that order. This module composes the deltas
// from bindings the lower layers already own; nothing here is new machinery.

import { branchFor, repoId } from "./change-sets.ts";
import { ApiError } from "./errors.ts";
import type { GitOps } from "./git.ts";
import {
    native,
    type ContextImpactJson,
    type JsonObject,
    type JsonValue,
    type LabeledContextJson,
} from "./native.ts";
import type { PackageStager } from "./packages.ts";
import type { ChangeSetRow, SourceTreeRow } from "./store.ts";
import { bindingPaths, readSurfaces, type ModelView } from "./surfaces.ts";

export type LintDelta = {
    introduced: JsonValue[];
    resolved: JsonValue[];
};

// The impact panel's honesty: how many contexts it ran, where they came
// from, and how well the touched variables' rules are exercised by saved
// samples. "No outcome changes" with a thin denominator is blindness, not
// safety, and the panel must be able to say which.
export type ReviewDenominator = {
    samples: number;
    synthesized: number;
    variables: {
        id: string;
        sampleCount: number;
        defaultCovered: boolean;
        rules: { index: number; covered: boolean }[];
    }[];
};

export type ReviewContext = {
    label: string;
    source: "sample" | "synthetic";
    context: JsonObject;
};

export type PackageReview = {
    path: string;
    changes: JsonValue[];
    contexts: ReviewContext[];
    contextImpacts: ContextImpactJson[];
    impactError: string | null;
    denominator: ReviewDenominator;
    lint: LintDelta;
    // Surfaces whose bound entities this change set touches, with the
    // approval requirement each declares. In this tranche the requirement
    // renders and informs; GitHub remains the authority (Backend A), and
    // role-based enforcement waits for C5.
    surfaces: {
        id: string;
        title: string;
        approval: string | null;
        caution: string | null;
    }[];
};

// A touched package outside the reviewer's view: existence is disclosed,
// content is not (design/console-identity-authz.md 6.2). The count keeps
// the reviewer honest about what they cannot judge.
export type RedactedPackageReview = {
    path: string;
    redacted: true;
    files: number;
};

export type ChangeSetReview = {
    basePin: string;
    headPin: string;
    files: string[];
    packages: (PackageReview | RedactedPackageReview)[];
};

type ReviewDeps = {
    git: GitOps;
    stager: PackageStager;
};

export async function buildReview(
    deps: ReviewDeps,
    input: {
        tree: SourceTreeRow;
        changeSet: ChangeSetRow;
        token: string;
        // Whether the reviewer may view a touched package; absent means
        // everything is visible (the pre-grants callers). A package this
        // refuses renders redacted: counted, never shown.
        canView?: (packagePath: string) => Promise<boolean>;
    },
): Promise<ChangeSetReview> {
    const { git, stager } = deps;
    const { tree, changeSet, token } = input;
    const repo = repoId(tree);
    const branch = branchFor(changeSet.id);
    const headPin = await git.getRef(token, repo, branch);
    if (headPin === null) {
        throw new ApiError(
            409,
            `the branch for change set ${changeSet.id} is gone; there is nothing to review`,
        );
    }
    // Review against the merge base, exactly like the pull request will
    // merge: base drift is not part of this change.
    const comparison = await git.compare(
        token,
        repo,
        changeSet.baseRef,
        branch,
    );
    const basePin = comparison.mergeBase;
    const [beforeTree, afterTree] = await Promise.all([
        stager.stageTree(tree, basePin, token),
        stager.stageTree(tree, headPin, token),
    ]);
    const packages = await native.discoverPackages(afterTree);

    const touched = touchedPackages(comparison.files, packages);
    const reviews: (PackageReview | RedactedPackageReview)[] = [];
    for (const [packagePath, files] of touched) {
        if (input.canView !== undefined && !(await input.canView(packagePath))) {
            reviews.push({
                path: packagePath,
                redacted: true,
                files: files.length,
            });
            continue;
        }
        reviews.push(
            await reviewPackage(
                stager.packageRoot(beforeTree, packagePath),
                stager.packageRoot(afterTree, packagePath),
                packagePath,
                files,
            ),
        );
    }
    return {
        basePin,
        headPin,
        files: comparison.files,
        packages: reviews,
    };
}

// Repo-relative changed files, grouped by the package that contains them
// (longest package path wins; files outside every package are dropped).
// Values are package-relative paths.
function touchedPackages(
    files: string[],
    packages: string[],
): Map<string, string[]> {
    const byLength = [...packages].sort(
        (left, right) => right.length - left.length,
    );
    const touched = new Map<string, string[]>();
    for (const file of files) {
        for (const packagePath of byLength) {
            const prefix = packagePath === "." ? "" : `${packagePath}/`;
            if (file.startsWith(prefix)) {
                const relative = file.slice(prefix.length);
                const existing = touched.get(packagePath) ?? [];
                existing.push(relative);
                touched.set(packagePath, existing);
                break;
            }
        }
    }
    return touched;
}

async function reviewPackage(
    beforeRoot: string,
    afterRoot: string,
    packagePath: string,
    files: string[],
): Promise<PackageReview> {
    const model = (await native.semanticModel(afterRoot)) as ModelView & {
        evaluationContextSamples?: {
            evaluationContext: string;
            key: string;
            value?: unknown;
        }[];
    };

    const touchedVariables = touchedVariableIds(files, model);

    // The contexts the impact runs under: every saved sample, then
    // synthesized boundary contexts for the touched variables. The
    // conditions themselves say which contexts matter; the fixtures
    // machinery reads them.
    const contexts: ReviewContext[] = [];
    const labels = new Set<string>();
    for (const sample of model.evaluationContextSamples ?? []) {
        if (
            typeof sample.value !== "object" ||
            sample.value === null ||
            Array.isArray(sample.value)
        ) {
            continue;
        }
        const label = `sample:${sample.evaluationContext}/${sample.key}`;
        if (!labels.has(label)) {
            labels.add(label);
            contexts.push({
                label,
                source: "sample",
                context: sample.value as JsonObject,
            });
        }
    }
    let synthesizedCount = 0;
    if (touchedVariables.length > 0) {
        try {
            const fixtures = await native.resolveFixtures(
                afterRoot,
                touchedVariables,
            );
            for (const fixture of fixtures) {
                const label = `synthetic:${fixture.target.id}/${fixture.caseId}`;
                if (!labels.has(label)) {
                    labels.add(label);
                    synthesizedCount += 1;
                    contexts.push({
                        label,
                        source: "synthetic",
                        context: fixture.context,
                    });
                }
            }
        } catch {
            // A package that cannot compile synthesizes nothing; the lint
            // delta below is already telling that story.
        }
    }

    const labeled: LabeledContextJson[] = contexts.map((context) => ({
        label: context.label,
        context: context.context,
    }));
    const diff = await native.diffPackagesWithContexts(
        beforeRoot,
        afterRoot,
        labeled,
    );

    const denominator: ReviewDenominator = {
        samples: contexts.length - synthesizedCount,
        synthesized: synthesizedCount,
        variables: await coverageFor(afterRoot, touchedVariables, model),
    };

    const lint = await lintDelta(beforeRoot, afterRoot);

    const changedRepoPaths = new Set(files);
    const surfaces = readSurfaces(model)
        .filter((surface) =>
            bindingPaths(surface, model).some((bindingPath) =>
                [...changedRepoPaths].some(
                    (file) =>
                        file === bindingPath ||
                        file.startsWith(`${bindingPath}/`),
                ),
            ),
        )
        .map((surface) => ({
            id: surface.id,
            title: surface.title,
            approval: surface.approval,
            caution: surface.caution,
        }));

    return {
        path: packagePath,
        changes: diff.changes,
        contexts,
        contextImpacts: diff.context_impacts,
        impactError: diff.impact_error ?? null,
        denominator,
        lint,
        surfaces,
    };
}

// The variables this change plausibly moves: variable files edited
// directly, plus variables typed over a catalog whose data changed. The
// list feeds context synthesis and the coverage denominator; the impact
// itself always runs whole-package, because dependencies propagate.
function touchedVariableIds(files: string[], model: ModelView): string[] {
    const ids = new Set<string>();
    const known = new Set((model.variables ?? []).map((v) => v.id));
    for (const file of files) {
        const variableMatch = file.match(/^variables\/(.+)\.toml$/);
        if (variableMatch !== null && known.has(variableMatch[1] as string)) {
            ids.add(variableMatch[1] as string);
            continue;
        }
        for (const catalog of model.catalogs ?? []) {
            if (!file.startsWith(`data/catalogs/${catalog.id}/`)) {
                continue;
            }
            for (const variable of model.variables ?? []) {
                const declared = variable.declaration.value ?? "";
                if (
                    variable.declaration.kind === "catalog"
                        ? declared === catalog.id
                        : declared.includes(`catalog=${catalog.id}`)
                ) {
                    ids.add(variable.id);
                }
            }
        }
    }
    return [...ids].sort();
}

async function coverageFor(
    afterRoot: string,
    touchedVariables: string[],
    model: ModelView,
): Promise<ReviewDenominator["variables"]> {
    const known = new Set((model.variables ?? []).map((v) => v.id));
    const present = touchedVariables.filter((id) => known.has(id));
    if (present.length === 0) {
        return [];
    }
    const report = (await native.inspectReport(afterRoot, {
        variables: present,
    })) as {
        variables?: {
            id: string;
            sample_coverage?: {
                sample_count: number;
                default_covered: boolean;
                rules: { index: number; covered: boolean }[];
            };
        }[];
    };
    return (report.variables ?? []).map((variable) => ({
        id: variable.id,
        sampleCount: variable.sample_coverage?.sample_count ?? 0,
        defaultCovered: variable.sample_coverage?.default_covered ?? false,
        rules: variable.sample_coverage?.rules ?? [],
    }));
}

async function lintDelta(
    beforeRoot: string,
    afterRoot: string,
): Promise<LintDelta> {
    const [before, after] = await Promise.all([
        native.lintPackage(beforeRoot).catch(() => null),
        native.lintPackage(afterRoot),
    ]);
    const beforeKeys = new Map<string, JsonValue>();
    for (const diagnostic of before?.diagnostics ?? []) {
        beforeKeys.set(diagnosticKey(diagnostic), diagnostic);
    }
    const afterKeys = new Map<string, JsonValue>();
    for (const diagnostic of after.diagnostics) {
        afterKeys.set(diagnosticKey(diagnostic), diagnostic);
    }
    const introduced = [...afterKeys.entries()]
        .filter(([key]) => !beforeKeys.has(key))
        .map(([, diagnostic]) => diagnostic);
    const resolved = [...beforeKeys.entries()]
        .filter(([key]) => !afterKeys.has(key))
        .map(([, diagnostic]) => diagnostic);
    return { introduced, resolved };
}

// Identity for delta purposes: the rule, where it fired, and what it said.
// Line drift within a file does not count as a new diagnostic.
function diagnosticKey(diagnostic: JsonValue): string {
    const value = diagnostic as {
        rule?: unknown;
        severity?: unknown;
        message?: unknown;
        location?: { path?: unknown };
    };
    return JSON.stringify([
        value.rule ?? null,
        value.severity ?? null,
        value.location?.path ?? null,
        value.message ?? null,
    ]);
}
