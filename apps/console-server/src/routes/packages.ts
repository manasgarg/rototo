// Reading packages out of a source tree (design/console-git-ops.md "Reading
// a package"): resolve the ref to a pin first; everything below the resolve
// works on the pin, so the staged reads can never be wrong, only briefly
// old.

import { readFile, readdir } from "node:fs/promises";
import path from "node:path";

import { Hono } from "hono";
import type { Context } from "hono";

import type { ConsoleContext } from "../context.ts";
import type { Subject } from "../decide.ts";
import { ApiError } from "../errors.ts";
import type { CommitRecord } from "../git.ts";
import { branchFor, repoId } from "../change-sets.ts";
import { native } from "../native.ts";
import { containedPath, isPin, resolveExtend } from "../packages.ts";
import type { SourceTreeRow } from "../store.ts";
import {
    audienceAllows,
    bindingPaths,
    boundVariables,
    CONSOLE_LINT_PATH,
    CONSOLE_LINT_SCRIPT,
    lintScriptVendored,
    readSurfaces,
    schemaFreshness,
    suggestSurfaces,
    surfaceItems,
    type ModelView,
} from "../surfaces.ts";

type TreeAccess = {
    tree: SourceTreeRow;
    subject: Subject;
    token: string;
};

export function packageRoutes(ctx: ConsoleContext): Hono {
    const app = new Hono();

    app.onError((error, c) => {
        if (error instanceof ApiError) {
            return c.json(
                {
                    error: {
                        message: error.message,
                        ...(error.conflictPaths === undefined
                            ? {}
                            : { paths: error.conflictPaths }),
                    },
                },
                error.status as 400,
            );
        }
        return c.json({ error: { message: error.message } }, 500);
    });

    // Shared lookup: the tree must exist, the caller must be signed in and
    // allowed to view it, and reading needs a credential to fetch with.
    const access = async (c: Context): Promise<TreeAccess> => {
        const tree = ctx.store.getSourceTree(c.req.param("tree") ?? "");
        if (tree === null) {
            throw new ApiError(404, "no such source tree");
        }
        const subject = ctx.subjectFor(c.req.header("cookie"));
        if (subject === null) {
            throw new ApiError(401, "sign in first");
        }
        const verdict = await ctx.decision.decide(subject, "view", {
            kind: "source-tree",
            sourceTree: tree.id,
        });
        if (!verdict.allow) {
            throw new ApiError(403, verdict.reason);
        }
        const credential = await ctx.actingCredential(subject, tree);
        if (credential === null) {
            throw new ApiError(
                403,
                "no GitHub credential is available to read this tree with",
            );
        }
        return { tree, subject, token: credential.token };
    };

    const stagedPin = (c: Context): string => {
        const pin = c.req.query("pin") ?? "";
        if (!isPin(pin)) {
            throw new ApiError(
                400,
                "pin must be a full commit SHA; resolve refs through the packages listing first",
            );
        }
        return pin;
    };

    const packagePathOf = (c: Context): string => {
        const value = c.req.query("path");
        if (value === undefined || value === "") {
            throw new ApiError(400, "path is required");
        }
        return value;
    };

    // The packages in a tree at a ref, with the pin the ref resolved to.
    app.get("/source-trees/:tree/packages", async (c) => {
        const { tree, token } = await access(c);
        const ref = c.req.query("ref") ?? tree.defaultBranch ?? "main";
        const pin = isPin(ref)
            ? ref
            : await ctx.git.getRef(token, repoId(tree), ref);
        if (pin === null) {
            throw new ApiError(404, `ref ${ref} does not exist`);
        }
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packages = await native.discoverPackages(treeRoot);
        return c.json({
            ref,
            pin,
            packages: packages.map((packagePath) => ({ path: packagePath })),
        });
    });

    // One package at a pin: the semantic model and the lint report, the two
    // read surfaces every workbench view is built from.
    app.get("/source-trees/:tree/package", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const packagePath = packagePathOf(c);
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
        const [model, lint] = await Promise.all([
            native.semanticModel(packageRoot),
            native.lintPackage(packageRoot),
        ]);
        return c.json({ pin, path: packagePath, model, lint });
    });

    // The package's files, for the raw-text editor's picker.
    app.get("/source-trees/:tree/package-files", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const packagePath = packagePathOf(c);
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
        const entries = await readdir(packageRoot, {
            withFileTypes: true,
            recursive: true,
        });
        const files = entries
            .filter(
                (entry) =>
                    entry.isFile() &&
                    !entry.name.startsWith(".") &&
                    !path
                        .relative(packageRoot, entry.parentPath)
                        .split(path.sep)
                        .some((part) => part.startsWith(".")),
            )
            .map((entry) =>
                path
                    .relative(
                        packageRoot,
                        path.join(entry.parentPath, entry.name),
                    )
                    .split(path.sep)
                    .join("/"),
            )
            .sort();
        return c.json({ pin, path: packagePath, files });
    });

    // The contexts a preview can run under: the package's saved samples
    // (with their JSON) and synthesized boundary contexts from the fixtures
    // machinery. The picker is first-class; this is its inventory.
    app.get("/source-trees/:tree/contexts", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const packagePath = packagePathOf(c);
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
        const model = (await native.semanticModel(packageRoot)) as {
            evaluationContextSamples?: {
                evaluationContext: string;
                key: string;
                value?: unknown;
            }[];
        };
        const samples = (model.evaluationContextSamples ?? []).map(
            (sample) => ({
                evaluationContext: sample.evaluationContext,
                key: sample.key,
                context: sample.value ?? null,
            }),
        );
        let synthesized: unknown[] = [];
        try {
            synthesized = await native.resolveFixtures(packageRoot);
        } catch {
            // A package that cannot compile has no fixtures; the samples
            // (and the lint report) still tell the story.
        }
        return c.json({ pin, path: packagePath, samples, synthesized });
    });

    // The lenient batch preview: every variable resolved and traced under
    // one context. Powers the rule-walk panel and the lit-up graph; a
    // variable the context cannot satisfy carries its error honestly.
    app.post("/source-trees/:tree/preview", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const packagePath = packagePathOf(c);
        const body = (await c.req.json().catch(() => null)) as {
            context?: Record<string, unknown>;
        } | null;
        if (body === null || typeof body.context !== "object") {
            throw new ApiError(400, "expected { context: <object> }");
        }
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
        let outcomes: unknown;
        try {
            outcomes = await native.traceResolutionOutcomes(
                packageRoot,
                (body.context ?? {}) as Record<string, never>,
            );
        } catch (error) {
            // The whole batch refuses only when the context itself is
            // invalid (schema validation) or the package cannot compile.
            throw new ApiError(400, (error as Error).message);
        }
        return c.json({ pin, path: packagePath, outcomes });
    });

    // One variable can resolve without a caller context even when the package
    // declares required context schemas for other variables. The semantic
    // index decides whether that is valid; this route lets the entity screen
    // ask that narrower question instead of running the whole package batch.
    app.post("/source-trees/:tree/variable-preview", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const packagePath = packagePathOf(c);
        const body = (await c.req.json().catch(() => null)) as {
            variable?: string;
            context?: Record<string, unknown>;
        } | null;
        if (
            body === null ||
            typeof body.variable !== "string" ||
            typeof body.context !== "object"
        ) {
            throw new ApiError(
                400,
                "expected { variable: <id>, context: <object> }",
            );
        }
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
        try {
            const trace = await native.traceResolution(
                packageRoot,
                body.variable,
                (body.context ?? {}) as Record<string, never>,
            );
            return c.json({
                pin,
                path: packagePath,
                outcome: { id: body.variable, trace },
            });
        } catch (error) {
            throw new ApiError(400, (error as Error).message);
        }
    });

    // Behavior scheduled to change on its own: env.now boundaries that have
    // not passed yet, from the core's expression analysis.
    app.get("/source-trees/:tree/upcoming", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const packagePath = packagePathOf(c);
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
        const now = new Date().toISOString();
        const changes = await native.upcomingChanges(packageRoot, now);
        return c.json({ pin, path: packagePath, now, changes });
    });

    // The inspect report: per-entity diagnostics, dependencies, consumers,
    // and sample coverage — the validity facet's denominator lives here.
    app.get("/source-trees/:tree/inspect", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const packagePath = packagePathOf(c);
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
        const report = await native.inspectReport(packageRoot, {
            variables: "all",
            catalogs: "all",
        });
        return c.json({ pin, path: packagePath, report });
    });

    // History for the time views: commits on a ref, newest first, scoped to
    // the package's directory. `until` answers "what was this value on
    // March 3rd": the first commit at or before the instant is the pin to
    // read the package at.
    app.get("/source-trees/:tree/history", async (c) => {
        const { tree, token } = await access(c);
        const ref = c.req.query("ref") ?? tree.defaultBranch ?? "main";
        const packagePath = c.req.query("path");
        const until = c.req.query("until");
        const commits = await ctx.git.listCommits(token, repoId(tree), {
            ref,
            ...(packagePath === undefined ||
            packagePath === "" ||
            packagePath === "."
                ? {}
                : { path: packagePath }),
            ...(until === undefined ? {} : { until }),
        });
        return c.json({ ref, commits });
    });

    // The composition tree (ring 2): every package in the tree, with the
    // extends edges their manifests declare. No new declaration exists;
    // discovery plus the semantic model already know the shape.
    app.get("/source-trees/:tree/composition", async (c) => {
        const { tree, token } = await access(c);
        const ref = c.req.query("ref") ?? tree.defaultBranch ?? "main";
        const pin = isPin(ref)
            ? ref
            : await ctx.git.getRef(token, repoId(tree), ref);
        if (pin === null) {
            throw new ApiError(404, `ref ${ref} does not exist`);
        }
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packages = await native.discoverPackages(treeRoot);
        const known = new Set(packages);
        const nodes: { path: string }[] = [];
        const edges: {
            from: string;
            source: string;
            to: string | null;
        }[] = [];
        for (const packagePath of packages) {
            nodes.push({ path: packagePath });
            const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
            const model = (await native.semanticModel(packageRoot)) as {
                extends?: { source: string }[];
            };
            for (const extend of model.extends ?? []) {
                edges.push({
                    from: packagePath,
                    source: extend.source,
                    to: resolveExtend(packagePath, extend.source, known),
                });
            }
        }
        return c.json({ ref, pin, nodes, edges });
    });

    // Overlays of a base: every package in the tree whose extends chain
    // reaches it. The composition tree already knows the edges; this walks
    // them upward.
    const overlaysOf = async (
        treeRoot: string,
        packages: string[],
        basePath: string,
    ): Promise<string[]> => {
        const known = new Set(packages);
        const bases = new Map<string, Set<string>>();
        for (const packagePath of packages) {
            const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
            const model = (await native
                .semanticModel(packageRoot)
                .catch(() => null)) as {
                extends?: { source: string }[];
            } | null;
            const direct = new Set<string>();
            for (const extend of model?.extends ?? []) {
                const to = resolveExtend(packagePath, extend.source, known);
                if (to !== null) {
                    direct.add(to);
                }
            }
            bases.set(packagePath, direct);
        }
        const reaches = (from: string, seen: Set<string>): boolean => {
            if (seen.has(from)) {
                return false;
            }
            seen.add(from);
            for (const parent of bases.get(from) ?? []) {
                if (parent === basePath || reaches(parent, seen)) {
                    return true;
                }
            }
            return false;
        };
        return packages.filter(
            (packagePath) =>
                packagePath !== basePath &&
                reaches(packagePath, new Set<string>()),
        );
    };

    // Ring-2 validity: fleet health. Every overlay of the base, composed
    // and linted, aggregated — "3 of 12 tenant overlays fail lint against
    // this base" is what makes evolving a base under tenants safe. This is
    // lint the console already runs, fanned out per overlay and summarized.
    app.get("/source-trees/:tree/fleet", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const basePath = packagePathOf(c);
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packages = await native.discoverPackages(treeRoot);
        if (!packages.includes(basePath)) {
            throw new ApiError(404, `no such package: ${basePath}`);
        }
        const overlays = await overlaysOf(treeRoot, packages, basePath);
        const health = [];
        for (const overlayPath of overlays) {
            try {
                // The composed view: the overlay layered over everything it
                // extends, exactly what a load of that package would see.
                const overlayRoot = await ctx.stager.composedRoot(
                    treeRoot,
                    overlayPath,
                );
                const lint = await native.lintPackage(overlayRoot);
                const diagnostics = lint.diagnostics as {
                    severity?: string;
                }[];
                const errors = diagnostics.filter(
                    (diagnostic) => diagnostic.severity === "error",
                ).length;
                const warnings = diagnostics.filter(
                    (diagnostic) => diagnostic.severity === "warning",
                ).length;
                health.push({
                    path: overlayPath,
                    ok: errors === 0,
                    errors,
                    warnings,
                });
            } catch (error) {
                health.push({
                    path: overlayPath,
                    ok: false,
                    errors: 1,
                    warnings: 0,
                    failure: (error as Error).message,
                });
            }
        }
        return c.json({
            pin,
            path: basePath,
            overlays: health,
            failing: health.filter((entry) => !entry.ok).length,
        });
    });

    // Ring-2 execution: the same context resolved across the base and its
    // overlays, as a matrix — "log_level for this context: debug in dev,
    // info in staging, warn in prod". Lenient per package: a member whose
    // composition cannot resolve carries its error instead of a column of
    // silence.
    app.post("/source-trees/:tree/matrix", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const basePath = packagePathOf(c);
        const body = (await c.req.json().catch(() => null)) as {
            context?: Record<string, unknown>;
            variables?: string[];
        } | null;
        if (body === null || typeof body.context !== "object") {
            throw new ApiError(400, "expected { context: <object> }");
        }
        const wanted =
            Array.isArray(body.variables) && body.variables.length > 0
                ? new Set(body.variables)
                : null;
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packages = await native.discoverPackages(treeRoot);
        if (!packages.includes(basePath)) {
            throw new ApiError(404, `no such package: ${basePath}`);
        }
        const overlays = await overlaysOf(treeRoot, packages, basePath);
        const columns = [];
        for (const memberPath of [basePath, ...overlays]) {
            try {
                const memberRoot = await ctx.stager.composedRoot(
                    treeRoot,
                    memberPath,
                );
                const outcomes = (await native.traceResolutionOutcomes(
                    memberRoot,
                    (body.context ?? {}) as Record<string, never>,
                )) as {
                    id: string;
                    trace?: { resolution: { value: unknown } };
                    error?: string;
                }[];
                columns.push({
                    path: memberPath,
                    outcomes: outcomes
                        .filter(
                            (outcome) =>
                                wanted === null || wanted.has(outcome.id),
                        )
                        .map((outcome) => ({
                            id: outcome.id,
                            value: outcome.trace?.resolution.value ?? null,
                            error: outcome.error ?? null,
                        })),
                });
            } catch (error) {
                columns.push({
                    path: memberPath,
                    failure: (error as Error).message,
                    outcomes: [],
                });
            }
        }
        return c.json({ pin, path: basePath, columns });
    });

    // The package's surfaces: console/surfaces catalog entries validated on
    // load, audience-filtered, with cold-start suggestions when there are
    // none. A surface is ordinary catalog data; this route just knows how to
    // read it.
    app.get("/source-trees/:tree/surfaces", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const packagePath = packagePathOf(c);
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
        const model = (await native.semanticModel(packageRoot)) as ModelView;
        // Every session is internal until tenants arrive; the filter is the
        // mechanism, "internal" is today's only parameter.
        const surfaces = readSurfaces(model).filter((surface) =>
            audienceAllows(surface, "internal"),
        );
        const freshness = schemaFreshness(model);
        // The vendorable lint script: packages carrying it get the same
        // surface failures in CI. The content rides along only while it is
        // missing, so accepting the offer is one raw-file edit.
        const vendored = lintScriptVendored(model);
        return c.json({
            pin,
            path: packagePath,
            surfaces,
            diagnostics: freshness === null ? [] : [freshness],
            suggestions: surfaces.length === 0 ? suggestSurfaces(model) : [],
            lintScript: {
                path: CONSOLE_LINT_PATH,
                vendored,
                ...(vendored ? {} : { content: CONSOLE_LINT_SCRIPT }),
            },
        });
    });

    // One surface, rendered at floor fidelity, with its four read
    // affordances: items with inferred controls, upcoming changes on bound
    // variables, the bound files' history, and open change sets touching
    // the bindings.
    app.get("/source-trees/:tree/surface", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const packagePath = packagePathOf(c);
        const id = c.req.query("id");
        if (id === undefined || id === "") {
            throw new ApiError(400, "id is required");
        }
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
        const model = (await native.semanticModel(packageRoot)) as ModelView;
        const surface = readSurfaces(model).find(
            (candidate) => candidate.id === id,
        );
        if (surface === undefined) {
            throw new ApiError(404, `no such surface: ${id}`);
        }
        const items = surfaceItems(surface, model);

        const now = new Date().toISOString();
        const bound = boundVariables(surface);
        const upcoming = (
            await native.upcomingChanges(packageRoot, now)
        ).filter((change) => bound.has(change.variable));

        // Surface history: the bound files' commits, reachable from the pin
        // being read so the view stays self-consistent.
        const repo = repoId(tree);
        const prefix =
            packagePath === "." || packagePath === ""
                ? ""
                : `${packagePath.replace(/\/+$/, "")}/`;
        const paths = bindingPaths(surface, model);
        const commits = new Map<string, CommitRecord>();
        for (const bindingPath of paths) {
            const records = await ctx.git.listCommits(token, repo, {
                ref: pin,
                path: prefix + bindingPath,
                perPage: 20,
            });
            for (const record of records) {
                commits.set(record.sha, record);
            }
        }
        const history = [...commits.values()]
            .sort((left, right) => right.date.localeCompare(left.date))
            .slice(0, 20);

        // Pending change sets: open ones whose files touch the bindings.
        const pending: {
            id: string;
            title: string;
            state: string;
            prNumber: number | null;
        }[] = [];
        const open = ctx.store
            .listChangeSets(tree.id)
            .filter((row) => row.state === "draft" || row.state === "proposed");
        for (const row of open) {
            try {
                const comparison = await ctx.git.compare(
                    token,
                    repo,
                    row.baseRef,
                    branchFor(row.id),
                );
                const touches = comparison.files.some((file) =>
                    paths.some(
                        (bindingPath) =>
                            file === prefix + bindingPath ||
                            file.startsWith(`${prefix + bindingPath}/`),
                    ),
                );
                if (touches) {
                    pending.push({
                        id: row.id,
                        title: row.title,
                        state: row.state,
                        prNumber: row.prNumber,
                    });
                }
            } catch {
                // A branch deleted mid-listing is an abandoned change set
                // the reconciler has not caught up with; skip it.
            }
        }

        return c.json({
            pin,
            path: packagePath,
            surface,
            items,
            now,
            upcoming,
            history,
            pending,
        });
    });

    // One file's text, for the raw-text editor.
    app.get("/source-trees/:tree/file", async (c) => {
        const { tree, token } = await access(c);
        const pin = stagedPin(c);
        const packagePath = packagePathOf(c);
        const file = c.req.query("file");
        if (file === undefined || file === "") {
            throw new ApiError(400, "file is required");
        }
        const treeRoot = await ctx.stager.stageTree(tree, pin, token);
        const packageRoot = ctx.stager.packageRoot(treeRoot, packagePath);
        const absolute = containedPath(packageRoot, file, "file path");
        let content: string;
        try {
            content = await readFile(absolute, "utf8");
        } catch {
            throw new ApiError(404, `no such file: ${file}`);
        }
        return c.json({ pin, path: packagePath, file, content });
    });

    return app;
}
