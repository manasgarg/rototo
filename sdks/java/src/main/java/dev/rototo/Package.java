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

    /** Every list id in the loaded package. */
    public java.util.List<String> listIds() {
        String json = Native.packageListIdsNative(openHandle());
        return Json.asStringList(Json.parse(json));
    }

    /** One list: id, description, memberType, and members. */
    public Map<String, Object> readList(String id) {
        Objects.requireNonNull(id, "id");
        String json = Native.packageReadListNative(openHandle(), id);
        return Json.asObject(Json.parse(json));
    }

    /** Every entry id of one catalog. */
    public java.util.List<String> entryIds(String catalog) {
        Objects.requireNonNull(catalog, "catalog");
        String json = Native.packageEntryIdsNative(openHandle(), catalog);
        return Json.asStringList(Json.parse(json));
    }

    /** One raw catalog entry, exactly as authored. */
    public Object readEntry(String catalog, String entry) {
        Objects.requireNonNull(catalog, "catalog");
        Objects.requireNonNull(entry, "entry");
        String json = Native.packageReadEntryNative(openHandle(), catalog, entry);
        return Json.parse(json);
    }

    /** Follow one reference by address: catalog=email_template:entry=welcome#/body. */
    public Object resolveReference(String address) {
        Objects.requireNonNull(address, "address");
        String json = Native.packageResolveReferenceNative(openHandle(), address);
        return Json.parse(json);
    }

    /** Follow a raw entry-reference string against its pinned catalogs. */
    public Object resolveEntryRef(String value, java.util.List<String> pins) {
        Objects.requireNonNull(value, "value");
        Objects.requireNonNull(pins, "pins");
        String json = Native.packageResolveEntryRefNative(
                openHandle(), value, Json.stringify(pins));
        return Json.parse(json);
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
