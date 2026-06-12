import { randomBytes } from "node:crypto";
import { NextResponse } from "next/server";
import { authCookieOptions, OAUTH_STATE_COOKIE } from "@/lib/auth";
import { createOAuthState } from "@/lib/db";
import { oauthBaseUrl } from "@/lib/github";

export const runtime = "nodejs";

const GITHUB_OAUTH_SCOPES = "read:user repo";

export async function GET() {
  const clientId = process.env.GITHUB_CLIENT_ID;
  const clientSecret = process.env.GITHUB_CLIENT_SECRET;
  if (!clientId || !clientSecret) {
    return NextResponse.json(
      { error: "GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET are required" },
      { status: 500 },
    );
  }

  const state = randomBytes(24).toString("base64url");
  createOAuthState(state);

  const params = new URLSearchParams({
    client_id: clientId,
    redirect_uri: `${oauthBaseUrl()}/api/auth/github/callback`,
    scope: GITHUB_OAUTH_SCOPES,
    state,
  });
  const response = NextResponse.redirect(
    `https://github.com/login/oauth/authorize?${params.toString()}`,
  );
  response.cookies.set(OAUTH_STATE_COOKIE, state, authCookieOptions(60 * 10));
  return response;
}
