import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import type {
  DraftChangeRecord,
  DraftSessionRecord,
  WorkspaceRecord,
} from "./db";
import { workspaceArchiveSource, workspaceRepoPath } from "./github";
import { parsePrimitiveVariableFile, type PrimitiveVariableEdit } from "./variable-toml";

export type DraftLintTarget = WorkspaceRecord & {
  source: string;
  ref: string;
};

export async function listPrimitiveVariableEditsFromRoot(input: {
  workspace: WorkspaceRecord;
  root: string;
}): Promise<PrimitiveVariableEdit[]> {
  const variablesDir = join(input.root, "variables");
  let entries;
  try {
    entries = await readdir(variablesDir, { withFileTypes: true });
  } catch (error) {
    if (isNotFoundError(error)) {
      return [];
    }
    throw error;
  }

  const variables = await Promise.all(
    entries
      .filter((entry) => entry.isFile() && entry.name.endsWith(".toml"))
      .sort((left, right) => left.name.localeCompare(right.name))
      .map(async (entry) => {
        const relativePath = `variables/${entry.name}`;
        const filePath = workspaceRepoPath(input.workspace.path, relativePath);
        const text = await readFile(join(variablesDir, entry.name), "utf8");
        return parsePrimitiveVariableFile(filePath, text);
      }),
  );
  return variables.filter((variable): variable is PrimitiveVariableEdit => variable !== null);
}

export function draftLintTarget(
  workspace: WorkspaceRecord,
  draft: DraftSessionRecord,
): DraftLintTarget {
  // Local-path workspace sources only come from dev/test registrations and
  // have no remote branches to stage; lint the local workspace directly.
  if (!workspace.source.includes("://")) {
    return { ...workspace, ref: draft.branch };
  }
  return {
    ...workspace,
    ref: draft.branch,
    source: workspaceArchiveSource(workspace.owner, workspace.name, draft.branch, workspace.path),
  };
}

export function expectedVariableFilePath(workspace: WorkspaceRecord, variableId: string): string {
  return workspaceRepoPath(workspace.path, `variables/${variableId}.toml`);
}

export function draftPrTitle(workspace: WorkspaceRecord): string {
  const path = workspace.path === "." ? "root workspace" : workspace.path;
  return `Update rototo workspace ${path}`;
}

export function draftPrBody(input: {
  workspace: WorkspaceRecord;
  draft: DraftSessionRecord;
  changes: DraftChangeRecord[];
  diagnostics: { severity?: string }[];
}): string {
  const errors = input.diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  const warnings = input.diagnostics.filter(
    (diagnostic) => diagnostic.severity === "warning",
  ).length;
  const lintStatus =
    errors > 0 ? `${errors} error(s)` : warnings > 0 ? `${warnings} warning(s)` : "clean";
  const changeLines =
    input.changes.length === 0
      ? ["- No tracked semantic changes."]
      : input.changes.map((change) => {
          const before = formatJsonSummary(change.beforeJson);
          const after = formatJsonSummary(change.afterJson);
          return `- variable \`${change.variableId}\` value \`${change.valueKey}\`: \`${before}\` -> \`${after}\``;
        });

  return [
    "## Rototo Admin",
    "",
    `Workspace: \`${input.workspace.owner}/${input.workspace.name}:${input.workspace.path}\``,
    `Base ref: \`${input.draft.baseRef}\``,
    `Draft branch: \`${input.draft.branch}\``,
    `Lint status: ${lintStatus}`,
    "",
    "## Semantic changes",
    "",
    ...changeLines,
  ].join("\n");
}

function formatJsonSummary(value: string): string {
  try {
    return JSON.stringify(JSON.parse(value));
  } catch {
    return value;
  }
}

function isNotFoundError(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  );
}
