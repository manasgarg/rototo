package dev.rototo;

import java.util.Map;

/** Compact record of the most recent refresh event. */
public final class RefreshEventSummary {
    private final String eventId;
    private final String eventType;
    private final String releaseId;
    private final double completedAt;

    public RefreshEventSummary(
            String eventId, String eventType, String releaseId, double completedAt) {
        this.eventId = eventId;
        this.eventType = eventType;
        this.releaseId = releaseId;
        this.completedAt = completedAt;
    }

    static RefreshEventSummary fromJson(Map<String, Object> value) {
        Double completedAt = Json.asNullableDouble(value.get("completedAt"));
        return new RefreshEventSummary(
                Json.asString(value.get("eventId")),
                Json.asString(value.get("eventType")),
                Json.asNullableString(value.get("releaseId")),
                completedAt == null ? 0.0 : completedAt);
    }

    public String eventId() {
        return eventId;
    }

    public String eventType() {
        return eventType;
    }

    public String releaseId() {
        return releaseId;
    }

    public double completedAt() {
        return completedAt;
    }
}
