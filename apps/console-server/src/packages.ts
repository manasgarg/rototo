// Staged package reading (design/console-git-ops.md rule 2): the Rust core
// caches trees by (remote, pin), so nothing here can go stale and there is
// no invalidation. TypeScript resolves refs; only full commit SHAs cross
// this boundary.

import { createHash, randomUUID } from "node:crypto";
import { existsSync, renameSync, rmSync } from "node:fs";
import path from "node:path";

import { native, type NativePinStore } from "./native.ts";
import type { SourceTreeRow } from "./store.ts";

const PIN = /^[0-9a-f]{40}$/;

export function isPin(value: string): boolean {
    return PIN.test(value);
}

// A base package another package extends, resolved to a tree-relative
// package path when the source is a local path inside the tree; remote
// sources (git+, https) stay external and return null.
export function resolveExtend(
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

export type StagerOptions = {
    // Where staged trees live; under the data dir when one is set, or the
    // XDG cache home otherwise (config.cacheDir).
    cacheRoot: string;
    maxBytes?: number;
    // Test seam: where a source tree's git remote actually is. Production
    // is always the GitHub HTTPS remote.
    remoteFor?: (tree: SourceTreeRow) => string;
};

export class PackageStager {
    readonly remoteFor: (tree: SourceTreeRow) => string;
    private readonly pins: NativePinStore;
    private readonly composedRootDir: string;

    constructor(options: StagerOptions) {
        this.pins = new native._PinStore(options.cacheRoot, options.maxBytes);
        this.composedRootDir = `${options.cacheRoot}-composed`;
        this.remoteFor =
            options.remoteFor ??
            ((tree) => `https://github.com/${tree.owner}/${tree.name}.git`);
    }

    // The staged tree for a pin, as an absolute path.
    async stageTree(
        tree: SourceTreeRow,
        pin: string,
        token: string | null,
    ): Promise<string> {
        return this.pins.stage(this.remoteFor(tree), pin, token ?? undefined);
    }

    // A package root inside a staged tree; refuses paths that escape it.
    packageRoot(treeRoot: string, packagePath: string): string {
        return containedPath(treeRoot, packagePath, "package path");
    }

    // The composed view of a package (its extends chain resolved and
    // layered), for the ring-2 reads: fleet health and the matrix. Staged
    // trees are immutable per pin, so the composed copy is cacheable by the
    // package root's path; racing requests compose into a scratch dir and
    // the first rename wins.
    async composedRoot(treeRoot: string, packagePath: string): Promise<string> {
        const root = this.packageRoot(treeRoot, packagePath);
        const key = createHash("sha256")
            .update(root)
            .digest("hex")
            .slice(0, 24);
        const dest = path.join(this.composedRootDir, key);
        if (existsSync(dest)) {
            return dest;
        }
        const scratch = `${dest}.tmp-${randomUUID()}`;
        await native.stageComposed(root, scratch);
        try {
            renameSync(scratch, dest);
        } catch {
            rmSync(scratch, { recursive: true, force: true });
        }
        return dest;
    }
}

// Resolves `relative` under `root` and refuses traversal outside it.
export function containedPath(
    root: string,
    relative: string,
    what: string,
): string {
    const resolved = path.resolve(root, relative);
    if (resolved !== root && !resolved.startsWith(root + path.sep)) {
        throw new Error(`${what} escapes the staged tree: ${relative}`);
    }
    return resolved;
}
