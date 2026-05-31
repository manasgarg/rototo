use crate::diagnostics::LintDiagnostic;

pub(super) fn sort_diagnostics(diagnostics: &mut [LintDiagnostic]) {
    diagnostics.sort_by(|left, right| diagnostic_sort_key(left).cmp(&diagnostic_sort_key(right)));
}

fn diagnostic_sort_key(diagnostic: &LintDiagnostic) -> (u8, &str, usize, usize, String, &str) {
    let location_rank = match diagnostic.primary.kind {
        crate::diagnostics::DiagnosticLocationKind::WorkspaceRoot => 0,
        crate::diagnostics::DiagnosticLocationKind::Document
        | crate::diagnostics::DiagnosticLocationKind::Span => 1,
    };
    let (line, character) = diagnostic.primary.byte_start().map_or_else(
        || {
            diagnostic
                .primary
                .range
                .map(|range| (range.start.line, range.start.character))
                .unwrap_or((0, 0))
        },
        |byte_start| (byte_start, 0),
    );
    (
        location_rank,
        diagnostic.primary.path.as_str(),
        line,
        character,
        diagnostic.rule.as_string(),
        diagnostic.message.as_str(),
    )
}
