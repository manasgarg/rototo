package dev.rototo;

public final class VariableResolution {
    private final String id;
    private final Object value;
    private final Object source;

    public VariableResolution(String id, Object value, Object source) {
        this.id = id;
        this.value = value;
        this.source = source;
    }

    public String id() {
        return id;
    }

    public Object value() {
        return value;
    }

    public Object source() {
        return source;
    }
}
