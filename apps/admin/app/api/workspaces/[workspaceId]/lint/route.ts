import { NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import { getWorkspaceForUser } from "@/lib/db";
import { lintWorkspace } from "@/lib/rototo";

export const runtime = "nodejs";

export async function GET(
  _request: Request,
  context: { params: Promise<{ workspaceId: string }> },
) {
  const user = await currentUser();
  if (!user) {
    return NextResponse.json({ error: "unauthorized" }, { status: 401 });
  }

  const { workspaceId } = await context.params;
  const workspace = getWorkspaceForUser(workspaceId, user.githubUserId);
  if (!workspace) {
    return NextResponse.json({ error: "workspace not found" }, { status: 404 });
  }

  try {
    const lint = await lintWorkspace(workspace, user.githubToken);
    return NextResponse.json({ workspace, lint });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return NextResponse.json({ error: message }, { status: 500 });
  }
}
