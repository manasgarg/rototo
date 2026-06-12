import { NextRequest, NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import { deleteRepoForUser } from "@/lib/db";

export const runtime = "nodejs";

export async function DELETE(
  _request: NextRequest,
  context: { params: Promise<{ repoId: string }> },
) {
  const user = await currentUser();
  if (!user) {
    return NextResponse.json({ error: "unauthorized" }, { status: 401 });
  }

  const { repoId } = await context.params;
  const removed = deleteRepoForUser(repoId, user.githubUserId);
  if (!removed) {
    return NextResponse.json({ error: "repository not found" }, { status: 404 });
  }
  return NextResponse.json({ ok: true });
}
