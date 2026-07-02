use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::sync::Arc;

use cel::common::ast::{EntryExpr, Expr, LiteralValue, operators};
use cel::{Context as CelContext, ExecutionError, IdedExpr, Value as CelValue};
use glob::Pattern;
use regex::Regex;
use semver::{Version, VersionReq};
use serde_json::{Number, Value as JsonValue};

use crate::error::{Result, RototoError};
use crate::predicate::{CidrBlock, parse_rfc3339_timestamp};
use crate::resolve::bucket_value;

#[derive(Clone, Debug)]
pub(crate) struct Expression {
    source: String,
    references: ExpressionReferences,
    /// The expression compiled by the `cel` engine. It drives both evaluation
    /// and the lint analysis (references, type constraints, result hint).
    cel_ast: IdedExpr,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ExpressionReferences {
    pub(crate) context_paths: BTreeSet<String>,
    pub(crate) entry_paths: BTreeSet<String>,
    pub(crate) qualifiers: BTreeSet<String>,
    /// Scalar types a context path is compared against, inferred from how the
    /// expression uses it. A path can carry more than one expectation when it is
    /// used in several places. Paths used in ways that do not pin a scalar type
    /// (for example the value argument of `bucket`) do not appear here.
    pub(crate) context_path_types: BTreeMap<String, BTreeSet<ContextScalarType>>,
    /// Root identifiers the expression uses that rototo does not provide. Lint
    /// turns these into diagnostics; evaluation would otherwise fail with cel's
    /// raw "undefined variable" error.
    pub(crate) invalid_roots: BTreeSet<ExpressionRootIssue>,
    /// Whether the expression references `env.resolving.*`, the entity being
    /// resolved. This is only available inside `[[trace]]` policies; lint rejects
    /// it elsewhere to keep qualifier/rule/query evaluation independent of the
    /// caller.
    pub(crate) uses_resolving: bool,
}

/// A reference to a root identifier that is not part of rototo's evaluation
/// environment. The expression environment exposes exactly `context`, `entry`
/// (in queries), and `env` (with members `qualifier["<id>"]` and `now`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ExpressionRootIssue {
    /// The legacy bare `qualifier["<id>"]` root, before qualifiers moved under
    /// `env`. Kept distinct so the diagnostic can point at the new spelling.
    LegacyQualifier,
    /// `env.<member>` where `<member>` is not a real env member.
    UnknownEnvMember(String),
    /// Any other unknown root identifier (e.g. a typo of `context`).
    UnknownRoot(String),
}

impl ExpressionRootIssue {
    pub(crate) fn describe(&self) -> String {
        match self {
            ExpressionRootIssue::LegacyQualifier => {
                "expression uses the legacy qualifier[\"<id>\"] root; reference qualifiers as \
                 env.qualifier[\"<id>\"]"
                    .to_owned()
            }
            ExpressionRootIssue::UnknownEnvMember(member) => {
                format!("expression references unknown env member: env.{member}")
            }
            ExpressionRootIssue::UnknownRoot(root) => {
                format!("expression references unknown identifier: {root}")
            }
        }
    }
}

/// The JSON Schema scalar families an expression can require of a context path.
///
/// `Ip` and `Timestamp` are refined string families: the path must still be a
/// string, but it additionally has to carry the matching JSON Schema `format`
/// (`ipv4`/`ipv6`, `date-time`). They are inferred when a path is used as the
/// subject of `cidr`/time functions, and — now that catalog and evaluation
/// context validators assert formats — a declared `format` is a real value-level
/// guarantee, so requiring it here keeps those functions sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ContextScalarType {
    Bool,
    Number,
    String,
    Ip,
    Timestamp,
}

impl ContextScalarType {
    /// Whether a JSON Schema `type` token names this scalar family. `integer`
    /// and `number` both satisfy a `Number` expectation; the refined string
    /// families are still `string` at the `type` level.
    pub(crate) fn matches_schema_type(self, schema_type: &str) -> bool {
        match self {
            ContextScalarType::Bool => schema_type == "boolean",
            ContextScalarType::Number => schema_type == "number" || schema_type == "integer",
            ContextScalarType::String | ContextScalarType::Ip | ContextScalarType::Timestamp => {
                schema_type == "string"
            }
        }
    }

