// Local mode: trust the workstation. No login UI; the ambient GitHub token
// is resolved once from the same chain the old console used (explicit
// flag/env token, stored device-flow credentials, `gh auth token`), and the
// identity shown in the UI falls back to git config when no token exists.
// Local mode has one implicit principal with every capability; nothing here
// feeds authorization.

import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";

import type { GitHubFacts, GitHubUser } from "./github.ts";

const execFileAsync = promisify(execFile);

export type CredentialSource =
    "flag" | "environment" | "device-flow" | "gh-cli";

export type AmbientToken = {
    token: string;
    source: CredentialSource;
};

export type LocalIdentity =
    | ({ kind: "github" } & GitHubUser)
    | { kind: "git-config"; name: string | null; email: string | null };

export async function resolveAmbientToken(
    explicitToken: string | null,
    dataDir: string | null,
): Promise<AmbientToken | null> {
    if (explicitToken !== null && explicitToken.trim().length > 0) {
        // clap fills --package-token from ROTOTO_PACKAGE_TOKEN too; here the
        // caller passes flag values and env values through the same slot, so
        // report the broader source.
        return { token: explicitToken.trim(), source: "environment" };
    }

    if (dataDir !== null) {
        try {
            const contents = await readFile(
                path.join(dataDir, "credentials.json"),
                "utf8",
            );
            const credentials = JSON.parse(contents) as {
                github_token?: string;
            };
            const token = credentials.github_token?.trim() ?? "";
            if (token.length > 0) {
                return { token, source: "device-flow" };
            }
        } catch {
            // No stored credentials; keep walking the chain.
        }
    }

    try {
        const { stdout } = await execFileAsync("gh", ["auth", "token"]);
        const token = stdout.trim();
        if (token.length > 0) {
            return { token, source: "gh-cli" };
        }
    } catch {
        // gh missing or signed out; the chain ends.
    }

    return null;
}

type LocalAuthDeps = {
    explicitToken: string | null;
    dataDir: string | null;
    github: GitHubFacts;
};

export class LocalAuth {
    private readonly deps: LocalAuthDeps;
    private ambient: AmbientToken | null = null;
    private identity: { token: string; user: GitHubUser } | null = null;
    private resolved = false;

    constructor(deps: LocalAuthDeps) {
        this.deps = deps;
    }

    async token(): Promise<AmbientToken | null> {
        if (!this.resolved) {
            this.ambient = await resolveAmbientToken(
                this.deps.explicitToken,
                this.deps.dataDir,
            );
            this.resolved = true;
        }
        return this.ambient;
    }

    // The GitHub identity behind the ambient token, fetched once per token;
    // git-config fallback when there is no token or it is rejected.
    async localIdentity(): Promise<LocalIdentity> {
        const ambient = await this.token();
        if (ambient !== null) {
            if (this.identity?.token === ambient.token) {
                return { kind: "github", ...this.identity.user };
            }
            try {
                const user = await this.deps.github.viewer(ambient.token);
                this.identity = { token: ambient.token, user };
                return { kind: "github", ...user };
            } catch {
                // Fall through to git config.
            }
        }
        return {
            kind: "git-config",
            name: await gitConfig("user.name"),
            email: await gitConfig("user.email"),
        };
    }
}

async function gitConfig(key: string): Promise<string | null> {
    try {
        const { stdout } = await execFileAsync("git", ["config", "--get", key]);
        const value = stdout.trim();
        return value.length > 0 ? value : null;
    } catch {
        return null;
    }
}
