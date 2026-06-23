use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::net::IpAddr;

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
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ExpressionReferences {
    pub(crate) context_paths: BTreeSet<String>,
    pub(crate) entry_paths: BTreeSet<String>,
    pub(crate) qualifiers: BTreeSet<String>,
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
        Ok(Self {
            source,
            ast,
            references,
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
        evaluate_expr(&self.ast, context, entry, resolve_qualifier)
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

fn evaluate_expr(
    expr: &Expr,
    context: &JsonValue,
    entry: Option<&JsonValue>,
    resolve_qualifier: &mut dyn FnMut(&str) -> Result<bool>,
) -> Result<JsonValue> {
    match expr {
        Expr::Literal(value) => Ok(value.clone()),
        Expr::List(values) => {
            let values = values
                .iter()
                .map(|value| evaluate_expr(value, context, entry, resolve_qualifier))
                .collect::<Result<Vec<_>>>()?;
            Ok(JsonValue::Array(values))
        }
        Expr::Path(path) => evaluate_path(path, context, entry).cloned(),
        Expr::Qualifier(qualifier) => Ok(JsonValue::Bool(resolve_qualifier(qualifier)?)),
        Expr::Unary { op, expr } => match op {
            UnaryOp::Not => Ok(JsonValue::Bool(!evaluate_bool_expr(
                expr,
                context,
                entry,
                resolve_qualifier,
            )?)),
        },
        Expr::Binary { op, left, right } => {
            evaluate_binary(*op, left, right, context, entry, resolve_qualifier)
        }
        Expr::Call { name, args } => evaluate_call(name, args, context, entry, resolve_qualifier),
    }
}

fn evaluate_binary(
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
    context: &JsonValue,
    entry: Option<&JsonValue>,
    resolve_qualifier: &mut dyn FnMut(&str) -> Result<bool>,
) -> Result<JsonValue> {
    match op {
        BinaryOp::Or => {
            let left = evaluate_bool_expr(left, context, entry, resolve_qualifier)?;
            if left {
                return Ok(JsonValue::Bool(true));
            }
            Ok(JsonValue::Bool(evaluate_bool_expr(
                right,
                context,
                entry,
                resolve_qualifier,
            )?))
        }
        BinaryOp::And => {
            let left = evaluate_bool_expr(left, context, entry, resolve_qualifier)?;
            if !left {
                return Ok(JsonValue::Bool(false));
            }
            Ok(JsonValue::Bool(evaluate_bool_expr(
                right,
                context,
                entry,
                resolve_qualifier,
            )?))
        }
        BinaryOp::Eq
        | BinaryOp::Neq
        | BinaryOp::Lt
        | BinaryOp::Lte
        | BinaryOp::Gt
        | BinaryOp::Gte
        | BinaryOp::In => {
            let left = evaluate_expr(left, context, entry, resolve_qualifier)?;
            let right = evaluate_expr(right, context, entry, resolve_qualifier)?;
            let result = match op {
                BinaryOp::Eq => json_values_equal(&left, &right),
                BinaryOp::Neq => !json_values_equal(&left, &right),
                BinaryOp::Lt => {
                    compare_values(&left, &right, |ordering| ordering == Ordering::Less)
                }
                BinaryOp::Lte => compare_values(&left, &right, |ordering| {
                    matches!(ordering, Ordering::Less | Ordering::Equal)
                }),
                BinaryOp::Gt => {
                    compare_values(&left, &right, |ordering| ordering == Ordering::Greater)
                }
                BinaryOp::Gte => compare_values(&left, &right, |ordering| {
                    matches!(ordering, Ordering::Greater | Ordering::Equal)
                }),
                BinaryOp::In => right.as_array().is_some_and(|values| {
                    values.iter().any(|value| json_values_equal(value, &left))
                }),
                BinaryOp::Or | BinaryOp::And => unreachable!(),
            };
            Ok(JsonValue::Bool(result))
        }
    }
}

fn evaluate_call(
    name: &str,
    args: &[Expr],
    context: &JsonValue,
    entry: Option<&JsonValue>,
    resolve_qualifier: &mut dyn FnMut(&str) -> Result<bool>,
) -> Result<JsonValue> {
    if name == "has" {
        require_arg_count(name, args, 1)?;
        let result = match &args[0] {
            Expr::Path(path) => optional_path(path, context, entry).is_some(),
            _ => {
                return Err(RototoError::new(
                    "has() requires a context or entry path argument",
                ));
            }
        };
        return Ok(JsonValue::Bool(result));
    }

    let values = args
        .iter()
        .map(|arg| evaluate_expr(arg, context, entry, resolve_qualifier))
        .collect::<Result<Vec<_>>>()?;

    let bool_result = match name {
        "present" => {
            require_arg_count(name, args, 2)?;
            let pointer = expect_string(name, &values[1])?;
            values[0].pointer(pointer).is_some()
        }
        "missing" => {
            require_arg_count(name, args, 2)?;
            let pointer = expect_string(name, &values[1])?;
            values[0].pointer(pointer).is_none()
        }
        "startsWith" | "starts_with" | "prefix" => {
            require_arg_count(name, args, 2)?;
            expect_string(name, &values[0])?.starts_with(expect_string(name, &values[1])?)
        }
        "endsWith" | "ends_with" | "suffix" => {
            require_arg_count(name, args, 2)?;
            expect_string(name, &values[0])?.ends_with(expect_string(name, &values[1])?)
        }
        "contains" => {
            require_arg_count(name, args, 2)?;
            contains_value(&values[0], &values[1])
        }
        "matches" | "regex" => {
            require_arg_count(name, args, 2)?;
            Regex::new(expect_string(name, &values[1])?)
                .map_err(|err| RototoError::new(format!("regex is invalid: {err}")))?
                .is_match(expect_string(name, &values[0])?)
        }
        "glob" => {
            require_arg_count(name, args, 2)?;
            Pattern::new(expect_string(name, &values[1])?)
                .map_err(|err| RototoError::new(format!("glob pattern is invalid: {err}")))?
                .matches(expect_string(name, &values[0])?)
        }
        "semver" => {
            require_arg_count(name, args, 2)?;
            let version = Version::parse(expect_string(name, &values[0])?)
                .map_err(|err| RototoError::new(format!("semver version is invalid: {err}")))?;
            VersionReq::parse(expect_string(name, &values[1])?)
                .map_err(|err| RototoError::new(format!("semver requirement is invalid: {err}")))?
                .matches(&version)
        }
        "timeAfter" | "time_after" => {
            require_arg_count(name, args, 2)?;
            parse_time_arg(name, &values[0])? > parse_time_arg(name, &values[1])?
        }
        "timeAtOrAfter" | "time_at_or_after" => {
            require_arg_count(name, args, 2)?;
            parse_time_arg(name, &values[0])? >= parse_time_arg(name, &values[1])?
        }
        "timeBefore" | "time_before" => {
            require_arg_count(name, args, 2)?;
            parse_time_arg(name, &values[0])? < parse_time_arg(name, &values[1])?
        }
        "timeAtOrBefore" | "time_at_or_before" => {
            require_arg_count(name, args, 2)?;
            parse_time_arg(name, &values[0])? <= parse_time_arg(name, &values[1])?
        }
        "timeBetween" | "time_between" => {
            require_arg_count(name, args, 3)?;
            let actual = parse_time_arg(name, &values[0])?;
            actual >= parse_time_arg(name, &values[1])?
                && actual < parse_time_arg(name, &values[2])?
        }
        "bucket" => {
            require_arg_count(name, args, 4)?;
            let salt = expect_string(name, &values[1])?;
            let start = expect_i64(name, &values[2])?;
            let end = expect_i64(name, &values[3])?;
            let bucket = bucket_value(salt, &values[0]);
            i64::from(bucket) >= start && i64::from(bucket) < end
        }
        "cidr" => {
            require_arg_count(name, args, 2)?;
            let ip = expect_string(name, &values[0])?
                .parse::<IpAddr>()
                .map_err(|err| RototoError::new(format!("ip address is invalid: {err}")))?;
            cidr_blocks(name, &values[1])?
                .iter()
                .any(|block| block.contains(ip))
        }
        "path" => {
            require_arg_count(name, args, 2)?;
            let pointer = expect_string(name, &values[1])?;
            return values[0].pointer(pointer).cloned().ok_or_else(|| {
                RototoError::new(format!("path() did not find JSON Pointer: {pointer}"))
            });
        }
        "size" => {
            require_arg_count(name, args, 1)?;
            let len = match &values[0] {
                JsonValue::Array(values) => values.len(),
                JsonValue::Object(values) => values.len(),
                JsonValue::String(value) => value.chars().count(),
                _ => {
                    return Err(RototoError::new(
                        "size() requires an array, object, or string",
                    ));
                }
            };
            return Ok(JsonValue::Number(Number::from(len)));
        }
        _ => {
            return Err(RototoError::new(format!(
                "unknown expression function: {name}"
            )));
        }
    };
    Ok(JsonValue::Bool(bool_result))
}

fn evaluate_bool_expr(
    expr: &Expr,
    context: &JsonValue,
    entry: Option<&JsonValue>,
    resolve_qualifier: &mut dyn FnMut(&str) -> Result<bool>,
) -> Result<bool> {
    evaluate_expr(expr, context, entry, resolve_qualifier)?
        .as_bool()
        .ok_or_else(|| RototoError::new("expression operand must be bool"))
}

fn evaluate_path<'a>(
    path: &PathExpr,
    context: &'a JsonValue,
    entry: Option<&'a JsonValue>,
) -> Result<&'a JsonValue> {
    optional_path(path, context, entry).ok_or_else(|| {
        RototoError::new(format!(
            "expression path is missing: {}.{}",
            match path.root {
                PathRoot::Context => "context",
                PathRoot::Entry => "entry",
            },
            path.segments.join(".")
        ))
    })
}

fn optional_path<'a>(
    path: &PathExpr,
    context: &'a JsonValue,
    entry: Option<&'a JsonValue>,
) -> Option<&'a JsonValue> {
    let mut current = match path.root {
        PathRoot::Context => context,
        PathRoot::Entry => entry?,
    };
    for segment in &path.segments {
        current = current.get(segment)?;
    }
    Some(current)
}

