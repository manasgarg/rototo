import { createHash } from "node:crypto";

const GITHUB_USER_AGENT = "rototo-admin";

export type GitHubUser = {
  id: number;
  login: string;
  name: string | null;
  avatar_url: string | null;
};

export type GitHubRepo = {
  id: number;
  name: string;
  full_name: string;
  owner: { login: string };
  default_branch: string;
  private: boolean;
  permissions?: {
    admin?: boolean;
    maintain?: boolean;
    push?: boolean;
    pull?: boolean;
    triage?: boolean;
  };
};

export type DiscoveredWorkspace = {
  path: string;
  ref: string;
  source: string;
};

export type GitHubContentFile = {
  path: string;
  sha: string;
  content: string;
};

export type GitHubPullRequest = {
  html_url: string;
  number: number;
  state?: "open" | "closed";
  merged_at?: string | null;
  updated_at?: string;
};

type GitHubTreeResponse = {
  truncated: boolean;
  tree: Array<{
    path: string;
    type: "blob" | "tree" | "commit";
  }>;
};

type GitHubRefResponse = {
  object: {
    sha: string;
    type: string;
  };
};

type GitHubContentResponse = {
  type: string;
  path: string;
  sha: string;
  encoding?: string;
  content?: string;
};

export class GitHubApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly responseText: string,
  ) {
    super(`GitHub API ${status}: ${responseText.slice(0, 300)}`);
    this.name = "GitHubApiError";
  }

  responseMessage(): string | null {
    try {
      const body = JSON.parse(this.responseText) as { message?: unknown };
      return typeof body.message === "string" ? body.message : null;
    } catch {
      return null;
    }
  }
}

export function parseRepoSpec(value: string): { owner: string; name: string } {
  const trimmed = value.trim();
  const match = /^([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)$/.exec(trimmed);
  if (!match) {
    throw new Error("repo must be in owner/name form");
  }
  return { owner: match[1], name: match[2] };
}

export async function exchangeGitHubCode(code: string): Promise<string> {
  const clientId = process.env.GITHUB_CLIENT_ID;
  const clientSecret = process.env.GITHUB_CLIENT_SECRET;
  if (!clientId || !clientSecret) {
    throw new Error("GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET are required");
  }

  const response = await fetch("https://github.com/login/oauth/access_token", {
    method: "POST",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
      "User-Agent": GITHUB_USER_AGENT,
    },
    body: JSON.stringify({
      client_id: clientId,
      client_secret: clientSecret,
      code,
    }),
  });
  const body = (await response.json()) as {
    access_token?: string;
    error?: string;
    error_description?: string;
  };
  if (!response.ok || !body.access_token) {
    throw new Error(body.error_description ?? body.error ?? "GitHub OAuth failed");
  }
  return body.access_token;
}

export async function getGitHubViewer(token: string): Promise<GitHubUser> {
  return githubApi<GitHubUser>(token, "/user");
}

export async function getGitHubRepo(
  token: string,
  owner: string,
  name: string,
): Promise<GitHubRepo> {
  return githubApi<GitHubRepo>(
    token,
    `/repos/${encodeURIComponent(owner)}/${encodeURIComponent(name)}`,
  );
}

export async function assertGitHubRepoWriteAccess(input: {
  token: string;
  owner: string;
  name: string;
}): Promise<void> {
  const repo = await getGitHubRepo(input.token, input.owner, input.name);
  if (repo.permissions && !repo.permissions.push && !repo.permissions.admin) {
    throw new Error(
      `Your GitHub credential can read ${input.owner}/${input.name}, but cannot push to it. Grant repository write access before editing this workspace.`,
    );
  }
}

