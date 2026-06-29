package dev.rototo;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

public final class JavaSdkTest {
    public static void main(String[] args) throws Exception {
        api();
        contract();
        refresh();
        events();
    }

    private static void api() throws Exception {
        assertEquals(expectedVersion(), Rototo.version(), "version");
        try (Package pkg = await(Package.load("examples/basic"))) {
            VariableResolution variable = pkg.resolveVariable(
                    "premium-message",
                    Map.of("user", Map.of("tier", "premium")));
            assertEquals("premium-message", variable.id(), "variable id");
            assertEquals(Map.of("kind", "literal"), variable.source(), "source");
            assertEquals("Welcome back, premium member.", variable.value(), "value");

            Boolean qualifier = pkg.resolveQualifier(
                    "premium-users",
                    Map.of("user", Map.of("tier", "free")));
            assertEquals(false, qualifier, "qualifier value");

            VariableResolution skippedValidation = pkg.resolveVariable(
                    "premium-message",
                    Map.of("user", Map.of("tier", Map.of("bad", "shape"))),
                    ResolveOptions.validateContext(false));
            assertEquals(Map.of("kind", "literal"), skippedValidation.source(), "validation skip fallback");
        }

        try (Package inspected = await(Package.inspect("examples/basic"))) {
            PackageLint lint = await(inspected.lint());
            assertEquals(0, lint.diagnostics().size(), "inspection lint diagnostics");
            assertRototoError(
                    () -> inspected.resolveVariable("premium-message", Map.of()),
                    "package was loaded without a runtime model");
        }
    }

    private static void contract() throws Exception {
        List<String> lines = Files.readAllLines(Path.of("tests/sdk-contract/cases.jsonl"));
        for (String line : lines) {
            if (line.isBlank()) {
                continue;
            }
            Map<String, Object> testCase = Json.asObject(Json.parse(line));
            String name = Json.asString(testCase.get("name"));
            String operation = Json.asString(testCase.get("operation"));
            String packageSource = Json.asString(testCase.get("package"));
            Map<String, Object> expect = Json.asObject(testCase.get("expect"));
            boolean ok = Json.asBoolean(expect.get("ok"));

            if (operation.equals("load_package")) {
                CompletableFuture<Package> future = Package.load(packageSource);
                if (ok) {
                    await(future).close();
                } else {
                    assertRototoError(future, expectedError(expect));
                }
                continue;
            }

            try (Package pkg = await(Package.load(packageSource))) {
                switch (operation) {
                    case "lint_package":
                        if (ok) {
                            PackageLint lint = await(pkg.lint());
                            assertEquals(
                                    Json.asLong(expect.get("diagnostics")),
                                    (long) lint.diagnostics().size(),
                                    name + " diagnostics");
                        } else {
                            assertRototoError(pkg.lint(), expectedError(expect));
                        }
                        break;
                    case "resolve_qualifier":
                        runQualifierCase(name, pkg, testCase, expect, ok);
                        break;
                    case "resolve_variable":
                        runVariableCase(name, pkg, testCase, expect, ok);
                        break;
                    case "package_identity": {
                        Map<String, Object> result = Json.asObject(expect.get("result"));
                        PackageIdentity identity = pkg.identity();
                        assertEquals(
                                result.get("releaseId") == null,
                                identity.releaseId() == null,
                                name + " releaseId");
                        assertEquals(
                                result.get("immutable"), identity.immutable(), name + " immutable");
                        break;
                    }
                    default:
                        throw new AssertionError("unsupported contract operation: " + operation);
                }
            }
        }
    }

    private static void runQualifierCase(
            String name,
            Package pkg,
            Map<String, Object> testCase,
            Map<String, Object> expect,
            boolean ok) throws Exception {
        if (!ok) {
            assertRototoError(
                    () -> pkg.resolveQualifier(
                            Json.asString(testCase.get("id")),
                            Json.asObject(testCase.get("context"))),
                    expectedError(expect));
            return;
        }
        Boolean actual = pkg.resolveQualifier(
                Json.asString(testCase.get("id")),
                Json.asObject(testCase.get("context")));
        assertEquals(Json.asBoolean(expect.get("result")), actual, name + " value");
    }

    private static void runVariableCase(
            String name,
            Package pkg,
            Map<String, Object> testCase,
            Map<String, Object> expect,
            boolean ok) throws Exception {
        if (!ok) {
            assertRototoError(
                    () -> pkg.resolveVariable(
                            Json.asString(testCase.get("id")),
                            Json.asObject(testCase.get("context"))),
                    expectedError(expect));
            return;
        }
        Map<String, Object> result = Json.asObject(expect.get("result"));
        VariableResolution actual = pkg.resolveVariable(
                Json.asString(testCase.get("id")),
                Json.asObject(testCase.get("context")));
        assertEquals(Json.asString(result.get("id")), actual.id(), name + " id");
        assertEquals(result.get("value"), actual.value(), name + " value");
        assertEquals(result.get("source"), actual.source(), name + " source");
    }

