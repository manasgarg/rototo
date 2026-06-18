package dev.rototo;

import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.atomic.AtomicLong;

public final class Workspace implements AutoCloseable {
    private final AtomicLong handle;

    private Workspace(long handle) {
        this.handle = new AtomicLong(handle);
    }

    public static CompletableFuture<Workspace> load(String source) {
        return load(source, LoadOptions.defaults());
    }

    public static CompletableFuture<Workspace> load(String source, LoadOptions options) {
        Objects.requireNonNull(source, "source");
        LoadOptions resolved = options == null ? LoadOptions.defaults() : options;
        return CompletableFuture.supplyAsync(
                () -> new Workspace(Native.workspaceLoadNative(
                        source,
                        resolved.workspaceToken(),
                        resolved.lint().wireValue())),
                Rototo.executor());
    }

    public static CompletableFuture<Workspace> inspect(String source) {
        return inspect(source, InspectOptions.defaults());
    }

    public static CompletableFuture<Workspace> inspect(String source, InspectOptions options) {
        Objects.requireNonNull(source, "source");
        InspectOptions resolved = options == null ? InspectOptions.defaults() : options;
        return CompletableFuture.supplyAsync(
                () -> new Workspace(Native.workspaceInspectNative(
                        source,
                        resolved.workspaceToken())),
                Rototo.executor());
    }

    public String root() {
        return Native.workspaceRootNative(openHandle());
    }

    public CompletableFuture<WorkspaceLint> lint() {
        return CompletableFuture.supplyAsync(() -> {
            Map<String, Object> value = Json.asObject(Json.parse(Native.workspaceLintNative(openHandle())));
            return new WorkspaceLint(
                    Json.asString(value.get("root")),
                    Json.asList(value.get("diagnostics")));
        }, Rototo.executor());
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
            String json = Native.workspaceResolveVariableNative(
                    openHandle(),
                    id,
                    Json.stringify(context),
                    resolved.validateContext());
            Map<String, Object> value = Json.asObject(Json.parse(json));
            return new VariableResolution(
                    Json.asString(value.get("id")),
                    value.get("value"),
                    value.get("source"));
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
            String json = Native.workspaceResolveQualifierNative(
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

    @Override
    public void close() {
        long current = handle.getAndSet(0);
        if (current != 0) {
            Native.workspaceFreeNative(current);
        }
    }

    private long openHandle() {
        long current = handle.get();
        if (current == 0) {
            throw new RototoException("workspace has been closed");
        }
        return current;
    }
}
