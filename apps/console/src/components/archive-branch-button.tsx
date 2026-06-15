import { GitBranch } from "lucide-react";
import { useState } from "react";

import { apiFetch } from "@/lib/api";
import { useRouter } from "@/lib/navigation";

export function ArchiveBranchButton({
    branch,
    disabled,
    branchId,
    workspaceId,
}: {
    branch: string;
    disabled?: boolean;
    branchId: string;
    workspaceId: string;
}) {
    const router = useRouter();
    const [pending, setPending] = useState(false);
    const [message, setMessage] = useState<string | null>(null);

    async function archive() {
        if (
            !window.confirm(
                `Archive branch ${branch}? The GitHub branch will stay in the repository.`,
            )
        ) {
            return;
        }
        setPending(true);
        setMessage(null);
        try {
            const response = await apiFetch(
                `/api/workspaces/${workspaceId}/branches/${branchId}/archive`,
                { method: "POST" },
            );
            const body = (await response.json()) as { error?: string };
            if (!response.ok) {
                throw new Error(body.error ?? "failed to archive branch");
            }
            router.push(`/app/workspaces/${workspaceId}/branches`);
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
                onClick={archive}
                type="button"
            >
                {pending ? (
                    <span className="spin" />
                ) : (
                    <GitBranch aria-hidden size={15} />
                )}
                {pending ? "Archiving" : "Archive branch"}
            </button>
            {message ? (
                <p className="form-note" data-tone="err">
                    {message}
                </p>
            ) : null}
        </div>
    );
}
