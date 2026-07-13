use super::*;

pub(super) async fn inspect_variable(
    snapshot: &PackageLintSnapshot,
    runtime: Option<&RuntimePackage>,
    request: &PackageInspectRequest,
    id: &str,
) -> Result<VariableInspectReport> {
    let variable = snapshot
        .index
        .variables
        .get(id)
        .ok_or_else(|| RototoError::new(format!("variable not found: variable://{id}")))?;
    let (_source_uri, path) = document_uri_path(snapshot, variable.doc);
    let dependencies = variable_dependencies(snapshot, id);
    let diagnostics = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_belongs_to_variable(diagnostic, id))
        .cloned()
        .collect();
    let trace = match (runtime, &request.context) {
        (Some(runtime), Some(context)) => {
            runtime.validate_context_for_variable(id, context)?;
            Some(trace_variable_unchecked(runtime, id, context)?)
        }
        _ => None,
    };
    let evaluation_contexts: Vec<String> = snapshot
        .evaluation_context_compatibility()
        .variables
        .remove(id)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let sample_coverage = runtime
        .and_then(|runtime| variable_sample_coverage(snapshot, runtime, id, &evaluation_contexts));

    Ok(VariableInspectReport {
        id: id.to_owned(),
        uri: format!("variable://{id}"),
        path,
        description: variable.description.as_ref().and_then(present_string_value),
        evaluation_contexts,
        context_attributes: variable_context_attributes(snapshot, variable),
        type_source: variable_type_source_label(variable),
        schema: variable_schema_dependency(snapshot, id),
        values: variable_values(variable, &snapshot.index),
        resolve: variable_resolve(&snapshot.index, variable),
        dependencies,
        sample_coverage,
        diagnostics,
        trace,
    })
}

pub(super) fn variable_type_source_label(variable: &VariableNode) -> String {
    match &variable.type_source {
        TypeSourceNode::Primitive(type_name) => type_name.value.clone(),
        TypeSourceNode::Catalog(catalog) => format!("catalog:{}", catalog.value),
        TypeSourceNode::Schema(schema) => format!("schema {}", schema.value),
        TypeSourceNode::Missing { .. } => "missing".to_owned(),
        TypeSourceNode::Conflict { .. } => "conflict".to_owned(),
        TypeSourceNode::Invalid { .. } => "invalid".to_owned(),
    }
}

pub(super) fn variable_schema_dependency(
    _snapshot: &PackageLintSnapshot,
    _variable: &str,
) -> Option<String> {
    None
}

pub(super) fn variable_values(
    variable: &VariableNode,
    _index: &SemanticIndex,
) -> Vec<ValueInspectReport> {
    let mut values = Vec::new();
    for value in variable.values.inline_values.values() {
        values.push(value_report(value));
    }
    values.sort_by(|left, right| left.key.cmp(&right.key));
    values
}

pub(super) fn value_report(value: &ValueNode) -> ValueInspectReport {
    let origin = match &value.origin {
        ValueOrigin::Inline { .. } => "inline".to_owned(),
    };
    ValueInspectReport {
        key: value.key.clone(),
        origin,
        value: value.value.clone(),
        location: value.location.clone(),
    }
}

