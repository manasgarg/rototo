import { useRouter } from "@/lib/navigation";
import { useState } from "react";
import { GitBranchPlus } from "lucide-react";
import { apiFetch } from "@/lib/api";

export function StartDraftButton({ workspaceId }: { workspaceId: string }) {
    const router = useRouter();
    const [pending, setPending] = useState(false);
    const [message, setMessage] = useState<string | null>(null);

    async function startDraft() {
        setPending(true);
        setMessage(null);
        try {
            const response = await apiFetch(
                `/api/workspaces/${workspaceId}/drafts`,
                {
                    method: "POST",
                    body: "{}",
                },
            );
            const body = (await response.json()) as {
                draft?: { id: string };
                error?: string;
            };
            if (!response.ok || !body.draft) {
                throw new Error(body.error ?? "failed to start draft");
            }
            router.push(
                `/app/workspaces/${workspaceId}/drafts/${body.draft.id}`,
            );
        } catch (error) {
            setMessage(error instanceof Error ? error.message : String(error));
            setPending(false);
        }
    }

    return (
        <div className="action-row">
            {message ? (
                <p className="form-note" data-tone="err">
                    {message}
                </p>
            ) : null}
            <button
                className="btn btn-primary btn-sm"
                disabled={pending}
                onClick={startDraft}
                type="button"
            >
                {pending ? (
                    <span className="spin" />
                ) : (
                    <GitBranchPlus aria-hidden size={15} />
                )}
                {pending ? "Starting draft" : "Edit workspace"}
            </button>
        </div>
    );
}
