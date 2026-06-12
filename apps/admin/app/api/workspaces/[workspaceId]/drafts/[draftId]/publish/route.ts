import { NextRequest, NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import {
  getDraftSessionForUser,
  getWorkspaceForUser,
  listDraftChanges,
  markDraftPublished,
} from "@/lib/db";
import { createGitHubPullRequest, githubErrorMessage } from "@/lib/github";
import { lintWorkspace } from "@/lib/rototo";
import { draftLintTarget, draftPrBody, draftPrTitle } from "@/lib/workspace-edit";

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
  if (draft.status !== "open") {
    return NextResponse.json({ error: "draft is already published" }, { status: 400 });
  }

  try {
    const changes = listDraftChanges(draft.id);
    if (changes.length === 0) {
      return NextResponse.json({ error: "draft has no tracked changes" }, { status: 400 });
    }
    const lint = await lintWorkspace(draftLintTarget(workspace, draft), user.githubToken);
    const errors = lint.diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
    if (errors > 0) {
      return NextResponse.json(
        { error: `draft has ${errors} lint error(s); fix lint before publishing` },
        { status: 400 },
      );
    }

    const pr = await createGitHubPullRequest({
      token: user.githubToken,
      owner: workspace.owner,
      name: workspace.name,
      title: draftPrTitle(workspace),
      body: draftPrBody({
        workspace,
        draft,
        changes,
        diagnostics: lint.diagnostics,
      }),
      head: draft.branch,
      base: draft.baseRef,
    });
    markDraftPublished({
      draftId: draft.id,
      prNumber: pr.number,
      prState: pr.merged_at ? "merged" : (pr.state ?? "open"),
      prUrl: pr.html_url,
    });
    return NextResponse.json({ pullRequest: pr });
  } catch (error) {
    const message = githubErrorMessage(error, "Publishing the draft");
    return NextResponse.json({ error: message }, { status: 400 });
  }
}
