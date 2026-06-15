import { useRouter } from "@/lib/navigation";
import { useState } from "react";
import { ExternalLink, GitPullRequest } from "lucide-react";
import { apiFetch } from "@/lib/api";

export function PublishBranchButton({
    workspaceId,
    branchId,
    disabled,
    writeKind = "pullRequest",
}: {
    workspaceId: string;
    branchId: string;
    disabled?: boolean;
    writeKind?: "disabled" | "pullRequest" | "directPush";
}) {
    const router = useRouter();
    const [pending, setPending] = useState(false);
    const [message, setMessage] = useState<string | null>(null);
    const [pullRequestUrl, setPullRequestUrl] = useState<string | null>(null);
    const [directPublished, setDirectPublished] = useState(false);

    if (pullRequestUrl) {
        return (
            <div className="applied-row">
                <a
                    className="btn btn-primary"
                    href={pullRequestUrl}
                    rel="noreferrer"
                    target="_blank"
                >
                    <ExternalLink aria-hidden size={15} />
                    Open pull request
                </a>
                <p className="form-note" data-tone="ok">
                    Published — nice. Review it on GitHub.
                </p>
            </div>
        );
    }
    if (directPublished) {
        return (
            <div className="applied-row">
                <p className="form-note" data-tone="ok">
                    Published by direct push.
                </p>
            </div>
        );
    }

    async function publish() {
        setPending(true);
        setMessage(null);
        try {
            const response = await apiFetch(
                `/api/workspaces/${workspaceId}/branches/${branchId}/publish`,
                {
                    method: "POST",
                },
            );
            const body = (await response.json()) as {
                pullRequest?: { html_url: string };
                directPush?: unknown;
                error?: string;
            };
            if (!response.ok || (!body.pullRequest && !body.directPush)) {
                throw new Error(body.error ?? "failed to publish branch");
            }
            if (body.pullRequest) {
                setPullRequestUrl(body.pullRequest.html_url);
            } else {
                setDirectPublished(true);
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
            <button
                className="btn btn-primary"
                disabled={disabled || pending}
                onClick={publish}
                type="button"
            >
                {pending ? (
                    <span className="spin" />
                ) : (
                    <GitPullRequest aria-hidden size={15} />
                )}
                {pending
                    ? "Publishing"
                    : writeKind === "directPush"
                      ? "Publish by direct push"
                      : "Publish as pull request"}
            </button>
            {message ? (
                <p className="form-note" data-tone="err">
                    {message}
                </p>
            ) : null}
        </div>
    );
}
