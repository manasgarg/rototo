"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import { ExternalLink, GitPullRequest } from "lucide-react";

export function PublishDraftButton({
  workspaceId,
  draftId,
  disabled,
}: {
  workspaceId: string;
  draftId: string;
  disabled?: boolean;
}) {
  const router = useRouter();
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [pullRequestUrl, setPullRequestUrl] = useState<string | null>(null);

  if (pullRequestUrl) {
    return (
      <div className="applied-row">
        <a className="btn btn-primary" href={pullRequestUrl} rel="noreferrer" target="_blank">
          <ExternalLink aria-hidden size={15} />
          Open pull request
        </a>
        <p className="form-note" data-tone="ok">
          Published — nice. Review it on GitHub.
        </p>
      </div>
    );
  }

  async function publish() {
    setPending(true);
    setMessage(null);
    try {
      const response = await fetch(`/api/workspaces/${workspaceId}/drafts/${draftId}/publish`, {
        method: "POST",
      });
      const body = (await response.json()) as {
        pullRequest?: { html_url: string };
        error?: string;
      };
      if (!response.ok || !body.pullRequest) {
        throw new Error(body.error ?? "failed to publish draft");
      }
      setPullRequestUrl(body.pullRequest.html_url);
      router.refresh();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setPending(false);
    }
  }

  return (
    <div className="action-row">
      <button
        className="btn btn-primary"
        disabled={disabled || pending}
        onClick={publish}
        type="button"
      >
        {pending ? <span className="spin" /> : <GitPullRequest aria-hidden size={15} />}
        {pending ? "Publishing" : "Publish as pull request"}
      </button>
      {message ? (
        <p className="form-note" data-tone="err">
          {message}
        </p>
      ) : null}
    </div>
  );
}
