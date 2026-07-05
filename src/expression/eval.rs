use super::*;

pub(super) type FnResult = std::result::Result<CelValue, ExecutionError>;

pub(super) fn cel_evaluate(
    cel_ast: &IdedExpr,
    references: &ExpressionReferences,
    context: &JsonValue,
    entry: Option<&JsonValue>,
    now: &str,
    resolving: Option<ResolvingTarget<'_>>,
    refs: &mut dyn RefResolver,
) -> Result<JsonValue> {
    let mut ctx = CelContext::default();
    register_functions(&mut ctx);
    ctx.add_variable_from_value("context", to_cel(context)?);
    ctx.add_variable_from_value("entry", to_cel(&entry.cloned().unwrap_or(JsonValue::Null))?);

    // `env` holds the values rototo provides to every expression. `env.now` is
    // the evaluation timestamp captured once per resolution. `env.resolving`
    // is present only for trace policies; it names the entity being resolved.
    let mut env = serde_json::json!({
        "now": now,
    });
    if let Some(target) = resolving {
        env["resolving"] = target.to_env_value();
    }
    ctx.add_variable_from_value("env", to_cel(&env)?);

    // `variables` binds the resolved value of every variable the expression
    // references, so expressions compose over other variables. Only referenced
    // ids are resolved; the resolver owns memoization and cycle detection.
    let mut variables = serde_json::Map::new();
    for id in &references.variables {
        variables.insert(id.clone(), refs.variable_value(id)?);
    }
    ctx.add_variable_from_value("variables", to_cel(&JsonValue::Object(variables))?);

    // `enums` binds the member list of every enum the expression references,
    // so membership tests can name the set (`context.plan in enums.plan_tiers`)
    // instead of restating its literals.
    let mut enums = serde_json::Map::new();
    for id in &references.enums {
        enums.insert(id.clone(), refs.enum_members(id)?);
    }
    ctx.add_variable_from_value("enums", to_cel(&JsonValue::Object(enums))?);

    let value = ctx
        .resolve(cel_ast)
        .map_err(|err| RototoError::new(format!("expression evaluation failed: {err}")))?;
    value
        .json()
        .map_err(|err| RototoError::new(format!("expression result is not JSON: {err}")))
}

pub(super) fn to_cel(value: &JsonValue) -> Result<CelValue> {
    cel::to_value(value)
        .map_err(|err| RototoError::new(format!("value is not representable in cel: {err}")))
}

