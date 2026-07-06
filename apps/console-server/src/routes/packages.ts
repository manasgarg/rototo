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
import { repoId } from "../change-sets.ts";
import { native } from "../native.ts";
import { containedPath, isPin } from "../packages.ts";
import type { SourceTreeRow } from "../store.ts";

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
        const token = await ctx.actingToken(subject);
        if (token === null) {
            throw new ApiError(
                403,
                "no GitHub credential is available to read this tree with",
            );
        }
        return { tree, subject, token };
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

// A base package another package extends, resolved to a tree-relative
// package path when the source is a local path inside the tree; remote
// sources (git+, https) stay external and return null.
function resolveExtend(
    fromPackage: string,
    source: string,
    known: Set<string>,
): string | null {
    if (source.includes("://")) {
        return null;
    }
    const fromDir = fromPackage === "." ? "" : fromPackage;
    const resolved = path.posix.normalize(path.posix.join(fromDir, source));
    const candidate = resolved === "" ? "." : resolved;
    if (candidate.startsWith("..")) {
        return null;
    }
    return known.has(candidate) ? candidate : null;
}
