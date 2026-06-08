package com.rototo;

import java.lang.reflect.Array;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

final class Json {
    private Json() {}

    static String stringify(Object value) {
        StringBuilder out = new StringBuilder();
        writeValue(out, value);
        return out.toString();
    }

    static Object parse(String json) {
        return new Parser(json).parse();
    }

    @SuppressWarnings("unchecked")
    static Map<String, Object> asObject(Object value) {
        if (value instanceof Map<?, ?>) {
            return (Map<String, Object>) value;
        }
        throw new RototoException("expected JSON object");
    }

    @SuppressWarnings("unchecked")
    static List<Object> asList(Object value) {
        if (value instanceof List<?>) {
            return (List<Object>) value;
        }
        throw new RototoException("expected JSON array");
    }

    static String asString(Object value) {
        if (value instanceof String) {
            return (String) value;
        }
        throw new RototoException("expected JSON string");
    }

    static String asNullableString(Object value) {
        return value == null ? null : asString(value);
    }

    static boolean asBoolean(Object value) {
        if (value instanceof Boolean) {
            return (Boolean) value;
        }
        throw new RototoException("expected JSON boolean");
    }

    static long asLong(Object value) {
        if (value instanceof Number) {
            return ((Number) value).longValue();
        }
        throw new RototoException("expected JSON number");
    }

    static Double asNullableDouble(Object value) {
        if (value == null) {
            return null;
        }
        if (value instanceof Number) {
            return ((Number) value).doubleValue();
        }
        throw new RototoException("expected JSON number");
    }

    private static void writeValue(StringBuilder out, Object value) {
        if (value == null) {
            out.append("null");
        } else if (value instanceof String) {
            writeString(out, (String) value);
        } else if (value instanceof Boolean) {
            out.append(value);
        } else if (value instanceof Number) {
            writeNumber(out, (Number) value);
        } else if (value instanceof Map<?, ?>) {
            writeObject(out, (Map<?, ?>) value);
        } else if (value instanceof Iterable<?>) {
            writeArray(out, (Iterable<?>) value);
        } else if (value.getClass().isArray()) {
            writeJavaArray(out, value);
        } else {
            throw new RototoException("value cannot be converted to JSON: " + value.getClass());
        }
    }

    private static void writeObject(StringBuilder out, Map<?, ?> value) {
        out.append('{');
        boolean first = true;
        for (Map.Entry<?, ?> entry : value.entrySet()) {
            if (!(entry.getKey() instanceof String)) {
                throw new RototoException("JSON object keys must be strings");
            }
            if (!first) {
                out.append(',');
            }
            writeString(out, (String) entry.getKey());
            out.append(':');
            writeValue(out, entry.getValue());
            first = false;
        }
        out.append('}');
    }

    private static void writeArray(StringBuilder out, Iterable<?> value) {
        out.append('[');
        boolean first = true;
        for (Object item : value) {
            if (!first) {
                out.append(',');
            }
            writeValue(out, item);
            first = false;
        }
        out.append(']');
    }

    private static void writeJavaArray(StringBuilder out, Object value) {
        out.append('[');
        int length = Array.getLength(value);
        for (int index = 0; index < length; index++) {
            if (index > 0) {
                out.append(',');
            }
            writeValue(out, Array.get(value, index));
        }
        out.append(']');
    }

    private static void writeNumber(StringBuilder out, Number value) {
        if (value instanceof Double) {
            double number = (Double) value;
            if (!Double.isFinite(number)) {
                throw new RototoException("JSON numbers must be finite");
            }
        } else if (value instanceof Float) {
            float number = (Float) value;
            if (!Float.isFinite(number)) {
                throw new RototoException("JSON numbers must be finite");
            }
        }
        out.append(value);
    }

    private static void writeString(StringBuilder out, String value) {
        out.append('"');
        for (int index = 0; index < value.length(); index++) {
            char ch = value.charAt(index);
            switch (ch) {
                case '"':
                    out.append("\\\"");
                    break;
                case '\\':
                    out.append("\\\\");
                    break;
                case '\b':
                    out.append("\\b");
                    break;
                case '\f':
                    out.append("\\f");
                    break;
                case '\n':
                    out.append("\\n");
                    break;
                case '\r':
                    out.append("\\r");
                    break;
                case '\t':
                    out.append("\\t");
                    break;
                default:
                    if (ch < 0x20) {
                        out.append(String.format("\\u%04x", (int) ch));
                    } else {
                        out.append(ch);
                    }
            }
        }
        out.append('"');
    }

    private static final class Parser {
        private final String json;
        private int index;

        Parser(String json) {
            this.json = json;
        }

        Object parse() {
            Object value = parseValue();
            skipWhitespace();
            if (index != json.length()) {
                throw error("unexpected trailing content");
            }
            return value;
        }

