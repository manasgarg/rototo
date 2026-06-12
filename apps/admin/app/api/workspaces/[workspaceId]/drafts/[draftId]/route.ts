import { NextRequest, NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import { getDraftSessionForUser, getWorkspaceForUser, updateDraftBranch } from "@/lib/db";
import { githubErrorMessage, renameGitHubBranch } from "@/lib/github";

export const runtime = "nodejs";

export async function PATCH(
  request: NextRequest,
  context: { params: Promise<{ workspaceId: string; draftId: string }> },
) {
  const user = await currentUser();
  if (!user) {
    return NextResponse.json({ error: "unauthorized" }, { status: 401 });
  }

  const { workspaceId, draftId } = await context.params;
  const workspace = getWorkspaceForUser(workspaceId, user.githubUserId);
  if (!workspace) {
    return NextResponse.json({ error: "workspace not found" }, { status: 404 });
  }
  const draft = getDraftSessionForUser(draftId, workspace.id, user.githubUserId);
  if (!draft) {
    return NextResponse.json({ error: "draft not found" }, { status: 404 });
  }
  if (draft.status !== "open") {
    return NextResponse.json({ error: "draft is already published" }, { status: 400 });
  }

  try {
    const body = (await request.json()) as { branch?: string };
    const branch = body.branch?.trim();
    if (!branch) {
      return NextResponse.json({ error: "branch is required" }, { status: 400 });
    }
    if (branch === draft.branch) {
      return NextResponse.json({ draft });
    }
    const renamed = await renameGitHubBranch({
      token: user.githubToken,
      owner: workspace.owner,
      name: workspace.name,
      branch: draft.branch,
      newName: branch,
    });
    const updated = updateDraftBranch({
      draftId: draft.id,
      previousBranch: draft.branch,
      branch: renamed.name,
    });
    return NextResponse.json({ draft: updated });
  } catch (error) {
    const message = githubErrorMessage(error, "Renaming the draft branch");
    return NextResponse.json({ error: message }, { status: 400 });
  }
}
