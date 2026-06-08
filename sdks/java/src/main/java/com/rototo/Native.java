package com.rototo;

final class Native {
    static {
        NativeLibrary.load();
    }

    private Native() {}

    static native String versionNative();

    static native long workspaceLoadNative(String source, String workspaceToken, String lint);

    static native long workspaceInspectNative(String source, String workspaceToken);

    static native String workspaceRootNative(long handle);

    static native String workspaceLintNative(long handle);

    static native String workspaceResolveVariableNative(
            long handle,
            String id,
            String contextJson,
            boolean validateContext);

    static native String workspaceResolveQualifierNative(
            long handle,
            String id,
            String contextJson,
            boolean validateContext);

    static native void workspaceFreeNative(long handle);

    static native long refreshingWorkspaceLoadNative(
            String source,
            double periodSeconds,
            boolean hasPeriodSeconds,
            String workspaceToken,
            String lint);

    static native String refreshingWorkspaceResolveVariableNative(
            long handle,
            String id,
            String contextJson,
            boolean validateContext);

    static native String refreshingWorkspaceResolveQualifierNative(
            long handle,
            String id,
            String contextJson,
            boolean validateContext);

    static native String refreshingWorkspaceRefreshNowNative(long handle);

    static native String refreshingWorkspaceStatusNative(long handle);

    static native void refreshingWorkspaceShutdownNative(long handle);

    static native void refreshingWorkspaceFreeNative(long handle);
}
