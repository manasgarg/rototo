package dev.rototo;

/** Error raised for rototo SDK failures. */
public class RototoException extends RuntimeException {
    public RototoException(String message) {
        super(message);
    }

    public RototoException(String message, Throwable cause) {
        super(message, cause);
    }
}
