package com.rototo;

import java.util.List;

public final class WorkspaceLint {
    private final String root;
    private final List<Object> diagnostics;

    public WorkspaceLint(String root, List<Object> diagnostics) {
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
