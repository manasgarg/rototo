use std::collections::BTreeMap;

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, LintStage, RototoRuleId, SemanticEntity, Severity,
};
use crate::lua_lint;

use super::super::engine::LintContext;
use super::super::index::{CustomLintRegistration, CustomRuleDefinitionNode};
use super::super::stages::push_register_diagnostic;
use super::RegisteredLintSelector;
use super::runner;
use crate::address::{Address, EntityClass};

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
        registration.selector.address,
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

/// The entity classes a lint target's leaf step may name today. The other
/// classes (enum, layer, linter, manifest, governance) become targetable
/// when their handler data marshalling exists.
const TARGETABLE_LEAF_CLASSES: &[EntityClass] = &[
    EntityClass::Package,
    EntityClass::Variable,
    EntityClass::Catalog,
    EntityClass::Entry,
    EntityClass::EvaluationContext,
    EntityClass::Sample,
];

fn parse_registered_lint_selector(
    target: &str,
) -> std::result::Result<RegisteredLintSelector, (RototoRuleId, String)> {
    if target.starts_with('/') || target == "package" {
        return Err(unsupported_registration_target(
            target,
            "targets use the address grammar, for example package=, variable=<id>, \
             or catalog=<id>:entry=<key>",
        ));
    }
    let address = Address::parse(target)
        .map_err(|err| unsupported_registration_target(target, err.to_string()))?;
    if address.pointer().is_some() {
        return Err(unsupported_registration_target(
            target,
            "# pointer targets are not supported yet",
        ));
    }
    let leaf = address.last_step().class;
    if !TARGETABLE_LEAF_CLASSES.contains(&leaf) {
        return Err(unsupported_registration_target(
            target,
            format!("{}= entities cannot be targeted yet", leaf.as_str()),
        ));
    }
    Ok(RegisteredLintSelector { address })
}

fn unsupported_registration_target(
    target: &str,
    hint: impl std::fmt::Display,
) -> (RototoRuleId, String) {
    (
        RototoRuleId::CustomLintRegistrationInvalid,
        format!("custom lint registration has unsupported target: {target}; {hint}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector(target: &str) -> RegisteredLintSelector {
        parse_registered_lint_selector(target)
            .unwrap_or_else(|(_, message)| panic!("{target} should be accepted: {message}"))
    }

    fn rejection(target: &str) -> String {
        parse_registered_lint_selector(target)
            .err()
            .unwrap_or_else(|| panic!("{target} should be rejected"))
            .1
    }

    /// Every depth a lint target accepts: the package, class collectives,
    /// namespace subtrees, entities (namespaced included), and nested
    /// entities. The selector stores the canonical address.
    #[test]
    fn lint_targets_accept_every_documented_depth() {
        for target in [
            "package=",
            "variable=",
            "variable=payments/",
            "variable=flag",
            "variable=payments/max_tokens",
            "catalog=",
            "catalog=banner",
            "catalog=banner:entry=",
            "catalog=banner:entry=default",
            "catalog=acme/banner:entry=promo/summer",
            "evaluation-context=request",
            "evaluation-context=request:sample=",
            "evaluation-context=request:sample=basic",
        ] {
            assert_eq!(selector(target).address.to_string(), target);
        }
    }

    #[test]
    fn legacy_target_spellings_get_the_migration_hint() {
        for target in [
            "/",
            "/variables/flag",
            "/catalogs/banner/entries",
            "package",
        ] {
            let message = rejection(target);
            assert!(
                message.contains("targets use the address grammar"),
                "{target}: {message}"
            );
        }
    }

    #[test]
    fn pointer_targets_are_not_supported_yet() {
        let message = rejection("variable=flag#/resolve/default");
        assert!(
            message.contains("# pointer targets are not supported yet"),
            "{message}"
        );
    }

    #[test]
    fn untargetable_classes_are_rejected_with_the_class_named() {
        for (target, class) in [
            ("enum=tier", "enum="),
            ("layer=rollout", "layer="),
            ("linter=budget", "linter="),
            ("manifest=", "manifest="),
            ("governance=", "governance="),
        ] {
            let message = rejection(target);
            assert!(
                message.contains(&format!("{class} entities cannot be targeted yet")),
                "{target}: {message}"
            );
        }
    }

    #[test]
    fn malformed_targets_carry_the_address_parse_reason() {
        let message = rejection("variables");
        assert!(message.contains("missing the `=`"), "{message}");
        let message = rejection("variable=Payments");
        assert!(message.contains("lowercase snake_case"), "{message}");
    }
}
