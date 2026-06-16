import { useState } from "react";
import { RefreshCcw } from "lucide-react";

import { apiFetch } from "@/lib/api";
import { useRouter } from "@/lib/navigation";

export function RefreshSourceTreeButton({
    sourceTreeId,
    sourceTreeName,
}: {
    sourceTreeId: string;
    sourceTreeName: string;
}) {
    const router = useRouter();
    const [pending, setPending] = useState(false);
    const [message, setMessage] = useState<string | null>(null);

    async function refresh() {
        setPending(true);
        setMessage(null);
        try {
            const response = await apiFetch(
                `/api/source-trees/${sourceTreeId}/refresh`,
                { method: "POST" },
            );
            const body = (await response.json()) as { error?: string };
            if (!response.ok) {
                throw new Error(body.error ?? "failed to refresh source tree");
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
                className="btn btn-ghost btn-icon"
                disabled={pending}
                onClick={refresh}
                title={`Refresh ${sourceTreeName}`}
                type="button"
            >
                {pending ? (
                    <span className="spin" />
                ) : (
                    <RefreshCcw aria-hidden size={15} />
                )}
            </button>
        </>
    );
}
