import { useRef, type ReactNode } from "react";

export function MobileNav({ children, title }: { children: ReactNode; title: string }) {
  const ref = useRef<HTMLDetailsElement>(null);

  return (
    <details className="mobile-nav" ref={ref}>
      <summary>
        <span className="label">navigate</span>
        <strong>{title}</strong>
      </summary>
      <div
        className="mobile-nav-panel"
        onClick={(event) => {
          if (ref.current && (event.target as HTMLElement).closest("a")) {
            ref.current.open = false;
          }
        }}
      >
        <nav className="side-nav">{children}</nav>
      </div>
    </details>
  );
}
