package dev.rototo;

import java.util.Map;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Consumer;

public final class RefreshingPackage implements AutoCloseable {
    private final AtomicLong handle;

    private RefreshingPackage(long handle) {
        this.handle = new AtomicLong(handle);
    }

    public static CompletableFuture<RefreshingPackage> load(String source) {
        return load(source, RefreshingPackageOptions.defaults());
    }

    public static CompletableFuture<RefreshingPackage> load(
            String source,
            RefreshingPackageOptions options) {
        Objects.requireNonNull(source, "source");
        RefreshingPackageOptions resolved =
                options == null ? RefreshingPackageOptions.defaults() : options;
        Double periodSeconds = resolved.periodSeconds();
        return CompletableFuture.supplyAsync(
                () -> new RefreshingPackage(Native.refreshingPackageLoadNative(
                        source,
                        periodSeconds == null ? 0.0 : periodSeconds,
                        periodSeconds != null,
                        resolved.packageToken(),
                        resolved.lint().wireValue(),
                        resolved.fallbackSource(),
                        resolved.packageTokens() == null
                                ? null
                                : Json.stringify(resolved.packageTokens()))),
                Rototo.executor());
    }

    public VariableResolution resolveVariable(
            String id,
            Map<String, ?> context) {
        return resolveVariable(id, context, ResolveOptions.defaults());
    }

    public VariableResolution resolveVariable(
            String id,
            Map<String, ?> context,
            ResolveOptions options) {
        Objects.requireNonNull(id, "id");
        Objects.requireNonNull(context, "context");
        ResolveOptions resolved = options == null ? ResolveOptions.defaults() : options;
        String json = Native.refreshingPackageResolveVariableNative(
                openHandle(),
                id,
                Json.stringify(context),
                resolved.validateContext(),
                resolved.trace());
        Map<String, Object> value = Json.asObject(Json.parse(json));
        return new VariableResolution(
                Json.asString(value.get("id")),
                value.get("value"),
                value.get("source"));
    }

    public CompletableFuture<String> refreshNow() {
        return CompletableFuture.supplyAsync(
                () -> Native.refreshingPackageRefreshNowNative(openHandle()),
                Rototo.executor());
    }

    public CompletableFuture<RefreshStatus> status() {
        return CompletableFuture.supplyAsync(() -> {
            Map<String, Object> value =
                    Json.asObject(Json.parse(Native.refreshingPackageStatusNative(openHandle())));
            return new RefreshStatus(
                    value.get("currentFingerprint"),
                    Json.asNullableDouble(value.get("lastSuccess")),
                    Json.asNullableDouble(value.get("lastAttempt")),
                    Json.asLong(value.get("consecutiveFailures")),
                    Json.asNullableString(value.get("lastError")),
                    Json.asBoolean(value.get("refreshing")),
                    Json.asBoolean(value.get("immutable")),
                    Json.asBoolean(value.get("servingFallback")));
        }, Rototo.executor());
    }

    public CompletableFuture<PackageIdentity> identity() {
        return CompletableFuture.supplyAsync(
                () -> PackageIdentity.fromJson(Json.asObject(
                        Json.parse(Native.refreshingPackageIdentityNative(openHandle())))),
                Rototo.executor());
    }

    public CompletableFuture<RefreshSnapshot> snapshot() {
        return CompletableFuture.supplyAsync(
                () -> RefreshSnapshot.fromJson(Json.asObject(
                        Json.parse(Native.refreshingPackageSnapshotNative(openHandle())))),
                Rototo.executor());
    }

    /**
     * Deliver refresh events to {@code listener} on a background daemon thread.
     * The thread runs until the package is shut down or closed, which closes the
     * stream. A lagging listener skips dropped events rather than failing;
     * recover ground truth from {@link #snapshot()}.
     */
    public void addRefreshListener(Consumer<RefreshEvent> listener) {
        Objects.requireNonNull(listener, "listener");
        long eventsHandle = Native.refreshingPackageSubscribeEventsNative(openHandle());
        Thread thread = new Thread(() -> {
            try {
                while (true) {
                    String json = Native.refreshEventsNextNative(eventsHandle);
                    if (json == null) {
                        return;
                    }
                    listener.accept(RefreshEvent.fromJson(Json.asObject(Json.parse(json))));
                }
            } finally {
                Native.refreshEventsFreeNative(eventsHandle);
            }
        }, "rototo-refresh-listener");
        thread.setDaemon(true);
        thread.start();
    }

    /**
     * Deliver resolution trace stream items to {@code listener} on a background
     * daemon thread. Each item is a map: a captured trace
     * ({@code {"kind": "trace", "trace": {...}}}) or a drop marker
     * ({@code {"kind": "dropped", "count": n}}). Tracing is computed only while a
     * listener is attached; with no subscriber a {@code [[trace]]} policy costs
     * nothing.
     */
    public void addTraceListener(Consumer<Map<String, Object>> listener) {
        Objects.requireNonNull(listener, "listener");
        long eventsHandle = Native.refreshingPackageSubscribeTraceEventsNative(openHandle());
        Thread thread = new Thread(() -> {
            try {
                while (true) {
                    String json = Native.traceEventsNextNative(eventsHandle);
                    if (json == null) {
                        return;
                    }
                    listener.accept(Json.asObject(Json.parse(json)));
                }
            } finally {
                Native.traceEventsFreeNative(eventsHandle);
            }
        }, "rototo-trace-listener");
        thread.setDaemon(true);
        thread.start();
    }

    public CompletableFuture<Void> shutdown() {
        return CompletableFuture.runAsync(() -> {
            long current = handle.getAndSet(0);
            if (current != 0) {
                try {
                    Native.refreshingPackageShutdownNative(current);
                } finally {
                    Native.refreshingPackageFreeNative(current);
                }
            }
        }, Rototo.executor());
    }

    @Override
    public void close() {
        long current = handle.getAndSet(0);
        if (current != 0) {
            try {
                Native.refreshingPackageShutdownNative(current);
            } finally {
                Native.refreshingPackageFreeNative(current);
            }
        }
    }

    private long openHandle() {
        long current = handle.get();
        if (current == 0) {
            throw new RototoException("refreshing package has been closed");
        }
        return current;
    }
}
