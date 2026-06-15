import { useEffect, useRef, useState } from "react";
import { Github, KeyRound, TerminalSquare, TriangleAlert } from "lucide-react";
import { Navigate } from "react-router";

import { RototoMark } from "@/components/rototo-mark";
import { api } from "@/lib/api";
import { useMe } from "@/lib/me";

/** Device-flow start payload kept while local sign-in polling is active. */
type DeviceStart = {
    userCode: string;
    verificationUri: string;
    intervalSeconds: number;
};

export function LoginScreen() {
    const { me, loading, reload } = useMe();
    const [device, setDevice] = useState<DeviceStart | null>(null);
    const [deviceError, setDeviceError] = useState<string | null>(null);
    const [starting, setStarting] = useState(false);
    const polling = useRef(false);

    useEffect(() => {
        if (!device || polling.current) {
            return;
        }
        polling.current = true;
        let cancelled = false;
        let delay = device.intervalSeconds * 1000;
        const poll = async () => {
            while (!cancelled) {
                await new Promise((resolve) => setTimeout(resolve, delay));
                if (cancelled) {
                    return;
                }
                try {
                    const result = await api<{ status: string }>(
                        "/api/auth/device/poll",
                        {
                            method: "POST",
                            body: "{}",
                        },
                    );
                    if (result.status === "authorized") {
                        reload();
                        return;
                    }
                    if (result.status === "slow-down") {
                        delay += 5000;
                    }
                } catch (failure) {
                    if (!cancelled) {
                        setDeviceError(
                            failure instanceof Error
                                ? failure.message
                                : String(failure),
                        );
                        setDevice(null);
                    }
                    return;
                }
            }
        };
        void poll().finally(() => {
            polling.current = false;
        });
        return () => {
            cancelled = true;
        };
    }, [device, reload]);

    if (loading) {
        return null;
    }
    if (me?.user) {
        return <Navigate replace to="/app" />;
    }

    const startDeviceFlow = async () => {
        setStarting(true);
        setDeviceError(null);
        try {
            setDevice(
                await api<DeviceStart>("/api/auth/device/start", {
                    method: "POST",
                    body: "{}",
                }),
            );
        } catch (failure) {
            setDeviceError(
                failure instanceof Error ? failure.message : String(failure),
            );
        } finally {
            setStarting(false);
        }
    };

    return (
        <main className="login-page">
            <section className="login-panel">
                <div className="brand">
                    <span className="brand-mark">
                        <RototoMark size={30} />
                    </span>
                    <span className="brand-name">rototo</span>
                    <span className="brand-tag label">console</span>
                </div>
                <div className="section-header-text">
                    <h1 className="login-title">
                        {me?.deployment === "hosted"
                            ? "Sign in"
                            : "Connect GitHub"}
                    </h1>
                    <p className="hint">
                        The rototo console reads workspaces from the GitHub
                        repositories your account can already access. Edits land
                        on draft branches and ship as pull requests — nothing
                        merges without review.
                    </p>
                </div>

                {me?.authError ? (
                    <div className="banner banner-err">
                        <TriangleAlert aria-hidden size={16} />
                        <span>{me.authError}</span>
                    </div>
                ) : null}
                {deviceError ? (
                    <div className="banner banner-err">
                        <TriangleAlert aria-hidden size={16} />
                        <span>{deviceError}</span>
                    </div>
                ) : null}

                {me?.deployment === "hosted" ? (
                    <a
                        className="btn btn-primary"
                        href="/api/auth/github/start"
                    >
                        <Github aria-hidden size={16} />
                        Continue with GitHub
                    </a>
                ) : null}

                {me?.deployment === "local" ? (
                    device ? (
                        <div className="card">
                            <div className="card-head-text">
                                <h3>Authorize this console</h3>
                                <p className="hint">
                                    Open{" "}
                                    <a
                                        href={device.verificationUri}
                                        rel="noreferrer"
                                        target="_blank"
                                    >
                                        {device.verificationUri}
                                    </a>{" "}
                                    and enter the code below. This screen
                                    continues automatically.
                                </p>
                            </div>
                            <div className="stat-card">
                                <span className="label">device code</span>
                                <span className="stat-value mono">
                                    {device.userCode}
                                </span>
                            </div>
                        </div>
                    ) : (
                        <>
                            {me.deviceFlow ? (
                                <button
                                    className="btn btn-primary"
                                    disabled={starting}
                                    onClick={startDeviceFlow}
                                    type="button"
                                >
                                    <Github aria-hidden size={16} />
                                    {starting
                                        ? "Starting…"
                                        : "Sign in with GitHub (device code)"}
                                </button>
                            ) : null}
                            <div className="card">
                                <div className="card-head-text">
                                    <h3>Or supply a token directly</h3>
                                </div>
                                <div className="spec">
                                    <div className="spec-row">
                                        <span>
                                            <KeyRound aria-hidden size={13} />{" "}
                                            <span className="mono">
                                                ROTOTO_WORKSPACE_TOKEN=&lt;token&gt;
                                                rototo console
                                            </span>
                                        </span>
                                    </div>
                                    <div className="spec-row">
                                        <span>
                                            <TerminalSquare
                                                aria-hidden
                                                size={13}
                                            />{" "}
                                            <span className="mono">
                                                gh auth login
                                            </span>{" "}
                                            — the console picks up the GitHub
                                            CLI&apos;s token on restart.
                                        </span>
                                    </div>
                                </div>
                            </div>
                        </>
                    )
                ) : null}
            </section>
        </main>
    );
}