export async function discoverGitHubWorkspaces(input: {
  token: string;
  owner: string;
  name: string;
  ref: string;
}): Promise<DiscoveredWorkspace[]> {
  const tree = await githubApi<GitHubTreeResponse>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(
      input.name,
    )}/git/trees/${encodeURIComponent(input.ref)}?recursive=1`,
  );
  if (tree.truncated) {
    throw new Error("GitHub tree response was truncated; workspace discovery is incomplete");
  }
  return tree.tree
    .filter((entry) => entry.type === "blob" && entry.path.endsWith("rototo-workspace.toml"))
    .map((entry) => {
      const path = workspacePath(entry.path);
      return {
        path,
        ref: input.ref,
        source: workspaceArchiveSource(input.owner, input.name, input.ref, path),
      };
    })
    .sort((left, right) => left.path.localeCompare(right.path));
}

export async function getGitHubBranchHeadSha(input: {
  token: string;
  owner: string;
  name: string;
  branch: string;
}): Promise<string> {
  const ref = await githubApi<GitHubRefResponse>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(
      input.name,
    )}/git/ref/${encodeURIComponent(`heads/${input.branch}`)}`,
  );
  if (ref.object.type !== "commit") {
    throw new Error(`GitHub ref ${input.branch} does not point to a commit`);
  }
  return ref.object.sha;
}

export async function listGitHubBranches(input: {
  token: string;
  owner: string;
  name: string;
}): Promise<string[]> {
  const branches = await githubApi<Array<{ name: string }>>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(
      input.name,
    )}/branches?per_page=100`,
  );
  return branches.map((branch) => branch.name);
}

export async function compareGitHubRefs(input: {
  token: string;
  owner: string;
  name: string;
  base: string;
  head: string;
}): Promise<{ aheadBy: number; files: string[] }> {
  const comparison = await githubApi<{
    ahead_by: number;
    files?: Array<{ filename: string }>;
  }>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(
      input.name,
    )}/compare/${encodeURIComponent(input.base)}...${encodeURIComponent(input.head)}`,
  );
  return {
    aheadBy: comparison.ahead_by,
    files: (comparison.files ?? []).map((file) => file.filename),
  };
}

export async function createGitHubBranch(input: {
  token: string;
  owner: string;
  name: string;
  branch: string;
  sha: string;
}): Promise<void> {
  await githubApi<unknown>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(input.name)}/git/refs`,
    {
      method: "POST",
      body: JSON.stringify({
        ref: `refs/heads/${input.branch}`,
        sha: input.sha,
      }),
    },
  );
}

export async function renameGitHubBranch(input: {
  token: string;
  owner: string;
  name: string;
  branch: string;
  newName: string;
}): Promise<{ name: string }> {
  return githubApi<{ name: string }>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(
      input.name,
    )}/branches/${encodeURIComponent(input.branch)}/rename`,
    {
      method: "POST",
      body: JSON.stringify({ new_name: input.newName }),
    },
  );
}

export async function getGitHubFile(input: {
  token: string;
  owner: string;
  name: string;
  path: string;
  ref: string;
}): Promise<GitHubContentFile> {
  const file = await githubApi<GitHubContentResponse>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(
      input.name,
    )}/contents/${encodeRepoPath(input.path)}?ref=${encodeURIComponent(input.ref)}`,
  );
  if (file.type !== "file" || file.encoding !== "base64" || !file.content) {
    throw new Error(`GitHub path is not a readable file: ${input.path}`);
  }
  return {
    path: file.path,
    sha: file.sha,
    content: Buffer.from(file.content.replace(/\n/g, ""), "base64").toString("utf8"),
  };
}

export async function updateGitHubFile(input: {
  token: string;
  owner: string;
  name: string;
  path: string;
  branch: string;
  sha: string;
  content: string;
  message: string;
}): Promise<void> {
  await githubApi<unknown>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(
      input.name,
    )}/contents/${encodeRepoPath(input.path)}`,
    {
      method: "PUT",
      body: JSON.stringify({
        message: input.message,
        content: Buffer.from(input.content, "utf8").toString("base64"),
        sha: input.sha,
        branch: input.branch,
      }),
    },
  );
}

