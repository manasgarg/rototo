"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import { Trash2 } from "lucide-react";

export function RemoveRepoButton({
  repoId,
  repoName,
}: {
  repoId: string;
  repoName: string;
}) {
  const router = useRouter();
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function remove() {
    if (
      !window.confirm(
        `Remove ${repoName} from the console? Its workspaces and drafts disappear here; the GitHub repository is untouched.`,
      )
    ) {
      return;
    }
    setPending(true);
    setMessage(null);
    try {
      const response = await fetch(`/api/repos/${repoId}`, { method: "DELETE" });
      const body = (await response.json()) as { error?: string };
      if (!response.ok) {
        throw new Error(body.error ?? "failed to remove repository");
      }
      router.refresh();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
      setPending(false);
    }
  }

  return (
    <>
      {message ? (
        <span className="form-note" data-tone="err">
          {message}
        </span>
      ) : null}
      <button
        className="btn btn-ghost btn-icon btn-remove"
        disabled={pending}
        onClick={remove}
        title={`Remove ${repoName}`}
        type="button"
      >
        {pending ? <span className="spin" /> : <Trash2 aria-hidden size={15} />}
      </button>
    </>
  );
}
