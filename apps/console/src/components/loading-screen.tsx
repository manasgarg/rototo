import { RototoMark } from "@/components/rototo-mark";

export function LoadingScreen() {
    return (
        <div className="shell">
            <aside className="sidebar">
                <div className="brand">
                    <span className="brand-mark">
                        <RototoMark />
                    </span>
                    <span className="brand-name">rototo</span>
                </div>
                <div className="side-nav">
                    <div
                        className="skeleton"
                        style={{ height: 12, width: 80, margin: "10px 8px" }}
                    />
                    {Array.from({ length: 6 }, (_, index) => (
                        <div
                            className="skeleton"
                            key={index}
                            style={{ height: 34 }}
                        />
                    ))}
                </div>
            </aside>
            <div className="main">
                <header className="topbar">
                    <div
                        className="skeleton"
                        style={{ height: 20, width: 220 }}
                    />
                </header>
                <main className="content">
                    <div className="content-inner">
                        <div
                            className="skeleton"
                            style={{ height: 28, width: 300 }}
                        />
                        <div
                            className="skeleton"
                            style={{ height: 16, width: 420 }}
                        />
                        <div className="skeleton" style={{ height: 120 }} />
                        <div className="skeleton" style={{ height: 64 }} />
                        <div className="skeleton" style={{ height: 64 }} />
                        <div className="skeleton" style={{ height: 64 }} />
                    </div>
                </main>
            </div>
        </div>
    );
}
