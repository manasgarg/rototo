package dev.rototo;

import java.util.Map;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.atomic.AtomicLong;

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
                        resolved.lint().wireValue())),
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
                resolved.validateContext());
        Map<String, Object> value = Json.asObject(Json.parse(json));
        return new VariableResolution(
                Json.asString(value.get("id")),
                value.get("value"),
                value.get("source"));
    }

    public Boolean resolveQualifier(
            String id,
            Map<String, ?> context) {
        return resolveQualifier(id, context, ResolveOptions.defaults());
    }

    public Boolean resolveQualifier(
            String id,
            Map<String, ?> context,
            ResolveOptions options) {
        Objects.requireNonNull(id, "id");
        Objects.requireNonNull(context, "context");
        ResolveOptions resolved = options == null ? ResolveOptions.defaults() : options;
        String json = Native.refreshingPackageResolveQualifierNative(
                openHandle(),
                id,
                Json.stringify(context),
                resolved.validateContext());
        return Json.asBoolean(Json.parse(json));
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
                    Json.asBoolean(value.get("immutable")));
        }, Rototo.executor());
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
