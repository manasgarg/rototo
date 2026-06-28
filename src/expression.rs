use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::sync::Arc;

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
    ast: Expr,
    references: ExpressionReferences,
    /// The same expression compiled by the `cel` engine, which now does
    /// evaluation. The rototo `ast`/`references` above stay for lint analysis.
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
}

/// The JSON Schema scalar families an expression can require of a context path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ContextScalarType {
    Bool,
    Number,
    String,
}

impl ContextScalarType {
    /// Whether a JSON Schema `type` token names this scalar family. `integer`
    /// and `number` both satisfy a `Number` expectation.
    pub(crate) fn matches_schema_type(self, schema_type: &str) -> bool {
        match self {
            ContextScalarType::Bool => schema_type == "boolean",
            ContextScalarType::Number => schema_type == "number" || schema_type == "integer",
            ContextScalarType::String => schema_type == "string",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            ContextScalarType::Bool => "boolean",
            ContextScalarType::Number => "number",
            ContextScalarType::String => "string",
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

pub(crate) fn simple_rule_qualifier(expression: &str) -> Option<String> {
    let expression = strip_condition_parens(expression.trim());
    let quoted = expression
        .strip_prefix("qualifier[")?
        .strip_suffix(']')?
        .trim();
    serde_json::from_str::<String>(quoted).ok()
}

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
        let tokens = Lexer::new(&source).tokens()?;
        let ast = Parser::new(tokens).parse()?;
        let mut references = ExpressionReferences::default();
        collect_references(&ast, &mut references);
        collect_type_constraints(&ast, &mut references.context_path_types);
        let cel_ast = cel::Program::compile(&source)
            .map_err(|err| ExpressionParseError::new(err.to_string()))?
            .expression()
            .clone();
        Ok(Self {
            source,
            ast,
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
        expression_result_hint(&self.ast)
    }

    pub(crate) fn evaluate_bool(
        &self,
        context: &JsonValue,
        entry: Option<&JsonValue>,
        resolve_qualifier: &mut dyn FnMut(&str) -> Result<bool>,
    ) -> Result<bool> {
        let value = self.evaluate_value(context, entry, resolve_qualifier)?;
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
        resolve_qualifier: &mut dyn FnMut(&str) -> Result<bool>,
    ) -> Result<JsonValue> {
        cel_evaluate(
            &self.cel_ast,
            &self.references,
            context,
            entry,
            resolve_qualifier,
        )
    }
}

fn expression_result_hint(expr: &Expr) -> ExpressionResultHint {
    match expr {
        Expr::Literal(JsonValue::Bool(_))
        | Expr::Qualifier(_)
        | Expr::Unary { .. }
        | Expr::Binary { .. } => ExpressionResultHint::Bool,
        Expr::Call { name, .. } if matches!(name.as_str(), "path" | "size") => {
            ExpressionResultHint::Value
        }
        Expr::Call { .. } => ExpressionResultHint::Bool,
        Expr::Literal(_) | Expr::List(_) | Expr::Path(_) => ExpressionResultHint::Value,
    }
}

#[derive(Clone, Debug)]
enum Expr {
    Literal(JsonValue),
    List(Vec<Expr>),
    Path(PathExpr),
    Qualifier(String),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Clone, Debug)]
struct PathExpr {
    root: PathRoot,
    segments: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathRoot {
    Context,
    Entry,
}

#[derive(Clone, Copy, Debug)]
enum UnaryOp {
    Not,
}

#[derive(Clone, Copy, Debug)]
enum BinaryOp {
    Or,
    And,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    In,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Ident(String),
    String(String),
    Number(String),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Dot,
    Comma,
    Bang,
    EqEq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    AndAnd,
    OrOr,
    Eof,
}

struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            index: 0,
        }
    }

