package dev.rototo;

import java.util.Map;

/** Refresh state joined with package identity: what is true now. */
public final class RefreshSnapshot {
    private final PackageIdentity identity;
    private final Double lastAttempt;
    private final Double lastSuccess;
    private final RefreshEventSummary lastEvent;
    private final long consecutiveFailures;
    private final String lastError;
    private final boolean refreshing;
    private final boolean immutable;
    private final boolean servingFallback;

    public RefreshSnapshot(
            PackageIdentity identity,
            Double lastAttempt,
            Double lastSuccess,
            RefreshEventSummary lastEvent,
            long consecutiveFailures,
            String lastError,
            boolean refreshing,
            boolean immutable,
            boolean servingFallback) {
        this.identity = identity;
        this.lastAttempt = lastAttempt;
        this.lastSuccess = lastSuccess;
        this.lastEvent = lastEvent;
        this.consecutiveFailures = consecutiveFailures;
        this.lastError = lastError;
        this.refreshing = refreshing;
        this.immutable = immutable;
        this.servingFallback = servingFallback;
    }

    static RefreshSnapshot fromJson(Map<String, Object> value) {
        Object lastEvent = value.get("lastEvent");
        return new RefreshSnapshot(
                PackageIdentity.fromJson(Json.asObject(value.get("identity"))),
                Json.asNullableDouble(value.get("lastAttempt")),
                Json.asNullableDouble(value.get("lastSuccess")),
                lastEvent == null ? null : RefreshEventSummary.fromJson(Json.asObject(lastEvent)),
                Json.asLong(value.get("consecutiveFailures")),
                Json.asNullableString(value.get("lastError")),
                Json.asBoolean(value.get("refreshing")),
                Json.asBoolean(value.get("immutable")),
                Json.asBoolean(value.get("servingFallback")));
    }

    public PackageIdentity identity() {
        return identity;
    }

    public Double lastAttempt() {
        return lastAttempt;
    }

    public Double lastSuccess() {
        return lastSuccess;
    }

    public RefreshEventSummary lastEvent() {
        return lastEvent;
    }

    public long consecutiveFailures() {
        return consecutiveFailures;
    }

    public String lastError() {
        return lastError;
    }

    public boolean refreshing() {
        return refreshing;
    }

    public boolean immutable() {
        return immutable;
    }

    /** True while the serving package came from the fallback source. */
    public boolean servingFallback() {
        return servingFallback;
    }
}
