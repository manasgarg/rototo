package dev.rototo;

import java.util.concurrent.Executor;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Entry points shared by the Java SDK. */
public final class Rototo {
    private static final ExecutorService EXECUTOR = Executors.newCachedThreadPool(runnable -> {
        Thread thread = new Thread(runnable, "rototo-java-sdk");
        thread.setDaemon(true);
        return thread;
    });

    public static final String VERSION = Native.versionNative();

    private Rototo() {}

    public static String version() {
        return VERSION;
    }

    static Executor executor() {
        return EXECUTOR;
    }
}
