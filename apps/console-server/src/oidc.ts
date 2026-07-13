// Generic OIDC sign-in (design/console-identity-authz.md 3.1): one
// implementation covers Okta, Entra ID, Google, Auth0, Keycloak, and the
// rest, configured at deployment time. The authorization-code flow against
// the issuer's discovery document, with the ID token verified against the
// issuer's JWKS. No SDK: the flow is a handful of HTTP calls and one
// signature check, and owning them keeps the injectable test seam honest.

import { createPublicKey, timingSafeEqual, verify } from "node:crypto";

// What an ID token asserts once verified. `subject` is keyed as
// `iss` + `sub` by the caller; email and name are display snapshots.
export type OidcClaims = {
    issuer: string;
    subject: string;
    email: string | null;
    emailVerified: boolean;
    name: string | null;
    picture: string | null;
};

// The test seam: exchange an authorization code for verified claims.
export type OidcExchange = (
    code: string,
    redirectUri: string,
    nonce: string,
) => Promise<OidcClaims>;

type Discovery = {
    authorization_endpoint: string;
    token_endpoint: string;
    jwks_uri: string;
    issuer: string;
};

type Jwk = {
    kid?: string;
    kty: string;
    alg?: string;
    [key: string]: unknown;
};

export class OidcProvider {
    private readonly issuer: string;
    private readonly clientId: string;
    private readonly clientSecret: string;
    private discovery: Discovery | null = null;
    private jwks: Jwk[] | null = null;

    constructor(config: {
        issuer: string;
        clientId: string;
        clientSecret: string;
    }) {
        this.issuer = config.issuer;
        this.clientId = config.clientId;
        this.clientSecret = config.clientSecret;
    }

    async authorizeUrl(
        redirectUri: string,
        state: string,
        nonce: string,
    ): Promise<string> {
        const discovery = await this.discover();
        const url = new URL(discovery.authorization_endpoint);
        url.searchParams.set("response_type", "code");
        url.searchParams.set("client_id", this.clientId);
        url.searchParams.set("redirect_uri", redirectUri);
        url.searchParams.set("scope", "openid email profile");
        url.searchParams.set("state", state);
        url.searchParams.set("nonce", nonce);
        return url.toString();
    }

    // The code exchange plus ID-token verification: signature against the
    // issuer's JWKS, then iss, aud, exp, and nonce.
    readonly exchange: OidcExchange = async (code, redirectUri, nonce) => {
        const discovery = await this.discover();
        const response = await fetch(discovery.token_endpoint, {
            method: "POST",
            headers: {
                "content-type": "application/x-www-form-urlencoded",
                accept: "application/json",
            },
            body: new URLSearchParams({
                grant_type: "authorization_code",
                code,
                redirect_uri: redirectUri,
                client_id: this.clientId,
                client_secret: this.clientSecret,
            }),
        });
        if (!response.ok) {
            throw new Error(
                `the OIDC token endpoint answered ${response.status}`,
            );
        }
        const body = (await response.json()) as { id_token?: string };
        if (body.id_token === undefined) {
            throw new Error("the OIDC token response carried no id_token");
        }
        return this.verifyIdToken(body.id_token, nonce);
    };

    private async verifyIdToken(
        idToken: string,
        nonce: string,
    ): Promise<OidcClaims> {
        const [headerPart, payloadPart, signaturePart] = idToken.split(".");
        if (
            headerPart === undefined ||
            payloadPart === undefined ||
            signaturePart === undefined
        ) {
            throw new Error("the ID token is not a JWT");
        }
        const header = JSON.parse(
            Buffer.from(headerPart, "base64url").toString("utf8"),
        ) as { alg?: string; kid?: string };
        if (header.alg !== "RS256") {
            throw new Error(
                `the ID token uses ${header.alg ?? "no"} signing; this console supports RS256`,
            );
        }
        const key = await this.signingKey(header.kid);
        const valid = verify(
            "RSA-SHA256",
            Buffer.from(`${headerPart}.${payloadPart}`, "utf8"),
            createPublicKey({ key: key as never, format: "jwk" }),
            Buffer.from(signaturePart, "base64url"),
        );
        if (!valid) {
            throw new Error("the ID token signature does not verify");
        }
        const claims = JSON.parse(
            Buffer.from(payloadPart, "base64url").toString("utf8"),
        ) as {
            iss?: string;
            sub?: string;
            aud?: string | string[];
            exp?: number;
            nonce?: string;
            email?: string;
            email_verified?: boolean;
            name?: string;
            picture?: string;
        };
        const issuer = (claims.iss ?? "").replace(/\/$/, "");
        if (issuer !== this.issuer.replace(/\/$/, "")) {
            throw new Error(`the ID token names issuer ${claims.iss}`);
        }
        const audience = Array.isArray(claims.aud)
            ? claims.aud
            : [claims.aud ?? ""];
        if (!audience.includes(this.clientId)) {
            throw new Error("the ID token was not issued to this console");
        }
        if (claims.exp === undefined || claims.exp * 1000 <= Date.now()) {
            throw new Error("the ID token has expired");
        }
        if (!constantTimeEqual(claims.nonce ?? "", nonce)) {
            throw new Error("the ID token nonce does not match this sign-in");
        }
        if (claims.sub === undefined || claims.sub === "") {
            throw new Error("the ID token asserts no subject");
        }
        return {
            issuer,
            subject: claims.sub,
            email: claims.email ?? null,
            emailVerified: claims.email_verified === true,
            name: claims.name ?? null,
            picture: claims.picture ?? null,
        };
    }

    private async discover(): Promise<Discovery> {
        if (this.discovery !== null) {
            return this.discovery;
        }
        const response = await fetch(
            `${this.issuer}/.well-known/openid-configuration`,
        );
        if (!response.ok) {
            throw new Error(
                `OIDC discovery at ${this.issuer} answered ${response.status}`,
            );
        }
        this.discovery = (await response.json()) as Discovery;
        return this.discovery;
    }

    private async signingKey(kid: string | undefined): Promise<Jwk> {
        // Fetch (or refetch on a miss, covering key rotation) and pick by
        // kid; a single-key JWKS may omit kids entirely.
        const pick = (keys: Jwk[]): Jwk | undefined =>
            kid === undefined
                ? keys.find((key) => key.kty === "RSA")
                : keys.find((key) => key.kid === kid);
        if (this.jwks !== null) {
            const key = pick(this.jwks);
            if (key !== undefined) {
                return key;
            }
        }
        const discovery = await this.discover();
        const response = await fetch(discovery.jwks_uri);
        if (!response.ok) {
            throw new Error(`the issuer's JWKS answered ${response.status}`);
        }
        this.jwks = ((await response.json()) as { keys: Jwk[] }).keys;
        const key = pick(this.jwks);
        if (key === undefined) {
            throw new Error("no JWKS key matches the ID token's kid");
        }
        return key;
    }
}

function constantTimeEqual(left: string, right: string): boolean {
    const a = Buffer.from(left, "utf8");
    const b = Buffer.from(right, "utf8");
    return a.length === b.length && timingSafeEqual(a, b);
}
