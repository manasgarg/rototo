use std::collections::BTreeMap;

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, LintStage, RototoRuleId, SemanticEntity, Severity,
};
use crate::lua_lint;

use super::super::engine::LintContext;
use super::super::index::{CustomLintRegistration, CustomRuleDefinitionNode};
use super::super::stages::push_register_diagnostic;
use super::runner;
use super::{RegisteredLintAddress, RegisteredLintSelector};

pub(crate) async fn register_custom_lints(ctx: &mut LintContext) {
    let files = ctx
        .index
        .custom_lints
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>();

    for file in files {
        let Some(document) = ctx.source.documents.get(&file.doc).cloned() else {
            continue;
        };
        if let Some(read_error) = &document.read_error {
            push_register_diagnostic(
                &mut ctx.diagnostics,
                RototoRuleId::CustomLintFailed,
                SemanticEntity::CustomLint {
                    path: file.path.clone(),
                },
                file.location.clone(),
                format!("failed to read custom lint {}: {read_error}", file.path),
            );
            continue;
        }

        let registrations = match runner::register_pipeline_lint(
            std::path::PathBuf::from(&file.path),
            document.text.clone(),
        )
        .await
        {
            Ok(registrations) => registrations,
            Err(err) => {
                push_register_diagnostic(
                    &mut ctx.diagnostics,
                    RototoRuleId::CustomLintFailed,
                    SemanticEntity::CustomLint {
                        path: file.path.clone(),
                    },
                    file.location.clone(),
                    err.to_string(),
                );
                continue;
            }
        };

        if registrations.is_empty() {
            push_register_diagnostic(
                &mut ctx.diagnostics,
                RototoRuleId::CustomLintFileUnregistered,
                SemanticEntity::CustomLint {
                    path: file.path.clone(),
                },
                file.location.clone(),
                format!("custom lint file registers no handlers: {}", file.path),
            );
        }

        for registration in registrations {
            match validate_custom_registration(&registration) {
                Ok((stage, selector, definition)) => {
                    let rule = definition.rule.clone();
                    match ctx.index.custom_lints.rules.get(&rule) {
                        Some(existing) if !existing.definition.same_metadata(&definition) => {
                            push_register_diagnostic(
                                &mut ctx.diagnostics,
                                RototoRuleId::CustomLintRuleConflict,
                                SemanticEntity::CustomLint {
                                    path: file.path.clone(),
                                },
                                file.location.clone(),
                                format!("custom lint rule metadata conflicts: {rule}"),
                            );
                            continue;
                        }
                        Some(_) => {}
                        None => {
                            ctx.index
                                .custom_lints
                                .rules
                                .insert(rule.clone(), CustomRuleDefinitionNode { definition });
                        }
                    }
                    ctx.index
                        .custom_lints
                        .registrations
                        .push(CustomLintRegistration {
                            file_path: file.path.clone(),
                            rule,
                            stage,
                            selector,
                            handler: registration.handler,
                            location: file.location.clone(),
                        });
                }
                Err((rule, message)) => push_register_diagnostic(
                    &mut ctx.diagnostics,
                    rule,
                    SemanticEntity::CustomLint {
                        path: file.path.clone(),
                    },
                    file.location.clone(),
                    message,
                ),
            }
        }
    }

    lint_duplicate_custom_registrations(ctx);
}

fn lint_duplicate_custom_registrations(ctx: &mut LintContext) {
    let mut seen: BTreeMap<String, &CustomLintRegistration> = BTreeMap::new();

    for registration in &ctx.index.custom_lints.registrations {
        let key = registration_key(registration);
        if let Some(first) = seen.get(&key) {
            push_register_diagnostic(
                &mut ctx.diagnostics,
                RototoRuleId::CustomLintRegistrationDuplicate,
                SemanticEntity::CustomLint {
                    path: registration.file_path.clone(),
                },
                registration.location.clone(),
                format!(
                    "custom lint registration duplicates an earlier registration: {}",
                    registration.handler
                ),
            );
            if let Some(diagnostic) = ctx.diagnostics.last_mut() {
                diagnostic
                    .related
                    .push(crate::diagnostics::RelatedLocation {
                        location: first.location.clone(),
                        message: "first matching registration".to_owned(),
                    });
            }
        } else {
            seen.insert(key, registration);
        }
    }
}

fn registration_key(registration: &CustomLintRegistration) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        registration.file_path,
        lint_stage_key(registration.stage),
        selector_key(&registration.selector),
        registration.rule,
        registration.handler
    )
}

fn lint_stage_key(stage: LintStage) -> &'static str {
    match stage {
        LintStage::Discover => "discover",
        LintStage::Parse => "parse",
        LintStage::Project => "project",
        LintStage::Register => "register",
        LintStage::Reference => "reference",
        LintStage::Value => "value",
        LintStage::Graph => "graph",
        LintStage::Policy => "policy",
    }
}

fn selector_key(selector: &RegisteredLintSelector) -> String {
    address_key(&selector.address)
}

