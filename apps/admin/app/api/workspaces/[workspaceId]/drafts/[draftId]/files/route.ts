import { NextRequest, NextResponse } from "next/server";
import { currentUser } from "@/lib/auth";
import {
  getDraftSessionForUser,
  getWorkspaceForUser,
  recordDraftEvent,
} from "@/lib/db";
import {
  deleteGitHubFile,
  getGitHubFile,
  githubErrorMessage,
  updateGitHubFile,
} from "@/lib/github";
import { dropLspSessionsForDraft } from "@/lib/lsp-bridge";
import { invalidateStagedWorkspaces } from "@/lib/rototo";
import { draftLintTarget } from "@/lib/workspace-edit";

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
      filePath?: string;
      content?: string;
    };
    const filePath = body.filePath?.trim();
    if (!filePath || typeof body.content !== "string") {
      return NextResponse.json({ error: "filePath and content are required" }, { status: 400 });
    }
    if (!belongsToWorkspace(workspace.path, filePath)) {
      return NextResponse.json({ error: "file path does not belong to workspace" }, { status: 400 });
    }

    const file = await getGitHubFile({
      token: user.githubToken,
      owner: workspace.owner,
      name: workspace.name,
      path: filePath,
      ref: draft.branch,
    });
    if (file.content !== body.content) {
      await updateGitHubFile({
        token: user.githubToken,
        owner: workspace.owner,
        name: workspace.name,
        path: filePath,
        branch: draft.branch,
        sha: file.sha,
        content: body.content,
        message: `Update ${filePath}`,
      });
      recordDraftEvent({
        draftId: draft.id,
        kind: "file.updated",
        summary: `Updated ${filePath}`,
        detail: { filePath },
      });
      // Staged checkouts of the draft branch go stale after a commit.
      dropLspSessionsForDraft(draft.id);
      invalidateStagedWorkspaces(draftLintTarget(workspace, draft).source);
    }
    return NextResponse.json({ ok: true });
  } catch (error) {
    const message = githubErrorMessage(error, "Saving the draft file");
    return NextResponse.json({ error: message }, { status: 400 });
  }
}

export async function DELETE(
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
    const body = (await request.json()) as { filePath?: string };
    const filePath = body.filePath?.trim();
    if (!filePath) {
      return NextResponse.json({ error: "filePath is required" }, { status: 400 });
    }
    if (!belongsToWorkspace(workspace.path, filePath)) {
      return NextResponse.json({ error: "file path does not belong to workspace" }, { status: 400 });
    }

    const file = await getGitHubFile({
      token: user.githubToken,
      owner: workspace.owner,
      name: workspace.name,
      path: filePath,
      ref: draft.branch,
    });
    await deleteGitHubFile({
      token: user.githubToken,
      owner: workspace.owner,
      name: workspace.name,
      path: filePath,
      branch: draft.branch,
      sha: file.sha,
      message: `Delete ${filePath}`,
    });
    recordDraftEvent({
      draftId: draft.id,
      kind: "file.deleted",
      summary: `Deleted ${filePath}`,
      detail: { filePath },
    });
    dropLspSessionsForDraft(draft.id);
    invalidateStagedWorkspaces(draftLintTarget(workspace, draft).source);
    return NextResponse.json({ ok: true });
  } catch (error) {
    const message = githubErrorMessage(error, "Deleting the draft file");
    return NextResponse.json({ error: message }, { status: 400 });
  }
}

function belongsToWorkspace(workspacePath: string, filePath: string): boolean {
  if (filePath.startsWith("/") || filePath.split("/").includes("..")) {
    return false;
  }
  return workspacePath === "." || filePath.startsWith(`${workspacePath}/`);
}
