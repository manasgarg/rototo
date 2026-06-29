package dev.rototo;

import java.util.List;

public final class PackageLint {
    private final String root;
    private final List<Object> diagnostics;

    public PackageLint(String root, List<Object> diagnostics) {
        this.root = root;
        this.diagnostics = List.copyOf(diagnostics);
    }

    public String root() {
        return root;
    }

    public List<Object> diagnostics() {
        return diagnostics;
    }
}
