use super::*;

/// Argument-position completion: when the cursor sits inside a function call
/// whose argument has a closed or well-known set, offer that set ahead of the
/// generic operands. Today that is `bucket(value, salt, start, end)`: the
/// value argument is a diversion unit, so declared layer units are offered,
/// and the salt argument is a layer id by convention (sharing a layer's salt
/// makes the expression agree with that layer's bucket positions).
pub(super) fn call_argument_completion_items(
    index: &SemanticIndex,
    prefix: &str,
) -> Vec<PackageCompletionItem> {
    let Some((function, argument)) = enclosing_call_argument(prefix) else {
        return Vec::new();
    };
    match (function.as_str(), argument) {
        ("bucket", 0) => index
            .layers
            .values()
            .filter_map(|layer| match &layer.unit {
                ProjectField::Present(unit) => Some(unit.value.source().to_owned()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|unit| {
                PackageCompletionItem::new(unit, PackageCompletionItemKind::Value, "layer unit")
            })
            .collect(),
        ("bucket", 1) => index
            .layers
            .keys()
            .map(|id| {
                PackageCompletionItem::new(
                    format!("\"{id}\""),
                    PackageCompletionItemKind::Value,
                    "layer id",
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}
