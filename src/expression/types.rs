use super::*;

/// Record the scalar family the operator or function at `expr` requires of any
/// direct context-path operand. Ambiguous uses (such as the value argument of
/// `bucket`) are intentionally left unconstrained.
pub(super) fn cel_constraints(
    expr: &IdedExpr,
    types: &mut BTreeMap<String, BTreeSet<ContextScalarType>>,
) {
    let Expr::Call(call) = &expr.expr else {
        return;
    };
    match call.func_name.as_str() {
        operators::EQUALS | operators::NOT_EQUALS => {
            constrain_pair(&call.args, types, cel_literal_scalar);
        }
        operators::LESS
        | operators::LESS_EQUALS
        | operators::GREATER
        | operators::GREATER_EQUALS => {
            constrain_pair(&call.args, types, cel_literal_ordering);
        }
        operators::IN => {
            if call.args.len() == 2
                && let Some(path) = cel_context_path(&call.args[0])
                && let Expr::List(list) = &call.args[1].expr
            {
                for element in &list.elements {
                    if let Some(scalar) = cel_literal_scalar(element) {
                        types.entry(path.clone()).or_default().insert(scalar);
                    }
                }
            }
        }
        operators::LOGICAL_AND | operators::LOGICAL_OR | operators::LOGICAL_NOT => {
            for arg in &call.args {
                if let Some(path) = cel_context_path(arg) {
                    types
                        .entry(path)
                        .or_default()
                        .insert(ContextScalarType::Bool);
                }
            }
        }
        name if string_arg0_function(name) => {
            if let Some(path) = call.args.first().and_then(cel_context_path) {
                types
                    .entry(path)
                    .or_default()
                    .insert(ContextScalarType::String);
            }
        }
        name => {
            if let Some(refined) = refined_arg0_function(name)
                && let Some(path) = call.args.first().and_then(cel_context_path)
            {
                types.entry(path).or_default().insert(refined);
            }
        }
    }
}

pub(super) fn constrain_pair(
    args: &[IdedExpr],
    types: &mut BTreeMap<String, BTreeSet<ContextScalarType>>,
    classify: fn(&IdedExpr) -> Option<ContextScalarType>,
) {
    if args.len() != 2 {
        return;
    }
    for (path_side, literal_side) in [(0, 1), (1, 0)] {
        if let Some(path) = cel_context_path(&args[path_side])
            && let Some(scalar) = classify(&args[literal_side])
        {
            types.entry(path).or_default().insert(scalar);
        }
    }
}

pub(super) fn cel_context_path(expr: &IdedExpr) -> Option<String> {
    match cel_path(expr) {
        Some((root, segments)) if root == "context" && !segments.is_empty() => {
            Some(segments.join("."))
        }
        _ => None,
    }
}

pub(super) fn cel_literal_scalar(expr: &IdedExpr) -> Option<ContextScalarType> {
    match &expr.expr {
        Expr::Literal(LiteralValue::Boolean(_)) => Some(ContextScalarType::Bool),
        Expr::Literal(LiteralValue::Int(_) | LiteralValue::UInt(_) | LiteralValue::Double(_)) => {
            Some(ContextScalarType::Number)
        }
        Expr::Literal(LiteralValue::String(_)) => Some(ContextScalarType::String),
        _ => None,
    }
}

pub(super) fn cel_literal_ordering(expr: &IdedExpr) -> Option<ContextScalarType> {
    match cel_literal_scalar(expr) {
        Some(ContextScalarType::Bool) => None,
        other => other,
    }
}

pub(super) fn result_hint_from_cel(expr: &IdedExpr) -> ExpressionResultHint {
    match &expr.expr {
        Expr::Literal(LiteralValue::Boolean(_)) => ExpressionResultHint::Bool,
        Expr::Call(call) => match call.func_name.as_str() {
            operators::LOGICAL_AND
            | operators::LOGICAL_OR
            | operators::LOGICAL_NOT
            | operators::EQUALS
            | operators::NOT_EQUALS
            | operators::LESS
            | operators::LESS_EQUALS
            | operators::GREATER
            | operators::GREATER_EQUALS
            | operators::IN => ExpressionResultHint::Bool,
            "path" | "size" => ExpressionResultHint::Value,
            operators::INDEX => ExpressionResultHint::Value,
            _ => ExpressionResultHint::Bool,
        },
        _ => ExpressionResultHint::Value,
    }
}

pub(super) fn string_arg0_function(name: &str) -> bool {
    matches!(
        name,
        "startsWith"
            | "starts_with"
            | "prefix"
            | "endsWith"
            | "ends_with"
            | "suffix"
            | "matches"
            | "regex"
            | "glob"
            | "semver"
    )
}

/// Functions whose first argument is a refined string: a CIDR test reads an IP,
/// the time comparisons read a timestamp. The path inherits the matching
/// JSON Schema `format` requirement. `semver` stays a plain string in
/// [`string_arg0_function`] because JSON Schema has no standard version format
/// the validators can enforce on the value.
pub(super) fn refined_arg0_function(name: &str) -> Option<ContextScalarType> {
    match name {
        "cidr" | "inCidr" | "in_cidr" => Some(ContextScalarType::Ip),
        "timeAfter" | "time_after" | "timeAtOrAfter" | "time_at_or_after" | "timeBefore"
        | "time_before" | "timeAtOrBefore" | "time_at_or_before" | "timeBetween"
        | "time_between" => Some(ContextScalarType::Timestamp),
        _ => None,
    }
}

// ---- Outcome-driven context synthesis. ----
// Fixtures need a `context` object that drives an expression to a chosen
// boolean outcome: one that makes a condition variable match, one that triggers a
// specific variable rule, one that falls through to the default. We derive that
// context straight from the cel AST by inverting the comparison shapes rototo's
// expressions are built from. Synthesis is best-effort: shapes it cannot invert
// (regex, present/missing, free-form calls) yield `None`, and every synthesized
// context is verified by real resolution before a fixture is emitted, so a wrong
// guess is discarded rather than trusted.
