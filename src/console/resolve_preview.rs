use std::collections::BTreeMap;
use std::pin::Pin;

use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::lint::WorkspaceSemanticModel;
use crate::sdk::{ResolveContext, Workspace};

/* Resolution previews against saved request contexts. These run the real
runtime (the same evaluation applications get) and then annotate the
declared rules and predicates with what each one saw. */

/// Variable resolution result for one saved request context.
///
/// The console computes this on demand with the same runtime path applications
/// use, then decorates it with rule-walk detail for the UI. It is never stored;
/// saved context files and the staged workspace version are the durable inputs.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedContextResolution {
    pub name: String,
    pub path: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_key: Option<String>,
    /// The walk through the rules: each step is a qualifier evaluation, in
    /// order, ending at the first match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<ResolutionStep>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_default: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One rule considered during a variable preview.
///
/// Steps are emitted in declaration order and stop at the first matching rule,
/// mirroring runtime resolution. The vector lives only in the preview response.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionStep {
    pub index: usize,
    pub qualifier: String,
    pub matched: bool,
    pub evaluation: QualifierEvaluation,
}

/// Qualifier preview for one saved request context.
///
/// This is used on inspect screens to explain named conditions outside a
/// variable rule walk. It is computed per request and may carry an error when
/// the context JSON cannot be evaluated.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualifierContextEvaluation {
    pub name: String,
    pub path: String,
    pub evaluation: Option<QualifierEvaluation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A qualifier's resolution against one context: its verdict plus every
/// predicate, with the context value the predicate read and — for
/// `qualifier.<id>` predicates — the nested qualifier's own resolution.
/// Preview routes rebuild it from the current staged workspace and discard it
/// after the response.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualifierEvaluation {
    pub id: String,
    pub matched: Option<bool>,
    pub predicates: Vec<PredicateEvaluation>,
}

/// Predicate-level detail for one qualifier preview.
///
/// The value records what the predicate declared, what context value it read,
/// and any nested qualifier evaluation. It is reconstructed each time a preview
/// response is built.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PredicateEvaluation {
    pub index: usize,
    pub attribute: Option<String>,
    pub op: Option<String>,
    pub value_literal: Option<String>,
    pub context_value: Option<String>,
    pub nested: Option<Box<QualifierEvaluation>>,
}

/// Compact truth table for all qualifiers against one saved context.
///
/// Branch edit screens use this to show how a pending edit behaves across
/// sample contexts. It is computed on demand and discarded after serialization.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditContextPreview {
    pub name: String,
    pub qualifier_truth: BTreeMap<String, bool>,
}

pub async fn evaluate_qualifier_with_context(
    runtime: &Workspace,
    model: &WorkspaceSemanticModel,
    qualifier_id: &str,
    context: &JsonValue,
) -> QualifierEvaluation {
    let mut seen = vec![qualifier_id.to_owned()];
    evaluate_recursive(runtime, model, qualifier_id, context, &mut seen).await
}

fn evaluate_recursive<'a>(
    runtime: &'a Workspace,
    model: &'a WorkspaceSemanticModel,
    qualifier_id: &'a str,
    context: &'a JsonValue,
    seen: &'a mut Vec<String>,
) -> Pin<Box<dyn Future<Output = QualifierEvaluation> + Send + 'a>> {
    Box::pin(async move {
        let matched = match ResolveContext::from_json(context.clone()) {
            Ok(resolve_context) => runtime
                .resolve_qualifier(qualifier_id, &resolve_context)
                .await
                .ok()
                .map(|resolution| resolution.value),
            Err(_) => None,
        };
        let qualifier = model
            .qualifiers
            .iter()
            .find(|candidate| candidate.id == qualifier_id);
        let mut predicates = Vec::new();
        for predicate in qualifier
            .map(|q| q.predicates.as_slice())
            .unwrap_or_default()
        {
            let attribute = predicate
                .attribute
                .as_ref()
                .and_then(|field| field.value.clone());
            let mut nested = None;
            let mut context_value = None;
            if let Some(attribute) = attribute.as_deref() {
                if let Some(nested_id) = attribute.strip_prefix("qualifier.") {
                    if !seen.iter().any(|id| id == nested_id) {
                        seen.push(nested_id.to_owned());
                        nested = Some(Box::new(
                            evaluate_recursive(runtime, model, nested_id, context, seen).await,
                        ));
                    }
                } else {
                    context_value =
                        context_path_value(context, attribute).map(|value| value.to_string());
                }
            }
            predicates.push(PredicateEvaluation {
                index: predicate.index,
                attribute,
                op: predicate.op.as_ref().and_then(|field| field.value.clone()),
                value_literal: predicate.value.as_ref().map(|value| value.to_string()),
                context_value,
                nested,
            });
        }
        QualifierEvaluation {
            id: qualifier_id.to_owned(),
            matched,
            predicates,
        }
    })
}

