package dev.rototo;

import java.util.Map;

/** Identity of one layer in a layered package. */
public final class PackageLayerIdentity {
    private final String source;
    private final Object fingerprint;
    private final String releaseId;
    private final boolean immutable;

    public PackageLayerIdentity(
            String source, Object fingerprint, String releaseId, boolean immutable) {
        this.source = source;
        this.fingerprint = fingerprint;
        this.releaseId = releaseId;
        this.immutable = immutable;
    }

    static PackageLayerIdentity fromJson(Map<String, Object> value) {
        return new PackageLayerIdentity(
                Json.asString(value.get("source")),
                value.get("fingerprint"),
                Json.asNullableString(value.get("releaseId")),
                Json.asBoolean(value.get("immutable")));
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

    public boolean immutable() {
        return immutable;
    }
}
