package com.rototo;

import java.util.Map;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.atomic.AtomicLong;

public final class RefreshingWorkspace implements AutoCloseable {
    private final AtomicLong handle;

    private RefreshingWorkspace(long handle) {
        this.handle = new AtomicLong(handle);
    }

    public static CompletableFuture<RefreshingWorkspace> load(String source) {
        return load(source, RefreshingWorkspaceOptions.defaults());
    }

    public static CompletableFuture<RefreshingWorkspace> load(
            String source,
            RefreshingWorkspaceOptions options) {
        Objects.requireNonNull(source, "source");
        RefreshingWorkspaceOptions resolved =
                options == null ? RefreshingWorkspaceOptions.defaults() : options;
        Double periodSeconds = resolved.periodSeconds();
        return CompletableFuture.supplyAsync(
                () -> new RefreshingWorkspace(Native.refreshingWorkspaceLoadNative(
                        source,
                        periodSeconds == null ? 0.0 : periodSeconds,
                        periodSeconds != null,
                        resolved.workspaceToken(),
                        resolved.lint().wireValue())),
                Rototo.executor());
    }

    public CompletableFuture<VariableResolution> resolveVariable(
            String id,
            Map<String, ?> context) {
        return resolveVariable(id, context, ResolveOptions.defaults());
    }

    public CompletableFuture<VariableResolution> resolveVariable(
            String id,
            Map<String, ?> context,
            ResolveOptions options) {
        Objects.requireNonNull(id, "id");
        Objects.requireNonNull(context, "context");
        ResolveOptions resolved = options == null ? ResolveOptions.defaults() : options;
        return CompletableFuture.supplyAsync(() -> {
            String json = Native.refreshingWorkspaceResolveVariableNative(
                    openHandle(),
                    id,
                    Json.stringify(context),
                    resolved.validateContext());
            Map<String, Object> value = Json.asObject(Json.parse(json));
            return new VariableResolution(
                    Json.asString(value.get("id")),
                    Json.asString(value.get("valueKey")),
                    value.get("value"));
        }, Rototo.executor());
    }

    public CompletableFuture<QualifierResolution> resolveQualifier(
            String id,
            Map<String, ?> context) {
        return resolveQualifier(id, context, ResolveOptions.defaults());
    }

    public CompletableFuture<QualifierResolution> resolveQualifier(
            String id,
            Map<String, ?> context,
            ResolveOptions options) {
        Objects.requireNonNull(id, "id");
        Objects.requireNonNull(context, "context");
        ResolveOptions resolved = options == null ? ResolveOptions.defaults() : options;
        return CompletableFuture.supplyAsync(() -> {
            String json = Native.refreshingWorkspaceResolveQualifierNative(
                    openHandle(),
                    id,
                    Json.stringify(context),
                    resolved.validateContext());
            Map<String, Object> value = Json.asObject(Json.parse(json));
            return new QualifierResolution(
                    Json.asString(value.get("id")),
                    Json.asBoolean(value.get("value")));
        }, Rototo.executor());
    }

    public CompletableFuture<String> refreshNow() {
        return CompletableFuture.supplyAsync(
                () -> Native.refreshingWorkspaceRefreshNowNative(openHandle()),
                Rototo.executor());
    }

    public CompletableFuture<RefreshStatus> status() {
        return CompletableFuture.supplyAsync(() -> {
            Map<String, Object> value =
                    Json.asObject(Json.parse(Native.refreshingWorkspaceStatusNative(openHandle())));
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
                    Native.refreshingWorkspaceShutdownNative(current);
                } finally {
                    Native.refreshingWorkspaceFreeNative(current);
                }
            }
        }, Rototo.executor());
    }

    @Override
    public void close() {
        long current = handle.getAndSet(0);
        if (current != 0) {
            try {
                Native.refreshingWorkspaceShutdownNative(current);
            } finally {
                Native.refreshingWorkspaceFreeNative(current);
            }
        }
    }

    private long openHandle() {
        long current = handle.get();
        if (current == 0) {
            throw new RototoException("refreshing workspace has been closed");
        }
        return current;
    }
}
