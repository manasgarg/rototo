use super::*;

use super::lint::{
    compact_json_option, diagnostic_location_label_for_location, semantic_target_label,
};

pub(crate) fn print_package_diff(diff: &PackageDiff, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(diff).map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!("{} {}", style::label("before"), style::bold(&diff.before));
    println!("{} {}", style::label("after"), style::bold(&diff.after));
    if diff.changes.is_empty() {
        println!(
            "{} {}",
            style::label("semantic changes"),
            style::dim("none")
        );
    } else {
        println!("{}", style::label("semantic changes"));
        for change in &diff.changes {
            print_semantic_change_header(change);
            if let Some(location) = &change.before_location {
                println!(
                    "    {} {}",
                    style::dim("before location:"),
                    style::info(&diagnostic_location_label_for_location(location))
                );
            }
            if let Some(location) = &change.after_location {
                println!(
                    "    {} {}",
                    style::dim("after location:"),
                    style::info(&diagnostic_location_label_for_location(location))
                );
            }
            if change.before.is_some() || change.after.is_some() {
                print_diff_value_line("before", &change.before, DiffSide::Before)?;
                print_diff_value_line("after", &change.after, DiffSide::After)?;
            }
            if let Some(detail) = &change.detail {
                println!(
                    "    {} {}",
                    style::dim("impact:"),
                    style::info(&compact_json(detail)?)
                );
            }
        }
    }
    if !diff.resolution_impacts.is_empty() {
        println!("{}", style::label("resolution impact"));
        for impact in &diff.resolution_impacts {
            println!(
                "  {} {}",
                style::dim("variable:"),
                style::sea(&impact.variable)
            );
            print_resolution_impact_line("before", &impact.before, DiffSide::Before)?;
            print_resolution_impact_line("after", &impact.after, DiffSide::After)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) enum DiffSide {
    Before,
    After,
}

pub(super) fn print_semantic_change_header(change: &rototo::model::SemanticChange) {
    let target = semantic_target_label(&change.target);
    let description = semantic_change_description(&change.kind);
    if !style::enabled() {
        println!("  {}  {}", change.kind, target);
        println!("    change: {description}");
        return;
    }

    let marker = semantic_change_marker(change);
    println!(
        "  {} {}  {}",
        style_diff_marker(marker),
        style_diff_kind(description, marker),
        style::sea(&target)
    );
    println!("    {} {}", style::dim("kind:"), style::dim(&change.kind));
}

pub(super) fn semantic_change_marker(change: &rototo::model::SemanticChange) -> DiffChangeMarker {
    if change.kind.ends_with("_added") || (change.before.is_none() && change.after.is_some()) {
        DiffChangeMarker::Added
    } else if change.kind.ends_with("_removed")
        || (change.before.is_some() && change.after.is_none())
    {
        DiffChangeMarker::Removed
    } else {
        DiffChangeMarker::Changed
    }
}

#[derive(Clone, Copy)]
pub(super) enum DiffChangeMarker {
    Added,
    Removed,
    Changed,
}

pub(super) fn style_diff_marker(marker: DiffChangeMarker) -> String {
    match marker {
        DiffChangeMarker::Added => style::ok("+"),
        DiffChangeMarker::Removed => style::err("−"),
        DiffChangeMarker::Changed => style::warn("~"),
    }
}

pub(super) fn style_diff_kind(kind: &str, marker: DiffChangeMarker) -> String {
    match marker {
        DiffChangeMarker::Added => style::ok(kind),
        DiffChangeMarker::Removed => style::err(kind),
        DiffChangeMarker::Changed => style::warn(kind),
    }
}

pub(super) fn semantic_change_description(kind: &str) -> &'static str {
    match kind {
        "variable_added" => "variable added",
        "variable_removed" => "variable removed",
        "variable_type_changed" => "variable type changed",
        "variable_value_added" => "variable value added",
        "variable_value_removed" => "variable value removed",
        "variable_value_changed" => "variable value changed",
        "variable_resolve_default_changed" => "variable resolve default changed",
        "variable_rule_added" => "variable resolve rule added",
        "variable_rule_removed" => "variable resolve rule removed",
        "variable_rule_when_changed" => "variable rule condition changed",
        "variable_rule_value_changed" => "variable rule value changed",
        "catalog_added" => "catalog added",
        "catalog_removed" => "catalog removed",
        "catalog_schema_changed" => "catalog schema changed",
        "variable_resolution_changed" => "variable resolution changed",
        "layer_added" => "layer added",
        "layer_removed" => "layer removed",
        "layer_diversion_changed" => "layer diversion changed",
        "allocation_added" => "allocation added",
        "allocation_removed" => "allocation removed",
        "allocation_status_changed" => "allocation status changed",
        "allocation_eligibility_changed" => "allocation eligibility changed",
        "allocation_arms_expanded" => "allocation arms expanded into unclaimed buckets",
        "allocation_arms_reassigned" => "allocation arms reassigned claimed buckets",
        "catalog_entry_added" => "catalog value added",
        "catalog_entry_removed" => "catalog value removed",
        "catalog_entry_changed" => "catalog value changed",
        _ => "semantic change",
    }
}

pub(super) fn print_diff_value_line(
    label: &str,
    value: &Option<serde_json::Value>,
    side: DiffSide,
) -> Result<()> {
    let value = compact_json_option(value)?;
    if !style::enabled() {
        println!("    {label}: {value}");
        return Ok(());
    }

    println!(
        "    {} {}",
        style_diff_side_label(label, side),
        style_diff_side_value(&value, side)
    );
    Ok(())
}

pub(super) fn print_resolution_impact_line(
    label: &str,
    resolution: &rototo::model::VariableResolution,
    side: DiffSide,
) -> Result<()> {
    let source = resolution_source_label(&resolution.source);
    let value = compact_json(&resolution.value)?;
    if !style::enabled() {
        println!("    {label}: {source} {value}");
        return Ok(());
    }

    println!(
        "    {} {} {}",
        style_diff_side_label(label, side),
        style::dim(&source),
        style_diff_side_value(&value, side)
    );
    Ok(())
}

pub(super) fn style_diff_side_label(label: &str, side: DiffSide) -> String {
    let label = format!("{label}:");
    match side {
        DiffSide::Before => style::err(&label),
        DiffSide::After => style::ok(&label),
    }
}

pub(super) fn style_diff_side_value(value: &str, side: DiffSide) -> String {
    if value == "<none>" {
        return style::dim(value);
    }
    match side {
        DiffSide::Before => style::err(value),
        DiffSide::After => style::ok(value),
    }
}

pub(super) fn resolution_source_label(source: &rototo::model::VariableResolutionSource) -> String {
    match source {
        rototo::model::VariableResolutionSource::Literal => "literal".to_owned(),
        rototo::model::VariableResolutionSource::Catalog { catalog, value } => {
            format!("{catalog}:{value}")
        }
        rototo::model::VariableResolutionSource::CatalogList { catalog, values } => {
            format!("{catalog}:[{}]", values.join(","))
        }
    }
}
