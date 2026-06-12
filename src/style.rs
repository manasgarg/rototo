//! Terminal styling for human-facing CLI output, following rototo's CLI
//! design system: status glyphs (`✓ ~ + − ! ✗ →`), the sea/ok/warn/err/info/
//! cyan/dim color roles from the terminal palette, and uppercase mono
//! micro-labels for section headers.
//!
//! Styling applies only on an interactive terminal. When stdout is piped,
//! `NO_COLOR` is set, or output is JSON, every helper degrades to the plain
//! text the CLI has always printed, so scripts and tests keep a stable
//! contract. `FORCE_COLOR` opts back in for non-TTY captures.

use std::io::IsTerminal;
use std::sync::OnceLock;

use rototo::diagnostics::Severity;

static ENABLED: OnceLock<bool> = OnceLock::new();

pub(crate) fn init() {
    let enabled = if std::env::var_os("NO_COLOR").is_some() {
        false
    } else if std::env::var_os("FORCE_COLOR").is_some() {
        true
    } else {
        std::io::stdout().is_terminal()
    };
    let _ = ENABLED.set(enabled);
}

fn enabled() -> bool {
    *ENABLED.get_or_init(|| false)
}

/* The terminal palette from the design system (term-* tokens, oklch converted
to sRGB). */
const SEA: (u8, u8, u8) = (0, 207, 177);
const OK: (u8, u8, u8) = (79, 222, 122);
const WARN: (u8, u8, u8) = (253, 197, 0);
const ERR: (u8, u8, u8) = (255, 103, 95);
const INFO: (u8, u8, u8) = (53, 196, 255);
const DIM: (u8, u8, u8) = (133, 143, 141);

fn paint(color: (u8, u8, u8), text: &str) -> String {
    if !enabled() || text.is_empty() {
        return text.to_owned();
    }
    format!(
        "\x1b[38;2;{};{};{}m{}\x1b[0m",
        color.0, color.1, color.2, text
    )
}

pub(crate) fn sea(text: &str) -> String {
    paint(SEA, text)
}

pub(crate) fn ok(text: &str) -> String {
    paint(OK, text)
}

pub(crate) fn warn(text: &str) -> String {
    paint(WARN, text)
}

pub(crate) fn err(text: &str) -> String {
    paint(ERR, text)
}

pub(crate) fn info(text: &str) -> String {
    paint(INFO, text)
}

pub(crate) fn dim(text: &str) -> String {
    paint(DIM, text)
}

pub(crate) fn bold(text: &str) -> String {
    if !enabled() {
        return text.to_owned();
    }
    format!("\x1b[1m{text}\x1b[0m")
}

/// The uppercase mono micro-label motif for section headers. Plain output
/// keeps the original lowercase `name:` form.
pub(crate) fn label(text: &str) -> String {
    if !enabled() {
        return format!("{text}:");
    }
    paint(DIM, &text.to_uppercase())
}

/// `✓ <text>` on a terminal; `ok: <text>` when plain.
pub(crate) fn ok_line(text: &str) -> String {
    if !enabled() {
        return format!("ok: {text}");
    }
    format!("{} {}", ok("✓"), text)
}

/// `✗ <text>` on a terminal; `error: <text>` when plain.
pub(crate) fn err_line(text: &str) -> String {
    if !enabled() {
        return format!("error: {text}");
    }
    format!("{} {}", err("✗"), text)
}

/// Severity prefix for a diagnostic line: glyph + tinted rule id on a
/// terminal, the original `severity[rule]` form when plain.
pub(crate) fn severity_prefix(severity: &Severity, rule: &str) -> String {
    if !enabled() {
        let label = match severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        return format!("{label}[{rule}]");
    }
    match severity {
        Severity::Error => format!("{} {}", err("✗"), err(rule)),
        Severity::Warning => format!("{} {}", warn("!"), warn(rule)),
    }
}

/// `→` on a terminal, `->` when plain.
pub(crate) fn arrow() -> &'static str {
    if enabled() { "→" } else { "->" }
}

/// The hairline separator between entities.
pub(crate) fn hairline() -> String {
    if !enabled() {
        return "  ----------------------------------------".to_owned();
    }
    format!("  {}", dim(&"─".repeat(40)))
}