/// Display-only lookup of the context value a predicate reads.
fn context_path_value<'a>(context: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let mut current = context;
    for segment in path.split('.') {
        current = current.as_object()?.get(segment)?;
    }
    Some(current)
}

/// Source text for one saved context example.
///
/// Workspace routes build these by reading `contexts/*.json` from the staged
/// checkout, then pass them into preview functions. The struct is an in-memory
/// transfer object, not a persisted console record.
pub struct SavedContextInput {
    pub name: String,
    pub path: String,
    pub text: String,
}

/// Resolves one variable against each saved context, annotating the rule walk.
pub async fn resolve_saved_contexts(
    runtime: &Workspace,
    model: &WorkspaceSemanticModel,
    variable_id: &str,
    contexts: &[SavedContextInput],
) -> Vec<SavedContextResolution> {
    let rules = model
        .variables
        .iter()
        .find(|variable| variable.id == variable_id)
        .and_then(|variable| variable.resolve.as_ref())
        .map(|resolve| resolve.rules.as_slice())
        .unwrap_or_default();

    let mut resolutions = Vec::new();
    for context_input in contexts {
        match resolve_one(runtime, model, variable_id, rules, context_input).await {
            Ok(resolution) => resolutions.push(resolution),
            Err(error) => resolutions.push(SavedContextResolution {
                name: context_input.name.clone(),
                path: context_input.path.clone(),
                ok: false,
                value_key: None,
                steps: None,
                used_default: None,
                error: Some(error),
            }),
        }
    }
    resolutions
}

async fn resolve_one(
    runtime: &Workspace,
    model: &WorkspaceSemanticModel,
    variable_id: &str,
    rules: &[crate::lint::RuleModel],
    context_input: &SavedContextInput,
) -> std::result::Result<SavedContextResolution, String> {
    let context: JsonValue =
        serde_json::from_str(&context_input.text).map_err(|err| err.to_string())?;
    let resolve_context =
        ResolveContext::from_json(context.clone()).map_err(|err| err.to_string())?;
    let resolution = runtime
        .resolve_variable(variable_id, &resolve_context)
        .await
        .map_err(|err| err.to_string())?;

    let mut steps = Vec::new();
    let mut matched_rule = false;
    for rule in rules {
        let Some(qualifier) = rule
            .qualifier
            .as_ref()
            .and_then(|field| field.value.clone())
        else {
            continue;
        };
        let evaluation =
            evaluate_qualifier_with_context(runtime, model, &qualifier, &context).await;
        let Some(matched) = evaluation.matched else {
            return Err(format!("qualifier {qualifier} could not be evaluated"));
        };
        steps.push(ResolutionStep {
            index: rule.index,
            qualifier,
            matched,
            evaluation,
        });
        if matched {
            matched_rule = true;
            break;
        }
    }

    Ok(SavedContextResolution {
        name: context_input.name.clone(),
        path: context_input.path.clone(),
        ok: true,
        value_key: Some(resolution.value_key),
        steps: Some(steps),
        used_default: Some(!matched_rule),
        error: None,
    })
}

/// Evaluates every workspace qualifier against each saved request context.
pub async fn edit_context_previews(
    runtime: &Workspace,
    qualifier_ids: &[String],
    contexts: &[SavedContextInput],
) -> Vec<EditContextPreview> {
    let mut previews = Vec::new();
    for context_input in contexts {
        let Ok(context) = serde_json::from_str::<JsonValue>(&context_input.text) else {
            continue;
        };
        let Ok(resolve_context) = ResolveContext::from_json(context) else {
            continue;
        };
        let mut qualifier_truth = BTreeMap::new();
        for qualifier_id in qualifier_ids {
            if let Ok(resolution) = runtime
                .resolve_qualifier(qualifier_id, &resolve_context)
                .await
            {
                qualifier_truth.insert(qualifier_id.clone(), resolution.value);
            }
        }
        previews.push(EditContextPreview {
            name: context_input.name.clone(),
            qualifier_truth,
        });
    }
    previews
}

pub async fn qualifier_context_evaluations(
    runtime: &Workspace,
    model: &WorkspaceSemanticModel,
    qualifier_id: &str,
    contexts: &[SavedContextInput],
) -> Vec<QualifierContextEvaluation> {
    let mut evaluations = Vec::new();
    for context_input in contexts {
        match serde_json::from_str::<JsonValue>(&context_input.text) {
            Ok(context) => {
                let evaluation =
                    evaluate_qualifier_with_context(runtime, model, qualifier_id, &context).await;
                evaluations.push(QualifierContextEvaluation {
                    name: context_input.name.clone(),
                    path: context_input.path.clone(),
                    evaluation: Some(evaluation),
                    error: None,
                });
            }
            Err(error) => evaluations.push(QualifierContextEvaluation {
                name: context_input.name.clone(),
                path: context_input.path.clone(),
                evaluation: None,
                error: Some(error.to_string()),
            }),
        }
    }
    evaluations
}
