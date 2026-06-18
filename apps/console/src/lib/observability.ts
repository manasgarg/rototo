/** Development observability event posted to the server NDJSON sink. */
type ConsoleEvent = {
    kind: string;
    [key: string]: unknown;
};

let disabled = false;

export function recordConsoleEvent(event: ConsoleEvent): void {
    if (disabled || !import.meta.env.DEV) {
        return;
    }
    const body = JSON.stringify({
        ...event,
        pagePath: window.location.pathname,
        href: window.location.href,
        userAgent: window.navigator.userAgent,
        at: new Date().toISOString(),
    });
    void fetch("/api/dev/observability/events", {
        method: "POST",
        keepalive: true,
        headers: {
            "content-type": "application/json",
            "x-rototo-console": "1",
        },
        body,
    }).then(
        (response) => {
            if (response.status === 404) {
                disabled = true;
            }
        },
        () => {},
    );
}

export function installGlobalErrorTelemetry(): void {
    window.addEventListener("error", (event) => {
        recordConsoleEvent({
            kind: "frontend-error",
            message: event.message,
            source: event.filename,
            line: event.lineno,
            column: event.colno,
            error: describeError(event.error),
        });
    });
    window.addEventListener("unhandledrejection", (event) => {
        recordConsoleEvent({
            kind: "unhandled-rejection",
            reason: describeError(event.reason),
        });
    });
}

export function describeError(error: unknown): unknown {
    if (error instanceof Error) {
        return {
            name: error.name,
            message: error.message,
            stack: error.stack,
        };
    }
    return String(error);
}
