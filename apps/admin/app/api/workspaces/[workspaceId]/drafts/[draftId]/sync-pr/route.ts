import { NextRequest, NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import {
  getDraftSessionForUser,
  getWorkspaceForUser,
  updateDraftPullRequestState,
} from "@/lib/db";
import { getGitHubPullRequest, githubErrorMessage } from "@/lib/github";

export const runtime = "nodejs";

export async function POST(
  _request: NextRequest,
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
  const prNumber = draft.prNumber ?? pullRequestNumberFromUrl(draft.prUrl);
  if (!prNumber) {
    return NextResponse.json({ error: "draft does not have a pull request" }, { status: 400 });
  }

  try {
    const pr = await getGitHubPullRequest({
      token: user.githubToken,
      owner: workspace.owner,
      name: workspace.name,
      number: prNumber,
    });
    const updated = updateDraftPullRequestState({
      draftId: draft.id,
      prNumber: pr.number,
      prState: pullRequestState(pr.state, pr.merged_at ?? null),
      prUrl: pr.html_url,
      prMergedAt: pr.merged_at ?? null,
    });
    return NextResponse.json({ draft: updated });
  } catch (error) {
    const message = githubErrorMessage(error, "Syncing the pull request");
    return NextResponse.json({ error: message }, { status: 400 });
  }
}

function pullRequestNumberFromUrl(url: string | null): number | null {
  if (!url) {
    return null;
  }
  const match = /\/pull\/(\d+)(?:$|[/?#])/.exec(url);
  return match ? Number.parseInt(match[1], 10) : null;
}

function pullRequestState(state: string | undefined, mergedAt: string | null): string {
  if (mergedAt) {
    return "merged";
  }
  return state ?? "unknown";
}