fn address_key(address: &RegisteredLintAddress) -> String {
    match address {
        RegisteredLintAddress::Package => "/".to_owned(),
        RegisteredLintAddress::Variables => "/variables".to_owned(),
        RegisteredLintAddress::Variable { id } => format!("/variables/{id}"),
        RegisteredLintAddress::VariableValues { variable } => {
            format!("/variables/{variable}/values")
        }
        RegisteredLintAddress::VariableValue { variable, key } => {
            format!("/variables/{variable}/values/{key}")
        }
        RegisteredLintAddress::VariableRules { variable } => {
            format!("/variables/{variable}/rules")
        }
        RegisteredLintAddress::VariableRule { variable, index } => {
            format!("/variables/{variable}/rules/{index}")
        }
        RegisteredLintAddress::Catalogs => "/catalogs".to_owned(),
        RegisteredLintAddress::Catalog { id } => format!("/catalogs/{id}"),
        RegisteredLintAddress::CatalogEntries { catalog } => format!("/catalogs/{catalog}/entries"),
        RegisteredLintAddress::CatalogEntry { catalog, key } => {
            format!("/catalogs/{catalog}/entries/{key}")
        }
        RegisteredLintAddress::EvaluationContexts => "/evaluation-contexts".to_owned(),
        RegisteredLintAddress::EvaluationContext { id } => format!("/evaluation-contexts/{id}"),
        RegisteredLintAddress::EvaluationContextSamples { evaluation_context } => {
            format!("/evaluation-contexts/{evaluation_context}/samples")
        }
        RegisteredLintAddress::EvaluationContextSample {
            evaluation_context,
            key,
        } => format!("/evaluation-contexts/{evaluation_context}/samples/{key}"),
    }
}

fn validate_custom_registration(
    registration: &lua_lint::RawCustomLintRegistration,
) -> std::result::Result<
    (LintStage, RegisteredLintSelector, CustomRuleDefinition),
    (RototoRuleId, String),
> {
    let stage = LintStage::Policy;
    let selector = parse_registered_lint_selector(&registration.target)?;
    if !registration.handler_exists {
        return Err((
            RototoRuleId::CustomLintRegistrationInvalid,
            format!(
                "custom lint registration handler is not callable: {}",
                registration.handler
            ),
        ));
    }

    let rule = CustomRuleId::parse(&registration.id).map_err(|err| {
        (
            RototoRuleId::CustomLintRegistrationInvalid,
            format!(
                "custom lint registration rule id is invalid: {}: {err}",
                registration.id
            ),
        )
    })?;
    let severity = match registration.severity.as_deref() {
        None | Some("error") => Severity::Error,
        Some("warning") => Severity::Warning,
        Some(severity) => {
            return Err((
                RototoRuleId::CustomLintRegistrationInvalid,
                format!("custom lint registration rule severity is unsupported: {severity}"),
            ));
        }
    };
    let definition = CustomRuleDefinition::with_severity(
        rule,
        severity,
        registration.title.clone(),
        registration.help.clone(),
    );

    Ok((stage, selector, definition))
}

fn parse_registered_lint_selector(
    target: &str,
) -> std::result::Result<RegisteredLintSelector, (RototoRuleId, String)> {
    let address = parse_registered_lint_address(target)?;
    Ok(RegisteredLintSelector { address })
}

fn parse_registered_lint_address(
    target: &str,
) -> std::result::Result<RegisteredLintAddress, (RototoRuleId, String)> {
    let normalized = if target == "/" {
        "/"
    } else {
        target.trim_end_matches('/')
    };
    if !normalized.starts_with('/') {
        return unsupported_registration_target(target);
    }
    let segments = normalized
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let address = match segments.as_slice() {
        [] => RegisteredLintAddress::Package,
        ["variables"] => RegisteredLintAddress::Variables,
        ["variables", id] => RegisteredLintAddress::Variable {
            id: parse_address_id(target, id)?,
        },
        ["variables", variable, "values"] => RegisteredLintAddress::VariableValues {
            variable: parse_address_id(target, variable)?,
        },
        ["variables", variable, "values", key] => RegisteredLintAddress::VariableValue {
            variable: parse_address_id(target, variable)?,
            key: parse_address_id(target, key)?,
        },
        ["variables", variable, "rules"] => RegisteredLintAddress::VariableRules {
            variable: parse_address_id(target, variable)?,
        },
        ["variables", variable, "rules", index] => RegisteredLintAddress::VariableRule {
            variable: parse_address_id(target, variable)?,
            index: parse_address_index(target, index)?,
        },
        ["catalogs"] => RegisteredLintAddress::Catalogs,
        ["catalogs", id] => RegisteredLintAddress::Catalog {
            id: parse_address_id(target, id)?,
        },
        ["catalogs", catalog, "entries"] => RegisteredLintAddress::CatalogEntries {
            catalog: parse_address_id(target, catalog)?,
        },
        ["catalogs", catalog, "entries", key] => RegisteredLintAddress::CatalogEntry {
            catalog: parse_address_id(target, catalog)?,
            key: parse_address_id(target, key)?,
        },
        ["evaluation-contexts"] => RegisteredLintAddress::EvaluationContexts,
        ["evaluation-contexts", id] => RegisteredLintAddress::EvaluationContext {
            id: parse_address_id(target, id)?,
        },
        ["evaluation-contexts", evaluation_context, "samples"] => {
            RegisteredLintAddress::EvaluationContextSamples {
                evaluation_context: parse_address_id(target, evaluation_context)?,
            }
        }
        ["evaluation-contexts", evaluation_context, "samples", key] => {
            RegisteredLintAddress::EvaluationContextSample {
                evaluation_context: parse_address_id(target, evaluation_context)?,
                key: parse_address_id(target, key)?,
            }
        }
        _ => return unsupported_registration_target(target),
    };
    Ok(address)
}

