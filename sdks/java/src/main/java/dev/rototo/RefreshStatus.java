package dev.rototo;

public final class RefreshStatus {
    private final Object currentFingerprint;
    private final Double lastSuccess;
    private final Double lastAttempt;
    private final long consecutiveFailures;
    private final String lastError;
    private final boolean refreshing;
    private final boolean immutable;

    public RefreshStatus(
            Object currentFingerprint,
            Double lastSuccess,
            Double lastAttempt,
            long consecutiveFailures,
            String lastError,
            boolean refreshing,
            boolean immutable) {
        this.currentFingerprint = currentFingerprint;
        this.lastSuccess = lastSuccess;
        this.lastAttempt = lastAttempt;
        this.consecutiveFailures = consecutiveFailures;
        this.lastError = lastError;
        this.refreshing = refreshing;
        this.immutable = immutable;
    }

    public Object currentFingerprint() {
        return currentFingerprint;
    }

    public Double lastSuccess() {
        return lastSuccess;
    }

    public Double lastAttempt() {
        return lastAttempt;
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
}
