// The console's GitHub App credential (design/console-git-ops.md, "Whose
// token does the work"): installation tokens minted from the App's private
// key, per repository, cached in memory for their roughly one-hour life and
// never written to disk. When this token acts, the console is the
// enforcement point — decide() is the law, not a prediction — which is why
// the acting-credential selection lives beside it.

import { createSign, randomUUID } from "node:crypto";

const GITHUB_API = "https://api.github.com";
const USER_AGENT = "rototo-console";
// Refresh with headroom: GitHub tokens live ~60 minutes.
const TOKEN_MARGIN_MS = 5 * 60 * 1000;

export interface AppCredentials {
    // The installation token for a repository, or null when the App is not
    // installed there (or no App is configured at all).
    installationToken(owner: string, name: string): Promise<string | null>;
}

export const NO_APP: AppCredentials = {
    installationToken: () => Promise.resolve(null),
};

export class GitHubAppCredentials implements AppCredentials {
    private readonly appId: string;
    private readonly privateKey: string;
    private readonly cache = new Map<
        string,
        { token: string; expiresAt: number }
    >();

    constructor(config: { appId: string; privateKey: string }) {
        this.appId = config.appId;
        this.privateKey = config.privateKey;
    }

    async installationToken(
        owner: string,
        name: string,
    ): Promise<string | null> {
        const key = `${owner}/${name}`;
        const cached = this.cache.get(key);
        if (cached !== undefined && cached.expiresAt > Date.now()) {
            return cached.token;
        }
        const jwt = this.appJwt();
        const installation = await fetch(
            `${GITHUB_API}/repos/${owner}/${name}/installation`,
            { headers: this.headers(jwt) },
        );
        if (installation.status === 404) {
            return null;
        }
        if (!installation.ok) {
            throw new Error(
                `GitHub App installation lookup failed (${installation.status})`,
            );
        }
        const { id } = (await installation.json()) as { id: number };
        const minted = await fetch(
            `${GITHUB_API}/app/installations/${id}/access_tokens`,
            { method: "POST", headers: this.headers(jwt) },
        );
        if (!minted.ok) {
            throw new Error(
                `GitHub App token minting failed (${minted.status})`,
            );
        }
        const body = (await minted.json()) as {
            token: string;
            expires_at: string;
        };
        this.cache.set(key, {
            token: body.token,
            expiresAt: Date.parse(body.expires_at) - TOKEN_MARGIN_MS,
        });
        return body.token;
    }

    // The App JWT: RS256 over { iat, exp, iss }, ten minutes max.
    private appJwt(): string {
        const now = Math.floor(Date.now() / 1000);
        const header = base64url(
            JSON.stringify({ alg: "RS256", typ: "JWT", kid: randomUUID() }),
        );
        const payload = base64url(
            JSON.stringify({ iat: now - 60, exp: now + 540, iss: this.appId }),
        );
        const signer = createSign("RSA-SHA256");
        signer.update(`${header}.${payload}`);
        const signature = signer.sign(this.privateKey).toString("base64url");
        return `${header}.${payload}.${signature}`;
    }

    private headers(jwt: string): Record<string, string> {
        return {
            accept: "application/vnd.github+json",
            authorization: `Bearer ${jwt}`,
            "user-agent": USER_AGENT,
            "x-github-api-version": "2022-11-28",
        };
    }
}

function base64url(value: string): string {
    return Buffer.from(value, "utf8").toString("base64url");
}

// Whose token does the work, per operation: the person's own GitHub
// credential when they have one (GitHub enforces; the console predicts),
// else the App's installation token (the console enforces via decide()),
// else nothing — read-only.
export type ActingCredential = {
    token: string;
    mode: "user" | "app";
};
