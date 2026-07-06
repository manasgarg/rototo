//! Time-boundary analysis: where an expression compares `env.now` against a
//! literal instant. The `upcoming_changes` lint surface uses this to find
//! behavior that is scheduled to change when those instants pass.

use super::references::{Reference, cel_children, cel_reference};
use super::*;
use crate::predicate::Rfc3339Timestamp;

/// One literal instant an expression tests `env.now` against.
#[derive(Clone, Debug)]
pub(crate) struct TimeBoundary {
    /// The instant exactly as the expression spells it.
    pub(crate) instant: String,
    /// The parsed instant, for ordering against a reference "now".
    pub(crate) timestamp: Rfc3339Timestamp,
    /// The function or operator doing the comparison
    /// (`timeAtOrAfter`, `timeBetween`, `>=`, ...).
    pub(crate) comparison: String,
}

impl Expression {
    /// Every literal instant this expression compares `env.now` against,
    /// through the time functions or a bare comparison operator. Instants
    /// compared against anything other than `env.now` (for example a
    /// caller-supplied `context.<path>`) are not boundaries: they do not
    /// flip on their own as the clock advances.
    pub(crate) fn time_boundaries(&self) -> Vec<TimeBoundary> {
        let mut boundaries = Vec::new();
        collect_boundaries(&self.cel_ast, &mut boundaries);
        boundaries
    }
}

/// The two-argument time functions, in every registered spelling.
const TIME_COMPARISONS: [&str; 8] = [
    "timeAfter",
    "time_after",
    "timeAtOrAfter",
    "time_at_or_after",
    "timeBefore",
    "time_before",
    "timeAtOrBefore",
    "time_at_or_before",
];

fn collect_boundaries(expr: &IdedExpr, boundaries: &mut Vec<TimeBoundary>) {
    if let Expr::Call(call) = &expr.expr {
        let name = call.func_name.as_str();
        if TIME_COMPARISONS.contains(&name) && call.args.len() == 2 {
            push_pair(name, &call.args[0], &call.args[1], boundaries);
        } else if (name == "timeBetween" || name == "time_between")
            && call.args.len() == 3
            && is_env_now(&call.args[0])
        {
            push_literal(name, &call.args[1], boundaries);
            push_literal(name, &call.args[2], boundaries);
        } else if let Some(operator) = comparison_operator(name)
            && call.args.len() == 2
        {
            push_pair(operator, &call.args[0], &call.args[1], boundaries);
        }
    }
    for child in cel_children(expr) {
        collect_boundaries(child, boundaries);
    }
}

/// A boundary needs `env.now` on one side and a literal instant on the other,
/// in either order.
fn push_pair(comparison: &str, a: &IdedExpr, b: &IdedExpr, boundaries: &mut Vec<TimeBoundary>) {
    if is_env_now(a) {
        push_literal(comparison, b, boundaries);
    } else if is_env_now(b) {
        push_literal(comparison, a, boundaries);
    }
}

fn push_literal(comparison: &str, expr: &IdedExpr, boundaries: &mut Vec<TimeBoundary>) {
    let Expr::Literal(LiteralValue::String(instant)) = &expr.expr else {
        return;
    };
    let Some(timestamp) = parse_rfc3339_timestamp(instant) else {
        return;
    };
    boundaries.push(TimeBoundary {
        instant: instant.to_string(),
        timestamp,
        comparison: comparison.to_owned(),
    });
}

fn is_env_now(expr: &IdedExpr) -> bool {
    matches!(cel_reference(expr), Some(Reference::EnvNow))
}

fn comparison_operator(func_name: &str) -> Option<&'static str> {
    match func_name {
        name if name == operators::EQUALS => Some("=="),
        name if name == operators::NOT_EQUALS => Some("!="),
        name if name == operators::LESS => Some("<"),
        name if name == operators::LESS_EQUALS => Some("<="),
        name if name == operators::GREATER => Some(">"),
        name if name == operators::GREATER_EQUALS => Some(">="),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::Expression;

    fn boundaries(source: &str) -> Vec<(String, String)> {
        let expression = Expression::parse(source).unwrap();
        expression
            .time_boundaries()
            .into_iter()
            .map(|boundary| (boundary.comparison, boundary.instant))
            .collect()
    }

    #[test]
    fn finds_time_function_boundaries_on_env_now() {
        assert_eq!(
            boundaries(r#"timeAtOrAfter(env.now, "2027-10-01T00:00:00Z")"#),
            vec![(
                "timeAtOrAfter".to_owned(),
                "2027-10-01T00:00:00Z".to_owned()
            )]
        );
        // Reversed argument order still names a boundary.
        assert_eq!(
            boundaries(r#"timeBefore("2027-01-01T00:00:00Z", env.now)"#),
            vec![("timeBefore".to_owned(), "2027-01-01T00:00:00Z".to_owned())]
        );
        // A window contributes both edges.
        assert_eq!(
            boundaries(r#"timeBetween(env.now, "2027-01-01T00:00:00Z", "2027-02-01T00:00:00Z")"#),
            vec![
                ("timeBetween".to_owned(), "2027-01-01T00:00:00Z".to_owned()),
                ("timeBetween".to_owned(), "2027-02-01T00:00:00Z".to_owned()),
            ]
        );
    }

    #[test]
    fn finds_bare_comparison_boundaries() {
        assert_eq!(
            boundaries(r#"env.now >= "2027-06-01T00:00:00Z""#),
            vec![(">=".to_owned(), "2027-06-01T00:00:00Z".to_owned())]
        );
        // Nested inside logical structure.
        assert_eq!(
            boundaries(
                r#"context.tier == "premium" && (env.now < "2027-06-01T00:00:00Z" || variables["beta"])"#
            ),
            vec![("<".to_owned(), "2027-06-01T00:00:00Z".to_owned())]
        );
    }

    #[test]
    fn ignores_non_boundaries() {
        // context-side comparisons do not flip as the clock advances.
        assert!(boundaries(r#"context.starts_at >= "2027-06-01T00:00:00Z""#).is_empty());
        // Not an RFC3339 instant.
        assert!(boundaries(r#"env.now >= "someday""#).is_empty());
        // env.now against another expression, not a literal.
        assert!(boundaries(r#"env.now >= context.starts_at"#).is_empty());
    }
}
