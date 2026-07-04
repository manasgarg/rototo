package dev.rototo;

import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.atomic.AtomicLong;

public final class Package implements AutoCloseable {
    private final AtomicLong handle;

    private Package(long handle) {
        this.handle = new AtomicLong(handle);
    }

    public static CompletableFuture<Package> load(String source) {
        return load(source, LoadOptions.defaults());
    }

    public static CompletableFuture<Package> load(String source, LoadOptions options) {
        Objects.requireNonNull(source, "source");
        LoadOptions resolved = options == null ? LoadOptions.defaults() : options;
        return CompletableFuture.supplyAsync(
                () -> new Package(Native.packageLoadNative(
                        source,
                        resolved.packageToken(),
                        resolved.lint().wireValue(),
                        resolved.fallbackSource(),
                        resolved.packageTokens() == null
                                ? null
                                : Json.stringify(resolved.packageTokens()))),
                Rototo.executor());
    }

    public static CompletableFuture<Package> inspect(String source) {
        return inspect(source, InspectOptions.defaults());
    }

    public static CompletableFuture<Package> inspect(String source, InspectOptions options) {
        Objects.requireNonNull(source, "source");
        InspectOptions resolved = options == null ? InspectOptions.defaults() : options;
        return CompletableFuture.supplyAsync(
                () -> new Package(Native.packageInspectNative(
                        source,
                        resolved.packageToken())),
                Rototo.executor());
    }

    public String root() {
        return Native.packageRootNative(openHandle());
    }

    /**
     * True when this package was loaded from the fallback source because the
     * primary source failed.
     */
    public boolean servedFallback() {
        return Native.packageServedFallbackNative(openHandle());
    }

    public PackageIdentity identity() {
        return PackageIdentity.fromJson(
                Json.asObject(Json.parse(Native.packageIdentityNative(openHandle()))));
    }

    public CompletableFuture<PackageLint> lint() {
        return CompletableFuture.supplyAsync(() -> {
            Map<String, Object> value = Json.asObject(Json.parse(Native.packageLintNative(openHandle())));
            return new PackageLint(
                    Json.asString(value.get("root")),
                    Json.asList(value.get("diagnostics")));
        }, Rototo.executor());
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
        String json = Native.packageResolveVariableNative(
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

    @Override
    public void close() {
        long current = handle.getAndSet(0);
        if (current != 0) {
            Native.packageFreeNative(current);
        }
    }

    private long openHandle() {
        long current = handle.get();
        if (current == 0) {
            throw new RototoException("package has been closed");
        }
        return current;
    }
}
