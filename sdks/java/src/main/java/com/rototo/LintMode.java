package com.rototo;

public enum LintMode {
    DENY("deny"),
    SKIP("skip");

    private final String wireValue;

    LintMode(String wireValue) {
        this.wireValue = wireValue;
    }

    String wireValue() {
        return wireValue;
    }
}
