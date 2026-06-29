import { useRouter } from "@/lib/navigation";
import { useState } from "react";
import { GitBranchPlus } from "lucide-react";
import { apiFetch } from "@/lib/api";

export function StartBranchButton({
    packageId,
    disabled,
    disabledReason,
}: {
    packageId: string;
    disabled?: boolean;
    disabledReason?: string;
}) {
    const router = useRouter();
    const [pending, setPending] = useState(false);
    const [message, setMessage] = useState<string | null>(null);

    async function startBranch() {
        setPending(true);
        setMessage(null);
        try {
            const response = await apiFetch(
                `/api/packages/${packageId}/branches`,
                {
                    method: "POST",
                    body: "{}",
                },
            );
            const body = (await response.json()) as {
                branch?: { id: string };
                error?: string;
            };
            if (!response.ok || !body.branch) {
                throw new Error(body.error ?? "failed to start branch");
            }
            router.push(
                `/app/packages/${packageId}/branches/${body.branch.id}`,
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
                disabled={disabled || pending}
                onClick={startBranch}
                type="button"
            >
                {pending ? (
                    <span className="spin" />
                ) : (
                    <GitBranchPlus aria-hidden size={15} />
                )}
                {pending ? "Starting branch" : "Edit package"}
            </button>
            {disabled && disabledReason ? (
                <p className="form-note">{disabledReason}</p>
            ) : null}
        </div>
    );
}
