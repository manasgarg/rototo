package dev.rototo;

import java.util.Map;

/** A refresh state-transition event. */
public final class RefreshEvent {
    private final long schemaVersion;
    private final String eventId;
    private final String eventType;
    private final String source;
    private final PackageIdentity previous;
    private final PackageIdentity current;
    private final double attemptedAt;
    private final double completedAt;
    private final long durationMs;
    private final String outcome;
    private final long consecutiveFailures;
    private final String error;
    private final SdkIdentity sdk;

    public RefreshEvent(
            long schemaVersion,
            String eventId,
            String eventType,
            String source,
            PackageIdentity previous,
            PackageIdentity current,
            double attemptedAt,
            double completedAt,
            long durationMs,
            String outcome,
            long consecutiveFailures,
            String error,
            SdkIdentity sdk) {
        this.schemaVersion = schemaVersion;
        this.eventId = eventId;
        this.eventType = eventType;
        this.source = source;
        this.previous = previous;
        this.current = current;
        this.attemptedAt = attemptedAt;
        this.completedAt = completedAt;
        this.durationMs = durationMs;
        this.outcome = outcome;
        this.consecutiveFailures = consecutiveFailures;
        this.error = error;
        this.sdk = sdk;
    }

    static RefreshEvent fromJson(Map<String, Object> value) {
        Object previous = value.get("previous");
        Object current = value.get("current");
        Double attemptedAt = Json.asNullableDouble(value.get("attemptedAt"));
        Double completedAt = Json.asNullableDouble(value.get("completedAt"));
        return new RefreshEvent(
                Json.asLong(value.get("schemaVersion")),
                Json.asString(value.get("eventId")),
                Json.asString(value.get("eventType")),
                Json.asString(value.get("source")),
                previous == null ? null : PackageIdentity.fromJson(Json.asObject(previous)),
                current == null ? null : PackageIdentity.fromJson(Json.asObject(current)),
                attemptedAt == null ? 0.0 : attemptedAt,
                completedAt == null ? 0.0 : completedAt,
                Json.asLong(value.get("durationMs")),
                Json.asNullableString(value.get("outcome")),
                Json.asLong(value.get("consecutiveFailures")),
                Json.asNullableString(value.get("error")),
                SdkIdentity.fromJson(Json.asObject(value.get("sdk"))));
    }

    public long schemaVersion() {
        return schemaVersion;
    }

    public String eventId() {
        return eventId;
    }

    public String eventType() {
        return eventType;
    }

    public String source() {
        return source;
    }

    public PackageIdentity previous() {
        return previous;
    }

    public PackageIdentity current() {
        return current;
    }

    public double attemptedAt() {
        return attemptedAt;
    }

    public double completedAt() {
        return completedAt;
    }

    public long durationMs() {
        return durationMs;
    }

    public String outcome() {
        return outcome;
    }

    public long consecutiveFailures() {
        return consecutiveFailures;
    }

    public String error() {
        return error;
    }

    public SdkIdentity sdk() {
        return sdk;
    }
}
