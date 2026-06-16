import { useRouter } from "@/lib/navigation";
import { useState } from "react";
import { Trash2 } from "lucide-react";
import { apiFetch } from "@/lib/api";

export function RemoveSourceTreeButton({
    sourceTreeId,
    sourceTreeName,
}: {
    sourceTreeId: string;
    sourceTreeName: string;
}) {
    const router = useRouter();
    const [pending, setPending] = useState(false);
    const [message, setMessage] = useState<string | null>(null);

    async function remove() {
        if (
            !window.confirm(
                `Remove ${sourceTreeName} from the console? Its workspaces and branches disappear here; the GitHub repository is untouched.`,
            )
        ) {
            return;
        }
        setPending(true);
        setMessage(null);
        try {
            const response = await apiFetch(`/api/source-trees/${sourceTreeId}`, {
                method: "DELETE",
            });
            const body = (await response.json()) as { error?: string };
            if (!response.ok) {
                throw new Error(body.error ?? "failed to remove source tree");
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
                title={`Remove ${sourceTreeName}`}
                type="button"
            >
                {pending ? (
                    <span className="spin" />
                ) : (
                    <Trash2 aria-hidden size={15} />
                )}
            </button>
        </>
    );
}
