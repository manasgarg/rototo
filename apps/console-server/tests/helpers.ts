// Shared test scaffolding: a fake GitHub, a seeded team-mode app, and a
// signed-in session without touching the network.

import path from "node:path";

import type { GitHubFacts, GitHubUser, RepoFacts } from "../src/github.ts";
import {
    buildApp,
    CONSOLE_HEADER,
    type App,
    type AppDeps,
} from "../src/app.ts";
import { resolveConfig, type ServerConfig } from "../src/config.ts";
import { issueSession, SESSION_COOKIE } from "../src/sessions.ts";
import { Store, type SourceTreeRow } from "../src/store.ts";
import { TokenCrypto } from "../src/token-crypto.ts";
import { FakeGit } from "./fake-git.ts";

export class FakeGitHub implements GitHubFacts {
    // token -> viewer
    readonly viewers = new Map<string, GitHubUser>();
    // `${token}:${owner}/${name}` -> facts
    readonly repos = new Map<string, RepoFacts>();

    async viewer(token: string): Promise<GitHubUser> {
        const user = this.viewers.get(token);
        if (user === undefined) {
            throw new Error("the GitHub token was rejected (401)");
        }
        return user;
    }

    async repoFacts(
        token: string,
        owner: string,
        name: string,
    ): Promise<RepoFacts> {
        return this.repos.get(`${token}:${owner}/${name}`) ?? null;
    }

    grantRepo(
        token: string,
        repo: string,
        permissions: Partial<{
            pull: boolean;
            push: boolean;
            maintain: boolean;
            admin: boolean;
        }>,
    ): void {
        this.repos.set(`${token}:${repo}`, {
            permissions: {
                pull: permissions.pull ?? false,
                push: permissions.push ?? false,
                maintain: permissions.maintain ?? false,
                admin: permissions.admin ?? false,
            },
            defaultBranch: "main",
        });
    }
}

export const TEST_KEY = TokenCrypto.generate().keyBase64();

export function teamConfig(
    overrides: Partial<ServerConfig> = {},
): ServerConfig {
    const config = resolveConfig(
        {
            ROTOTO_GITHUB_CLIENT_ID: "client-id",
            ROTOTO_GITHUB_CLIENT_SECRET: "client-secret",
            ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY: TEST_KEY,
        },
        {},
    );
    return { ...config, ...overrides };
}

export function localConfig(
    overrides: Partial<ServerConfig> = {},
): ServerConfig {
    // No explicit token and no data dir; tests that need a GitHub identity
    // in local mode stub the facts instead.
    const config = resolveConfig({}, { packageToken: "ambient-token" });
    return { ...config, ...overrides };
}

export type TeamHarness = {
    app: App;
    store: Store;
    github: FakeGitHub;
    config: ServerConfig;
    // Signs a principal in: creates principal + github identity (with an
    // encrypted copy of `token`) + session; returns request headers.
    signIn(options: { login: string; token: string; displayName?: string }): {
        principalId: string;
        headers: Record<string, string>;
    };
};

export function teamHarness(
    overrides: Partial<ServerConfig> = {},
    deps: Partial<AppDeps> = {},
): TeamHarness {
    const config = teamConfig(overrides);
    const store = new Store(null);
    const github = new FakeGitHub();
    const app = buildApp({ config, store, github, ...deps });
    const crypto = TokenCrypto.fromEnvValue(TEST_KEY);
    let nextSubject = 1000;
    return {
        app,
        store,
        github,
        config,
        signIn({ login, token, displayName }) {
            const principal = store.createPrincipal(displayName ?? login);
            store.attachIdentity(
                principal.id,
                {
                    provider: "github",
                    subject: String(nextSubject++),
                    login,
                    email: null,
                    emailVerified: false,
                    name: displayName ?? login,
                    avatarUrl: null,
                },
                crypto.encrypt(token),
            );
            const session = issueSession(store, principal.id);
            return {
                principalId: principal.id,
                headers: { cookie: `${SESSION_COOKIE}=${session}` },
            };
        },
    };
}

export function mutationHeaders(
    headers: Record<string, string> = {},
): Record<string, string> {
    return {
        [CONSOLE_HEADER]: "1",
        "content-type": "application/json",
        ...headers,
    };
}

export async function json(response: Response): Promise<any> {
    if (!response.headers.get("content-type")?.includes("json")) {
        throw new Error(
            `expected JSON, got ${response.status} ${await response.text()}`,
        );
    }
    return response.json();
}

export const REPO_ROOT = path.resolve(import.meta.dirname, "../../..");

export type GitHarness = TeamHarness & {
    fakeGit: FakeGit;
    tree: SourceTreeRow;
    packagePath: string;
    // main's pin after seeding.
    basePin: string;
    cleanup(): void;
    get(path: string, headers: Record<string, string>): Promise<Response>;
    post(
        path: string,
        body: unknown,
        headers: Record<string, string>,
    ): Promise<Response>;
};

// A team-mode app wired to a local bare repository acting as GitHub:
// examples/basic seeded under packages/basic on main, the tree registered,
// staged reads fetching from the same repo the fake mutates.
export function gitHarness(
    overrides: Partial<ServerConfig> = {},
    deps: Partial<AppDeps> = {},
): GitHarness {
    const fakeGit = FakeGit.init();
    const basePin = fakeGit.seedBranch(
        "main",
        path.join(REPO_ROOT, "examples/basic"),
        "packages/basic",
    );
    const harness = teamHarness(overrides, {
        git: fakeGit,
        gitRemote: () => fakeGit.gitDir,
        pinCacheRoot: path.join(fakeGit.gitDir, "..", "pins"),
        ...deps,
    });
    const tree = harness.store.insertSourceTree({
        kind: "github",
        owner: "acme",
        name: "config",
        defaultBranch: "main",
        createdBy: null,
    });
    return {
        ...harness,
        fakeGit,
        tree,
        packagePath: "packages/basic",
        basePin,
        cleanup: () => fakeGit.cleanup(),
        get: (requestPath, headers) =>
            Promise.resolve(
                harness.app.fetch(
                    new Request(`http://console.test${requestPath}`, {
                        headers,
                    }),
                ),
            ),
        post: (requestPath, body, headers) =>
            Promise.resolve(
                harness.app.fetch(
                    new Request(`http://console.test${requestPath}`, {
                        method: "POST",
                        headers: mutationHeaders(headers),
                        body: JSON.stringify(body),
                    }),
                ),
            ),
    };
}
