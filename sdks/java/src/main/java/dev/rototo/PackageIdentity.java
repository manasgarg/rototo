package dev.rototo;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/** Stable identity of the package currently active in this process. */
public final class PackageIdentity {
    private final String source;
    private final Object fingerprint;
    private final String releaseId;
    private final double loadedAt;
    private final boolean immutable;
    private final List<PackageLayerIdentity> layers;

    public PackageIdentity(
            String source,
            Object fingerprint,
            String releaseId,
            double loadedAt,
            boolean immutable,
            List<PackageLayerIdentity> layers) {
        this.source = source;
        this.fingerprint = fingerprint;
        this.releaseId = releaseId;
        this.loadedAt = loadedAt;
        this.immutable = immutable;
        this.layers = layers;
    }

    static PackageIdentity fromJson(Map<String, Object> value) {
        List<PackageLayerIdentity> layers = new ArrayList<>();
        for (Object layer : Json.asList(value.get("layers"))) {
            layers.add(PackageLayerIdentity.fromJson(Json.asObject(layer)));
        }
        Double loadedAt = Json.asNullableDouble(value.get("loadedAt"));
        return new PackageIdentity(
                Json.asString(value.get("source")),
                value.get("fingerprint"),
                Json.asNullableString(value.get("releaseId")),
                loadedAt == null ? 0.0 : loadedAt,
                Json.asBoolean(value.get("immutable")),
                layers);
    }

    public String source() {
        return source;
    }

    public Object fingerprint() {
        return fingerprint;
    }

    public String releaseId() {
        return releaseId;
    }

    public double loadedAt() {
        return loadedAt;
    }

    public boolean immutable() {
        return immutable;
    }

    public List<PackageLayerIdentity> layers() {
        return layers;
    }
}
