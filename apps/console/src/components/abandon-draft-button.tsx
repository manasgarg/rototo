import { GitBranch } from "lucide-react";
import { useState } from "react";

import { apiFetch } from "@/lib/api";
import { useRouter } from "@/lib/navigation";

export function AbandonDraftButton({
    branch,
    disabled,
    draftId,
    workspaceId,
}: {
    branch: string;
    disabled?: boolean;
    draftId: string;
    workspaceId: string;
}) {
    const router = useRouter();
    const [pending, setPending] = useState(false);
    const [message, setMessage] = useState<string | null>(null);

    async function abandon() {
        if (
            !window.confirm(
                `Let go of draft branch ${branch}? The GitHub branch will stay in the repository, but this console draft will close.`,
            )
        ) {
            return;
        }
        setPending(true);
        setMessage(null);
        try {
            const response = await apiFetch(
                `/api/workspaces/${workspaceId}/drafts/${draftId}/abandon`,
                { method: "POST" },
            );
            const body = (await response.json()) as { error?: string };
            if (!response.ok) {
                throw new Error(body.error ?? "failed to let go of draft");
            }
            router.push(`/app/workspaces/${workspaceId}/drafts`);
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
                className="btn btn-danger"
                disabled={disabled || pending}
                onClick={abandon}
                type="button"
            >
                {pending ? (
                    <span className="spin" />
                ) : (
                    <GitBranch aria-hidden size={15} />
                )}
                {pending ? "Letting go" : "Let go of branch"}
            </button>
            {message ? (
                <p className="form-note" data-tone="err">
                    {message}
                </p>
            ) : null}
        </div>
    );
}
