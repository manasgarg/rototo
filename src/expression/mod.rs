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
    /// Variable ids referenced through the `variables` root
    /// (`variables.some_id` / `variables["some_id"]`). The referenced variable's
    /// resolved value is bound in place, so expressions compose over other
    /// variables.
    pub(crate) variables: BTreeSet<String>,
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
    /// it elsewhere to keep rule and query evaluation independent of the caller.
    pub(crate) uses_resolving: bool,
}

/// A reference to a root identifier that is not part of rototo's evaluation
/// environment. The expression environment exposes exactly `context`, `entry`
/// (in queries), `variables`, and `env` (with member `now`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ExpressionRootIssue {
    /// The retired qualifier roots (`qualifier["<id>"]` and
    /// `env.qualifier["<id>"]`). Qualifiers dissolved into bool variables;
    /// kept distinct so the diagnostic can point at the replacement.
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
                "expression uses the retired qualifier root; qualifiers dissolved into bool \
                 variables, referenced as variables[\"<id>\"]"
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
        refs: &mut dyn RefResolver,
    ) -> Result<bool> {
        let value = self.evaluate_value(context, entry, now, refs)?;
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
        refs: &mut dyn RefResolver,
    ) -> Result<JsonValue> {
        cel_evaluate(
            &self.cel_ast,
            &self.references,
            context,
            entry,
            now,
            None,
            refs,
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
        refs: &mut dyn RefResolver,
    ) -> Result<bool> {
        let value = cel_evaluate(
            &self.cel_ast,
            &self.references,
            context,
            None,
            now,
            Some(resolving),
            refs,
        )?;
        value.as_bool().ok_or_else(|| {
            RototoError::new(format!(
                "trace policy did not evaluate to bool: {}",
                self.source
            ))
        })
    }
}

/// Resolves the ids an expression references to their resolved values.
/// Implemented by the resolution state (memoized, cycle-checked) and by small
/// adapters at call sites that cannot or must not resolve references.
pub(crate) trait RefResolver {
    fn variable_value(&mut self, id: &str) -> Result<JsonValue>;
}

/// The entity being resolved, exposed to a `[[trace]]` policy `when` as
/// `env.resolving.variable`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ResolvingTarget<'a> {
    Variable(&'a str),
}

impl ResolvingTarget<'_> {
    fn to_env_value(self) -> JsonValue {
        let ResolvingTarget::Variable(id) = self;
        serde_json::json!({ "variable": id })
    }
}

// ---- Lint analysis over the cel AST. ----
// rototo's lint needs to know which context/entry paths and variables an
// expression references, the scalar type each context path is used as, and
// whether the expression is boolean-typed. All of this is derived from the cel
// `IdedExpr` the engine already parsed — there is no separate rototo parser.

mod eval;
mod references;
mod synthesize;
mod types;

