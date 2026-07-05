use std::collections::BTreeMap;

use crate::diagnostics::{LintDiagnostic, RototoRuleId};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::stages::push_project_diagnostic;
use super::field_is_integer;

const ALLOCATION_STATUSES: &[&str] = &["draft", "running", "concluded"];

pub(super) fn lint_layer_shapes(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    // Variables name an allocation without a layer qualifier, so allocation
    // ids must be unique across the whole package, not just within a layer.
    let mut allocation_owners: BTreeMap<&str, &str> = BTreeMap::new();

    for layer in ctx.index.layers.values() {
        if !field_is_integer(&layer.schema_version, 1) {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::LayerSchemaVersion,
                layer.target(),
                layer.schema_version.location(),
                "layer must declare schema_version = 1",
            );
        }

        match &layer.unit {
            ProjectField::Present(unit) => {
                lint_layer_expression(&mut diagnostics, &ctx.index, layer, "unit", unit);
                if !unit.value.references().variables.is_empty() {
                    push_project_diagnostic(
                        &mut diagnostics,
                        RototoRuleId::LayerShape,
                        layer.target(),
                        unit.location.clone(),
                        "layer unit must read context only; variables are not available \
                         in the diversion",
                    );
                }
            }
            ProjectField::Invalid { location } | ProjectField::Missing { location } => {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::LayerShape,
                    layer.target(),
                    location.clone(),
                    "layer must declare unit as a CEL expression string",
                );
            }
        }

        let bucket_count = match &layer.buckets {
            ProjectField::Present(buckets) if buckets.value >= 1 => Some(buckets.value as u32),
            field => {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::LayerShape,
                    layer.target(),
                    field.location(),
                    "layer must declare buckets as a positive integer",
                );
                None
            }
        };

        if layer.allocations_invalid {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::LayerShape,
                layer.target(),
                layer.location.clone(),
                "allocation must use [[allocation]] tables",
            );
        }

        // (claimed bucket ranges, labeled by allocation/arm) for the overlap check
        let mut claims: Vec<(u32, u32, String)> = Vec::new();

        for allocation in &layer.allocations {
            if allocation.invalid_shape {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::LayerShape,
                    layer.target(),
                    allocation.location.clone(),
                    "allocation must be a table",
                );
                continue;
            }

            let allocation_id = match &allocation.id {
                ProjectField::Present(id) => {
                    if let Some(owner) =
                        allocation_owners.insert(id.value.as_str(), layer.id.as_str())
                    {
                        push_project_diagnostic(
                            &mut diagnostics,
                            RototoRuleId::LayerShape,
                            layer.target(),
                            id.location.clone(),
                            format!(
                                "allocation id is already declared in layer {owner}: {}",
                                id.value
                            ),
                        );
                    }
                    Some(id.value.as_str())
                }
                field => {
                    push_project_diagnostic(
                        &mut diagnostics,
                        RototoRuleId::LayerShape,
                        layer.target(),
                        field.location(),
                        "allocation must declare id",
                    );
                    None
                }
            };
            let allocation_label = allocation_id
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("allocation[{}]", allocation.index));

            if let Some(status) = &allocation.status {
                match status {
                    ProjectField::Present(status)
                        if ALLOCATION_STATUSES.contains(&status.value.as_str()) => {}
                    field => push_project_diagnostic(
                        &mut diagnostics,
                        RototoRuleId::LayerShape,
                        layer.target(),
                        field.location(),
                        "allocation status must be draft, running, or concluded",
                    ),
                }
            }

            match &allocation.eligibility {
                Some(ProjectField::Present(eligibility)) => {
                    lint_layer_expression(
                        &mut diagnostics,
                        &ctx.index,
                        layer,
                        "eligibility",
                        eligibility,
                    );
                }
                Some(ProjectField::Invalid { location } | ProjectField::Missing { location }) => {
                    push_project_diagnostic(
                        &mut diagnostics,
                        RototoRuleId::LayerShape,
                        layer.target(),
                        location.clone(),
                        "allocation eligibility must be a CEL expression string",
                    );
                }
                None => {}
            }

            if allocation.arms_invalid {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::LayerShape,
                    layer.target(),
                    allocation.location.clone(),
                    "arm must use [[allocation.arm]] tables",
                );
            }
            if allocation.arms.is_empty() && !allocation.arms_invalid {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::LayerShape,
                    layer.target(),
                    allocation.location.clone(),
                    "allocation must declare at least one [[allocation.arm]]",
                );
            }

            let mut arm_names: Vec<&str> = Vec::new();
            for arm in &allocation.arms {
                if arm.invalid_shape {
                    push_project_diagnostic(
                        &mut diagnostics,
                        RototoRuleId::LayerShape,
                        layer.target(),
                        arm.location.clone(),
                        "arm must be a table",
                    );
                    continue;
                }

                let arm_label = match &arm.name {
                    ProjectField::Present(name) => {
                        if arm_names.contains(&name.value.as_str()) {
                            push_project_diagnostic(
                                &mut diagnostics,
                                RototoRuleId::LayerShape,
                                layer.target(),
                                name.location.clone(),
                                format!("arm name is duplicated in the allocation: {}", name.value),
                            );
                        }
                        arm_names.push(name.value.as_str());
                        name.value.clone()
                    }
                    field => {
                        push_project_diagnostic(
                            &mut diagnostics,
                            RototoRuleId::LayerShape,
                            layer.target(),
                            field.location(),
                            "arm must declare name",
                        );
                        format!("arm[{}]", arm.index)
                    }
                };

                match &arm.buckets {
                    ProjectField::Present(buckets) => match parse_arm_buckets(&buckets.value) {
                        Some((start, end)) => {
                            if let Some(count) = bucket_count
                                && end >= count
                            {
                                push_project_diagnostic(
                                    &mut diagnostics,
                                    RototoRuleId::LayerShape,
                                    layer.target(),
                                    buckets.location.clone(),
                                    format!(
                                        "arm buckets {}-{} fall outside the layer's \
                                             {count} buckets (0-{})",
                                        start,
                                        end,
                                        count - 1
                                    ),
                                );
                            }
                            claims.push((start, end, format!("{allocation_label}/{arm_label}")));
                        }
                        None => push_project_diagnostic(
                            &mut diagnostics,
                            RototoRuleId::LayerShape,
                            layer.target(),
                            buckets.location.clone(),
                            format!(
                                "arm buckets must be \"<start>-<end>\" or \"<bucket>\": {}",
                                buckets.value
                            ),
                        ),
                    },
                    field => push_project_diagnostic(
                        &mut diagnostics,
                        RototoRuleId::LayerShape,
                        layer.target(),
                        field.location(),
                        "arm must declare buckets",
                    ),
                }
            }
        }

        claims.sort();
        for pair in claims.windows(2) {
            let (_, left_end, left_label) = &pair[0];
            let (right_start, _, right_label) = &pair[1];
            if right_start <= left_end {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::LayerBucketOverlap,
                    layer.target(),
                    layer.location.clone(),
                    format!(
                        "arms claim overlapping buckets: {left_label} and {right_label} \
                         both claim bucket {right_start}"
                    ),
                );
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

/// Root checks for a layer's `unit`/`eligibility` expressions: no `entry`
/// (there is no catalog entry in play), no unknown roots, no `env.resolving`.
fn lint_layer_expression(
    diagnostics: &mut Vec<LintDiagnostic>,
    index: &crate::lint::index::SemanticIndex,
    layer: &LayerNode,
    label: &str,
    expression: &Spanned<crate::expression::Expression>,
) {
    let references = expression.value.references();
    for issue in &references.invalid_roots {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::LayerShape,
            layer.target(),
            expression.location.clone(),
            issue.describe(),
        );
    }
    for enum_id in &references.enums {
        if !index.enums.contains_key(enum_id) {
            push_project_diagnostic(
                diagnostics,
                RototoRuleId::LayerShape,
                layer.target(),
                expression.location.clone(),
                format!("expression references unknown enum: {enum_id}"),
            );
        }
    }
    if !references.entry_paths.is_empty() {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::LayerShape,
            layer.target(),
            expression.location.clone(),
            format!("layer {label} cannot read entry; there is no catalog entry in play"),
        );
    }
    if references.uses_resolving {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::LayerShape,
            layer.target(),
            expression.location.clone(),
            "env.resolving is only available in [[trace]] policies",
        );
    }
}