fn parse_address_id(
    target: &str,
    segment: &str,
) -> std::result::Result<String, (RototoRuleId, String)> {
    if segment
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        Ok(segment.to_owned())
    } else {
        unsupported_registration_target(target)
    }
}

fn parse_address_index(
    target: &str,
    segment: &str,
) -> std::result::Result<usize, (RototoRuleId, String)> {
    segment.parse::<usize>().map_err(|_| {
        (
            RototoRuleId::CustomLintRegistrationInvalid,
            format!("custom lint registration has unsupported target: {target}"),
        )
    })
}

fn unsupported_registration_target<T>(
    target: &str,
) -> std::result::Result<T, (RototoRuleId, String)> {
    Err((
        RototoRuleId::CustomLintRegistrationInvalid,
        format!("custom lint registration has unsupported target: {target}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every supported custom-lint target address, its parsed form, and the
    /// canonical rendering. Parsing then rendering must return the input:
    /// the address grammar in the docs and the registry's internal keys are
    /// one grammar.
    #[test]
    fn registered_lint_addresses_round_trip_between_grammar_and_parse() {
        let cases: Vec<(&str, RegisteredLintAddress)> = vec![
            ("/", RegisteredLintAddress::Package),
            ("/variables", RegisteredLintAddress::Variables),
            (
                "/variables/flag",
                RegisteredLintAddress::Variable {
                    id: "flag".to_owned(),
                },
            ),
            (
                "/variables/flag/values",
                RegisteredLintAddress::VariableValues {
                    variable: "flag".to_owned(),
                },
            ),
            (
                "/variables/flag/values/on",
                RegisteredLintAddress::VariableValue {
                    variable: "flag".to_owned(),
                    key: "on".to_owned(),
                },
            ),
            (
                "/variables/flag/rules",
                RegisteredLintAddress::VariableRules {
                    variable: "flag".to_owned(),
                },
            ),
            (
                "/variables/flag/rules/2",
                RegisteredLintAddress::VariableRule {
                    variable: "flag".to_owned(),
                    index: 2,
                },
            ),
            ("/catalogs", RegisteredLintAddress::Catalogs),
            (
                "/catalogs/banner",
                RegisteredLintAddress::Catalog {
                    id: "banner".to_owned(),
                },
            ),
            (
                "/catalogs/banner/entries",
                RegisteredLintAddress::CatalogEntries {
                    catalog: "banner".to_owned(),
                },
            ),
            (
                "/catalogs/banner/entries/default",
                RegisteredLintAddress::CatalogEntry {
                    catalog: "banner".to_owned(),
                    key: "default".to_owned(),
                },
            ),
            (
                "/evaluation-contexts",
                RegisteredLintAddress::EvaluationContexts,
            ),
            (
                "/evaluation-contexts/request",
                RegisteredLintAddress::EvaluationContext {
                    id: "request".to_owned(),
                },
            ),
            (
                "/evaluation-contexts/request/samples",
                RegisteredLintAddress::EvaluationContextSamples {
                    evaluation_context: "request".to_owned(),
                },
            ),
            (
                "/evaluation-contexts/request/samples/basic",
                RegisteredLintAddress::EvaluationContextSample {
                    evaluation_context: "request".to_owned(),
                    key: "basic".to_owned(),
                },
            ),
        ];
        for (address, expected) in cases {
            let parsed = parse_registered_lint_address(address)
                .unwrap_or_else(|(_, message)| panic!("{address} should parse: {message}"));
            assert_eq!(parsed, expected, "{address} parsed to the wrong address");
            assert_eq!(
                address_key(&parsed),
                address,
                "{address} did not round-trip through address_key"
            );
        }
    }

    #[test]
    fn unsupported_lint_addresses_are_rejected() {
        for address in [
            "variables",
            "/unknown",
            "/catalogs/banner/entries/default/extra",
            "/variables/flag/rules/not-a-number",
            // A namespaced id contains '/', which the address grammar reads
            // as a path separator: namespaced entities cannot be addressed
            // today. Pinned here so a future grammar change is deliberate.
            "/variables/acme/in_trial",
        ] {
            assert!(
                parse_registered_lint_address(address).is_err(),
                "{address} should be rejected"
            );
        }
    }

    /// A trailing slash normalizes away rather than reading as an empty
    /// trailing segment.
    #[test]
    fn trailing_slashes_normalize_in_lint_addresses() {
        let parsed = parse_registered_lint_address("/variables/").unwrap();
        assert_eq!(parsed, RegisteredLintAddress::Variables);
    }
}
