type DraftLike = {
    status: string;
    prState: string | null;
};

export function DraftStatusPill({ draft }: { draft: DraftLike }) {
    if (draft.status === "abandoned") {
        return <Pill label="let go" tone="neutral" />;
    }
    if (draft.status === "published") {
        const state = draft.prState ?? "published";
        if (state === "merged") {
            return <Pill label="merged" tone="ok" />;
        }
        if (state === "closed") {
            return <Pill label="pr closed" tone="err" />;
        }
        if (state === "open") {
            return <Pill label="pr open" tone="info" />;
        }
        return <Pill label={state} tone="info" />;
    }
    return <Pill label="draft" tone="sea" />;
}

export function Pill({
    label,
    tone,
}: {
    label: string;
    tone: "ok" | "warn" | "err" | "info" | "neutral" | "sea";
}) {
    return (
        <span className={`pill pill-${tone}`}>
            <span className="d" />
            {label}
        </span>
    );
}