pub(super) fn variable_resolve(
    index: &SemanticIndex,
    variable: &VariableNode,
) -> ResolveInspectReport {
    let ResolveNode::Resolve {
        location,
        method,
        default,
        rules,
        query,
        assignments,
    } = &variable.resolve
    else {
        return ResolveInspectReport {
            method: "rules".to_owned(),
            default_value: None,
            rules: Vec::new(),
            query: None,
            allocation: None,
            location: variable.resolve.location(),
        };
    };
    let rules = match rules {
        RuleCollection::Rules(rules) => rules
            .iter()
            .map(|rule| RulePathwayInspectReport {
                index: rule.index,
                when: present_expression_source(&rule.when),
                value: present_json_value(&rule.value),
                location: rule.location.clone(),
            })
            .collect(),
        RuleCollection::Invalid { .. } => Vec::new(),
    };
    let query = query.as_ref().map(|query| QueryInspectReport {
        from: match &query.from {
            ProjectField::Present(from) => Some(from.value.clone()),
            _ => None,
        },
        filter: present_expression_source(&query.filter),
        sort: present_expression_source(&query.sort),
        order: query.order.as_ref().and_then(|order| match order {
            ProjectField::Present(order) => Some(order.value.clone()),
            _ => None,
        }),
        limit: query.limit.as_ref().and_then(|limit| match limit {
            ProjectField::Present(limit) => Some(limit.value),
            _ => None,
        }),
    });
    let allocation = assignments.as_ref().map(|assignments| {
        let allocation_id = present_string_value(&assignments.allocation);
        let declared = allocation_id.as_ref().and_then(|allocation_id| {
            index.layers.values().find_map(|layer| {
                layer
                    .allocations
                    .iter()
                    .find(|candidate| {
                        matches!(&candidate.id, ProjectField::Present(id) if &id.value == allocation_id)
                    })
                    .map(|allocation| (layer, allocation))
            })
        });
        AllocationInspectReport {
            allocation: allocation_id,
            layer: declared.map(|(layer, _)| layer.id.clone()),
            unit: declared.and_then(|(layer, _)| match &layer.unit {
                ProjectField::Present(unit) => Some(unit.value.source().to_owned()),
                _ => None,
            }),
            buckets: declared.and_then(|(layer, _)| match &layer.buckets {
                ProjectField::Present(buckets) => Some(buckets.value),
                _ => None,
            }),
            eligibility: declared.and_then(|(_, allocation)| match &allocation.eligibility {
                Some(ProjectField::Present(eligibility)) => {
                    Some(eligibility.value.source().to_owned())
                }
                _ => None,
            }),
            arms: declared
                .map(|(_, allocation)| {
                    allocation
                        .arms
                        .iter()
                        .map(|arm| AllocationArmInspectReport {
                            name: present_string_value(&arm.name),
                            buckets: present_string_value(&arm.buckets),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            assigns: assignments
                .assigns
                .iter()
                .map(|assign| AssignInspectReport {
                    arm: present_string_value(&assign.arm),
                    value: present_json_value(&assign.value),
                })
                .collect(),
        }
    });
    ResolveInspectReport {
        method: method
            .as_ref()
            .map(|method| method.value.clone())
            .unwrap_or_else(|| "rules".to_owned()),
        default_value: present_json_value(default),
        rules,
        query,
        allocation,
        location: location.clone(),
    }
}

pub(super) fn variable_dependencies(
    snapshot: &PackageLintSnapshot,
    variable: &str,
) -> DependencyInspectReport {
    let mut variables = BTreeSet::new();
    let mut context_paths = BTreeSet::new();
    let mut catalogs = BTreeSet::new();

    collect_variable_dependencies(
        snapshot,
        variable,
        &mut variables,
        &mut context_paths,
        &mut BTreeSet::new(),
    );
    variables.remove(variable);

    for edge in snapshot.references.edges() {
        if let (
            ReferenceSource::VariableCatalog {
                variable: source_variable,
            },
            ReferenceTarget::Catalog(catalog),
        ) = (&edge.source, &edge.target)
            && source_variable == variable
            && edge.is_resolved()
        {
            catalogs.insert(catalog.clone());
        }
    }

    DependencyInspectReport {
        variables: variables.into_iter().collect(),
        context_paths: context_paths.into_iter().collect(),
        catalogs: catalogs.into_iter().collect(),
    }
}

/// Every sample defined under any of the variable's compatible
/// evaluation contexts, as raw context values to resolve against.
pub(super) fn compatible_context_samples(
    snapshot: &PackageLintSnapshot,
    compatible: &[String],
) -> Vec<serde_json::Value> {
    let mut samples = Vec::new();
    for context_id in compatible {
        let Some(entries) = snapshot.index.evaluation_context_samples.get(context_id) else {
            continue;
        };
        for entry in entries.values() {
            if let Some(value) = &entry.value {
                samples.push(value.clone());
            }
        }
    }
    samples
}

pub(super) fn variable_sample_coverage(
    snapshot: &PackageLintSnapshot,
    runtime: &RuntimePackage,
    id: &str,
    compatible: &[String],
) -> Option<VariableSampleCoverageReport> {
    let samples = compatible_context_samples(snapshot, compatible);
    if samples.is_empty() {
        return None;
    }
    let mut selected_rules = BTreeSet::new();
    let mut default_covered = false;
    let mut sample_count = 0;
    for sample in &samples {
        let Ok(trace) = trace_variable_unchecked(runtime, id, sample) else {
            continue;
        };
        sample_count += 1;
        match trace.rules.iter().find(|rule| rule.matched) {
            Some(rule) => {
                selected_rules.insert(rule.index);
            }
            None => default_covered = true,
        }
    }
    let rules = variable_resolve_rules(snapshot.index.variables.get(id)?)
        .into_iter()
        .flatten()
        .map(|rule| RuleSampleCoverageReport {
            index: rule.index,
            covered: selected_rules.contains(&rule.index),
        })
        .collect();
    Some(VariableSampleCoverageReport {
        sample_count,
        default_covered,
        rules,
    })
}

pub(super) fn variable_context_attributes(
    snapshot: &PackageLintSnapshot,
    variable: &VariableNode,
) -> Vec<ContextAttributeInspectReport> {
    let mut expressions = Vec::new();
    if let Some(rules) = variable_resolve_rules(variable) {
        for rule in rules {
            if let Some(ProjectField::Present(expression)) = &rule.when {
                expressions.push(&expression.value);
            }
        }
    }
    for expression in resolve_query_expressions(variable) {
        expressions.push(expression);
    }
    context_attributes_for_expressions(snapshot, expressions)
}

/// Pair every context attribute the given expressions read with the scalar
/// types they expect of it and how each evaluation context declares it, so
/// inspect and show can surface both the type contract and any gap.
pub(super) fn context_attributes_for_expressions(
    snapshot: &PackageLintSnapshot,
    expressions: Vec<&Expression>,
) -> Vec<ContextAttributeInspectReport> {
    let mut constraints: BTreeMap<String, BTreeSet<ContextScalarType>> = BTreeMap::new();
    for expression in expressions {
        for path in &expression.references().context_paths {
            if !path.is_empty() {
                constraints.entry(path.clone()).or_default();
            }
        }
        for (path, expected) in &expression.references().context_path_types {
            constraints
                .entry(path.clone())
                .or_default()
                .extend(expected.iter().copied());
        }
    }

    constraints
        .into_iter()
        .map(|(path, expected)| {
            let declarations = snapshot
                .index
                .evaluation_contexts
                .values()
                .filter_map(|context| {
                    let schema = context.json.as_ref()?;
                    let declared_types = context_path_declaration(schema, &path)?;
                    Some(ContextAttributeDeclarationReport {
                        evaluation_context: context.id.clone(),
                        declared_types,
                    })
                })
                .collect::<Vec<_>>();
            let status = context_attribute_status(snapshot, &path, &expected, &declarations);
            ContextAttributeInspectReport {
                path,
                expected_types: expected
                    .iter()
                    .map(|scalar| scalar.label().to_owned())
                    .collect(),
                status,
                declarations,
            }
        })
        .collect()
}

pub(super) fn context_attribute_status(
    snapshot: &PackageLintSnapshot,
    path: &str,
    expected: &BTreeSet<ContextScalarType>,
    declarations: &[ContextAttributeDeclarationReport],
) -> String {
    if declarations.is_empty() {
        return "undeclared".to_owned();
    }
    if expected.is_empty() {
        return "ok".to_owned();
    }
    let satisfied = snapshot.index.evaluation_contexts.values().any(|context| {
        context.json.as_ref().is_some_and(|schema| {
            matches!(
                context_path_type_fit(schema, path, expected),
                ContextPathTypeFit::Ok
            )
        })
    });
    if satisfied { "ok" } else { "type_mismatch" }.to_owned()
}

/// Walk `variables["<id>"]` references transitively from `variable`, recording
/// every reachable variable (including `variable` itself) and every context
/// path any of their rule expressions read.
pub(super) fn collect_variable_dependencies(
    snapshot: &PackageLintSnapshot,
    variable: &str,
    variables: &mut BTreeSet<String>,
    context_paths: &mut BTreeSet<String>,
    seen: &mut BTreeSet<String>,
) {
    if !seen.insert(variable.to_owned()) {
        return;
    }
    variables.insert(variable.to_owned());
    let Some(node) = snapshot.index.variables.get(variable) else {
        return;
    };
    let mut expressions = Vec::new();
    if let Some(rules) = variable_resolve_rules(node) {
        for rule in rules {
            if let Some(ProjectField::Present(expression)) = &rule.when {
                expressions.push(&expression.value);
            }
        }
    }
    for expression in resolve_query_expressions(node) {
        expressions.push(expression);
    }
    for expression in expressions {
        for path in &expression.references().context_paths {
            if !path.is_empty() {
                context_paths.insert(path.clone());
            }
        }
        for nested in &expression.references().variables {
            if snapshot.index.variables.contains_key(nested) {
                collect_variable_dependencies(snapshot, nested, variables, context_paths, seen);
            }
        }
    }
}

/// The `[resolve]` query expressions (filter and sort), when the variable uses
/// the query method.
pub(super) fn resolve_query_expressions(
    variable: &VariableNode,
) -> Vec<&crate::expression::Expression> {
    let ResolveNode::Resolve {
        query: Some(query), ..
    } = &variable.resolve
    else {
        return Vec::new();
    };
    let mut expressions = Vec::new();
    for field in [&query.filter, &query.sort].into_iter().flatten() {
        if let ProjectField::Present(expression) = field {
            expressions.push(&expression.value);
        }
    }
    expressions
}
