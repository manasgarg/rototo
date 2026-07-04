use super::*;

pub(super) fn evaluation_context_report(
    snapshot: &PackageLintSnapshot,
    evaluation_context: &EvaluationContextNode,
) -> EvaluationContextInspectReport {
    let diagnostics = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic_belongs_to_evaluation_context(diagnostic, &evaluation_context.id)
        })
        .cloned()
        .collect();
    let (status, error) = if let Some(message) = &evaluation_context.invalid_message {
        ("invalid".to_owned(), Some(message.clone()))
    } else if evaluation_context.validator.is_some() {
        ("valid".to_owned(), None)
    } else {
        ("unavailable".to_owned(), None)
    };
    let json = evaluation_context.json.as_ref();

    EvaluationContextInspectReport {
        id: evaluation_context.id.clone(),
        path: evaluation_context.path.clone(),
        status,
        error,
        title: json
            .and_then(|json| json.get("title"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        description: json
            .and_then(|json| json.get("description"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        samples: evaluation_context_samples(snapshot, &evaluation_context.id),
        diagnostics,
    }
}

pub(super) fn evaluation_context_samples(
    snapshot: &PackageLintSnapshot,
    evaluation_context: &str,
) -> Vec<EvaluationContextSampleInspectReport> {
    snapshot
        .index
        .evaluation_context_samples
        .get(evaluation_context)
        .into_iter()
        .flat_map(|entries| entries.values())
        .filter_map(|entry| {
            entry
                .value
                .as_ref()
                .map(|value| EvaluationContextSampleInspectReport {
                    key: entry.key.clone(),
                    value: value.clone(),
                    location: entry.location.clone(),
                })
        })
        .collect()
}
