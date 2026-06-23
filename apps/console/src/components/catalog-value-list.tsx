import { ChevronRight, Database } from "lucide-react";

import { Link } from "@/lib/link";

export type CatalogValueListItem = {
    key: string;
    href: string;
    value: unknown;
};

export function CatalogValueList({ items }: { items: CatalogValueListItem[] }) {
    return (
        <div className="row-list">
            {items.map((item) => {
                const summary = catalogValueSummary(item.value);
                return (
                    <Link className="row" href={item.href} key={item.key}>
                        <span className="row-icon">
                            <Database aria-hidden size={16} />
                        </span>
                        <span className="row-text">
                            <span className="row-title mono">{item.key}</span>
                            <span className="row-sub mono">{summary.text}</span>
                        </span>
                        <span className="row-side">
                            <span className="tag">{summary.badge}</span>
                            <ChevronRight
                                aria-hidden
                                className="muted"
                                size={15}
                            />
                        </span>
                    </Link>
                );
            })}
        </div>
    );
}

function catalogValueSummary(value: unknown): { text: string; badge: string } {
    if (value === undefined) {
        return { text: "Value details unavailable", badge: "unparsed" };
    }
    if (isObjectRecord(value)) {
        const fields = Object.entries(value);
        const preview = fields
            .slice(0, 4)
            .map(([key, fieldValue]) => `${key} = ${formatValue(fieldValue)}`)
            .join("  ·  ");
        const more = fields.length > 4 ? `  ·  +${fields.length - 4} more` : "";
        return {
            text:
                fields.length > 0 ? `${preview}${more}` : "No fields declared",
            badge: `${fields.length} ${
                fields.length === 1 ? "field" : "fields"
            }`,
        };
    }
    return { text: `value = ${formatValue(value)}`, badge: "value" };
}

function isObjectRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}

function formatValue(value: unknown): string {
    return JSON.stringify(value) ?? String(value);
}
