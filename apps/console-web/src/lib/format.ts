// Instants come off the wire as RFC3339 strings with millisecond noise and
// assorted offsets; people read "when", not serialization detail.

export function formatInstant(value: string): string {
    const parsed = new Date(value);
    if (Number.isNaN(parsed.getTime())) {
        return value;
    }
    const iso = parsed.toISOString();
    return `${iso.slice(0, 10)} ${iso.slice(11, 16)} UTC`;
}
