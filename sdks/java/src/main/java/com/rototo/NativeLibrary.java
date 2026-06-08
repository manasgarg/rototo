package com.rototo;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Locale;

final class NativeLibrary {
    private static volatile boolean loaded;

    private NativeLibrary() {}

    static synchronized void load() {
        if (loaded) {
            return;
        }

        String explicitPath = System.getProperty("rototo.native.path");
        if (explicitPath != null && !explicitPath.isBlank()) {
            System.load(explicitPath);
            loaded = true;
            return;
        }

        String resourcePath = nativeResourcePath();
        try (InputStream input = NativeLibrary.class.getResourceAsStream(resourcePath)) {
            if (input != null) {
                Path extracted = Files.createTempFile("rototo-java-", "-" + libraryFileName());
                Files.copy(input, extracted, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
                extracted.toFile().deleteOnExit();
                extracted.toFile().setReadable(true);
                extracted.toFile().setExecutable(true);
                System.load(extracted.toAbsolutePath().toString());
                loaded = true;
                return;
            }
        } catch (IOException error) {
            throw new RototoException("failed to extract rototo native library", error);
        }

        System.loadLibrary("rototo_java");
        loaded = true;
    }

    private static String nativeResourcePath() {
        return "/com/rototo/native/" + platform() + "/" + libraryFileName();
    }

    private static String libraryFileName() {
        return System.mapLibraryName("rototo_java");
    }

    private static String platform() {
        return osPart() + "-" + archPart();
    }

    private static String osPart() {
        String os = System.getProperty("os.name").toLowerCase(Locale.ROOT);
        if (os.contains("linux")) {
            return "linux";
        }
        if (os.contains("mac") || os.contains("darwin")) {
            return "darwin";
        }
        if (os.contains("win")) {
            return "windows";
        }
        throw new RototoException("unsupported rototo Java SDK operating system: " + os);
    }

    private static String archPart() {
        String arch = System.getProperty("os.arch").toLowerCase(Locale.ROOT);
        if (arch.equals("amd64") || arch.equals("x86_64")) {
            return "x86_64";
        }
        if (arch.equals("aarch64") || arch.equals("arm64")) {
            return "aarch64";
        }
        throw new RototoException("unsupported rototo Java SDK architecture: " + arch);
    }
}
