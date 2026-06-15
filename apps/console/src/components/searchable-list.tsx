import {
    Children,
    ReactNode,
    isValidElement,
    useEffect,
    useMemo,
    useRef,
    useState,
} from "react";
import { Search } from "lucide-react";
import { shouldAutoFocus } from "./autofocus";

/** Props for one client-side searchable list instance. */
type SearchableListProps = {
    label: string;
    placeholder: string;
    children: ReactNode;
    className: string;
    emptyLabel: string;
};

export function SearchableList({
    label,
    placeholder,
    children,
    className,
    emptyLabel,
}: SearchableListProps) {
    const [query, setQuery] = useState("");
    const inputRef = useRef<HTMLInputElement>(null);
    const items = useMemo(() => Children.toArray(children), [children]);

    useEffect(() => {
        if (shouldAutoFocus()) {
            inputRef.current?.focus({ preventScroll: true });
        }
    }, []);
    const needle = query.trim().toLowerCase();
    const visibleItems = needle
        ? items.filter((item) => searchableText(item).includes(needle))
        : items;

    return (
        <div className="searchable-list">
            <label className="search-control">
                <span className="search-icon">
                    <Search aria-hidden size={15} />
                </span>
                <input
                    aria-label={label}
                    className="input"
                    onChange={(event) => setQuery(event.target.value)}
                    placeholder={placeholder}
                    ref={inputRef}
                    type="search"
                    value={query}
                />
            </label>
            {visibleItems.length === 0 ? (
                <div className="empty-state">
                    <span className="empty-puck">
                        <Search aria-hidden size={18} />
                    </span>
                    <p>{emptyLabel}</p>
                </div>
            ) : (
                <div className={className}>{visibleItems}</div>
            )}
        </div>
    );
}

function searchableText(item: ReactNode): string {
    if (!isValidElement<{ "data-search"?: unknown }>(item)) {
        return "";
    }
    return String(item.props["data-search"] ?? "").toLowerCase();
}