    fn tokens(mut self) -> std::result::Result<Vec<Token>, ExpressionParseError> {
        let mut tokens = Vec::new();
        while let Some(byte) = self.peek() {
            match byte {
                b' ' | b'\t' | b'\n' | b'\r' => {
                    self.index += 1;
                }
                b'(' => {
                    self.index += 1;
                    tokens.push(Token::LParen);
                }
                b')' => {
                    self.index += 1;
                    tokens.push(Token::RParen);
                }
                b'[' => {
                    self.index += 1;
                    tokens.push(Token::LBracket);
                }
                b']' => {
                    self.index += 1;
                    tokens.push(Token::RBracket);
                }
                b'.' => {
                    self.index += 1;
                    tokens.push(Token::Dot);
                }
                b',' => {
                    self.index += 1;
                    tokens.push(Token::Comma);
                }
                b'!' => {
                    self.index += 1;
                    if self.consume(b'=') {
                        tokens.push(Token::Neq);
                    } else {
                        tokens.push(Token::Bang);
                    }
                }
                b'=' => {
                    self.index += 1;
                    if self.consume(b'=') {
                        tokens.push(Token::EqEq);
                    } else {
                        return Err(ExpressionParseError::new("expected ==, found ="));
                    }
                }
                b'<' => {
                    self.index += 1;
                    if self.consume(b'=') {
                        tokens.push(Token::Lte);
                    } else {
                        tokens.push(Token::Lt);
                    }
                }
                b'>' => {
                    self.index += 1;
                    if self.consume(b'=') {
                        tokens.push(Token::Gte);
                    } else {
                        tokens.push(Token::Gt);
                    }
                }
                b'&' => {
                    self.index += 1;
                    if self.consume(b'&') {
                        tokens.push(Token::AndAnd);
                    } else {
                        return Err(ExpressionParseError::new("expected &&, found &"));
                    }
                }
                b'|' => {
                    self.index += 1;
                    if self.consume(b'|') {
                        tokens.push(Token::OrOr);
                    } else {
                        return Err(ExpressionParseError::new("expected ||, found |"));
                    }
                }
                b'"' | b'\'' => tokens.push(Token::String(self.string(byte)?)),
                b'-' | b'0'..=b'9' => tokens.push(Token::Number(self.number()?)),
                _ if is_ident_start(byte) => tokens.push(Token::Ident(self.ident())),
                _ => {
                    let ch = self.input[self.index..].chars().next().unwrap_or('\0');
                    return Err(ExpressionParseError::new(format!(
                        "unexpected character in expression: {ch}"
                    )));
                }
            }
        }
        tokens.push(Token::Eof);
        Ok(tokens)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn string(&mut self, quote: u8) -> std::result::Result<String, ExpressionParseError> {
        self.index += 1;
        let mut out = String::new();
        while let Some(byte) = self.peek() {
            self.index += 1;
            if byte == quote {
                return Ok(out);
            }
            if byte != b'\\' {
                out.push(byte as char);
                continue;
            }
            let Some(escaped) = self.peek() else {
                return Err(ExpressionParseError::new("unterminated string escape"));
            };
            self.index += 1;
            match escaped {
                b'"' => out.push('"'),
                b'\'' => out.push('\''),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'b' => out.push('\u{0008}'),
                b'f' => out.push('\u{000c}'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                _ => {
                    return Err(ExpressionParseError::new(format!(
                        "unsupported string escape: \\{}",
                        escaped as char
                    )));
                }
            }
        }
        Err(ExpressionParseError::new("unterminated string literal"))
    }

    fn number(&mut self) -> std::result::Result<String, ExpressionParseError> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        self.consume_digits();
        if self.peek() == Some(b'.') {
            self.index += 1;
            self.consume_digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            self.consume_digits();
        }
        Ok(self.input[start..self.index].to_owned())
    }

    fn consume_digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.index += 1;
        }
    }

    fn ident(&mut self) -> String {
        let start = self.index;
        self.index += 1;
        while self.peek().is_some_and(is_ident_continue) {
            self.index += 1;
        }
        self.input[start..self.index].to_owned()
    }
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0 }
    }

    fn parse(mut self) -> std::result::Result<Expr, ExpressionParseError> {
        let expr = self.parse_or()?;
        self.expect_eof()?;
        Ok(expr)
    }

    fn parse_or(&mut self) -> std::result::Result<Expr, ExpressionParseError> {
        let mut expr = self.parse_and()?;
        while self.matches(&Token::OrOr) {
            let right = self.parse_and()?;
            expr = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> std::result::Result<Expr, ExpressionParseError> {
        let mut expr = self.parse_equality()?;
        while self.matches(&Token::AndAnd) {
            let right = self.parse_equality()?;
            expr = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_equality(&mut self) -> std::result::Result<Expr, ExpressionParseError> {
        let mut expr = self.parse_comparison()?;
        loop {
            let op = if self.matches(&Token::EqEq) {
                BinaryOp::Eq
            } else if self.matches(&Token::Neq) {
                BinaryOp::Neq
            } else if self.matches_ident("in") {
                BinaryOp::In
            } else {
                break;
            };
            let right = self.parse_comparison()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_comparison(&mut self) -> std::result::Result<Expr, ExpressionParseError> {
        let mut expr = self.parse_unary()?;
        loop {
            let op = if self.matches(&Token::Lt) {
                BinaryOp::Lt
            } else if self.matches(&Token::Lte) {
                BinaryOp::Lte
            } else if self.matches(&Token::Gt) {
                BinaryOp::Gt
            } else if self.matches(&Token::Gte) {
                BinaryOp::Gte
            } else {
                break;
            };
            let right = self.parse_unary()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> std::result::Result<Expr, ExpressionParseError> {
        if self.matches(&Token::Bang) {
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> std::result::Result<Expr, ExpressionParseError> {
        match self.advance().clone() {
            Token::Ident(value) => self.parse_ident(value),
            Token::String(value) => Ok(Expr::Literal(JsonValue::String(value))),
            Token::Number(value) => Ok(Expr::Literal(parse_number_literal(&value)?)),
            Token::LParen => {
                let expr = self.parse_or()?;
                self.expect(Token::RParen, "expected )")?;
                Ok(expr)
            }
            Token::LBracket => self.parse_list(),
            token => Err(ExpressionParseError::new(format!(
                "expected expression, found {token:?}"
            ))),
        }
    }

    fn parse_ident(&mut self, value: String) -> std::result::Result<Expr, ExpressionParseError> {
        match value.as_str() {
            "true" => return Ok(Expr::Literal(JsonValue::Bool(true))),
            "false" => return Ok(Expr::Literal(JsonValue::Bool(false))),
            "null" => return Ok(Expr::Literal(JsonValue::Null)),
            _ => {}
        }

        if self.matches(&Token::LParen) {
            let args = self.parse_args()?;
            return Ok(Expr::Call { name: value, args });
        }

        if value == "qualifier" {
            return self.parse_qualifier_reference();
        }

        if value == "context" || value == "entry" {
            return self.parse_path(if value == "context" {
                PathRoot::Context
            } else {
                PathRoot::Entry
            });
        }

        Err(ExpressionParseError::new(format!(
            "unknown identifier in expression: {value}"
        )))
    }

    fn parse_args(&mut self) -> std::result::Result<Vec<Expr>, ExpressionParseError> {
        let mut args = Vec::new();
        if self.matches(&Token::RParen) {
            return Ok(args);
        }
        loop {
            args.push(self.parse_or()?);
            if self.matches(&Token::RParen) {
                break;
            }
            self.expect(Token::Comma, "expected , between function arguments")?;
        }
        Ok(args)
    }

    fn parse_list(&mut self) -> std::result::Result<Expr, ExpressionParseError> {
        let mut values = Vec::new();
        if self.matches(&Token::RBracket) {
            return Ok(Expr::List(values));
        }
        loop {
            values.push(self.parse_or()?);
            if self.matches(&Token::RBracket) {
                break;
            }
            self.expect(Token::Comma, "expected , between list values")?;
        }
        Ok(Expr::List(values))
    }

    fn parse_qualifier_reference(&mut self) -> std::result::Result<Expr, ExpressionParseError> {
        if self.matches(&Token::LBracket) {
            let Token::String(value) = self.advance().clone() else {
                return Err(ExpressionParseError::new(
                    "qualifier reference must use qualifier[\"id\"]",
                ));
            };
            self.expect(Token::RBracket, "expected ] after qualifier id")?;
            return Ok(Expr::Qualifier(value));
        }

        if self.matches(&Token::Dot) {
            let Token::Ident(value) = self.advance().clone() else {
                return Err(ExpressionParseError::new(
                    "qualifier reference must name a qualifier",
                ));
            };
            return Ok(Expr::Qualifier(value));
        }

        Err(ExpressionParseError::new(
            "qualifier reference must use qualifier[\"id\"]",
        ))
    }

    fn parse_path(&mut self, root: PathRoot) -> std::result::Result<Expr, ExpressionParseError> {
        let mut segments = Vec::new();
        loop {
            if self.matches(&Token::Dot) {
                let Token::Ident(value) = self.advance().clone() else {
                    return Err(ExpressionParseError::new("expected path segment after ."));
                };
                segments.push(value);
                continue;
            }
            if self.matches(&Token::LBracket) {
                let Token::String(value) = self.advance().clone() else {
                    return Err(ExpressionParseError::new(
                        "path bracket lookup must use a string literal",
                    ));
                };
                self.expect(Token::RBracket, "expected ] after path segment")?;
                segments.push(value);
                continue;
            }
            break;
        }
        Ok(Expr::Path(PathExpr { root, segments }))
    }

    fn advance(&mut self) -> &Token {
        let index = self.index;
        self.index += 1;
        &self.tokens[index]
    }

    fn current(&self) -> &Token {
        &self.tokens[self.index]
    }

    fn matches(&mut self, expected: &Token) -> bool {
        if self.current() == expected {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn matches_ident(&mut self, expected: &str) -> bool {
        if matches!(self.current(), Token::Ident(value) if value == expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn expect(
        &mut self,
        expected: Token,
        message: &'static str,
    ) -> std::result::Result<(), ExpressionParseError> {
        if self.matches(&expected) {
            Ok(())
        } else {
            Err(ExpressionParseError::new(message))
        }
    }

    fn expect_eof(&self) -> std::result::Result<(), ExpressionParseError> {
        if matches!(self.current(), Token::Eof) {
            Ok(())
        } else {
            Err(ExpressionParseError::new(format!(
                "unexpected token after expression: {:?}",
                self.current()
            )))
        }
    }
}

fn parse_number_literal(value: &str) -> std::result::Result<JsonValue, ExpressionParseError> {
    if value.contains('.') || value.contains('e') || value.contains('E') {
        let number = value
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .ok_or_else(|| ExpressionParseError::new(format!("invalid number literal: {value}")))?;
        return Ok(JsonValue::Number(number));
    }

    if let Ok(value) = value.parse::<i64>() {
        return Ok(JsonValue::Number(Number::from(value)));
    }
    let value = value
        .parse::<u64>()
        .map_err(|_| ExpressionParseError::new(format!("invalid number literal: {value}")))?;
    Ok(JsonValue::Number(Number::from(value)))
}

fn collect_references(expr: &Expr, references: &mut ExpressionReferences) {
    match expr {
        Expr::Literal(_) => {}
        Expr::List(values) => {
            for value in values {
                collect_references(value, references);
            }
        }
        Expr::Path(path) => {
            let key = path.segments.join(".");
            match path.root {
                PathRoot::Context => {
                    references.context_paths.insert(key);
                }
                PathRoot::Entry => {
                    references.entry_paths.insert(key);
                }
            }
        }
        Expr::Qualifier(qualifier) => {
            references.qualifiers.insert(qualifier.clone());
        }
        Expr::Unary { expr, .. } => collect_references(expr, references),
        Expr::Binary { left, right, .. } => {
            collect_references(left, references);
            collect_references(right, references);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_references(arg, references);
            }
        }
    }
}

/// Walk an expression and record, per context path, the scalar types the
/// expression requires of it. Only uses that unambiguously pin a scalar family
/// are recorded; ambiguous uses (such as the value argument of `bucket`) are
/// intentionally left unconstrained so they do not produce false type gaps.
fn collect_type_constraints(
    expr: &Expr,
    types: &mut BTreeMap<String, BTreeSet<ContextScalarType>>,
) {
    match expr {
        Expr::Binary { op, left, right } => {
            match op {
                BinaryOp::Eq | BinaryOp::Neq => {
                    constrain_against_literal(left, right, types, literal_scalar_type);
                    constrain_against_literal(right, left, types, literal_scalar_type);
                }
                BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte => {
                    constrain_against_literal(left, right, types, literal_ordering_type);
                    constrain_against_literal(right, left, types, literal_ordering_type);
                }
                BinaryOp::In => {
                    if let Some(path) = context_path(left)
                        && let Expr::List(items) = right.as_ref()
                    {
                        for item in items {
                            if let Some(scalar) = literal_scalar_type(item) {
                                types.entry(path.clone()).or_default().insert(scalar);
                            }
                        }
                    }
                }
                BinaryOp::And | BinaryOp::Or => {
                    constrain_bool_operand(left, types);
                    constrain_bool_operand(right, types);
                }
            }
            collect_type_constraints(left, types);
            collect_type_constraints(right, types);
        }
        Expr::Unary {
            op: UnaryOp::Not,
            expr,
        } => {
            constrain_bool_operand(expr, types);
            collect_type_constraints(expr, types);
        }
        Expr::Call { name, args } => {
            if string_arg0_function(name)
                && let Some(path) = args.first().and_then(context_path)
            {
                types
                    .entry(path)
                    .or_default()
                    .insert(ContextScalarType::String);
            }
            for arg in args {
                collect_type_constraints(arg, types);
            }
        }
        Expr::List(items) => {
            for item in items {
                collect_type_constraints(item, types);
            }
        }
        Expr::Literal(_) | Expr::Path(_) | Expr::Qualifier(_) => {}
    }
}

fn constrain_against_literal(
    candidate_path: &Expr,
    candidate_literal: &Expr,
    types: &mut BTreeMap<String, BTreeSet<ContextScalarType>>,
    classify: fn(&Expr) -> Option<ContextScalarType>,
) {
    if let Some(path) = context_path(candidate_path)
        && let Some(scalar) = classify(candidate_literal)
    {
        types.entry(path).or_default().insert(scalar);
    }
}

fn constrain_bool_operand(expr: &Expr, types: &mut BTreeMap<String, BTreeSet<ContextScalarType>>) {
    if let Some(path) = context_path(expr) {
        types
            .entry(path)
            .or_default()
            .insert(ContextScalarType::Bool);
    }
}

fn context_path(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) if path.root == PathRoot::Context => Some(path.segments.join(".")),
        _ => None,
    }
}

fn literal_scalar_type(expr: &Expr) -> Option<ContextScalarType> {
    match expr {
        Expr::Literal(JsonValue::Bool(_)) => Some(ContextScalarType::Bool),
        Expr::Literal(JsonValue::Number(_)) => Some(ContextScalarType::Number),
        Expr::Literal(JsonValue::String(_)) => Some(ContextScalarType::String),
        _ => None,
    }
}

fn literal_ordering_type(expr: &Expr) -> Option<ContextScalarType> {
    match expr {
        Expr::Literal(JsonValue::Number(_)) => Some(ContextScalarType::Number),
        Expr::Literal(JsonValue::String(_)) => Some(ContextScalarType::String),
        _ => None,
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
            | "cidr"
    )
}

// ---- Evaluation: rototo rents the `cel` engine. ----
// The hand-written tree-walking evaluator was replaced by compiling to cel and
// resolving against a Context that supplies the `context`/`entry`/`qualifier`
// variables plus rototo's custom functions. The rototo parser/AST above is kept
// only for lint analysis (references and type constraints).

type FnResult = std::result::Result<CelValue, ExecutionError>;

fn cel_evaluate(
    cel_ast: &IdedExpr,
    references: &ExpressionReferences,
    context: &JsonValue,
    entry: Option<&JsonValue>,
    resolve_qualifier: &mut dyn FnMut(&str) -> Result<bool>,
) -> Result<JsonValue> {
    let mut ctx = CelContext::default();
    register_functions(&mut ctx);
    ctx.add_variable_from_value("context", to_cel(context)?);
    ctx.add_variable_from_value("entry", to_cel(&entry.cloned().unwrap_or(JsonValue::Null))?);

    // `qualifier["id"]` reads a precomputed map. Only the qualifiers the
    // expression references are resolved (through the same callback as before,
    // which owns cycle detection); cel then indexes that map.
    let mut qualifiers = serde_json::Map::new();
    for id in &references.qualifiers {
        qualifiers.insert(id.clone(), JsonValue::Bool(resolve_qualifier(id)?));
    }
    ctx.add_variable_from_value("qualifier", to_cel(&JsonValue::Object(qualifiers))?);

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
        expr.evaluate_bool(context, entry, &mut resolve_qualifier)
    }

    fn eval_value(
        source: &str,
        context: &JsonValue,
        entry: Option<&JsonValue>,
    ) -> Result<JsonValue> {
        let expr = Expression::parse(source).unwrap();
        let mut resolve_qualifier = |_id: &str| Ok(false);
        expr.evaluate_value(context, entry, &mut resolve_qualifier)
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
        assert!(expr.evaluate_bool(&context, None, &mut qualifier).unwrap());
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
            r#"qualifier["enterprise-accounts"] && entry.id == "hero" && context.region == "eu""#,
        )
        .unwrap();
        assert!(expr.references().qualifiers.contains("enterprise-accounts"));
        assert!(expr.references().entry_paths.contains("id"));
        assert!(expr.references().context_paths.contains("region"));
    }

    #[test]
    fn evaluates_logical_precedence_and_short_circuiting() {
        let context = serde_json::json!({});

        assert!(eval_bool("true || false && false", &context, None).unwrap());
        assert!(!eval_bool("(true || false) && false", &context, None).unwrap());
        assert!(eval_bool("!false && (false || true)", &context, None).unwrap());

        let expr = Expression::parse(r#"true || qualifier["must-not-run"]"#).unwrap();
        // Qualifiers referenced by an expression are resolved eagerly (the cel
        // engine indexes a precomputed map), so the resolver runs regardless of
        // short-circuiting; it simply returns a value here.
        let mut resolve_qualifier = |id: &str| {
            let _ = id;
            Ok(false)
        };
        assert!(
            expr.evaluate_bool(&context, None, &mut resolve_qualifier)
                .unwrap()
        );

        let expr = Expression::parse(r#"false && qualifier["must-not-run"]"#).unwrap();
        // Qualifiers referenced by an expression are resolved eagerly (the cel
        // engine indexes a precomputed map), so the resolver runs regardless of
        // short-circuiting; it simply returns a value here.
        let mut resolve_qualifier = |id: &str| {
            let _ = id;
            Ok(false)
        };
        assert!(
            !expr
                .evaluate_bool(&context, None, &mut resolve_qualifier)
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
                r#"qualifier["enterprise-accounts"] && qualifier["mobile-users"]"#,
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
    fn reports_parse_errors_with_stable_messages() {
        let cases = [
            (r#"context.user.tier = "premium""#, "expected ==, found ="),
            ("account.tier", "unknown identifier in expression: account"),
            (
                "qualifier[context.id]",
                r#"qualifier reference must use qualifier["id"]"#,
            ),
            ("context.user.", "expected path segment after ."),
            (
                r#"context.user.tier == "premium"#,
                "unterminated string literal",
            ),
            (
                "true false",
                r#"unexpected token after expression: Ident("false")"#,
            ),
        ];

        for (source, expected) in cases {
            let err = Expression::parse(source).unwrap_err();
            assert_eq!(err.to_string(), expected, "{source}");
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
            qualifier["enterprise-accounts"]
                && qualifier["mobile-users"]
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
}
