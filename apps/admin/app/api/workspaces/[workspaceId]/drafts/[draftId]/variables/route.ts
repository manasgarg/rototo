import { NextRequest, NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import {
  getDraftSessionForUser,
  getWorkspaceForUser,
  recordDraftChange,
} from "@/lib/db";
import { getGitHubFile, githubErrorMessage, updateGitHubFile } from "@/lib/github";
import { dropLspSessionsForDraft } from "@/lib/lsp-bridge";
import { invalidateStagedWorkspaces } from "@/lib/rototo";
import { draftLintTarget, expectedVariableFilePath } from "@/lib/workspace-edit";
import { updatePrimitiveVariableDefault } from "@/lib/variable-toml";

export const runtime = "nodejs";

export async function POST(
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
    const body = (await request.json()) as {
      variableId?: string;
      filePath?: string;
      value?: string;
    };
    const variableId = body.variableId?.trim();
    const filePath = body.filePath?.trim();
    if (!variableId || !filePath || typeof body.value !== "string") {
      return NextResponse.json({ error: "variableId, filePath, and value are required" }, { status: 400 });
    }
    const expectedPath = expectedVariableFilePath(workspace, variableId);
    if (filePath !== expectedPath) {
      return NextResponse.json({ error: "variable file path does not match workspace" }, { status: 400 });
    }

    const file = await getGitHubFile({
      token: user.githubToken,
      owner: workspace.owner,
      name: workspace.name,
      path: filePath,
      ref: draft.branch,
    });
    const update = updatePrimitiveVariableDefault({
      filePath,
      text: file.content,
      value: body.value,
    });

    if (update.beforeLiteral !== update.afterLiteral) {
      await updateGitHubFile({
        token: user.githubToken,
        owner: workspace.owner,
        name: workspace.name,
        path: filePath,
        branch: draft.branch,
        sha: file.sha,
        content: update.text,
        message: `Update ${variableId} default value`,
      });
      // Staged checkouts of the draft branch go stale after a commit.
      dropLspSessionsForDraft(draft.id);
      invalidateStagedWorkspaces(draftLintTarget(workspace, draft).source);
    }

    const change = recordDraftChange({
      draftId: draft.id,
      filePath,
      variableId,
      valueKey: update.valueKey,
      before: update.before,
      after: update.after,
    });
    return NextResponse.json({ change });
  } catch (error) {
    const message = githubErrorMessage(error, "Saving the draft change");
    return NextResponse.json({ error: message }, { status: 400 });
  }
}
