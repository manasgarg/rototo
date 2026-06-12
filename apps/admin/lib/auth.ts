import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import { deleteSession, getSession, type SessionUser } from "./db";

export const SESSION_COOKIE = "rototo_admin_session";
export const OAUTH_STATE_COOKIE = "rototo_admin_oauth_state";

export async function currentUser(): Promise<SessionUser | null> {
  const cookieStore = await cookies();
  return getSession(cookieStore.get(SESSION_COOKIE)?.value);
}

export async function requireUser(): Promise<SessionUser> {
  const user = await currentUser();
  if (!user) {
    redirect("/login");
  }
  return user;
}

export async function clearCurrentSession(): Promise<void> {
  const cookieStore = await cookies();
  const sessionToken = cookieStore.get(SESSION_COOKIE)?.value;
  if (sessionToken) {
    deleteSession(sessionToken);
  }
}

export function authCookieOptions(maxAge?: number) {
  return {
    httpOnly: true,
    sameSite: "lax" as const,
    secure: process.env.NODE_ENV === "production",
    path: "/",
    ...(maxAge === undefined ? {} : { maxAge }),
  };
}