use eval::*;
use references::*;
pub(crate) use synthesize::{empty_context, merge_context};
use types::*;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// A fixed `env.now` so tests stay deterministic.
    const TEST_NOW: &str = "2026-06-29T00:00:00Z";

    /// A [`RefResolver`] over a fixed test table.
    struct TestRefs<'a> {
        variables: &'a [(&'a str, JsonValue)],
    }

    impl RefResolver for TestRefs<'_> {
        fn variable_value(&mut self, id: &str) -> Result<JsonValue> {
            self.variables
                .iter()
                .find(|(variable, _)| *variable == id)
                .map(|(_, value)| value.clone())
                .ok_or_else(|| RototoError::new(format!("unknown variable: {id}")))
        }
    }

    fn eval_bool(source: &str, context: &JsonValue, entry: Option<&JsonValue>) -> Result<bool> {
        eval_bool_with_variables(source, context, entry, &[])
    }

    fn eval_bool_with_variables(
        source: &str,
        context: &JsonValue,
        entry: Option<&JsonValue>,
        variables: &[(&str, JsonValue)],
    ) -> Result<bool> {
        let expr = Expression::parse(source).unwrap();
        let mut refs = TestRefs { variables };
        expr.evaluate_bool(context, entry, TEST_NOW, &mut refs)
    }

    fn eval_value(
        source: &str,
        context: &JsonValue,
        entry: Option<&JsonValue>,
    ) -> Result<JsonValue> {
        let expr = Expression::parse(source).unwrap();
        let mut refs = TestRefs { variables: &[] };
        expr.evaluate_value(context, entry, TEST_NOW, &mut refs)
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
        let mut refs = TestRefs { variables: &[] };
        assert!(
            expr.evaluate_bool(&context, None, TEST_NOW, &mut refs)
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
    fn tracks_variable_and_entry_references() {
        let expr = Expression::parse(
            r#"variables["enterprise_accounts"] && entry.id == "hero" && context.region == "eu""#,
        )
        .unwrap();
        assert!(expr.references().variables.contains("enterprise_accounts"));
        assert!(expr.references().entry_paths.contains("id"));
        assert!(expr.references().context_paths.contains("region"));
    }

    #[test]
    fn tracks_variable_references_in_both_spellings() {
        let expr = Expression::parse(
            r#"variables.premium_user && variables["beta_cohort"] && "sso" in variables.plan_features"#,
        )
        .unwrap();
        assert_eq!(
            expr.references().variables,
            string_set(&["premium_user", "beta_cohort", "plan_features"])
        );
        // The variables root is provided by rototo, never an unknown root, and
        // extra trailing segments select into the referenced variable's value.
        let nested = Expression::parse(r#"variables.limits.max_seats >= 5"#).unwrap();
        assert!(nested.references().invalid_roots.is_empty());
        assert_eq!(nested.references().variables, string_set(&["limits"]));
    }

    #[test]
    fn evaluates_variable_references() {
        let context = serde_json::json!({});
        let expr =
            Expression::parse(r#"variables["premium_user"] && variables.message == "on""#).unwrap();
        let mut refs = TestRefs {
            variables: &[
                ("premium_user", JsonValue::Bool(true)),
                ("message", serde_json::json!("on")),
            ],
        };
        assert!(
            expr.evaluate_bool(&context, None, TEST_NOW, &mut refs)
                .unwrap()
        );

        // Selecting into a referenced variable's structured value.
        let expr = Expression::parse(r#"variables.limits.max_seats >= 5"#).unwrap();
        let mut refs = TestRefs {
            variables: &[("limits", serde_json::json!({ "max_seats": 12 }))],
        };
        assert!(
            expr.evaluate_bool(&context, None, TEST_NOW, &mut refs)
                .unwrap()
        );

        // An unknown variable surfaces the resolver's error.
        let expr = Expression::parse(r#"variables.missing"#).unwrap();
        let mut refs = TestRefs { variables: &[] };
        let err = expr
            .evaluate_bool(&context, None, TEST_NOW, &mut refs)
            .unwrap_err();
        assert!(err.to_string().contains("unknown variable: missing"));
    }

    #[test]
    fn synthesizes_contexts_through_variable_references() {
        let premium = Expression::parse(r#"context.user.tier == "premium""#).unwrap();
        let expr = Expression::parse(r#"variables["premium_user"] && context.account.seats >= 50"#)
            .unwrap();
        let context = expr
            .synthesize_context(true, &mut |id, want| {
                assert_eq!(id, "premium_user");
                premium.synthesize_context(want, &mut |_, _| None)
            })
            .expect("expected composed synthesis");
        assert_eq!(
            context,
            serde_json::json!({
                "user": { "tier": "premium" },
                "account": { "seats": 50 }
            })
        );
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
        // variables binds the resolved values of referenced variables.
        assert!(
            eval_bool_with_variables(
                r#"variables["beta"]"#,
                &context,
                None,
                &[("beta", JsonValue::Bool(true))],
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

        // The retired env.qualifier spelling gets the pointed legacy diagnostic.
        let env_qualifier = Expression::parse(r#"env.qualifier["x"]"#).unwrap();
        assert!(
            env_qualifier
                .references()
                .invalid_roots
                .contains(&LegacyQualifier)
        );

        // Valid roots produce no issues.
        let ok = Expression::parse(
            r#"variables["x"] && env.now == "" && context.a == 1 && entry.b == 2"#,
        )
        .unwrap();
        assert!(ok.references().invalid_roots.is_empty());
    }

    #[test]
    fn comprehension_bound_identifiers_are_not_unknown_roots() {
        // Macros such as exists() expand to comprehensions whose iteration
        // variable is a bare identifier; chains rooted at it are bindings,
        // not references.
        let expression =
            Expression::parse("entry.audiences.exists(a, a.min_visits <= context.visits)").unwrap();
        let references = expression.references();
        assert!(references.invalid_roots.is_empty());
        assert!(references.entry_paths.contains("audiences"));
        assert!(references.context_paths.contains("visits"));

        // The binding does not leak: the same identifier outside the
        // comprehension is still an unknown root.
        let outside = Expression::parse("entry.list.exists(a, a.x) && a.y").unwrap();
        assert!(
            outside
                .references()
                .invalid_roots
                .contains(&ExpressionRootIssue::UnknownRoot("a".to_owned()))
        );
    }

    #[test]
    fn evaluates_logical_precedence_and_short_circuiting() {
        let context = serde_json::json!({});

        assert!(eval_bool("true || false && false", &context, None).unwrap());
        assert!(!eval_bool("(true || false) && false", &context, None).unwrap());
        assert!(eval_bool("!false && (false || true)", &context, None).unwrap());

        // Variables referenced by an expression are resolved eagerly (the cel
        // engine indexes a precomputed map), so the resolver runs regardless of
        // short-circuiting; it simply returns a value here.
        assert!(
            eval_bool_with_variables(
                r#"true || variables["must_not_run"]"#,
                &context,
                None,
                &[("must_not_run", JsonValue::Bool(false))],
            )
            .unwrap()
        );
        assert!(
            !eval_bool_with_variables(
                r#"false && variables["must_not_run"]"#,
                &context,
                None,
                &[("must_not_run", JsonValue::Bool(false))],
            )
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
    fn evaluates_context_paths_entry_paths_and_variables() {
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
            eval_bool_with_variables(
                r#"variables["enterprise_accounts"] && variables["mobile_users"]"#,
                &context,
                None,
                &[
                    ("enterprise_accounts", JsonValue::Bool(true)),
                    ("mobile_users", JsonValue::Bool(true)),
                ],
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
            (
                r#"timeAfter(context.user.created_at, "2026-06-21T00:00:00Z")"#,
                true,
            ),
            (
                r#"timeBefore(context.user.created_at, "2026-06-22T00:00:00Z")"#,
                true,
            ),
            (
                r#"time_at_or_before(context.user.created_at, "2026-06-21T12:30:00Z")"#,
                true,
            ),
            (
                r#"time_at_or_after(context.user.created_at, "2026-06-21T12:30:00Z")"#,
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
    fn extracts_references_from_nested_paths_functions_and_variables() {
        let expr = Expression::parse(
            r#"
            variables["enterprise_accounts"]
                && variables["mobile_users"]
                && has(context.user.tier)
                && context.request.country in ["DE", "NL"]
                && entry.metadata.channel == context.channel
                && path(entry.payload, "/title") == "Welcome"
            "#,
        )
        .unwrap();
        let references = expr.references();

        assert_eq!(
            references.variables,
            string_set(&["enterprise_accounts", "mobile_users"])
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

    /// Synthesize a context for `source` with no variable composition.
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
    fn synthesizes_through_condition_composition() {
        // `variables["premium"]` is satisfied by recursively synthesizing the
        // referenced condition variable's own expression and merging its
        // context in.
        let premium = Expression::parse(r#"context.user.tier == "premium""#).unwrap();
        let source = r#"variables["premium"] && context.account.seats >= 50"#;
        let expr = Expression::parse(source).unwrap();
        let context = expr
            .synthesize_context(true, &mut |id, want| {
                assert_eq!(id, "premium");
                premium.synthesize_context(want, &mut |_, _| None)
            })
            .expect("expected composed synthesis");

        let mut refs = TestRefs { variables: &[] };
        let premium_value = premium
            .evaluate_bool(&context, None, TEST_NOW, &mut refs)
            .unwrap();
        assert!(
            eval_bool_with_variables(
                source,
                &context,
                None,
                &[("premium", JsonValue::Bool(premium_value))],
            )
            .unwrap()
        );
    }

    #[test]
    fn returns_none_for_uninvertible_shapes() {
        // A free-form string function the synthesizer does not model.
        assert!(synth(r#"context.user.email.endsWith("@rototo.dev")"#, true).is_none());
    }

    #[test]
    fn bucket_synthesis_gives_up_after_the_candidate_budget() {
        // An empty bucket range is never satisfiable; the candidate search
        // stops at MAX_BUCKET_CANDIDATES and reports no context instead of
        // spinning forever.
        assert!(synth(r#"bucket(context.user.id, "salt", 7, 7)"#, true).is_none());
    }
}
