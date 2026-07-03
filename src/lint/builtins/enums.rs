use serde_json::Value as JsonValue;

use crate::diagnostics::RototoRuleId;

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::stages::push_project_diagnostic;

const MEMBER_TYPES: &[&str] = &["string", "int", "number", "bool"];

pub(super) fn lint_enum_shapes(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for declaration in ctx.index.enums.values() {
        if !integer_field_is(&declaration.schema_version, 1) {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::EnumSchemaVersion,
                declaration.target(),
                declaration.schema_version.location(),
                "enum must declare schema_version = 1",
            );
        }
        match &declaration.member_type {
            ProjectField::Present(member_type)
                if MEMBER_TYPES.contains(&member_type.value.as_str()) => {}
            ProjectField::Present(member_type) => {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::EnumShape,
                    declaration.target(),
                    member_type.location.clone(),
                    format!(
                        "enum declares unsupported member type: {}",
                        member_type.value
                    ),
                );
            }
            ProjectField::Invalid { location } | ProjectField::Missing { location } => {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::EnumShape,
                    declaration.target(),
                    location.clone(),
                    "enum must declare type as one of string, int, number, or bool",
                );
            }
        }
        if !ctx.index.enum_members.contains_key(&declaration.id) {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::EnumMembersMissing,
                declaration.target(),
                declaration.location.clone(),
                format!(
                    "enum declares no members: add data/enums/{}.toml",
                    declaration.id
                ),
            );
        }
    }

    for members in ctx.index.enum_members.values() {
        if let Some(location) = &members.deleted {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::EnumMembersShape,
                members.target(),
                location.clone(),
                "deleted enum members apply to a base package through extends; \
                 this package has no base member set for them to remove from",
            );
        }
        let declaration = ctx.index.enums.get(&members.id);
        if declaration.is_none() {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::EnumMembersUndeclared,
                members.target(),
                members.location.clone(),
                format!(
                    "enum members have no declaration: add model/enums/{}.toml",
                    members.id
                ),
            );
        }
        let values = match &members.members {
            ProjectField::Present(values) => values,
            ProjectField::Invalid { location } | ProjectField::Missing { location } => {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::EnumMembersShape,
                    members.target(),
                    location.clone(),
                    "enum members file must declare members as an array",
                );
                continue;
            }
        };
        if values.value.is_empty() {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::EnumMembersShape,
                members.target(),
                values.location.clone(),
                "enum must declare at least one member",
            );
            continue;
        }
        let member_type = declaration.and_then(|declaration| match &declaration.member_type {
            ProjectField::Present(member_type) => Some(member_type.value.as_str()),
            _ => None,
        });
        let mut seen = Vec::new();
        for member in &values.value {
            if let Some(member_type) = member_type
                && !member_matches_type(&member.value, member_type)
            {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::EnumMembersShape,
                    members.target(),
                    member.location.clone(),
                    format!(
                        "enum member does not match declared type {member_type}: {}",
                        member.value
                    ),
                );
            }
            if seen.contains(&&member.value) {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::EnumMembersShape,
                    members.target(),
                    member.location.clone(),
                    format!("enum member is duplicated: {}", member.value),
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
