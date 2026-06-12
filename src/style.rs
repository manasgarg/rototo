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
static TRUECOLOR: OnceLock<bool> = OnceLock::new();

pub(crate) fn init() {
    let enabled = if std::env::var_os("NO_COLOR").is_some() {
        false
    } else if std::env::var_os("FORCE_COLOR").is_some() {
        true
    } else {
        std::io::stdout().is_terminal()
    };
    let _ = ENABLED.set(enabled);
    let truecolor = std::env::var("COLORTERM")
        .map(|value| value.contains("truecolor") || value.contains("24bit"))
        .unwrap_or(false);
    let _ = TRUECOLOR.set(truecolor);
}

pub(crate) fn enabled() -> bool {
    *ENABLED.get_or_init(|| false)
}

fn truecolor() -> bool {
    *TRUECOLOR.get_or_init(|| false)
}

/* The terminal palette from the design system (term-* tokens, oklch converted
to sRGB), each with the nearest xterm-256 index for terminals without
truecolor support. */
pub(crate) struct Role {
    rgb: (u8, u8, u8),
    ansi256: u8,
}

pub(crate) const SEA: Role = Role {
    rgb: (0, 207, 177),
    ansi256: 43,
};
pub(crate) const OK: Role = Role {
    rgb: (79, 222, 122),
    ansi256: 78,
};
pub(crate) const WARN: Role = Role {
    rgb: (253, 197, 0),
    ansi256: 220,
};
pub(crate) const ERR: Role = Role {
    rgb: (255, 103, 95),
    ansi256: 203,
};
pub(crate) const INFO: Role = Role {
    rgb: (53, 196, 255),
    ansi256: 39,
};
pub(crate) const CYAN: Role = Role {
    rgb: (52, 221, 229),
    ansi256: 80,
};
pub(crate) const DIM: Role = Role {
    rgb: (133, 143, 141),
    ansi256: 245,
};

fn paint(role: &Role, text: &str) -> String {
    if !enabled() || text.is_empty() {
        return text.to_owned();
    }
    if truecolor() {
        format!(
            "\x1b[38;2;{};{};{}m{}\x1b[0m",
            role.rgb.0, role.rgb.1, role.rgb.2, text
        )
    } else {
        format!("\x1b[38;5;{}m{}\x1b[0m", role.ansi256, text)
    }
}

fn paint_bold(role: &Role, text: &str) -> String {
    if !enabled() || text.is_empty() {
        return text.to_owned();
    }
    if truecolor() {
        format!(
            "\x1b[1;38;2;{};{};{}m{}\x1b[0m",
            role.rgb.0, role.rgb.1, role.rgb.2, text
        )
    } else {
        format!("\x1b[1;38;5;{}m{}\x1b[0m", role.ansi256, text)
    }
}

pub(crate) fn sea(text: &str) -> String {
    paint(&SEA, text)
}

pub(crate) fn sea_bold(text: &str) -> String {
    paint_bold(&SEA, text)
}

pub(crate) fn ok(text: &str) -> String {
    paint(&OK, text)
}

pub(crate) fn warn(text: &str) -> String {
    paint(&WARN, text)
}

pub(crate) fn err(text: &str) -> String {
    paint(&ERR, text)
}

pub(crate) fn info(text: &str) -> String {
    paint(&INFO, text)
}

pub(crate) fn cyan(text: &str) -> String {
    paint(&CYAN, text)
}

pub(crate) fn dim(text: &str) -> String {
    paint(&DIM, text)
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
    paint(&DIM, &text.to_uppercase())
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

/// Renders documentation markdown for the terminal: headings in bold sea,
/// fenced code blocks framed with rounded corners, inline code in cyan,
/// internal links emphasized. Plain output returns the markdown unchanged so
/// piping stays markdown-friendly.
pub(crate) fn render_markdown(markdown: &str) -> String {
    if !enabled() {
        return markdown.to_owned();
    }
    let mut out = String::new();
    let mut in_code = false;
    for line in markdown.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("```") {
            if in_code {
                out.push_str(&dim("╰───"));
            } else {
                let lang = rest.trim();
                if lang.is_empty() {
                    out.push_str(&dim("╭───"));
                } else {
                    out.push_str(&format!("{} {}", dim("╭───"), dim(lang)));
                }
            }
            in_code = !in_code;
            out.push('\n');
            continue;
        }
        if in_code {
            out.push_str(&format!("{} {}", dim("│"), cyan(line)));
            out.push('\n');
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            let text = trimmed[level..].trim_start();
            if level <= 1 {
                out.push_str(&sea_bold(text));
            } else {
                out.push_str(&bold(&sea(text)));
            }
            out.push('\n');
            continue;
        }
        out.push_str(&render_inline_markdown(line));
        out.push('\n');
    }
    out
}

/// Inline spans: `code` in cyan, **bold** in bold.
fn render_inline_markdown(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        let (before, after_tick) = rest.split_at(start);
        out.push_str(&render_bold_spans(before));
        match after_tick[1..].find('`') {
            Some(end) => {
                out.push_str(&cyan(&after_tick[1..1 + end]));
                rest = &after_tick[end + 2..];
            }
            None => {
                out.push_str(after_tick);
                return out;
            }
        }
    }
    out.push_str(&render_bold_spans(rest));
    out
}

fn render_bold_spans(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("**") {
        let (before, after) = rest.split_at(start);
        out.push_str(before);
        match after[2..].find("**") {
            Some(end) => {
                out.push_str(&bold(&after[2..2 + end]));
                rest = &after[end + 4..];
            }
            None => {
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}
