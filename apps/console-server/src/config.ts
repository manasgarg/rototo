// Startup configuration. Auth mode is resolved once, the way the old
// console did it: GitHub OAuth app credentials in the environment turn on
// team mode; otherwise the server trusts the workstation (local mode).

export const GITHUB_CLIENT_ID_ENV = "ROTOTO_GITHUB_CLIENT_ID";
export const GITHUB_CLIENT_SECRET_ENV = "ROTOTO_GITHUB_CLIENT_SECRET";
export const TOKEN_ENCRYPTION_KEY_ENV = "ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY";
export const ADMINS_ENV = "ROTOTO_CONSOLE_ADMINS";
export const OIDC_ISSUER_ENV = "ROTOTO_CONSOLE_OIDC_ISSUER";
export const OIDC_CLIENT_ID_ENV = "ROTOTO_CONSOLE_OIDC_CLIENT_ID";
export const OIDC_CLIENT_SECRET_ENV = "ROTOTO_CONSOLE_OIDC_CLIENT_SECRET";
export const OIDC_DISPLAY_NAME_ENV = "ROTOTO_CONSOLE_OIDC_DISPLAY_NAME";
export const ENROLLMENT_ENV = "ROTOTO_CONSOLE_ENROLLMENT";
export const ENROLLMENT_DOMAINS_ENV = "ROTOTO_CONSOLE_ENROLLMENT_DOMAINS";
export const GITHUB_APP_ID_ENV = "ROTOTO_GITHUB_APP_ID";
export const GITHUB_APP_PRIVATE_KEY_ENV = "ROTOTO_GITHUB_APP_PRIVATE_KEY";
export const GITHUB_WEBHOOK_SECRET_ENV = "ROTOTO_GITHUB_WEBHOOK_SECRET";

export type AuthMode = "local" | "team";

// Completing authentication must not grant access; enrollment policy says
// who gets a principal at sign-in (design/console-identity-authz.md 3.4).
export type EnrollmentPolicy = "invite-only" | "domain-allowlist" | "open";

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
    //One generic OIDC provider covers Okta, Entra, Google, and the rest.
    oidc: {
        issuer: string;
        clientId: string;
        clientSecret: string;
        displayName: string;
    } | null;
    enrollment: EnrollmentPolicy;
    //Verified-email domains that auto-enroll under domain-allowlist.
    enrollmentDomains: string[];
    //The console's GitHub App, minting installation tokens for principals
    //with no GitHub credential of their own (design/console-git-ops.md).
    githubApp: { appId: string; privateKey: string } | null;
    //HMAC secret for GitHub webhook nudges; null disables the endpoint.
    webhookSecret: string | null;
    //Raw value of the token encryption key env var; decoded lazily so a
    //local-mode server never demands it.
    tokenEncryptionKey: string | null;
    //Bootstrap administrators as identity references (`github:<login>` or
    //`oidc:<email>`). Matched once at first sign-in to mint a durable
    //deployment-scope administer grant; ignored for that entry afterwards.
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

    const oidcIssuer = trimmed(env[OIDC_ISSUER_ENV]);
    const oidcClientId = trimmed(env[OIDC_CLIENT_ID_ENV]);
    const oidcClientSecret = trimmed(env[OIDC_CLIENT_SECRET_ENV]);
    if (
        oidcIssuer !== null &&
        (oidcClientId === null || oidcClientSecret === null)
    ) {
        throw new Error(
            `${OIDC_ISSUER_ENV} needs ${OIDC_CLIENT_ID_ENV} and ${OIDC_CLIENT_SECRET_ENV} too`,
        );
    }
    const oidc =
        oidcIssuer !== null &&
        oidcClientId !== null &&
        oidcClientSecret !== null
            ? {
                  issuer: stripTrailingSlash(oidcIssuer),
                  clientId: oidcClientId,
                  clientSecret: oidcClientSecret,
                  displayName: trimmed(env[OIDC_DISPLAY_NAME_ENV]) ?? "SSO",
              }
            : null;

    const appId = trimmed(env[GITHUB_APP_ID_ENV]);
    const appKey = trimmed(env[GITHUB_APP_PRIVATE_KEY_ENV]);
    if ((appId === null) !== (appKey === null)) {
        throw new Error(
            `${GITHUB_APP_ID_ENV} and ${GITHUB_APP_PRIVATE_KEY_ENV} must be set together`,
        );
    }

    const enrollment = trimmed(env[ENROLLMENT_ENV]) ?? "invite-only";
    if (
        enrollment !== "invite-only" &&
        enrollment !== "domain-allowlist" &&
        enrollment !== "open"
    ) {
        throw new Error(
            `${ENROLLMENT_ENV} must be invite-only, domain-allowlist, or open`,
        );
    }

    const host = overrides.host ?? "127.0.0.1";
    const port = overrides.port ?? 7687;
    const publicUrl = stripTrailingSlash(
        overrides.publicUrl ?? `http://${host}:${port}`,
    );
    const allowedOrigins = overrides.allowedOrigins ?? [
        new URL(publicUrl).origin,
    ];

    return {
        authMode: githubOAuth === null && oidc === null ? "local" : "team",
        host,
        port,
        publicUrl,
        allowedOrigins,
        dataDir: overrides.dataDir ?? null,
        githubOAuth,
        oidc,
        enrollment,
        enrollmentDomains: splitList(env[ENROLLMENT_DOMAINS_ENV]),
        githubApp:
            appId !== null && appKey !== null
                ? { appId, privateKey: appKey }
                : null,
        webhookSecret: trimmed(env[GITHUB_WEBHOOK_SECRET_ENV]),
        tokenEncryptionKey: trimmed(env[TOKEN_ENCRYPTION_KEY_ENV]),
        admins: splitList(env[ADMINS_ENV]),
        packageToken:
            overrides.packageToken ?? trimmed(env.ROTOTO_PACKAGE_TOKEN),
    };
}

function splitList(value: string | undefined): string[] {
    return (value ?? "")
        .split(",")
        .map((entry) => entry.trim())
        .filter((entry) => entry.length > 0);
}

function trimmed(value: string | undefined): string | null {
    const text = value?.trim() ?? "";
    return text.length > 0 ? text : null;
}

function stripTrailingSlash(url: string): string {
    return url.endsWith("/") ? url.slice(0, -1) : url;
}
