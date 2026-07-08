use serde_json::Value as JsonValue;

use crate::diagnostics::RototoRuleId;

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::stages::push_project_diagnostic;

const MEMBER_TYPES: &[&str] = &["string", "int", "number", "bool"];

pub(super) fn lint_enum_shapes(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for declaration in ctx.index.lists.values() {
        if !integer_field_is(&declaration.schema_version, 1) {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::ListSchemaVersion,
                declaration.target(),
                declaration.schema_version.location(),
                "list must declare schema_version = 1",
            );
        }
        let member_type = match &declaration.member_type {
            ProjectField::Present(member_type)
                if MEMBER_TYPES.contains(&member_type.value.as_str()) =>
            {
                Some(member_type.value.as_str())
            }
            ProjectField::Present(member_type) => {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::ListShape,
                    declaration.target(),
                    member_type.location.clone(),
                    format!(
                        "list declares unsupported member type: {}",
                        member_type.value
                    ),
                );
                None
            }
            ProjectField::Invalid { location } | ProjectField::Missing { location } => {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::ListShape,
                    declaration.target(),
                    location.clone(),
                    "list must declare type as one of string, int, number, or bool",
                );
                None
            }
        };

        if let Some(location) = &declaration.deleted {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::ListShape,
                declaration.target(),
                location.clone(),
                "deleted list members apply to a base package through an \
                 lists/<id>.update.toml marker; this package has no base \
                 member set for them to remove from",
            );
        }

        let values = match &declaration.members {
            ProjectField::Present(values) => values,
            ProjectField::Invalid { location } | ProjectField::Missing { location } => {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::ListShape,
                    declaration.target(),
                    location.clone(),
                    "list must declare members as an array",
                );
                continue;
            }
        };
        if values.value.is_empty() {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::ListShape,
                declaration.target(),
                values.location.clone(),
                "list must declare at least one member",
            );
            continue;
        }
        let mut seen = Vec::new();
        for member in &values.value {
            if let Some(member_type) = member_type
                && !member_matches_type(&member.value, member_type)
            {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::ListShape,
                    declaration.target(),
                    member.location.clone(),
                    format!(
                        "list member does not match declared type {member_type}: {}",
                        member.value
                    ),
                );
            }
            if seen.contains(&&member.value) {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::ListShape,
                    declaration.target(),
                    member.location.clone(),
                    format!("list member is duplicated: {}", member.value),
                );
            }
            seen.push(&member.value);
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn member_matches_type(value: &JsonValue, member_type: &str) -> bool {
    match member_type {
        "string" => value.is_string(),
        "int" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "bool" => value.is_boolean(),
        _ => true,
    }
}

fn integer_field_is(field: &ProjectField<i64>, expected: i64) -> bool {
    matches!(field, ProjectField::Present(value) if value.value == expected)
}
