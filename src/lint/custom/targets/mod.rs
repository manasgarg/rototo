mod data;
mod locations;

pub(super) use data::{registered_lint_package, registered_lint_targets};
pub(super) use locations::registered_lint_output_anchor;

use crate::lint::index::*;

fn find_value<'a>(variable: &'a VariableNode, key: &str) -> Option<&'a ValueNode> {
    variable
        .values
        .inline_values
        .values()
        .find(|value| value.key.as_str() == key)
}

fn find_rule(variable: &VariableNode, index: usize) -> Option<&VariableRuleNode> {
    match &variable.resolve {
        ResolveNode::Resolve {
            rules: RuleCollection::Rules(rules),
            ..
        } => rules.iter().find(|rule| rule.index == index),
        _ => None,
    }
}