export async function createGitHubFile(input: {
  token: string;
  owner: string;
  name: string;
  path: string;
  branch: string;
  content: string;
  message: string;
}): Promise<void> {
  await githubApi<unknown>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(
      input.name,
    )}/contents/${encodeRepoPath(input.path)}`,
    {
      method: "PUT",
      body: JSON.stringify({
        message: input.message,
        content: Buffer.from(input.content, "utf8").toString("base64"),
        branch: input.branch,
      }),
    },
  );
}

export async function deleteGitHubFile(input: {
  token: string;
  owner: string;
  name: string;
  path: string;
  branch: string;
  sha: string;
  message: string;
}): Promise<void> {
  await githubApi<unknown>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(
      input.name,
    )}/contents/${encodeRepoPath(input.path)}`,
    {
      method: "DELETE",
      body: JSON.stringify({
        message: input.message,
        sha: input.sha,
        branch: input.branch,
      }),
    },
  );
}

export async function listGitHubTree(input: {
  token: string;
  owner: string;
  name: string;
  ref: string;
}): Promise<GitHubTreeResponse["tree"]> {
  const tree = await githubApi<GitHubTreeResponse>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(
      input.name,
    )}/git/trees/${encodeURIComponent(input.ref)}?recursive=1`,
  );
  if (tree.truncated) {
    throw new Error("GitHub tree response was truncated");
  }
  return tree.tree;
}

export async function createGitHubPullRequest(input: {
  token: string;
  owner: string;
  name: string;
  title: string;
  body: string;
  head: string;
  base: string;
}): Promise<GitHubPullRequest> {
  return githubApi<GitHubPullRequest>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(input.name)}/pulls`,
    {
      method: "POST",
      body: JSON.stringify({
        title: input.title,
        body: input.body,
        head: input.head,
        base: input.base,
        maintainer_can_modify: true,
      }),
    },
  );
}

export async function getGitHubPullRequest(input: {
  token: string;
  owner: string;
  name: string;
  number: number;
}): Promise<GitHubPullRequest> {
  return githubApi<GitHubPullRequest>(
    input.token,
    `/repos/${encodeURIComponent(input.owner)}/${encodeURIComponent(
      input.name,
    )}/pulls/${input.number}`,
  );
}

export async function githubApi<T>(
  token: string,
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const response = await fetch(`https://api.github.com${path}`, {
    ...init,
    headers: {
      Accept: "application/vnd.github+json",
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
      "User-Agent": GITHUB_USER_AGENT,
      "X-GitHub-Api-Version": "2022-11-28",
      ...init.headers,
    },
  });
  if (!response.ok) {
    const text = await response.text();
    throw new GitHubApiError(response.status, text);
  }
  return (await response.json()) as T;
}

export function githubErrorMessage(error: unknown, action: string): string {
  if (error instanceof GitHubApiError) {
    const message = error.responseMessage();
    if (error.status === 403 && message === "Resource not accessible by integration") {
      return [
        `${action} failed because the GitHub credential cannot write to this repository.`,
        "Use GitHub OAuth App credentials, make sure the user has write access to the repository, then log out and sign in again so the token is authorized with the repo scope.",
      ].join(" ");
    }
    return error.message;
  }
  return error instanceof Error ? error.message : String(error);
}

export function workspaceArchiveSource(
  owner: string,
  name: string,
  ref: string,
  path: string,
): string {
  const archive = `https://api.github.com/repos/${encodeURIComponent(owner)}/${encodeURIComponent(
    name,
  )}/tarball/${encodeURIComponent(ref)}`;
  return path === "." ? archive : `${archive}#:${path}`;
}

export function oauthBaseUrl(): string {
  return process.env.ROTOTO_ADMIN_BASE_URL ?? "http://localhost:3000";
}

export function publicAppUrl(path: string): URL {
  return new URL(path, oauthBaseUrl());
}

export function stableWorkspaceKey(owner: string, name: string, path: string): string {
  return createHash("sha256").update(`${owner}/${name}:${path}`).digest("hex").slice(0, 12);
}

export function workspaceRepoPath(workspacePath: string, relativePath: string): string {
  return workspacePath === "." ? relativePath : `${workspacePath}/${relativePath}`;
}

export function encodeRepoPath(path: string): string {
  return path.split("/").map(encodeURIComponent).join("/");
}

function workspacePath(manifestPath: string): string {
  const path = manifestPath.replace(/\/rototo-workspace\.toml$/, "");
  return path === "rototo-workspace.toml" || path === "" ? "." : path;
}
