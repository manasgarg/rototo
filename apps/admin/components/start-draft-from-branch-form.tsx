"use client";

import { useRouter } from "next/navigation";
import { FormEvent, useState } from "react";
import { GitBranch } from "lucide-react";

export function StartDraftFromBranchForm({ workspaceId }: { workspaceId: string }) {
  const router = useRouter();
  const [branch, setBranch] = useState("");
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setPending(true);
    setMessage(null);
    try {
      const response = await fetch(`/api/workspaces/${workspaceId}/drafts`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ branch }),
      });
      const body = (await response.json()) as {
        draft?: { id: string };
        error?: string;
      };
      if (!response.ok || !body.draft) {
        throw new Error(body.error ?? "failed to open the branch as a draft");
      }
      router.push(`/app/workspaces/${workspaceId}/drafts/${body.draft.id}`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
      setPending(false);
    }
  }

  return (
    <form className="card" onSubmit={submit}>
      <div className="card-head-text">
        <h3>Edit an existing branch</h3>
        <p className="hint">
          Open a draft on a branch that already exists in the repository — for example
          one created outside the console. Publishing still opens a pull request to the
          base ref. If the branch already has an open draft, you join it.
        </p>
      </div>
      <div className="inline-form">
        <input
          aria-label="Existing branch name"
          autoComplete="off"
          className="input mono"
          disabled={pending}
          onChange={(event) => setBranch(event.target.value)}
          placeholder="feature/checkout-copy"
          value={branch}
        />
        <button
          className="btn btn-secondary"
          disabled={pending || branch.trim() === ""}
          type="submit"
        >
          {pending ? <span className="spin" /> : <GitBranch aria-hidden size={15} />}
          {pending ? "Opening" : "Open draft"}
        </button>
        {message ? (
          <p className="form-note" data-tone="err">
            {message}
          </p>
        ) : null}
      </div>
    </form>
  );
}
