package dev.rototo;

final class Native {
    static {
        NativeLibrary.load();
    }

    private Native() {}

    static native String versionNative();

    static native long packageLoadNative(
            String source,
            String packageToken,
            String lint,
            String fallbackSource,
            String packageTokensJson);

    static native long packageInspectNative(String source, String packageToken);

    static native String packageRootNative(long handle);

    static native boolean packageServedFallbackNative(long handle);

    static native String packageIdentityNative(long handle);

    static native String packageLintNative(long handle);

    static native String packageResolveVariableNative(
            long handle, String id, String contextJson, boolean validateContext, boolean trace);

    static native String packageListIdsNative(long handle);

    static native String packageReadListNative(long handle, String id);

    static native String packageEntryIdsNative(long handle, String catalog);

    static native String packageReadEntryNative(long handle, String catalog, String entry);

    static native String packageResolveReferenceNative(long handle, String address);

    static native String packageResolveEntryRefNative(long handle, String value, String pinsJson);

    static native void packageFreeNative(long handle);

    static native long refreshingPackageLoadNative(
            String source,
            double periodSeconds,
            boolean hasPeriodSeconds,
            String packageToken,
            String lint,
            String fallbackSource,
            String packageTokensJson);

    static native String refreshingPackageResolveVariableNative(
            long handle, String id, String contextJson, boolean validateContext, boolean trace);

    static native String refreshingPackageRefreshNowNative(long handle);

    static native String refreshingPackageStatusNative(long handle);

    static native String refreshingPackageIdentityNative(long handle);

    static native String refreshingPackageSnapshotNative(long handle);

    static native long refreshingPackageSubscribeEventsNative(long handle);

    static native String refreshEventsNextNative(long handle);

    static native void refreshEventsFreeNative(long handle);

    static native long refreshingPackageSubscribeTraceEventsNative(long handle);

    static native String traceEventsNextNative(long handle);

    static native void traceEventsFreeNative(long handle);

    static native void refreshingPackageShutdownNative(long handle);

    static native void refreshingPackageFreeNative(long handle);
}