pub(super) fn register_functions(ctx: &mut CelContext) {
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

pub(super) fn fn_starts_with(a: Arc<String>, b: Arc<String>) -> bool {
    a.starts_with(b.as_str())
}

pub(super) fn fn_ends_with(a: Arc<String>, b: Arc<String>) -> bool {
    a.ends_with(b.as_str())
}

pub(super) fn fn_contains(a: CelValue, b: CelValue) -> FnResult {
    Ok(contains_value(&cel_json("contains", &a)?, &cel_json("contains", &b)?).into())
}

pub(super) fn fn_matches(a: Arc<String>, b: Arc<String>) -> FnResult {
    let re = Regex::new(&b).map_err(|err| ExecutionError::function_error("matches", err))?;
    Ok(re.is_match(&a).into())
}

pub(super) fn fn_glob(a: Arc<String>, b: Arc<String>) -> FnResult {
    let pattern = Pattern::new(&b).map_err(|err| ExecutionError::function_error("glob", err))?;
    Ok(pattern.matches(&a).into())
}

pub(super) fn fn_semver(a: Arc<String>, b: Arc<String>) -> FnResult {
    let version =
        Version::parse(&a).map_err(|err| ExecutionError::function_error("semver", err))?;
    let requirement =
        VersionReq::parse(&b).map_err(|err| ExecutionError::function_error("semver", err))?;
    Ok(requirement.matches(&version).into())
}

pub(super) fn fn_bucket(value: CelValue, salt: Arc<String>, start: i64, end: i64) -> FnResult {
    let bucket = bucket_value(&salt, &cel_json("bucket", &value)?);
    Ok((i64::from(bucket) >= start && i64::from(bucket) < end).into())
}

pub(super) fn fn_cidr(ip: Arc<String>, blocks: CelValue) -> FnResult {
    let addr = ip
        .parse::<IpAddr>()
        .map_err(|err| ExecutionError::function_error("cidr", err))?;
    let blocks = cidr_blocks(&cel_json("cidr", &blocks)?)?;
    Ok(blocks.iter().any(|block| block.contains(addr)).into())
}

pub(super) fn fn_present(obj: CelValue, pointer: Arc<String>) -> FnResult {
    Ok(cel_json("present", &obj)?
        .pointer(&pointer)
        .is_some()
        .into())
}

pub(super) fn fn_missing(obj: CelValue, pointer: Arc<String>) -> FnResult {
    Ok(cel_json("missing", &obj)?
        .pointer(&pointer)
        .is_none()
        .into())
}

pub(super) fn fn_path(obj: CelValue, pointer: Arc<String>) -> FnResult {
    let found = cel_json("path", &obj)?
        .pointer(&pointer)
        .cloned()
        .ok_or_else(|| {
            ExecutionError::function_error("path", format!("did not find JSON Pointer: {pointer}"))
        })?;
    cel::to_value(&found).map_err(|err| ExecutionError::function_error("path", err))
}

pub(super) fn fn_size(value: CelValue) -> FnResult {
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

pub(super) fn fn_time_after(a: Arc<String>, b: Arc<String>) -> FnResult {
    Ok((parse_ts("timeAfter", &a)? > parse_ts("timeAfter", &b)?).into())
}

pub(super) fn fn_time_at_or_after(a: Arc<String>, b: Arc<String>) -> FnResult {
    Ok((parse_ts("timeAtOrAfter", &a)? >= parse_ts("timeAtOrAfter", &b)?).into())
}

pub(super) fn fn_time_before(a: Arc<String>, b: Arc<String>) -> FnResult {
    Ok((parse_ts("timeBefore", &a)? < parse_ts("timeBefore", &b)?).into())
}

pub(super) fn fn_time_at_or_before(a: Arc<String>, b: Arc<String>) -> FnResult {
    Ok((parse_ts("timeAtOrBefore", &a)? <= parse_ts("timeAtOrBefore", &b)?).into())
}

pub(super) fn fn_time_between(a: Arc<String>, lo: Arc<String>, hi: Arc<String>) -> FnResult {
    let actual = parse_ts("timeBetween", &a)?;
    Ok((actual >= parse_ts("timeBetween", &lo)? && actual < parse_ts("timeBetween", &hi)?).into())
}

pub(super) fn parse_ts(
    name: &str,
    value: &str,
) -> std::result::Result<crate::predicate::Rfc3339Timestamp, ExecutionError> {
    parse_rfc3339_timestamp(value).ok_or_else(|| {
        ExecutionError::function_error(name, "argument must be an RFC3339 timestamp")
    })
}

pub(super) fn cel_json(
    name: &str,
    value: &CelValue,
) -> std::result::Result<JsonValue, ExecutionError> {
    value
        .json()
        .map_err(|err| ExecutionError::function_error(name, err))
}

pub(super) fn cidr_blocks(
    value: &JsonValue,
) -> std::result::Result<Vec<CidrBlock>, ExecutionError> {
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

pub(super) fn contains_value(left: &JsonValue, right: &JsonValue) -> bool {
    match (left, right) {
        (JsonValue::String(left), JsonValue::String(right)) => left.contains(right),
        (JsonValue::Array(left), right) => left.iter().any(|value| json_values_equal(value, right)),
        _ => false,
    }
}

pub(super) fn json_values_equal(left: &JsonValue, right: &JsonValue) -> bool {
    match (left, right) {
        (JsonValue::Number(left), JsonValue::Number(right)) => json_numbers_equal(left, right),
        _ => left == right,
    }
}

pub(super) fn json_numbers_equal(left: &Number, right: &Number) -> bool {
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

pub(super) fn i64_f64_equal(integer: i64, float: f64) -> bool {
    float.is_finite()
        && float.fract() == 0.0
        && (float as i64) == integer
        && (integer as f64) == float
}

pub(super) fn u64_f64_equal(integer: u64, float: f64) -> bool {
    float.is_finite()
        && float.fract() == 0.0
        && float >= 0.0
        && (float as u64) == integer
        && (integer as f64) == float
}
