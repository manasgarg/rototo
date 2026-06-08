package dev.rototo;

public final class QualifierResolution {
    private final String id;
    private final boolean value;

    public QualifierResolution(String id, boolean value) {
        this.id = id;
        this.value = value;
    }

    public String id() {
        return id;
    }

    public boolean value() {
        return value;
    }
}
