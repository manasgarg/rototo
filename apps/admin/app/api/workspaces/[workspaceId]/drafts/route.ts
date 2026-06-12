import { NextRequest, NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import {
  createDraftSession,
  getWorkspaceForUser,
  listDraftSessionsForWorkspace,
} from "@/lib/db";
import {
  assertGitHubRepoWriteAccess,
  createGitHubBranch,
  githubErrorMessage,
  getGitHubBranchHeadSha,
  stableWorkspaceKey,
} from "@/lib/github";

export const runtime = "nodejs";

export async function POST(
  request: NextRequest,
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

  let requestedBranch: string | null = null;
  try {
    const body = (await request.json()) as { branch?: string };
    requestedBranch = body.branch?.trim() || null;
  } catch {
    // no body: create a fresh draft branch
  }

  try {
    const baseRef = workspace.ref;
    if (requestedBranch === baseRef) {
      return NextResponse.json(
        {
          error: `Editing ${baseRef} directly would skip review. Pick another branch, or start a new draft.`,
        },
        { status: 400 },
      );
    }
    if (requestedBranch) {
      const existing = listDraftSessionsForWorkspace(workspace.id, user.githubUserId).find(
        (draft) => draft.branch === requestedBranch && draft.status === "open",
      );
      if (existing) {
        return NextResponse.json({ draft: existing });
      }
    }

    await assertGitHubRepoWriteAccess({
      token: user.githubToken,
      owner: workspace.owner,
      name: workspace.name,
    });

    if (requestedBranch) {
      // Confirms the branch exists; throws a not-found error otherwise.
      await getGitHubBranchHeadSha({
        token: user.githubToken,
        owner: workspace.owner,
        name: workspace.name,
        branch: requestedBranch,
      });
      const draft = createDraftSession({
        workspaceId: workspace.id,
        githubUserId: user.githubUserId,
        branch: requestedBranch,
        baseRef,
      });
      return NextResponse.json({ draft });
    }

    const baseSha = await getGitHubBranchHeadSha({
      token: user.githubToken,
      owner: workspace.owner,
      name: workspace.name,
      branch: baseRef,
    });
    const branch = draftBranchName({
      login: user.githubLogin,
      owner: workspace.owner,
      name: workspace.name,
      path: workspace.path,
    });
    await createGitHubBranch({
      token: user.githubToken,
      owner: workspace.owner,
      name: workspace.name,
      branch,
      sha: baseSha,
    });
    const draft = createDraftSession({
      workspaceId: workspace.id,
      githubUserId: user.githubUserId,
      branch,
      baseRef,
    });
    return NextResponse.json({ draft });
  } catch (error) {
    const message = githubErrorMessage(error, "Starting a draft");
    return NextResponse.json({ error: message }, { status: 400 });
  }
}

function draftBranchName(input: {
  login: string;
  owner: string;
  name: string;
  path: string;
}): string {
  const stamp = new Date().toISOString().replace(/\D/g, "").slice(0, 14);
  const login = input.login.toLowerCase().replace(/[^a-z0-9_.-]+/g, "-");
  const key = stableWorkspaceKey(input.owner, input.name, input.path);
  return `rototo-admin/${login}/${key}/${stamp}`;
}