fn require_arg_count(name: &str, args: &[Expr], expected: usize) -> Result<()> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(RototoError::new(format!(
            "{name}() expects {expected} arguments, got {}",
            args.len()
        )))
    }
}

fn expect_string<'a>(name: &str, value: &'a JsonValue) -> Result<&'a str> {
    value
        .as_str()
        .ok_or_else(|| RototoError::new(format!("{name}() argument must be a string")))
}

fn expect_i64(name: &str, value: &JsonValue) -> Result<i64> {
    value
        .as_i64()
        .ok_or_else(|| RototoError::new(format!("{name}() argument must be an integer")))
}

fn parse_time_arg(name: &str, value: &JsonValue) -> Result<crate::predicate::Rfc3339Timestamp> {
    parse_rfc3339_timestamp(expect_string(name, value)?)
        .ok_or_else(|| RototoError::new(format!("{name}() argument must be an RFC3339 timestamp")))
}

fn cidr_blocks(name: &str, value: &JsonValue) -> Result<Vec<CidrBlock>> {
    let values = match value {
        JsonValue::String(value) => vec![value.as_str()],
        JsonValue::Array(values) => values
            .iter()
            .map(|value| expect_string(name, value))
            .collect::<Result<Vec<_>>>()?,
        _ => {
            return Err(RototoError::new(format!(
                "{name}() CIDR argument must be a string or list of strings"
            )));
        }
    };

    values
        .into_iter()
        .map(|value| {
            CidrBlock::parse(value)
                .ok_or_else(|| RototoError::new(format!("CIDR block is invalid: {value}")))
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

fn compare_values(
    left: &JsonValue,
    right: &JsonValue,
    predicate: impl FnOnce(Ordering) -> bool,
) -> bool {
    if let (Some(left), Some(right)) = (json_number_as_f64(left), json_number_as_f64(right)) {
        return left.partial_cmp(&right).is_some_and(predicate);
    }
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return predicate(left.cmp(right));
    }
    false
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

fn json_number_as_f64(value: &JsonValue) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
        .or_else(|| value.as_u64().map(|value| value as f64))
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
        let mut resolve_qualifier = |id: &str| {
            Err(RototoError::new(format!(
                "qualifier resolver should not run for {id}"
            )))
        };
        assert!(
            expr.evaluate_bool(&context, None, &mut resolve_qualifier)
                .unwrap()
        );

        let expr = Expression::parse(r#"false && qualifier["must-not-run"]"#).unwrap();
        let mut resolve_qualifier = |id: &str| {
            Err(RototoError::new(format!(
                "qualifier resolver should not run for {id}"
            )))
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
            (r#"context.seats == "42""#, false),
            (r#"context.tier > 10"#, false),
            (r#"context.tier in "premium""#, false),
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
                r#"qualifier["enterprise-accounts"] && qualifier.mobile-users"#,
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

        let cases = [
            (
                "context.user.missing == true",
                "expression path is missing: context.user.missing",
            ),
            (
                "entry.channel == \"email\"",
                "expression path is missing: entry.channel",
            ),
            (
                "context.user.tier && true",
                "expression operand must be bool",
            ),
            (
                "unknown_fn(context.user.tier)",
                "unknown expression function: unknown_fn",
            ),
            (
                "has(\"tier\")",
                "has() requires a context or entry path argument",
            ),
            ("size(true)", "size() requires an array, object, or string"),
            (
                "path(context.payload, \"/missing\") == true",
                "path() did not find JSON Pointer: /missing",
            ),
            (
                r#"regex(context.user.tier, "[")"#,
                "regex is invalid: regex parse error:",
            ),
            (
                r#"cidr(context.user.tier, "not-cidr")"#,
                "ip address is invalid:",
            ),
        ];

        for (source, expected) in cases {
            let err = eval_bool(source, &context, None).unwrap_err();
            assert!(
                err.to_string().starts_with(expected),
                "{source}: expected error starting with {expected:?}, got {err}"
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
            qualifier.enterprise-accounts
                && qualifier["mobile-users"]
                && has(context.user["tier"])
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
