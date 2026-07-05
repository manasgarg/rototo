// Startup configuration. Auth mode is resolved once, the way the old
// console did it: GitHub OAuth app credentials in the environment turn on
// team mode; otherwise the server trusts the workstation (local mode).

export const GITHUB_CLIENT_ID_ENV = "ROTOTO_GITHUB_CLIENT_ID";
export const GITHUB_CLIENT_SECRET_ENV = "ROTOTO_GITHUB_CLIENT_SECRET";
export const TOKEN_ENCRYPTION_KEY_ENV = "ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY";
export const ADMINS_ENV = "ROTOTO_CONSOLE_ADMINS";

export type AuthMode = "local" | "team";

export type ServerConfig = {
    authMode: AuthMode;
    host: string;
    port: number;
    //Origin used for OAuth redirects and cookies.
    publicUrl: string;
    allowedOrigins: string[];
    //Null means ephemeral state: in-memory store, no stored credentials.
    dataDir: string | null;
    githubOAuth: { clientId: string; clientSecret: string } | null;
    //Raw value of the token encryption key env var; decoded lazily so a
    //local-mode server never demands it.
    tokenEncryptionKey: string | null;
    //Bootstrap administrators as identity references (`github:<login>`).
    //Matched once at first sign-in to mint a durable deployment-scope
    //administer grant; ignored for that entry afterwards.
    admins: string[];
    //Explicit ambient token for local mode (flag or environment).
    packageToken: string | null;
};

export type ConfigOverrides = Partial<
    Pick<
        ServerConfig,
        | "host"
        | "port"
        | "publicUrl"
        | "allowedOrigins"
        | "dataDir"
        | "packageToken"
    >
>;

export function resolveConfig(
    env: Record<string, string | undefined>,
    overrides: ConfigOverrides = {},
): ServerConfig {
    const clientId = trimmed(env[GITHUB_CLIENT_ID_ENV]);
    const clientSecret = trimmed(env[GITHUB_CLIENT_SECRET_ENV]);
    if ((clientId === null) !== (clientSecret === null)) {
        throw new Error(
            `${GITHUB_CLIENT_ID_ENV} and ${GITHUB_CLIENT_SECRET_ENV} must be set together`,
        );
    }
    const githubOAuth =
        clientId !== null && clientSecret !== null
            ? { clientId, clientSecret }
            : null;

    const host = overrides.host ?? "127.0.0.1";
    const port = overrides.port ?? 7687;
    const publicUrl = stripTrailingSlash(
        overrides.publicUrl ?? `http://${host}:${port}`,
    );
    const allowedOrigins = overrides.allowedOrigins ?? [
        new URL(publicUrl).origin,
    ];

    return {
        authMode: githubOAuth === null ? "local" : "team",
        host,
        port,
        publicUrl,
        allowedOrigins,
        dataDir: overrides.dataDir ?? null,
        githubOAuth,
        tokenEncryptionKey: trimmed(env[TOKEN_ENCRYPTION_KEY_ENV]),
        admins: (env[ADMINS_ENV] ?? "")
            .split(",")
            .map((entry) => entry.trim())
            .filter((entry) => entry.length > 0),
        packageToken:
            overrides.packageToken ?? trimmed(env.ROTOTO_PACKAGE_TOKEN),
    };
}

function trimmed(value: string | undefined): string | null {
    const text = value?.trim() ?? "";
    return text.length > 0 ? text : null;
}

function stripTrailingSlash(url: string): string {
    return url.endsWith("/") ? url.slice(0, -1) : url;
}
