import { Link } from "@/lib/link";
import { Fragment, type ReactNode } from "react";
import { ArrowLeft, Pencil } from "lucide-react";
import { LogoutButton } from "@/components/logout-button";
import { MobileNav } from "@/components/mobile-nav";
import { RototoMark } from "@/components/rototo-mark";

/** Breadcrumb item rendered by the app shell for the current route. */
export type Crumb = {
    label: string;
    href?: string;
};

export function AppShell({
    actions,
    children,
    crumbs,
    editing,
    nav,
    title,
    user,
    wide,
}: {
    actions?: ReactNode;
    children: ReactNode;
    crumbs?: Crumb[];
    /* Branch editing mode: a persistent strip and a tinted surface so the
     user always knows their edits land on a branch. */
    editing?: { label: string; detail: string };
    nav: ReactNode;
    title: string;
    user: { githubLogin: string; githubAvatarUrl: string | null };
    wide?: boolean;
}) {
    return (
        <div className="shell">
            <aside className="sidebar">
                <Link className="brand" href="/app">
                    <span className="brand-mark">
                        <RototoMark />
                    </span>
                    <span className="brand-name">rototo</span>
                </Link>
                <nav className="side-nav">{nav}</nav>
                <div className="side-user">
                    {user.githubAvatarUrl ? (
                        // eslint-disable-next-line @next/next/no-img-element
                        <img
                            alt=""
                            className="avatar"
                            height={28}
                            src={user.githubAvatarUrl}
                            width={28}
                        />
                    ) : (
                        <span className="avatar-fallback">
                            {user.githubLogin.slice(0, 2)}
                        </span>
                    )}
                    <span className="side-user-name">{user.githubLogin}</span>
                    <LogoutButton />
                </div>
            </aside>
            <div className="main" data-mode={editing ? "editing" : undefined}>
                <header className="topbar">
                    <Link
                        className="topbar-brand"
                        href="/app"
                        title="rototo console"
                    >
                        <RototoMark size={24} />
                    </Link>
                    {crumbs && crumbs.length > 0 ? (
                        <div className="crumbs">
                            {crumbs.map((crumb, index) => (
                                <Fragment key={`${crumb.label}-${index}`}>
                                    {index > 0 ? (
                                        <span className="crumb-sep">›</span>
                                    ) : null}
                                    {crumb.href ? (
                                        <Link
                                            className="label"
                                            href={crumb.href}
                                        >
                                            {crumb.label}
                                        </Link>
                                    ) : (
                                        <span className="label">
                                            {crumb.label}
                                        </span>
                                    )}
                                </Fragment>
                            ))}
                        </div>
                    ) : null}
                    <div className="topbar-actions">{actions}</div>
                </header>
                {editing ? (
                    <div className="mode-strip">
                        <Pencil aria-hidden size={13} />
                        <span className="label mode-strip-label">
                            branch editing
                        </span>
                        <span className="mono mode-strip-branch">
                            {editing.label}
                        </span>
                        <span className="mode-strip-detail">
                            {editing.detail}
                        </span>
                    </div>
                ) : null}
                <MobileNav title={title}>{nav}</MobileNav>
                <main className="content">
                    <div
                        className={`content-inner ${
                            wide ? "content-inner-wide" : ""
                        }`}
                    >
                        {children}
                    </div>
                </main>
            </div>
        </div>
    );
}

export function NavGroupLabel({ children }: { children: ReactNode }) {
    return <div className="label nav-group-label">{children}</div>;
}

export function NavLink({
    active,
    count,
    href,
    icon,
    label,
}: {
    active: boolean;
    count?: number;
    href: string;
    icon: ReactNode;
    label: string;
}) {
    return (
        <Link className="nav-item" data-on={active} href={href}>
            {icon}
            <span className="nav-item-text">{label}</span>
            {count !== undefined ? (
                <span className="nav-count">{count}</span>
            ) : null}
        </Link>
    );
}

export function NavBack({ href, label }: { href: string; label: string }) {
    return (
        <Link className="nav-back" href={href}>
            <ArrowLeft aria-hidden size={14} />
            <span>{label}</span>
        </Link>
    );
}

export function NavContext({
    href,
    label,
    value,
}: {
    href?: string;
    label: string;
    value: string;
}) {
    return (
        <div className="nav-context">
            <span className="label">{label}</span>
            {href ? (
                <Link className="mono" href={href} title={value}>
                    {value}
                </Link>
            ) : (
                <span className="mono" title={value}>
                    {value}
                </span>
            )}
        </div>
    );
}