    /// The JSON Schema `format` tokens that satisfy a refined string family. Any
    /// one of them is enough (an IP path may be declared `ipv4` or `ipv6`).
    /// Non-refined families impose no format requirement.
    pub(crate) fn required_formats(self) -> &'static [&'static str] {
        match self {
            ContextScalarType::Ip => &["ipv4", "ipv6"],
            ContextScalarType::Timestamp => &["date-time"],
            ContextScalarType::Bool | ContextScalarType::Number | ContextScalarType::String => &[],
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            ContextScalarType::Bool => "boolean",
            ContextScalarType::Number => "number",
            ContextScalarType::String => "string",
            ContextScalarType::Ip => "an IP address",
            ContextScalarType::Timestamp => "a timestamp",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExpressionParseError {
    message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExpressionResultHint {
    Bool,
    Value,
}

#[cfg(feature = "console")]
pub(crate) fn simple_rule_qualifier(expression: &str) -> Option<String> {
    let expression = strip_condition_parens(expression.trim());
    let quoted = expression
        .strip_prefix("env.qualifier[")?
        .strip_suffix(']')?
        .trim();
    serde_json::from_str::<String>(quoted).ok()
}

#[cfg(feature = "console")]
fn strip_condition_parens(mut expression: &str) -> &str {
    loop {
        let trimmed = expression.trim();
        let Some(inner) = trimmed
            .strip_prefix('(')
            .and_then(|value| value.strip_suffix(')'))
        else {
            return trimmed;
        };
        expression = inner;
    }
}

impl ExpressionParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ExpressionParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ExpressionParseError {}

impl Expression {
    pub(crate) fn parse(
        source: impl Into<String>,
    ) -> std::result::Result<Self, ExpressionParseError> {
        let source = source.into();
        let cel_ast = cel::Program::compile(&source)
            .map_err(|err| ExpressionParseError::new(err.to_string()))?
            .expression()
            .clone();
        let references = references_from_cel(&cel_ast);
        Ok(Self {
            source,
            references,
            cel_ast,
        })
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn references(&self) -> &ExpressionReferences {
        &self.references
    }

    pub(crate) fn result_hint(&self) -> ExpressionResultHint {
        result_hint_from_cel(&self.cel_ast)
    }

    pub(crate) fn evaluate_bool(
        &self,
        context: &JsonValue,
        entry: Option<&JsonValue>,
        now: &str,
        resolve_qualifier: &mut dyn FnMut(&str) -> Result<bool>,
    ) -> Result<bool> {
        let value = self.evaluate_value(context, entry, now, resolve_qualifier)?;
        value.as_bool().ok_or_else(|| {
            RototoError::new(format!(
                "expression did not evaluate to bool: {}",
                self.source
            ))
        })
    }

    pub(crate) fn evaluate_value(
        &self,
        context: &JsonValue,
        entry: Option<&JsonValue>,
        now: &str,
        resolve_qualifier: &mut dyn FnMut(&str) -> Result<bool>,
    ) -> Result<JsonValue> {
        cel_evaluate(
            &self.cel_ast,
            &self.references,
            context,
            entry,
            now,
            None,
            resolve_qualifier,
        )
    }

    /// Evaluate a `[[trace]]` policy `when` to a bool, binding the entity being
    /// resolved as `env.resolving.*`. Only trace policies may reference
    /// `env.resolving`; other call sites use [`Expression::evaluate_bool`].
    pub(crate) fn evaluate_bool_traced(
        &self,
        context: &JsonValue,
        now: &str,
        resolving: ResolvingTarget<'_>,
        resolve_qualifier: &mut dyn FnMut(&str) -> Result<bool>,
    ) -> Result<bool> {
        let value = cel_evaluate(
            &self.cel_ast,
            &self.references,
            context,
            None,
            now,
            Some(resolving),
            resolve_qualifier,
        )?;
        value.as_bool().ok_or_else(|| {
            RototoError::new(format!(
                "trace policy did not evaluate to bool: {}",
                self.source
            ))
        })
    }
}

/// The entity being resolved, exposed to a `[[trace]]` policy `when` as
/// `env.resolving.variable` / `env.resolving.qualifier`. Both keys are always
/// present in the binding (null for the inapplicable kind) so a comparison
/// against the other kind is `false` rather than a missing-key error.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ResolvingTarget<'a> {
    Variable(&'a str),
    Qualifier(&'a str),
}

impl ResolvingTarget<'_> {
    fn to_env_value(self) -> JsonValue {
        let (variable, qualifier) = match self {
            ResolvingTarget::Variable(id) => (JsonValue::String(id.to_owned()), JsonValue::Null),
            ResolvingTarget::Qualifier(id) => (JsonValue::Null, JsonValue::String(id.to_owned())),
        };
        serde_json::json!({ "variable": variable, "qualifier": qualifier })
    }
}

// ---- Lint analysis over the cel AST. ----
// rototo's lint needs to know which context/entry paths and qualifiers an
// expression references, the scalar type each context path is used as, and
// whether the expression is boolean-typed. All of this is derived from the cel
// `IdedExpr` the engine already parsed — there is no separate rototo parser.

fn references_from_cel(expr: &IdedExpr) -> ExpressionReferences {
    let mut references = ExpressionReferences::default();
    collect_cel(expr, &mut references);
    references
}

/// One pass over the cel AST: record references (context/entry paths and
/// qualifier ids) and, per context path, the scalar family the surrounding
/// operator or function requires of it.
fn collect_cel(expr: &IdedExpr, references: &mut ExpressionReferences) {
    cel_constraints(expr, &mut references.context_path_types);

    match cel_reference(expr) {
        Some(Reference::Context(path)) => {
            references.context_paths.insert(path);
            return;
        }
        Some(Reference::Entry(path)) => {
            references.entry_paths.insert(path);
            return;
        }
        Some(Reference::Qualifier(id)) => {
            references.qualifiers.insert(id);
            return;
        }
        Some(Reference::EnvNow) => {
            return;
        }
        Some(Reference::ResolvingVariable | Reference::ResolvingQualifier) => {
            references.uses_resolving = true;
            return;
        }
        None => {
            if let Some(issue) = cel_root_issue(expr) {
                references.invalid_roots.insert(issue);
                return;
            }
        }
    }

    for child in cel_children(expr) {
        collect_cel(child, references);
    }
}

fn cel_children(expr: &IdedExpr) -> Vec<&IdedExpr> {
    match &expr.expr {
        Expr::Call(call) => call
            .target
            .as_deref()
            .into_iter()
            .chain(call.args.iter())
            .collect(),
        Expr::Comprehension(comprehension) => vec![
            &comprehension.iter_range,
            &comprehension.accu_init,
            &comprehension.loop_cond,
            &comprehension.loop_step,
            &comprehension.result,
        ],
        Expr::List(list) => list.elements.iter().collect(),
        Expr::Map(map) => map
            .entries
            .iter()
            .filter_map(|entry| match &entry.expr {
                EntryExpr::MapEntry(entry) => Some([&entry.key, &entry.value]),
                EntryExpr::StructField(_) => None,
            })
            .flatten()
            .collect(),
        Expr::Select(select) => vec![&select.operand],
        Expr::Struct(structure) => structure
            .entries
            .iter()
            .filter_map(|entry| match &entry.expr {
                EntryExpr::StructField(field) => Some(&field.value),
                EntryExpr::MapEntry(_) => None,
            })
            .collect(),
        Expr::Ident(_) | Expr::Literal(_) | Expr::Unspecified => Vec::new(),
    }
}

enum Reference {
    Context(String),
    Entry(String),
    Qualifier(String),
    EnvNow,
    ResolvingVariable,
    ResolvingQualifier,
}

fn cel_reference(expr: &IdedExpr) -> Option<Reference> {
    let (root, segments) = cel_path(expr)?;
    if segments.is_empty() {
        return None;
    }
    match root.as_str() {
        "context" => Some(Reference::Context(segments.join("."))),
        "entry" => Some(Reference::Entry(segments.join("."))),
        "env" => match segments.as_slice() {
            [member] if member == "now" => Some(Reference::EnvNow),
            [first, id] if first == "qualifier" => Some(Reference::Qualifier(id.clone())),
            [first, second] if first == "resolving" && second == "variable" => {
                Some(Reference::ResolvingVariable)
            }
            [first, second] if first == "resolving" && second == "qualifier" => {
                Some(Reference::ResolvingQualifier)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Classify a root chain that [`cel_reference`] did not recognize. Only chains
/// with at least one segment are considered, so bare identifiers (such as
/// comprehension variables) are never flagged.
fn cel_root_issue(expr: &IdedExpr) -> Option<ExpressionRootIssue> {
    let (root, segments) = cel_path(expr)?;
    if segments.is_empty() {
        return None;
    }
    match root.as_str() {
        "context" | "entry" => None,
        "env" => match segments.as_slice() {
            [member] if member == "now" => None,
            [first, _] if first == "qualifier" => None,
            [first, second]
                if first == "resolving" && (second == "variable" || second == "qualifier") =>
            {
                None
            }
            _ => Some(ExpressionRootIssue::UnknownEnvMember(segments.join("."))),
        },
        "qualifier" => Some(ExpressionRootIssue::LegacyQualifier),
        other => Some(ExpressionRootIssue::UnknownRoot(other.to_owned())),
    }
}

/// Unwrap a `root.a.b` / `root["a"]["b"]` chain into its root identifier and
/// dotted segments. Returns `None` for anything that is not such a chain.
fn cel_path(expr: &IdedExpr) -> Option<(String, Vec<String>)> {
    match &expr.expr {
        Expr::Ident(name) => Some((name.clone(), Vec::new())),
        Expr::Select(select) => {
            let (root, mut segments) = cel_path(&select.operand)?;
            segments.push(select.field.clone());
            Some((root, segments))
        }
        Expr::Call(call)
            if call.func_name == operators::INDEX
                && call.target.is_none()
                && call.args.len() == 2 =>
        {
            let Expr::Literal(LiteralValue::String(key)) = &call.args[1].expr else {
                return None;
            };
            let (root, mut segments) = cel_path(&call.args[0])?;
            segments.push(key.to_string());
            Some((root, segments))
        }
        _ => None,
    }
}

/// Record the scalar family the operator or function at `expr` requires of any
/// direct context-path operand. Ambiguous uses (such as the value argument of
/// `bucket`) are intentionally left unconstrained.
fn cel_constraints(expr: &IdedExpr, types: &mut BTreeMap<String, BTreeSet<ContextScalarType>>) {
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

fn constrain_pair(
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

fn cel_context_path(expr: &IdedExpr) -> Option<String> {
    match cel_path(expr) {
        Some((root, segments)) if root == "context" && !segments.is_empty() => {
            Some(segments.join("."))
        }
        _ => None,
    }
}

fn cel_literal_scalar(expr: &IdedExpr) -> Option<ContextScalarType> {
    match &expr.expr {
        Expr::Literal(LiteralValue::Boolean(_)) => Some(ContextScalarType::Bool),
        Expr::Literal(LiteralValue::Int(_) | LiteralValue::UInt(_) | LiteralValue::Double(_)) => {
            Some(ContextScalarType::Number)
        }
        Expr::Literal(LiteralValue::String(_)) => Some(ContextScalarType::String),
        _ => None,
    }
}

fn cel_literal_ordering(expr: &IdedExpr) -> Option<ContextScalarType> {
    match cel_literal_scalar(expr) {
        Some(ContextScalarType::Bool) => None,
        other => other,
    }
}

fn result_hint_from_cel(expr: &IdedExpr) -> ExpressionResultHint {
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
            operators::INDEX => {
                if matches!(
                    cel_path(expr),
                    Some((root, segments)) if root == "env" && segments.first().map(String::as_str) == Some("qualifier")
                ) {
                    ExpressionResultHint::Bool
                } else {
                    ExpressionResultHint::Value
                }
            }
            _ => ExpressionResultHint::Bool,
        },
        _ => ExpressionResultHint::Value,
    }
}

fn string_arg0_function(name: &str) -> bool {
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
fn refined_arg0_function(name: &str) -> Option<ContextScalarType> {
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
// boolean outcome: one that makes a qualifier match, one that triggers a
// specific variable rule, one that falls through to the default. We derive that
// context straight from the cel AST by inverting the comparison shapes rototo's
// expressions are built from. Synthesis is best-effort: shapes it cannot invert
// (regex, present/missing, free-form calls) yield `None`, and every synthesized
// context is verified by real resolution before a fixture is emitted, so a wrong
// guess is discarded rather than trusted.

/// The maximum number of candidate keys tried when inverting a `bucket`
/// predicate before giving up on hitting (or avoiding) its range.
const MAX_BUCKET_CANDIDATES: usize = 100_000;

impl Expression {
    /// Build a `context` object that drives this expression to `want`.
    ///
    /// `qualifier` resolves a referenced qualifier id to a context that makes
    /// that qualifier evaluate to the requested boolean; the caller owns cycle
    /// detection. Returns `None` for expression shapes synthesis cannot invert.
    pub(crate) fn synthesize_context(
        &self,
        want: bool,
        qualifier: &mut dyn FnMut(&str, bool) -> Option<JsonValue>,
    ) -> Option<JsonValue> {
        synthesize_bool(&self.cel_ast, want, qualifier)
    }
}

fn synthesize_bool(
    expr: &IdedExpr,
    want: bool,
    qualifier: &mut dyn FnMut(&str, bool) -> Option<JsonValue>,
) -> Option<JsonValue> {
    if let Some(Reference::Qualifier(id)) = cel_reference(expr) {
        return qualifier(&id, want);
    }
    if let Some(path) = cel_context_path(expr) {
        // A bare boolean context path, e.g. `context.flags.enabled`.
        return context_with_path(&path, JsonValue::Bool(want));
    }
    match &expr.expr {
        Expr::Literal(LiteralValue::Boolean(value)) => (**value == want).then(empty_context),
        Expr::Call(call) => synthesize_call(call, want, qualifier),
        _ => None,
    }
}

fn synthesize_call(
    call: &cel::common::ast::CallExpr,
    want: bool,
    qualifier: &mut dyn FnMut(&str, bool) -> Option<JsonValue>,
) -> Option<JsonValue> {
    match call.func_name.as_str() {
        operators::LOGICAL_NOT => synthesize_bool(call.args.first()?, !want, qualifier),
        operators::LOGICAL_AND => synthesize_junction(&call.args, want, true, qualifier),
        operators::LOGICAL_OR => synthesize_junction(&call.args, want, false, qualifier),
        operators::EQUALS => synthesize_equality(&call.args, want, true),
        operators::NOT_EQUALS => synthesize_equality(&call.args, want, false),
        operators::LESS => synthesize_ordering(&call.args, false, false, want),
        operators::LESS_EQUALS => synthesize_ordering(&call.args, false, true, want),
        operators::GREATER => synthesize_ordering(&call.args, true, false, want),
        operators::GREATER_EQUALS => synthesize_ordering(&call.args, true, true, want),
        operators::IN => synthesize_membership(&call.args, want),
        "bucket" => synthesize_bucket(call, want),
        _ => None,
    }
}

/// Invert `&&` / `or`. When the junction must take the value that requires every
/// operand to agree (an `&&` that must be true, an `or` that must be false), we
/// synthesize and merge all operands; a merge conflict between two operands
/// fails the whole junction. Otherwise one satisfied operand is enough.
fn synthesize_junction(
    args: &[IdedExpr],
    want: bool,
    is_and: bool,
    qualifier: &mut dyn FnMut(&str, bool) -> Option<JsonValue>,
) -> Option<JsonValue> {
    if want == is_and {
        let mut context = empty_context();
        for arg in args {
            merge_context(&mut context, synthesize_bool(arg, want, qualifier)?)?;
        }
        Some(context)
    } else {
        args.iter()
            .find_map(|arg| synthesize_bool(arg, want, qualifier))
    }
}

/// Invert `==` / `!=` between a context path and a literal. `equal` is true for
/// `==`. The path is assigned the literal exactly when the operator's natural
/// match aligns with the wanted outcome, otherwise a deliberately different
/// value of the same shape.
fn synthesize_equality(args: &[IdedExpr], want: bool, equal: bool) -> Option<JsonValue> {
    let (path, literal) = context_path_and_literal(args)?;
    let value = if want == equal {
        literal
    } else {
        alternative_value(&literal)
    };
    context_with_path(&path, value)
}

/// Invert an ordering comparison (`<`, `<=`, `>`, `>=`) between a context path
/// and a numeric literal, picking a value one step across the boundary so the
/// comparison takes the wanted outcome.
fn synthesize_ordering(
    args: &[IdedExpr],
    is_greater: bool,
    or_equal: bool,
    want: bool,
) -> Option<JsonValue> {
    if args.len() != 2 {
        return None;
    }
    // Normalize so the relation is always read as `path <op> number`, flipping
    // the direction when the literal is written on the left.
    let (path, number, greater) = if let Some(path) = cel_context_path(&args[0]) {
        (path, cel_number(&args[1])?, is_greater)
    } else if let Some(path) = cel_context_path(&args[1]) {
        (path, cel_number(&args[0])?, !is_greater)
    } else {
        return None;
    };
    let value = ordering_value(greater, or_equal, want, &number)?;
    context_with_path(&path, value)
}

/// Invert `context.path in [a, b, ...]`: pick a listed value to match, or a
/// value outside the list to miss.
fn synthesize_membership(args: &[IdedExpr], want: bool) -> Option<JsonValue> {
    if args.len() != 2 {
        return None;
    }
    let path = cel_context_path(&args[0])?;
    let Expr::List(list) = &args[1].expr else {
        return None;
    };
    let elements: Vec<JsonValue> = list.elements.iter().filter_map(cel_literal_json).collect();
    let value = if want {
        elements.first()?.clone()
    } else {
        list_non_member(&elements)?
    };
    context_with_path(&path, value)
}

/// Invert `bucket(context.path, salt, start, end)` by scanning candidate keys
/// until one buckets inside the range (to match) or outside it (to miss).
fn synthesize_bucket(call: &cel::common::ast::CallExpr, want: bool) -> Option<JsonValue> {
    if call.args.len() != 4 {
        return None;
    }
    let path = cel_context_path(&call.args[0])?;
    let salt = cel_string_literal(&call.args[1])?;
    let start = cel_int_literal(&call.args[2])?;
    let end = cel_int_literal(&call.args[3])?;
    for index in 0..MAX_BUCKET_CANDIDATES {
        let candidate = format!("bucket-fixture-{index:05}");
        let bucket = i64::from(bucket_value(&salt, &JsonValue::String(candidate.clone())));
        if (bucket >= start && bucket < end) == want {
            return context_with_path(&path, JsonValue::String(candidate));
        }
    }
    None
}

fn context_path_and_literal(args: &[IdedExpr]) -> Option<(String, JsonValue)> {
    if args.len() != 2 {
        return None;
    }
    for (path_side, literal_side) in [(0, 1), (1, 0)] {
        if let Some(path) = cel_context_path(&args[path_side])
            && let Some(literal) = cel_literal_json(&args[literal_side])
        {
            return Some((path, literal));
        }
    }
    None
}

/// A sentinel string distinct from any realistic configuration literal, used as
/// the "some other value" when falsifying a string equality.
const FIXTURE_OTHER: &str = "fixture-other";

/// A value of the same shape as `value` but deliberately different, used to
/// falsify an equality or satisfy an inequality. For strings we return one
/// canonical sentinel rather than a per-literal one, so that independently
/// synthesized "not this value" constraints on the same path agree when merged
/// (which is what makes falsifying an `or` of equalities work).
fn alternative_value(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::String(text) if text == FIXTURE_OTHER => {
            JsonValue::String(format!("{FIXTURE_OTHER}-2"))
        }
        JsonValue::String(_) => JsonValue::String(FIXTURE_OTHER.to_owned()),
        JsonValue::Bool(flag) => JsonValue::Bool(!flag),
        JsonValue::Number(_) => {
            step_number(value, 1).unwrap_or_else(|| JsonValue::String("fixture-other".to_owned()))
        }
        _ => JsonValue::String("fixture-other".to_owned()),
    }
}

/// A value guaranteed not to be in `elements`, for missing a membership test.
fn list_non_member(elements: &[JsonValue]) -> Option<JsonValue> {
    if !elements.is_empty() && elements.iter().all(JsonValue::is_number) {
        let max = elements
            .iter()
            .filter_map(JsonValue::as_i64)
            .max()
            .unwrap_or(0);
        return Some(JsonValue::Number(Number::from(max + 1)));
    }
    [
        JsonValue::String("fixture-not-in-list".to_owned()),
        JsonValue::String("fixture-other".to_owned()),
        JsonValue::Bool(false),
    ]
    .into_iter()
    .find(|candidate| {
        elements
            .iter()
            .all(|element| !json_values_equal(element, candidate))
    })
}

/// Choose a number that makes `path <greater/less><or_equal> number` evaluate to
/// `want`, stepping one unit across the boundary where strict comparison or a
/// wanted-false outcome needs it.
fn ordering_value(
    greater: bool,
    or_equal: bool,
    want: bool,
    number: &JsonValue,
) -> Option<JsonValue> {
    let delta: i64 = match (want, or_equal, greater) {
        (true, true, _) => 0,
        (true, false, true) => 1,
        (true, false, false) => -1,
        (false, true, true) => -1,
        (false, true, false) => 1,
        (false, false, _) => 0,
    };
    step_number(number, delta)
}

fn step_number(number: &JsonValue, delta: i64) -> Option<JsonValue> {
    if let Some(value) = number.as_i64() {
        Some(JsonValue::Number(Number::from(value + delta)))
    } else if let Some(value) = number.as_u64() {
        let stepped = i128::from(value) + i128::from(delta);
        u64::try_from(stepped)
            .ok()
            .map(|n| JsonValue::Number(Number::from(n)))
    } else if let Some(value) = number.as_f64() {
        Number::from_f64(value + delta as f64).map(JsonValue::Number)
    } else {
        None
    }
}

fn cel_number(expr: &IdedExpr) -> Option<JsonValue> {
    let value = cel_literal_json(expr)?;
    value.is_number().then_some(value)
}

fn cel_literal_json(expr: &IdedExpr) -> Option<JsonValue> {
    match &expr.expr {
        Expr::Literal(literal) => literal_to_json(literal),
        _ => None,
    }
}

fn literal_to_json(literal: &LiteralValue) -> Option<JsonValue> {
    match literal {
        LiteralValue::Boolean(value) => Some(JsonValue::Bool(**value)),
        LiteralValue::Int(value) => Some(JsonValue::Number(Number::from(**value))),
        LiteralValue::UInt(value) => Some(JsonValue::Number(Number::from(**value))),
        LiteralValue::Double(value) => Number::from_f64(**value).map(JsonValue::Number),
        LiteralValue::String(value) => Some(JsonValue::String(value.to_string())),
        LiteralValue::Null | LiteralValue::Bytes(_) => None,
    }
}

fn cel_string_literal(expr: &IdedExpr) -> Option<String> {
    match &expr.expr {
        Expr::Literal(LiteralValue::String(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn cel_int_literal(expr: &IdedExpr) -> Option<i64> {
    match &expr.expr {
        Expr::Literal(LiteralValue::Int(value)) => Some(**value),
        Expr::Literal(LiteralValue::UInt(value)) => i64::try_from(**value).ok(),
        _ => None,
    }
}

/// An empty `context` object, the synthesis identity that merges absorb.
pub(crate) fn empty_context() -> JsonValue {
    JsonValue::Object(serde_json::Map::new())
}

/// Wrap `value` at the dotted context `path`, producing `{a: {b: value}}`.
/// Returns `None` for an empty or malformed path.
pub(crate) fn context_with_path(path: &str, value: JsonValue) -> Option<JsonValue> {
    let mut root = serde_json::Map::new();
    insert_context_path(&mut root, path, value)?;
    Some(JsonValue::Object(root))
}

fn insert_context_path(
    object: &mut serde_json::Map<String, JsonValue>,
    path: &str,
    value: JsonValue,
) -> Option<()> {
    let mut segments = path.split('.').peekable();
    let mut current = object;
    while let Some(segment) = segments.next() {
        if segment.is_empty() {
            return None;
        }
        if segments.peek().is_none() {
            current.insert(segment.to_owned(), value);
            return Some(());
        }
        let entry = current
            .entry(segment.to_owned())
            .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
        current = entry.as_object_mut()?;
    }
    None
}

/// Deep-merge `source` into `target`. Two objects merge recursively; a scalar
/// already present with a different value is a conflict and fails the merge, so
/// contradictory constraints (the same path needing two values) cannot produce a
/// misleading context.
pub(crate) fn merge_context(target: &mut JsonValue, source: JsonValue) -> Option<()> {
    let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) else {
        return None;
    };
    merge_context_objects(target, source)
}

fn merge_context_objects(
    target: &mut serde_json::Map<String, JsonValue>,
    source: &serde_json::Map<String, JsonValue>,
) -> Option<()> {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (Some(existing), JsonValue::Object(source_object)) if existing.is_object() => {
                merge_context_objects(existing.as_object_mut()?, source_object)?;
            }
            (Some(existing), value) if existing != value => return None,
            (Some(_), _) => {}
            (None, value) => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
    Some(())
}

// ---- Evaluation: rototo rents the `cel` engine. ----
// The hand-written tree-walking evaluator was replaced by compiling to cel and
// resolving against a Context that supplies the `context`/`entry`/`env`
// variables plus rototo's custom functions. The rototo parser/AST above is kept
// only for lint analysis (references and type constraints).

type FnResult = std::result::Result<CelValue, ExecutionError>;

fn cel_evaluate(
    cel_ast: &IdedExpr,
    references: &ExpressionReferences,
    context: &JsonValue,
    entry: Option<&JsonValue>,
    now: &str,
    resolving: Option<ResolvingTarget<'_>>,
    resolve_qualifier: &mut dyn FnMut(&str) -> Result<bool>,
) -> Result<JsonValue> {
    let mut ctx = CelContext::default();
    register_functions(&mut ctx);
    ctx.add_variable_from_value("context", to_cel(context)?);
    ctx.add_variable_from_value("entry", to_cel(&entry.cloned().unwrap_or(JsonValue::Null))?);

    // `env` holds the values rototo provides to every expression. `env.now` is
    // the evaluation timestamp captured once per resolution. `env.qualifier["id"]`
    // reads a precomputed map: only the qualifiers the expression references are
    // resolved (through the same callback as before, which owns cycle detection);
    // cel then indexes that map. `env.resolving` is present only for trace
    // policies; it names the entity being resolved.
    let mut qualifiers = serde_json::Map::new();
    for id in &references.qualifiers {
        qualifiers.insert(id.clone(), JsonValue::Bool(resolve_qualifier(id)?));
    }
    let mut env = serde_json::json!({
        "now": now,
        "qualifier": JsonValue::Object(qualifiers),
    });
    if let Some(target) = resolving {
        env["resolving"] = target.to_env_value();
    }
    ctx.add_variable_from_value("env", to_cel(&env)?);

    let value = ctx
        .resolve(cel_ast)
        .map_err(|err| RototoError::new(format!("expression evaluation failed: {err}")))?;
    value
        .json()
        .map_err(|err| RototoError::new(format!("expression result is not JSON: {err}")))
}

fn to_cel(value: &JsonValue) -> Result<CelValue> {
    cel::to_value(value)
        .map_err(|err| RototoError::new(format!("value is not representable in cel: {err}")))
}

fn register_functions(ctx: &mut CelContext) {
    ctx.add_function("startsWith", fn_starts_with);
    ctx.add_function("starts_with", fn_starts_with);
    ctx.add_function("prefix", fn_starts_with);
    ctx.add_function("endsWith", fn_ends_with);
    ctx.add_function("ends_with", fn_ends_with);
    ctx.add_function("suffix", fn_ends_with);
    ctx.add_function("contains", fn_contains);
    ctx.add_function("matches", fn_matches);
    ctx.add_function("regex", fn_matches);
    ctx.add_function("glob", fn_glob);
    ctx.add_function("semver", fn_semver);
    ctx.add_function("bucket", fn_bucket);
    ctx.add_function("cidr", fn_cidr);
    ctx.add_function("present", fn_present);
    ctx.add_function("missing", fn_missing);
    ctx.add_function("path", fn_path);
    ctx.add_function("size", fn_size);
    ctx.add_function("timeAfter", fn_time_after);
    ctx.add_function("time_after", fn_time_after);
    ctx.add_function("timeAtOrAfter", fn_time_at_or_after);
    ctx.add_function("time_at_or_after", fn_time_at_or_after);
    ctx.add_function("timeBefore", fn_time_before);
    ctx.add_function("time_before", fn_time_before);
    ctx.add_function("timeAtOrBefore", fn_time_at_or_before);
    ctx.add_function("time_at_or_before", fn_time_at_or_before);
    ctx.add_function("timeBetween", fn_time_between);
    ctx.add_function("time_between", fn_time_between);
}

fn fn_starts_with(a: Arc<String>, b: Arc<String>) -> bool {
    a.starts_with(b.as_str())
}

fn fn_ends_with(a: Arc<String>, b: Arc<String>) -> bool {
    a.ends_with(b.as_str())
}

fn fn_contains(a: CelValue, b: CelValue) -> FnResult {
    Ok(contains_value(&cel_json("contains", &a)?, &cel_json("contains", &b)?).into())
}

fn fn_matches(a: Arc<String>, b: Arc<String>) -> FnResult {
    let re = Regex::new(&b).map_err(|err| ExecutionError::function_error("matches", err))?;
    Ok(re.is_match(&a).into())
}

fn fn_glob(a: Arc<String>, b: Arc<String>) -> FnResult {
    let pattern = Pattern::new(&b).map_err(|err| ExecutionError::function_error("glob", err))?;
    Ok(pattern.matches(&a).into())
}

fn fn_semver(a: Arc<String>, b: Arc<String>) -> FnResult {
    let version =
        Version::parse(&a).map_err(|err| ExecutionError::function_error("semver", err))?;
    let requirement =
        VersionReq::parse(&b).map_err(|err| ExecutionError::function_error("semver", err))?;
    Ok(requirement.matches(&version).into())
}

fn fn_bucket(value: CelValue, salt: Arc<String>, start: i64, end: i64) -> FnResult {
    let bucket = bucket_value(&salt, &cel_json("bucket", &value)?);
    Ok((i64::from(bucket) >= start && i64::from(bucket) < end).into())
}

fn fn_cidr(ip: Arc<String>, blocks: CelValue) -> FnResult {
    let addr = ip
        .parse::<IpAddr>()
        .map_err(|err| ExecutionError::function_error("cidr", err))?;
    let blocks = cidr_blocks(&cel_json("cidr", &blocks)?)?;
    Ok(blocks.iter().any(|block| block.contains(addr)).into())
}

fn fn_present(obj: CelValue, pointer: Arc<String>) -> FnResult {
    Ok(cel_json("present", &obj)?
        .pointer(&pointer)
        .is_some()
        .into())
}

fn fn_missing(obj: CelValue, pointer: Arc<String>) -> FnResult {
    Ok(cel_json("missing", &obj)?
        .pointer(&pointer)
        .is_none()
        .into())
}

fn fn_path(obj: CelValue, pointer: Arc<String>) -> FnResult {
    let found = cel_json("path", &obj)?
        .pointer(&pointer)
        .cloned()
        .ok_or_else(|| {
            ExecutionError::function_error("path", format!("did not find JSON Pointer: {pointer}"))
        })?;
    cel::to_value(&found).map_err(|err| ExecutionError::function_error("path", err))
}

fn fn_size(value: CelValue) -> FnResult {
    let len = match cel_json("size", &value)? {
        JsonValue::Array(values) => values.len(),
        JsonValue::Object(values) => values.len(),
        JsonValue::String(value) => value.chars().count(),
        _ => {
            return Err(ExecutionError::function_error(
                "size",
                "requires an array, object, or string",
            ));
        }
    };
    Ok((len as i64).into())
}

fn fn_time_after(a: Arc<String>, b: Arc<String>) -> FnResult {
    Ok((parse_ts("timeAfter", &a)? > parse_ts("timeAfter", &b)?).into())
}

fn fn_time_at_or_after(a: Arc<String>, b: Arc<String>) -> FnResult {
    Ok((parse_ts("timeAtOrAfter", &a)? >= parse_ts("timeAtOrAfter", &b)?).into())
}

fn fn_time_before(a: Arc<String>, b: Arc<String>) -> FnResult {
    Ok((parse_ts("timeBefore", &a)? < parse_ts("timeBefore", &b)?).into())
}

fn fn_time_at_or_before(a: Arc<String>, b: Arc<String>) -> FnResult {
    Ok((parse_ts("timeAtOrBefore", &a)? <= parse_ts("timeAtOrBefore", &b)?).into())
}

fn fn_time_between(a: Arc<String>, lo: Arc<String>, hi: Arc<String>) -> FnResult {
    let actual = parse_ts("timeBetween", &a)?;
    Ok((actual >= parse_ts("timeBetween", &lo)? && actual < parse_ts("timeBetween", &hi)?).into())
}

fn parse_ts(
    name: &str,
    value: &str,
) -> std::result::Result<crate::predicate::Rfc3339Timestamp, ExecutionError> {
    parse_rfc3339_timestamp(value).ok_or_else(|| {
        ExecutionError::function_error(name, "argument must be an RFC3339 timestamp")
    })
}

fn cel_json(name: &str, value: &CelValue) -> std::result::Result<JsonValue, ExecutionError> {
    value
        .json()
        .map_err(|err| ExecutionError::function_error(name, err))
}

fn cidr_blocks(value: &JsonValue) -> std::result::Result<Vec<CidrBlock>, ExecutionError> {
    let values = match value {
        JsonValue::String(value) => vec![value.as_str()],
        JsonValue::Array(values) => values
            .iter()
            .map(|value| {
                value.as_str().ok_or_else(|| {
                    ExecutionError::function_error(
                        "cidr",
                        "CIDR argument must be a string or list of strings",
                    )
                })
            })
            .collect::<std::result::Result<Vec<_>, _>>()?,
        _ => {
            return Err(ExecutionError::function_error(
                "cidr",
                "CIDR argument must be a string or list of strings",
            ));
        }
    };
    values
        .into_iter()
        .map(|value| {
            CidrBlock::parse(value).ok_or_else(|| {
                ExecutionError::function_error("cidr", format!("CIDR block is invalid: {value}"))
            })
        })
        .collect()
}

fn contains_value(left: &JsonValue, right: &JsonValue) -> bool {
    match (left, right) {
        (JsonValue::String(left), JsonValue::String(right)) => left.contains(right),
        (JsonValue::Array(left), right) => left.iter().any(|value| json_values_equal(value, right)),
        _ => false,
    }
}

fn json_values_equal(left: &JsonValue, right: &JsonValue) -> bool {
    match (left, right) {
        (JsonValue::Number(left), JsonValue::Number(right)) => json_numbers_equal(left, right),
        _ => left == right,
    }
}

fn json_numbers_equal(left: &Number, right: &Number) -> bool {
    if left == right {
        return true;
    }
    match (
        left.as_i64(),
        left.as_u64(),
        left.as_f64(),
        right.as_i64(),
        right.as_u64(),
        right.as_f64(),
    ) {
        (Some(left), _, _, Some(right), _, _) => left == right,
        (_, Some(left), _, _, Some(right), _) => left == right,
        (Some(left), _, _, _, _, Some(right)) => i64_f64_equal(left, right),
        (_, Some(left), _, _, _, Some(right)) => u64_f64_equal(left, right),
        (_, _, Some(left), Some(right), _, _) => i64_f64_equal(right, left),
        (_, _, Some(left), _, Some(right), _) => u64_f64_equal(right, left),
        (_, _, Some(left), _, _, Some(right)) => left == right,
        _ => false,
    }
}

fn i64_f64_equal(integer: i64, float: f64) -> bool {
    float.is_finite()
        && float.fract() == 0.0
        && (float as i64) == integer
        && (integer as f64) == float
}

fn u64_f64_equal(integer: u64, float: f64) -> bool {
    float.is_finite()
        && float.fract() == 0.0
        && float >= 0.0
        && (float as u64) == integer
        && (integer as f64) == float
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// A fixed `env.now` so tests stay deterministic.
    const TEST_NOW: &str = "2026-06-29T00:00:00Z";

    fn eval_bool(source: &str, context: &JsonValue, entry: Option<&JsonValue>) -> Result<bool> {
        eval_bool_with_qualifiers(source, context, entry, &[])
    }

    fn eval_bool_with_qualifiers(
        source: &str,
        context: &JsonValue,
        entry: Option<&JsonValue>,
        qualifiers: &[(&str, bool)],
    ) -> Result<bool> {
        let expr = Expression::parse(source).unwrap();
        let mut resolve_qualifier = |id: &str| {
            qualifiers
                .iter()
                .find(|(qualifier, _)| *qualifier == id)
                .map(|(_, value)| *value)
                .ok_or_else(|| RototoError::new(format!("unknown qualifier: {id}")))
        };
        expr.evaluate_bool(context, entry, TEST_NOW, &mut resolve_qualifier)
    }

    fn eval_value(
        source: &str,
        context: &JsonValue,
        entry: Option<&JsonValue>,
    ) -> Result<JsonValue> {
        let expr = Expression::parse(source).unwrap();
        let mut resolve_qualifier = |_id: &str| Ok(false);
        expr.evaluate_value(context, entry, TEST_NOW, &mut resolve_qualifier)
    }

    fn string_set(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_and_evaluates_basic_expression() {
        let expr =
            Expression::parse(r#"context.user.tier == "premium" && context.account.seats >= 10"#)
                .unwrap();
        let context = serde_json::json!({
            "user": { "tier": "premium" },
            "account": { "seats": 12 }
        });
        fn qualifier(_: &str) -> Result<bool> {
            Ok(false)
        }
        let mut qualifier = qualifier;
        assert!(
            expr.evaluate_bool(&context, None, TEST_NOW, &mut qualifier)
                .unwrap()
        );
    }

    fn context_types(source: &str) -> BTreeMap<String, BTreeSet<ContextScalarType>> {
        Expression::parse(source)
            .unwrap()
            .references()
            .context_path_types
            .clone()
    }

    #[test]
    fn infers_context_path_scalar_types_from_use() {
        use ContextScalarType::{Bool, Number, String};

        let eq = context_types(r#"context.user.tier == "premium""#);
        assert_eq!(eq.get("user.tier"), Some(&BTreeSet::from([String])));

        let ordering = context_types("context.account.seats >= 100");
        assert_eq!(
            ordering.get("account.seats"),
            Some(&BTreeSet::from([Number]))
        );

        let membership = context_types(r#"context.device.platform in ["ios","android"]"#);
        assert_eq!(
            membership.get("device.platform"),
            Some(&BTreeSet::from([String]))
        );

        let boolean = context_types("context.flags.enabled && context.user.tier == \"premium\"");
        assert_eq!(boolean.get("flags.enabled"), Some(&BTreeSet::from([Bool])));
        assert_eq!(boolean.get("user.tier"), Some(&BTreeSet::from([String])));

        let function = context_types(r#"semver(context.app.version, ">=1.2.0")"#);
        assert_eq!(function.get("app.version"), Some(&BTreeSet::from([String])));
    }

    #[test]
    fn infers_refined_string_types_from_cidr_and_time_functions() {
        use ContextScalarType::{Ip, Timestamp};

        let cidr = context_types(r#"cidr(context.user.ip, "10.0.0.0/8")"#);
        assert_eq!(cidr.get("user.ip"), Some(&BTreeSet::from([Ip])));

        let time = context_types(
            r#"timeBefore(context.window.start, "2026-01-01T00:00:00Z")
               && timeBetween(context.window.now, "2026-01-01T00:00:00Z", "2027-01-01T00:00:00Z")"#,
        );
        assert_eq!(time.get("window.start"), Some(&BTreeSet::from([Timestamp])));
        assert_eq!(time.get("window.now"), Some(&BTreeSet::from([Timestamp])));

        // semver stays a plain string: there is no enforced JSON Schema format.
        let semver = context_types(r#"semver(context.app.version, ">=1.0.0")"#);
        assert_eq!(
            semver.get("app.version"),
            Some(&BTreeSet::from([ContextScalarType::String]))
        );
    }

    #[test]
    fn leaves_bucket_value_argument_unconstrained() {
        let types = context_types(r#"bucket(context.user.id, "salt", 0, 1000)"#);
        assert!(
            !types.contains_key("user.id"),
            "bucket's value argument should not pin a scalar type: {types:?}"
        );
    }

    #[test]
    fn records_conflicting_uses_as_multiple_expectations() {
        use ContextScalarType::{Number, String};
        let types = context_types(r#"context.x == "a" && context.x >= 5"#);
        assert_eq!(types.get("x"), Some(&BTreeSet::from([String, Number])));
    }

    #[test]
    fn tracks_qualifier_and_entry_references() {
        let expr = Expression::parse(
            r#"env.qualifier["enterprise-accounts"] && entry.id == "hero" && context.region == "eu""#,
        )
        .unwrap();
        assert!(expr.references().qualifiers.contains("enterprise-accounts"));
        assert!(expr.references().entry_paths.contains("id"));
        assert!(expr.references().context_paths.contains("region"));
    }

    #[test]
    fn evaluates_env_members() {
        let context = serde_json::json!({});
        // env.now is the RFC3339 timestamp threaded into evaluation; it reads as
        // a plain string and feeds the time functions.
        assert!(eval_bool(r#"env.now == "2026-06-29T00:00:00Z""#, &context, None).unwrap());
        assert!(
            eval_bool(
                r#"timeAtOrAfter(env.now, "2020-01-01T00:00:00Z")"#,
                &context,
                None,
            )
            .unwrap()
        );
        // env.qualifier indexes the resolved qualifier map.
        assert!(
            eval_bool_with_qualifiers(
                r#"env.qualifier["beta"]"#,
                &context,
                None,
                &[("beta", true)],
            )
            .unwrap()
        );
    }

    #[test]
    fn flags_invalid_expression_roots() {
        use ExpressionRootIssue::{LegacyQualifier, UnknownEnvMember, UnknownRoot};

        let legacy = Expression::parse(r#"qualifier["x"]"#).unwrap();
        assert!(legacy.references().invalid_roots.contains(&LegacyQualifier));

        let bad_env = Expression::parse("env.bogus").unwrap();
        assert!(
            bad_env
                .references()
                .invalid_roots
                .contains(&UnknownEnvMember("bogus".to_owned()))
        );

        let unknown = Expression::parse("foo.bar").unwrap();
        assert!(
            unknown
                .references()
                .invalid_roots
                .contains(&UnknownRoot("foo".to_owned()))
        );

        // Valid roots produce no issues.
        let ok = Expression::parse(
            r#"env.qualifier["x"] && env.now == "" && context.a == 1 && entry.b == 2"#,
        )
        .unwrap();
        assert!(ok.references().invalid_roots.is_empty());
    }

    #[test]
    fn evaluates_logical_precedence_and_short_circuiting() {
        let context = serde_json::json!({});

        assert!(eval_bool("true || false && false", &context, None).unwrap());
        assert!(!eval_bool("(true || false) && false", &context, None).unwrap());
        assert!(eval_bool("!false && (false || true)", &context, None).unwrap());

        let expr = Expression::parse(r#"true || env.qualifier["must-not-run"]"#).unwrap();
        // Qualifiers referenced by an expression are resolved eagerly (the cel
        // engine indexes a precomputed map), so the resolver runs regardless of
        // short-circuiting; it simply returns a value here.
        let mut resolve_qualifier = |id: &str| {
            let _ = id;
            Ok(false)
        };
        assert!(
            expr.evaluate_bool(&context, None, TEST_NOW, &mut resolve_qualifier)
                .unwrap()
        );

        let expr = Expression::parse(r#"false && env.qualifier["must-not-run"]"#).unwrap();
        // Qualifiers referenced by an expression are resolved eagerly (the cel
        // engine indexes a precomputed map), so the resolver runs regardless of
        // short-circuiting; it simply returns a value here.
        let mut resolve_qualifier = |id: &str| {
            let _ = id;
            Ok(false)
        };
        assert!(
            !expr
                .evaluate_bool(&context, None, TEST_NOW, &mut resolve_qualifier)
                .unwrap()
        );
    }

    #[test]
    fn evaluates_comparison_membership_and_json_equality() {
        let context = serde_json::json!({
            "enabled": true,
            "optional": null,
            "seats": 42,
            "ratio": 2.5,
            "tier": "premium",
            "tags": ["a", "b"]
        });

        let cases = [
            (r#"context.seats == 42.0"#, true),
            (r#"context.seats != 43"#, true),
            (r#"context.seats < 43 && context.seats <= 42"#, true),
            (r#"context.ratio > 2 && context.ratio >= 2.5"#, true),
            (r#""bravo" > "alpha" && "alpha" <= "alpha""#, true),
            (r#"context.tier in ["free", "premium"]"#, true),
            (r#""b" in context.tags"#, true),
            (
                r#"context.optional == null && context.enabled == true"#,
                true,
            ),
            (r#"context.tags == ["a", "b"]"#, true),
            // Heterogeneous equality is false (not an error) under cel.
            (r#"context.seats == "42""#, false),
            // Cross-type ordering (`context.tier > 10`) and membership in a
            // non-collection (`context.tier in "premium"`) are no-overload
            // errors in cel, and the schema-aware checker rejects them at lint;
            // they are not exercised here.
        ];

        for (source, expected) in cases {
            assert_eq!(
                eval_bool(source, &context, None).unwrap(),
                expected,
                "{source}"
            );
        }
    }

    #[test]
    fn evaluates_context_paths_entry_paths_and_qualifiers() {
        let context = serde_json::json!({
            "account.plan": "enterprise",
            "account": {
                "seat-count": 12
            },
            "channel": "email"
        });
        let entry = serde_json::json!({
            "channel": "email",
            "active": true,
            "limits": {
                "daily": 100
            }
        });

        assert!(
            eval_bool(
                r#"context["account.plan"] == "enterprise" && context.account["seat-count"] == 12"#,
                &context,
                None,
            )
            .unwrap()
        );
        assert!(
            eval_bool(
                r#"entry.channel == context.channel && entry.active == true && entry.limits.daily >= 100"#,
                &context,
                Some(&entry),
            )
            .unwrap()
        );
        assert!(
            eval_bool_with_qualifiers(
                r#"env.qualifier["enterprise-accounts"] && env.qualifier["mobile-users"]"#,
                &context,
                None,
                &[("enterprise-accounts", true), ("mobile-users", true)],
            )
            .unwrap()
        );
    }

    #[test]
    fn evaluates_supported_functions() {
        let context = serde_json::json!({
            "user": {
                "id": "user-42",
                "email": "owner@rototo.dev",
                "ip": "192.168.1.10",
                "version": "1.4.2",
                "created_at": "2026-06-21T12:30:00Z"
            },
            "payload": {
                "features": ["checkout", "support"],
                "nested": { "name": "rototo" }
            },
            "tags": ["alpha", "beta"]
        });

        let cases = [
            (r#"has(context.user.id)"#, true),
            (r#"has(context.user.missing)"#, false),
            (r#"present(context.payload, "/features/0")"#, true),
            (r#"missing(context.payload, "/features/3")"#, true),
            (r#"startsWith(context.user.email, "owner@")"#, true),
            (r#"ends_with(context.user.email, ".dev")"#, true),
            (r#"contains(context.user.email, "rototo")"#, true),
            (r#"contains(context.tags, "beta")"#, true),
            (
                r#"matches(context.user.email, "^[^@]+@rototo\\.dev$")"#,
                true,
            ),
            (r#"glob(context.user.email, "*@rototo.dev")"#, true),
            (r#"semver(context.user.version, ">=1.0, <2.0")"#, true),
            (
                r#"timeBetween(context.user.created_at, "2026-06-21T00:00:00Z", "2026-06-22T00:00:00Z")"#,
                true,
            ),
            (r#"cidr(context.user.ip, "192.168.1.0/24")"#, true),
            (
                r#"cidr(context.user.ip, ["10.0.0.0/8", "192.168.0.0/16"])"#,
                true,
            ),
            (r#"bucket(context.user.id, "rollout", 0, 65536)"#, true),
            (r#"bucket(context.user.id, "rollout", 65536, 65537)"#, false),
        ];

        for (source, expected) in cases {
            assert_eq!(
                eval_bool(source, &context, None).unwrap(),
                expected,
                "{source}"
            );
        }

        assert_eq!(
            eval_value(r#"path(context.payload, "/nested/name")"#, &context, None).unwrap(),
            serde_json::json!("rototo")
        );
        assert_eq!(
            eval_value("size(context.tags)", &context, None).unwrap(),
            serde_json::json!(2)
        );
    }

    #[test]
    fn rejects_malformed_expressions_at_parse() {
        // Syntactically malformed expressions fail to compile. Exact messages
        // come from the cel parser, so the contract is "rejected at parse".
        // (Bare unknown identifiers like `account.tier` are valid cel and are
        // caught later by the schema-aware reference checks, not here.)
        let malformed = [
            r#"context.user.tier = "premium""#, // single `=`
            "context.user.",                    // trailing dot
            r#"context.user.tier == "premium"#, // unterminated string
            "true false",                       // two expressions
            "(context.user.tier",               // unbalanced paren
        ];

        for source in malformed {
            assert!(
                Expression::parse(source).is_err(),
                "{source}: expected a parse error"
            );
        }
    }

    #[test]
    fn reports_evaluation_errors_with_stable_messages() {
        let context = serde_json::json!({
            "user": {
                "tier": "premium"
            },
            "payload": {}
        });

        // These all fail at evaluation. Exact messages now come from the cel
        // engine, so the contract is "evaluation errors", not a specific string.
        let error_cases = [
            "context.user.missing == true",                 // missing context key
            "entry.channel == \"email\"",                   // no entry provided
            "context.user.tier && true",                    // non-bool operand
            "unknown_fn(context.user.tier)",                // unknown function
            "size(true)",                                   // size of a non-collection
            r#"path(context.payload, "/missing") == true"#, // missing JSON pointer
            r#"regex(context.user.tier, "[")"#,             // invalid regex
            r#"cidr(context.user.tier, "not-cidr")"#,       // invalid ip
        ];

        for source in error_cases {
            assert!(
                eval_bool(source, &context, None).is_err(),
                "{source}: expected an evaluation error"
            );
        }

        let err = eval_bool(r#""premium""#, &context, None).unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"expression did not evaluate to bool: "premium""#
        );
    }

    #[test]
    fn extracts_references_from_nested_paths_functions_and_qualifiers() {
        let expr = Expression::parse(
            r#"
            env.qualifier["enterprise-accounts"]
                && env.qualifier["mobile-users"]
                && has(context.user.tier)
                && context.request.country in ["DE", "NL"]
                && entry.metadata.channel == context.channel
                && path(entry.payload, "/title") == "Welcome"
            "#,
        )
        .unwrap();
        let references = expr.references();

        assert_eq!(
            references.qualifiers,
            string_set(&["enterprise-accounts", "mobile-users"])
        );
        assert_eq!(
            references.context_paths,
            string_set(&["channel", "request.country", "user.tier"])
        );
        assert_eq!(
            references.entry_paths,
            string_set(&["metadata.channel", "payload"])
        );
    }

    /// Synthesize a context for `source` with no qualifier composition.
    fn synth(source: &str, want: bool) -> Option<JsonValue> {
        Expression::parse(source)
            .unwrap()
            .synthesize_context(want, &mut |_, _| None)
    }

    /// Synthesizing for an outcome and evaluating against the result must
    /// reproduce that outcome. This round-trip is the property fixtures rely on.
    fn assert_round_trip(source: &str) {
        for want in [true, false] {
            let context = synth(source, want)
                .unwrap_or_else(|| panic!("expected synthesis for {source} (want={want})"));
            assert_eq!(
                eval_bool(source, &context, None).unwrap(),
                want,
                "synthesized context {context} for {source} did not evaluate to {want}",
            );
        }
    }

    #[test]
    fn synthesizes_equality_and_inequality() {
        assert_round_trip(r#"context.account.tier == "standard""#);
        assert_round_trip(r#"context.account.tier != "free""#);
        assert_round_trip("context.flags.enabled");
    }

    #[test]
    fn synthesizes_orderings() {
        assert_round_trip("context.account.seats >= 100");
        assert_round_trip("context.cart.total_usd > 250");
        assert_round_trip("context.user.age < 18");
        // Literal written on the left flips the relation direction.
        assert_round_trip("100 <= context.account.seats");
    }

    #[test]
    fn synthesizes_membership() {
        assert_round_trip(r#"context.request.country in ["DE", "FR", "ES"]"#);
        assert_round_trip("context.account.seats in [10, 20, 30]");
    }

    #[test]
    fn synthesizes_boolean_composition() {
        assert_round_trip(r#"context.user.tier == "premium" && context.account.seats >= 100"#);
        assert_round_trip(r#"context.lane == "dev" || context.lane == "stage""#);
        assert_round_trip(r#"!(context.user.tier == "free")"#);
    }

    #[test]
    fn synthesizes_bucket() {
        assert_round_trip(r#"bucket(context.user.id, "rollout-salt", 0, 1000)"#);
    }

    #[test]
    fn synthesizes_through_qualifier_composition() {
        // `env.qualifier["premium"]` is satisfied by recursively synthesizing the
        // referenced qualifier's own expression and merging its context in.
        let premium = Expression::parse(r#"context.user.tier == "premium""#).unwrap();
        let source = r#"env.qualifier["premium"] && context.account.seats >= 50"#;
        let expr = Expression::parse(source).unwrap();
        let context = expr
            .synthesize_context(true, &mut |id, want| {
                assert_eq!(id, "premium");
                premium.synthesize_context(want, &mut |_, _| None)
            })
            .expect("expected composed synthesis");

        let mut resolve_qualifier = |id: &str| match id {
            "premium" => premium.evaluate_bool(&context, None, TEST_NOW, &mut |_| Ok(false)),
            other => Err(RototoError::new(format!("unexpected qualifier: {other}"))),
        };
        assert!(
            expr.evaluate_bool(&context, None, TEST_NOW, &mut resolve_qualifier)
                .unwrap()
        );
    }

    #[test]
    fn returns_none_for_uninvertible_shapes() {
        // A free-form string function the synthesizer does not model.
        assert!(synth(r#"context.user.email.endsWith("@rototo.dev")"#, true).is_none());
    }
}
