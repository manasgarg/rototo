import { cookies } from "next/headers";
import { NextRequest, NextResponse } from "next/server";
import {
  authCookieOptions,
  OAUTH_STATE_COOKIE,
  SESSION_COOKIE,
} from "@/lib/auth";
import { consumeOAuthState, createSession } from "@/lib/db";
import { exchangeGitHubCode, getGitHubViewer, publicAppUrl } from "@/lib/github";

export const runtime = "nodejs";

export async function GET(request: NextRequest) {
  const url = new URL(request.url);
  const code = url.searchParams.get("code");
  const state = url.searchParams.get("state");
  const cookieStore = await cookies();
  const cookieState = cookieStore.get(OAUTH_STATE_COOKIE)?.value;
  if (!code || !state || !cookieState || state !== cookieState || !consumeOAuthState(state)) {
    return NextResponse.json({ error: "invalid GitHub OAuth state" }, { status: 400 });
  }

  const token = await exchangeGitHubCode(code);
  const viewer = await getGitHubViewer(token);
  const sessionToken = createSession({
    githubUserId: String(viewer.id),
    githubLogin: viewer.login,
    githubName: viewer.name,
    githubAvatarUrl: viewer.avatar_url,
    githubToken: token,
  });

  const response = NextResponse.redirect(publicAppUrl("/app"));
  response.cookies.set(SESSION_COOKIE, sessionToken, authCookieOptions());
  response.cookies.set(OAUTH_STATE_COOKIE, "", authCookieOptions(0));
  return response;
}
