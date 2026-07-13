package dev.rototo;

public final class RefreshStatus {
    private final Object currentFingerprint;
    private final Double lastSuccess;
    private final Double lastAttempt;
    private final long consecutiveFailures;
    private final String lastError;
    private final boolean refreshing;
    private final boolean immutable;
    private final boolean servingFallback;

    public RefreshStatus(
            Object currentFingerprint,
            Double lastSuccess,
            Double lastAttempt,
            long consecutiveFailures,
            String lastError,
            boolean refreshing,
            boolean immutable,
            boolean servingFallback) {
        this.currentFingerprint = currentFingerprint;
        this.lastSuccess = lastSuccess;
        this.lastAttempt = lastAttempt;
        this.consecutiveFailures = consecutiveFailures;
        this.lastError = lastError;
        this.refreshing = refreshing;
        this.immutable = immutable;
        this.servingFallback = servingFallback;
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

    /**
     * True while the serving package came from the fallback source instead of
     * the primary. Clears on the first successful refresh from the primary.
     */
    public boolean servingFallback() {
        return servingFallback;
    }
}
