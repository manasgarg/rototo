// Server-side sessions: opaque cookie, hashed token in the store, 14-day
// TTL. Sessions prove presence; identities hold credentials
// (design/console-identity-authz.md 3.5).

import { createHash, randomBytes } from "node:crypto";

import type { Store } from "./store.ts";

export const SESSION_COOKIE = "rototo_console_session";
export const OAUTH_STATE_COOKIE = "rototo_console_oauth_state";
export const SESSION_TTL_MS = 14 * 24 * 60 * 60 * 1000;

export function issueSession(store: Store, principalId: string): string {
    const token = randomBytes(32).toString("base64url");
    store.createSession(hashToken(token), principalId, SESSION_TTL_MS);
    return token;
}

export function sessionPrincipalId(
    store: Store,
    cookieHeader: string | undefined,
): string | null {
    const token = cookieValue(cookieHeader, SESSION_COOKIE);
    if (token === null) {
        return null;
    }
    return store.getSession(hashToken(token))?.principalId ?? null;
}

export function endSession(
    store: Store,
    cookieHeader: string | undefined,
): void {
    const token = cookieValue(cookieHeader, SESSION_COOKIE);
    if (token !== null) {
        store.deleteSession(hashToken(token));
    }
}

export function hashToken(token: string): string {
    return createHash("sha256").update(token).digest("hex");
}

export function cookieValue(
    cookieHeader: string | undefined,
    name: string,
): string | null {
    if (cookieHeader === undefined) {
        return null;
    }
    for (const pair of cookieHeader.split(";")) {
        const eq = pair.indexOf("=");
        if (eq === -1) {
            continue;
        }
        if (pair.slice(0, eq).trim() === name) {
            return pair.slice(eq + 1).trim();
        }
    }
    return null;
}

// Matches the old console's cookie options: httpOnly, SameSite=Lax,
// path=/, secure when the public URL is https.
export function setCookie(
    name: string,
    value: string,
    secure: boolean,
    maxAgeSeconds?: number,
): string {
    let cookie = `${name}=${value}; HttpOnly; SameSite=Lax; Path=/`;
    if (secure) {
        cookie += "; Secure";
    }
    if (maxAgeSeconds !== undefined) {
        cookie += `; Max-Age=${maxAgeSeconds}`;
    }
    return cookie;
}
