package com.rototo;

public final class VariableResolution {
    private final String id;
    private final String valueKey;
    private final Object value;

    public VariableResolution(String id, String valueKey, Object value) {
        this.id = id;
        this.valueKey = valueKey;
        this.value = value;
    }

    public String id() {
        return id;
    }

    public String valueKey() {
        return valueKey;
    }

    public Object value() {
        return value;
    }
}
