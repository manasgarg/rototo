// GitHub facts for Backend A, plus the OAuth web-flow exchange. Facts are
// fetched with the subject's own token and cached with a short TTL; that
// staleness is fine because Backend A is advisory — the write itself still
// runs with the user's token, so GitHub stays the authority at the moment
// of the operation (design/console-identity-authz.md 4.1).

import { createHash } from "node:crypto";

const GITHUB_API = "https://api.github.com";
const GITHUB_USER_AGENT = "rototo-console";
const FACTS_TTL_MS = 60_000;

export type GitHubUser = {
    id: number;
    login: string;
    name: string | null;
    email: string | null;
    avatarUrl: string | null;
};

export type RepoPermissions = {
    pull: boolean;
    push: boolean;
    maintain: boolean;
    admin: boolean;
};

// Facts about one repository as one token sees it; null when the repo does
// not exist or the token cannot see it (indistinguishable on purpose).
export type RepoFacts = {
    permissions: RepoPermissions;
    defaultBranch: string;
} | null;

export interface GitHubFacts {
    viewer(token: string): Promise<GitHubUser>;
    repoFacts(token: string, owner: string, name: string): Promise<RepoFacts>;
}

export class GitHubApi implements GitHubFacts {
    private readonly cache = new Map<
        string,
        { at: number; facts: RepoFacts }
    >();

    async viewer(token: string): Promise<GitHubUser> {
        const response = await this.get("/user", token);
        if (!response.ok) {
            throw new Error(
                `the GitHub token was rejected (${response.status})`,
            );
        }
        const body = (await response.json()) as {
            id: number;
            login: string;
            name: string | null;
            email: string | null;
            avatar_url: string | null;
        };
        return {
            id: body.id,
            login: body.login,
            name: body.name,
            email: body.email,
            avatarUrl: body.avatar_url,
        };
    }

    async repoFacts(
        token: string,
        owner: string,
        name: string,
    ): Promise<RepoFacts> {
        const key = `${sha256(token)}:${owner}/${name}`;
        const cached = this.cache.get(key);
        if (cached !== undefined && Date.now() - cached.at < FACTS_TTL_MS) {
            return cached.facts;
        }
        const response = await this.get(`/repos/${owner}/${name}`, token);
        let facts: RepoFacts;
        if (response.status === 404 || response.status === 403) {
            facts = null;
        } else if (!response.ok) {
            throw new Error(
                `GitHub repository lookup failed (${response.status})`,
            );
        } else {
            const body = (await response.json()) as {
                default_branch: string;
                permissions?: {
                    pull?: boolean;
                    push?: boolean;
                    maintain?: boolean;
                    admin?: boolean;
                };
            };
            facts = {
                permissions: {
                    pull: body.permissions?.pull ?? false,
                    push: body.permissions?.push ?? false,
                    maintain: body.permissions?.maintain ?? false,
                    admin: body.permissions?.admin ?? false,
                },
                defaultBranch: body.default_branch,
            };
        }
        this.cache.set(key, { at: Date.now(), facts });
        return facts;
    }

    private get(path: string, token: string): Promise<Response> {
        return fetch(`${GITHUB_API}${path}`, {
            headers: {
                accept: "application/vnd.github+json",
                authorization: `Bearer ${token}`,
                "user-agent": GITHUB_USER_AGENT,
                "x-github-api-version": "2022-11-28",
            },
        });
    }
}

// GitHub OAuth web-flow code exchange (team mode sign-in).
export async function exchangeOAuthCode(
    clientId: string,
    clientSecret: string,
    code: string,
): Promise<string> {
    const response = await fetch(
        "https://github.com/login/oauth/access_token",
        {
            method: "POST",
            headers: {
                accept: "application/json",
                "content-type": "application/json",
                "user-agent": GITHUB_USER_AGENT,
            },
            body: JSON.stringify({
                client_id: clientId,
                client_secret: clientSecret,
                code,
            }),
        },
    );
    const body = (await response.json()) as {
        access_token?: string;
        error?: string;
        error_description?: string;
    };
    if (!response.ok || body.access_token === undefined) {
        throw new Error(
            body.error_description ?? body.error ?? "GitHub OAuth failed",
        );
    }
    return body.access_token;
}

function sha256(value: string): string {
    return createHash("sha256").update(value).digest("hex");
}
