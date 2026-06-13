import { FormEvent, useState } from "react";
import { GitBranch } from "lucide-react";
import { useRouter } from "@/lib/navigation";
import { apiFetch } from "@/lib/api";

type FormNote = { tone: "ok" | "err"; text: string };

export function DraftBranchEditor({
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
    const [value, setValue] = useState(branch);
    const [pending, setPending] = useState(false);
    const [note, setNote] = useState<FormNote | null>(null);

    async function submit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault();
        setPending(true);
        setNote(null);
        try {
            const response = await apiFetch(
                `/api/workspaces/${workspaceId}/drafts/${draftId}`,
                {
                    method: "PATCH",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ branch: value }),
                },
            );
            const body = (await response.json()) as {
                error?: string;
                draft?: { branch?: string };
            };
            if (!response.ok) {
                throw new Error(body.error ?? "failed to update branch");
            }
            if (body.draft?.branch) {
                setValue(body.draft.branch);
            }
            setNote({ tone: "ok", text: "Branch renamed." });
            router.refresh();
        } catch (error) {
            setNote({
                tone: "err",
                text: error instanceof Error ? error.message : String(error),
            });
        } finally {
            setPending(false);
        }
    }

    return (
        <form className="inline-form" onSubmit={submit}>
            <input
                aria-label="Draft branch name"
                className="input mono"
                disabled={disabled || pending}
                onChange={(event) => setValue(event.target.value)}
                value={value}
            />
            <button
                className="btn btn-secondary"
                disabled={disabled || pending || value.trim() === branch}
                type="submit"
            >
                {pending ? (
                    <span className="spin" />
                ) : (
                    <GitBranch aria-hidden size={15} />
                )}
                {pending ? "Renaming" : "Rename"}
            </button>
            {note ? (
                <p className="form-note" data-tone={note.tone}>
                    {note.text}
                </p>
            ) : null}
        </form>
    );
}