    private static void refresh() throws Exception {
        RefreshingPackageOptions options = RefreshingPackageOptions.builder()
                .periodSeconds(30.0)
                .build();
        try (RefreshingPackage pkg = await(RefreshingPackage.load("examples/basic", options))) {
            VariableResolution resolution = pkg.resolveVariable(
                    "premium-message",
                    Map.of("user", Map.of("tier", "premium")));
            assertEquals(Map.of("kind", "literal"), resolution.source(), "refreshing resolution");
            RefreshStatus status = await(pkg.status());
            assertEquals(0L, status.consecutiveFailures(), "consecutive failures");
            assertEquals(false, status.refreshing(), "refreshing flag");
            await(pkg.shutdown());
        }
    }

    private static void events() throws Exception {
        Path root = Files.createTempDirectory("rototo-java-events");
        writeMessagePackage(root, "hello");
        try (RefreshingPackage pkg = await(RefreshingPackage.load(root.toString()))) {
            PackageIdentity identity = await(pkg.identity());
            // A local directory has no fingerprint, so no derived release id.
            assertEquals(true, identity.releaseId() == null, "local release id");

            RefreshSnapshot snapshot = await(pkg.snapshot());
            assertEquals(false, snapshot.lastSuccess() == null, "snapshot last success present");
            assertEquals("loaded", snapshot.lastEvent().eventType(), "snapshot last event type");

            CountDownLatch refreshed = new CountDownLatch(1);
            AtomicReference<RefreshEvent> captured = new AtomicReference<>();
            pkg.addRefreshListener(event -> {
                if ("refreshed".equals(event.eventType())) {
                    captured.set(event);
                    refreshed.countDown();
                }
            });

            writeMessagePackage(root, "updated");
            String outcome = await(pkg.refreshNow());
            assertEquals("refreshed", outcome, "refresh outcome");

            if (!refreshed.await(5, TimeUnit.SECONDS)) {
                throw new AssertionError("did not observe a refreshed event");
            }
            RefreshEvent event = captured.get();
            assertEquals(1L, event.schemaVersion(), "event schema version");
            assertEquals("rust", event.sdk().language(), "event sdk language");
            assertEquals(false, event.current() == null, "event has current identity");

            await(pkg.shutdown());
        } finally {
            deleteRecursively(root);
        }
    }

    private static void writeMessagePackage(Path root, String message) throws Exception {
        Files.createDirectories(root.resolve("variables"));
        Files.writeString(root.resolve("rototo-package.toml"), "schema_version = 1\n");
        Files.writeString(
                root.resolve("variables").resolve("message.toml"),
                "schema_version = 1\ntype = \"string\"\n\n[resolve]\ndefault = \"" + message + "\"\n");
    }

    private static void deleteRecursively(Path root) throws Exception {
        if (!Files.exists(root)) {
            return;
        }
        try (java.util.stream.Stream<Path> walk = Files.walk(root)) {
            walk.sorted(java.util.Comparator.reverseOrder()).forEach(path -> {
                try {
                    Files.deleteIfExists(path);
                } catch (Exception ignored) {
                    // best effort cleanup
                }
            });
        }
    }

    private static String expectedError(Map<String, Object> expect) {
        return Json.asString(Json.asObject(expect.get("error")).get("contains"));
    }

    private static <T> T await(CompletableFuture<T> future) throws Exception {
        return future.get(30, TimeUnit.SECONDS);
    }

    private static void assertRototoError(CompletableFuture<?> future, String contains) throws Exception {
        try {
            await(future);
        } catch (ExecutionException error) {
            Throwable cause = error.getCause();
            if (!(cause instanceof RototoException)) {
                throw new AssertionError("expected RototoException, got " + cause, cause);
            }
            if (!cause.getMessage().contains(contains)) {
                throw new AssertionError(
                        "expected error containing " + contains + ", got " + cause.getMessage());
            }
            return;
        }
        throw new AssertionError("expected RototoException containing " + contains);
    }

    private static void assertRototoError(ThrowingRunnable runnable, String contains) throws Exception {
        try {
            runnable.run();
        } catch (RototoException error) {
            if (!error.getMessage().contains(contains)) {
                throw new AssertionError(
                        "expected error containing " + contains + ", got " + error.getMessage());
            }
            return;
        }
        throw new AssertionError("expected RototoException containing " + contains);
    }

    @FunctionalInterface
    private interface ThrowingRunnable {
        void run() throws Exception;
    }

    private static void assertEquals(Object expected, Object actual, String label) {
        if (!expected.equals(actual)) {
            throw new AssertionError(label + ": expected " + expected + ", got " + actual);
        }
    }

    private static String expectedVersion() {
        String version = System.getProperty("rototo.expected.version");
        if (version == null || version.isBlank()) {
            throw new AssertionError("missing rototo.expected.version");
        }
        return version;
    }
}
