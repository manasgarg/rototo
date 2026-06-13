import { useState } from "react";
import { RefreshCw } from "lucide-react";
import { useRouter } from "@/lib/navigation";
import { apiFetch } from "@/lib/api";

export function SyncPrButton({
  draftId,
  workspaceId,
}: {
  draftId: string;
  workspaceId: string;
}) {
  const router = useRouter();
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function sync() {
    setPending(true);
    setMessage(null);
    try {
      const response = await apiFetch(
        `/api/workspaces/${workspaceId}/drafts/${draftId}/sync-pr`,
        { method: "POST" },
      );
      const body = (await response.json()) as { error?: string };
      if (!response.ok) {
        throw new Error(body.error ?? "failed to sync pull request");
      }
      router.refresh();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setPending(false);
    }
  }

  return (
    <div className="action-row">
      <button className="btn btn-secondary" disabled={pending} onClick={sync} type="button">
        {pending ? <span className="spin" /> : <RefreshCw aria-hidden size={15} />}
        {pending ? "Syncing" : "Sync state"}
      </button>
      {message ? (
        <p className="form-note" data-tone="err">
          {message}
        </p>
      ) : null}
    </div>
  );
}
