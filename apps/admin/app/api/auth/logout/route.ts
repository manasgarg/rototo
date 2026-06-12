import { NextRequest, NextResponse } from "next/server";
import { authCookieOptions, clearCurrentSession, SESSION_COOKIE } from "@/lib/auth";
import { publicAppUrl } from "@/lib/github";

export const runtime = "nodejs";

export async function POST(request: NextRequest) {
  await clearCurrentSession();
  const response = NextResponse.redirect(publicAppUrl("/login"));
  response.cookies.set(SESSION_COOKIE, "", authCookieOptions(0));
  return response;
}