        private Object parseValue() {
            skipWhitespace();
            if (index >= json.length()) {
                throw error("unexpected end of JSON");
            }
            char ch = json.charAt(index);
            if (ch == '{') {
                return parseObject();
            }
            if (ch == '[') {
                return parseArray();
            }
            if (ch == '"') {
                return parseString();
            }
            if (ch == 't') {
                expect("true");
                return Boolean.TRUE;
            }
            if (ch == 'f') {
                expect("false");
                return Boolean.FALSE;
            }
            if (ch == 'n') {
                expect("null");
                return null;
            }
            if (ch == '-' || (ch >= '0' && ch <= '9')) {
                return parseNumber();
            }
            throw error("unexpected JSON value");
        }

        private Map<String, Object> parseObject() {
            expect('{');
            Map<String, Object> object = new LinkedHashMap<>();
            skipWhitespace();
            if (tryConsume('}')) {
                return object;
            }
            while (true) {
                skipWhitespace();
                if (index >= json.length() || json.charAt(index) != '"') {
                    throw error("expected JSON object key");
                }
                String key = parseString();
                skipWhitespace();
                expect(':');
                object.put(key, parseValue());
                skipWhitespace();
                if (tryConsume('}')) {
                    return object;
                }
                expect(',');
            }
        }

        private List<Object> parseArray() {
            expect('[');
            List<Object> array = new ArrayList<>();
            skipWhitespace();
            if (tryConsume(']')) {
                return array;
            }
            while (true) {
                array.add(parseValue());
                skipWhitespace();
                if (tryConsume(']')) {
                    return array;
                }
                expect(',');
            }
        }

        private String parseString() {
            expect('"');
            StringBuilder out = new StringBuilder();
            while (index < json.length()) {
                char ch = json.charAt(index++);
                if (ch == '"') {
                    return out.toString();
                }
                if (ch == '\\') {
                    out.append(parseEscape());
                } else {
                    out.append(ch);
                }
            }
            throw error("unterminated JSON string");
        }

        private char parseEscape() {
            if (index >= json.length()) {
                throw error("unterminated JSON escape");
            }
            char ch = json.charAt(index++);
            switch (ch) {
                case '"':
                case '\\':
                case '/':
                    return ch;
                case 'b':
                    return '\b';
                case 'f':
                    return '\f';
                case 'n':
                    return '\n';
                case 'r':
                    return '\r';
                case 't':
                    return '\t';
                case 'u':
                    return parseUnicodeEscape();
                default:
                    throw error("invalid JSON escape");
            }
        }

        private char parseUnicodeEscape() {
            if (index + 4 > json.length()) {
                throw error("invalid JSON unicode escape");
            }
            int value = 0;
            for (int count = 0; count < 4; count++) {
                char ch = json.charAt(index++);
                value <<= 4;
                if (ch >= '0' && ch <= '9') {
                    value += ch - '0';
                } else if (ch >= 'a' && ch <= 'f') {
                    value += ch - 'a' + 10;
                } else if (ch >= 'A' && ch <= 'F') {
                    value += ch - 'A' + 10;
                } else {
                    throw error("invalid JSON unicode escape");
                }
            }
            return (char) value;
        }

        private Number parseNumber() {
            int start = index;
            if (tryConsume('-')) {}
            consumeDigits();
            boolean floating = false;
            if (tryConsume('.')) {
                floating = true;
                consumeDigits();
            }
            if (index < json.length()) {
                char ch = json.charAt(index);
                if (ch == 'e' || ch == 'E') {
                    floating = true;
                    index++;
                    if (index < json.length()
                            && (json.charAt(index) == '+' || json.charAt(index) == '-')) {
                        index++;
                    }
                    consumeDigits();
                }
            }
            String text = json.substring(start, index);
            try {
                return floating ? Double.parseDouble(text) : Long.parseLong(text);
            } catch (NumberFormatException error) {
                throw error("invalid JSON number");
            }
        }

        private void consumeDigits() {
            int start = index;
            while (index < json.length()) {
                char ch = json.charAt(index);
                if (ch < '0' || ch > '9') {
                    break;
                }
                index++;
            }
            if (index == start) {
                throw error("expected digit");
            }
        }

        private void expect(String literal) {
            if (!json.startsWith(literal, index)) {
                throw error("expected " + literal);
            }
            index += literal.length();
        }

        private void expect(char expected) {
            if (index >= json.length() || json.charAt(index) != expected) {
                throw error("expected " + expected);
            }
            index++;
        }

        private boolean tryConsume(char expected) {
            if (index < json.length() && json.charAt(index) == expected) {
                index++;
                return true;
            }
            return false;
        }

        private void skipWhitespace() {
            while (index < json.length()) {
                char ch = json.charAt(index);
                if (ch != ' ' && ch != '\n' && ch != '\r' && ch != '\t') {
                    return;
                }
                index++;
            }
        }

        private RototoException error(String message) {
            return new RototoException(message + " at JSON byte " + index);
        }
    }
}
