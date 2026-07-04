use super::*;

/// The maximum number of candidate keys tried when inverting a `bucket`
/// predicate before giving up on hitting (or avoiding) its range.
pub(super) const MAX_BUCKET_CANDIDATES: usize = 100_000;

impl Expression {
    /// Build a `context` object that drives this expression to `want`.
    ///
    /// `variable` resolves a referenced bool variable to a context that makes
    /// it evaluate to the requested boolean; the caller owns cycle detection.
    /// Returns `None` for expression shapes synthesis cannot invert.
    pub(crate) fn synthesize_context(
        &self,
        want: bool,
        variable: &mut dyn FnMut(&str, bool) -> Option<JsonValue>,
    ) -> Option<JsonValue> {
        synthesize_bool(&self.cel_ast, want, variable)
    }
}

pub(super) fn synthesize_bool(
    expr: &IdedExpr,
    want: bool,
    variable: &mut dyn FnMut(&str, bool) -> Option<JsonValue>,
) -> Option<JsonValue> {
    if let Some(Reference::Variable(id)) = cel_reference(expr) {
        return variable(&id, want);
    }
    if let Some(path) = cel_context_path(expr) {
        // A bare boolean context path, e.g. `context.flags.enabled`.
        return context_with_path(&path, JsonValue::Bool(want));
    }
    match &expr.expr {
        Expr::Literal(LiteralValue::Boolean(value)) => (**value == want).then(empty_context),
        Expr::Call(call) => synthesize_call(call, want, variable),
        _ => None,
    }
}

pub(super) fn synthesize_call(
    call: &cel::common::ast::CallExpr,
    want: bool,
    variable: &mut dyn FnMut(&str, bool) -> Option<JsonValue>,
) -> Option<JsonValue> {
    match call.func_name.as_str() {
        operators::LOGICAL_NOT => synthesize_bool(call.args.first()?, !want, variable),
        operators::LOGICAL_AND => synthesize_junction(&call.args, want, true, variable),
        operators::LOGICAL_OR => synthesize_junction(&call.args, want, false, variable),
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
pub(super) fn synthesize_junction(
    args: &[IdedExpr],
    want: bool,
    is_and: bool,
    variable: &mut dyn FnMut(&str, bool) -> Option<JsonValue>,
) -> Option<JsonValue> {
    if want == is_and {
        let mut context = empty_context();
        for arg in args {
            merge_context(&mut context, synthesize_bool(arg, want, variable)?)?;
        }
        Some(context)
    } else {
        args.iter()
            .find_map(|arg| synthesize_bool(arg, want, variable))
    }
}

/// Invert `==` / `!=` between a context path and a literal. `equal` is true for
/// `==`. The path is assigned the literal exactly when the operator's natural
/// match aligns with the wanted outcome, otherwise a deliberately different
/// value of the same shape.
pub(super) fn synthesize_equality(args: &[IdedExpr], want: bool, equal: bool) -> Option<JsonValue> {
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
pub(super) fn synthesize_ordering(
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
pub(super) fn synthesize_membership(args: &[IdedExpr], want: bool) -> Option<JsonValue> {
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
pub(super) fn synthesize_bucket(
    call: &cel::common::ast::CallExpr,
    want: bool,
) -> Option<JsonValue> {
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

pub(super) fn context_path_and_literal(args: &[IdedExpr]) -> Option<(String, JsonValue)> {
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
pub(super) fn alternative_value(value: &JsonValue) -> JsonValue {
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
pub(super) fn list_non_member(elements: &[JsonValue]) -> Option<JsonValue> {
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
pub(super) fn ordering_value(
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

pub(super) fn step_number(number: &JsonValue, delta: i64) -> Option<JsonValue> {
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

pub(super) fn cel_number(expr: &IdedExpr) -> Option<JsonValue> {
    let value = cel_literal_json(expr)?;
    value.is_number().then_some(value)
}

pub(super) fn cel_literal_json(expr: &IdedExpr) -> Option<JsonValue> {
    match &expr.expr {
        Expr::Literal(literal) => literal_to_json(literal),
        _ => None,
    }
}

pub(super) fn literal_to_json(literal: &LiteralValue) -> Option<JsonValue> {
    match literal {
        LiteralValue::Boolean(value) => Some(JsonValue::Bool(**value)),
        LiteralValue::Int(value) => Some(JsonValue::Number(Number::from(**value))),
        LiteralValue::UInt(value) => Some(JsonValue::Number(Number::from(**value))),
        LiteralValue::Double(value) => Number::from_f64(**value).map(JsonValue::Number),
        LiteralValue::String(value) => Some(JsonValue::String(value.to_string())),
        LiteralValue::Null | LiteralValue::Bytes(_) => None,
    }
}

pub(super) fn cel_string_literal(expr: &IdedExpr) -> Option<String> {
    match &expr.expr {
        Expr::Literal(LiteralValue::String(value)) => Some(value.to_string()),
        _ => None,
    }
}

pub(super) fn cel_int_literal(expr: &IdedExpr) -> Option<i64> {
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

pub(super) fn insert_context_path(
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

pub(super) fn merge_context_objects(
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
