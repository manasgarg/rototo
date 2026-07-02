use std::collections::{BTreeMap, BTreeSet};

use super::index::*;
use super::source::SourceStore;
use super::syntax::SyntaxIndex;
use crate::diagnostics::{
    DiagnosticLocation, SemanticEntity, SemanticField, SemanticTarget, SourcePosition, SourceRange,
};

#[derive(Default)]
pub(super) struct ReferenceIndex {
    declarations: BTreeMap<ReferenceTarget, DiagnosticLocation>,
    edges: Vec<ReferenceEdge>,
}

#[derive(Clone)]
pub(super) struct ReferenceEdge {
    pub(super) source: ReferenceSource,
    pub(super) semantic_target: SemanticTarget,
    pub(super) location: DiagnosticLocation,
    pub(super) target: ReferenceTarget,
    declaration: Option<DiagnosticLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ReferenceSource {
    QualifierWhenQualifier { qualifier: String },
    QualifierWhenContextAttribute { qualifier: String },
    VariableCatalog { variable: String },
    VariableResolveDefault { variable: String },
    VariableRuleConditionQualifier { variable: String, rule: usize },
    VariableRuleConditionVariable { variable: String, rule: usize },
    VariableRuleValue { variable: String, rule: usize },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ReferenceTarget {
    ContextAttribute(String),
    Qualifier(String),
    Variable(String),
    Catalog(String),
    CatalogEntry { catalog: String, value: String },
    VariableValue { variable: String, value: String },
}

#[derive(Clone)]
pub(super) struct QualifierReferenceEdge {
    pub(super) from: String,
    pub(super) to: String,
    pub(super) location: DiagnosticLocation,
}

impl ReferenceIndex {
    pub(super) fn build(
        index: &SemanticIndex,
        _source: &SourceStore,
        _syntax: &SyntaxIndex,
    ) -> Self {
        let mut references = Self::default();
        references.add_declarations(index);
        references.add_qualifier_references(index);
        references.add_variable_references(index);
        references
    }

    pub(super) fn edges(&self) -> &[ReferenceEdge] {
        &self.edges
    }

    pub(super) fn declaration(&self, target: &ReferenceTarget) -> Option<DiagnosticLocation> {
        self.declarations.get(target).cloned()
    }

    pub(super) fn reference_locations(&self, target: &ReferenceTarget) -> Vec<DiagnosticLocation> {
        self.edges
            .iter()
            .filter(|edge| &edge.target == target)
            .map(|edge| edge.location.clone())
            .collect()
    }

    pub(super) fn target_at_position(
        &self,
        path: &str,
        position: SourcePosition,
    ) -> Option<ReferenceTarget> {
        let mut candidates = Vec::new();

        for edge in &self.edges {
            if location_contains_position(&edge.location, path, position) {
                candidates.push(ReferenceTargetCandidate {
                    priority: 0,
                    span_size: edge
                        .location
                        .range
                        .map(source_range_size)
                        .unwrap_or(usize::MAX),
                    target: edge.target.clone(),
                });
            }
        }

        for (target, location) in &self.declarations {
            match target {
                ReferenceTarget::Qualifier(_) | ReferenceTarget::Variable(_)
                    if location.path == path =>
                {
                    candidates.push(ReferenceTargetCandidate {
                        priority: 5,
                        span_size: usize::MAX,
                        target: target.clone(),
                    });
                }
                ReferenceTarget::VariableValue { .. }
                    if location_contains_position(location, path, position) =>
                {
                    candidates.push(ReferenceTargetCandidate {
                        priority: 1,
                        span_size: location.range.map(source_range_size).unwrap_or(usize::MAX),
                        target: target.clone(),
                    });
                }
                ReferenceTarget::CatalogEntry { .. }
                    if location_contains_position(location, path, position) =>
                {
                    candidates.push(ReferenceTargetCandidate {
                        priority: 1,
                        span_size: location.range.map(source_range_size).unwrap_or(usize::MAX),
                        target: target.clone(),
                    });
                }
                ReferenceTarget::ContextAttribute(_) => {}
                ReferenceTarget::Qualifier(_)
                | ReferenceTarget::Variable(_)
                | ReferenceTarget::Catalog(_)
                | ReferenceTarget::CatalogEntry { .. }
                | ReferenceTarget::VariableValue { .. } => {}
            }
        }

        candidates.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.span_size.cmp(&right.span_size))
        });
        candidates
            .into_iter()
            .next()
            .map(|candidate| candidate.target)
    }

    pub(super) fn qualifier_reference_graph(
        &self,
    ) -> BTreeMap<String, Vec<QualifierReferenceEdge>> {
        let mut graph = self
            .declarations
            .keys()
            .filter_map(|target| match target {
                ReferenceTarget::Qualifier(qualifier) => Some((qualifier.clone(), Vec::new())),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();

        for edge in &self.edges {
            let ReferenceSource::QualifierWhenQualifier { qualifier } = &edge.source else {
                continue;
            };
            let ReferenceTarget::Qualifier(target) = &edge.target else {
                continue;
            };
            if !edge.is_resolved() {
                continue;
            }
            graph
                .entry(qualifier.clone())
                .or_default()
                .push(QualifierReferenceEdge {
                    from: qualifier.clone(),
                    to: target.clone(),
                    location: edge.location.clone(),
                });
        }

        graph
    }

    /// The variable-to-variable reference graph: an edge for every resolved
    /// `variables["<id>"]` reference in a variable's rule expressions, keyed by
    /// the referencing variable. Every declared variable is a node.
    pub(super) fn variable_reference_graph(&self) -> BTreeMap<String, Vec<QualifierReferenceEdge>> {
        let mut graph = self
            .declarations
            .keys()
            .filter_map(|target| match target {
                ReferenceTarget::Variable(variable) => Some((variable.clone(), Vec::new())),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();

        for edge in &self.edges {
            let ReferenceSource::VariableRuleConditionVariable { variable, .. } = &edge.source
            else {
                continue;
            };
            let ReferenceTarget::Variable(target) = &edge.target else {
                continue;
            };
            if !edge.is_resolved() {
                continue;
            }
            graph
                .entry(variable.clone())
                .or_default()
                .push(QualifierReferenceEdge {
                    from: variable.clone(),
                    to: target.clone(),
                    location: edge.location.clone(),
                });
        }

        graph
    }

    pub(super) fn referenced_qualifier_ids(&self) -> BTreeSet<String> {
        let mut referenced = BTreeSet::new();

        for edges in self.qualifier_reference_graph().values() {
            for edge in edges {
                if edge.from != edge.to {
                    referenced.insert(edge.to.clone());
                }
            }
        }

        for edge in &self.edges {
            if !matches!(
                edge.source,
                ReferenceSource::VariableRuleConditionQualifier { .. }
            ) || !edge.is_resolved()
            {
                continue;
            }
            let ReferenceTarget::Qualifier(qualifier) = &edge.target else {
                continue;
            };
            referenced.insert(qualifier.clone());
        }

        referenced
    }

    pub(super) fn resolution_reachable_qualifier_ids(&self) -> BTreeSet<String> {
        let graph = self.qualifier_reference_graph();
        let mut reachable = BTreeSet::new();
        let mut stack = Vec::new();

        for edge in &self.edges {
            if !matches!(
                edge.source,
                ReferenceSource::VariableRuleConditionQualifier { .. }
            ) || !edge.is_resolved()
            {
                continue;
            }
            let ReferenceTarget::Qualifier(qualifier) = &edge.target else {
                continue;
            };
            if reachable.insert(qualifier.clone()) {
                stack.push(qualifier.clone());
            }
        }

        while let Some(qualifier) = stack.pop() {
            for edge in graph.get(&qualifier).into_iter().flatten() {
                if reachable.insert(edge.to.clone()) {
                    stack.push(edge.to.clone());
                }
            }
        }

        reachable
    }

    fn add_declarations(&mut self, index: &SemanticIndex) {
        for qualifier in index.qualifiers.values() {
            self.declarations.insert(
                ReferenceTarget::Qualifier(qualifier.id.clone()),
                qualifier.location.clone(),
            );
        }

        for variable in index.variables.values() {
            self.declarations.insert(
                ReferenceTarget::Variable(variable.id.clone()),
                variable.location.clone(),
            );
            for value in variable.values.inline_values.values() {
                self.declarations.insert(
                    ReferenceTarget::VariableValue {
                        variable: variable.id.clone(),
                        value: value.key.clone(),
                    },
                    value.location.clone(),
                );
            }
        }

        for catalog in index.catalogs.values() {
            self.declarations.insert(
                ReferenceTarget::Catalog(catalog.id.clone()),
                catalog.location.clone(),
            );
        }

        for (catalog_id, entries) in &index.catalog_entries {
            for entry in entries.values() {
                self.declarations.insert(
                    ReferenceTarget::CatalogEntry {
                        catalog: catalog_id.clone(),
                        value: entry.key.clone(),
                    },
                    entry.location.clone(),
                );
            }
        }
    }

    fn add_qualifier_references(&mut self, index: &SemanticIndex) {
        for qualifier in index.qualifiers.values() {
            if let ProjectField::Present(when) = &qualifier.when {
                for target in &when.value.references().qualifiers {
                    self.push_edge(
                        ReferenceSource::QualifierWhenQualifier {
                            qualifier: qualifier.id.clone(),
                        },
                        qualifier.field_target(SemanticField::QualifierWhen),
                        when.location.clone(),
                        ReferenceTarget::Qualifier(target.clone()),
                    );
                }
                for path in &when.value.references().context_paths {
                    if path.is_empty() {
                        continue;
                    }
                    self.push_edge(
                        ReferenceSource::QualifierWhenContextAttribute {
                            qualifier: qualifier.id.clone(),
                        },
                        qualifier.field_target(SemanticField::QualifierWhen),
                        when.location.clone(),
                        ReferenceTarget::ContextAttribute(path.clone()),
                    );
                }
            }
        }
    }

    fn add_variable_references(&mut self, index: &SemanticIndex) {
        for variable in index.variables.values() {
            if let Some(type_kind) = variable_type_kind(&variable.type_source) {
                for catalog in type_kind.value.catalog_ids() {
                    self.push_edge(
                        ReferenceSource::VariableCatalog {
                            variable: variable.id.clone(),
                        },
                        SemanticTarget::field(
                            SemanticEntity::Variable {
                                id: variable.id.clone(),
                            },
                            SemanticField::VariableType,
                        ),
                        type_kind.location.clone(),
                        ReferenceTarget::Catalog(catalog.to_owned()),
                    );
                }
            } else if let TypeSourceNode::Catalog(catalog) = &variable.type_source {
                self.push_edge(
                    ReferenceSource::VariableCatalog {
                        variable: variable.id.clone(),
                    },
                    SemanticTarget::field(
                        SemanticEntity::Variable {
                            id: variable.id.clone(),
                        },
                        SemanticField::VariableType,
                    ),
                    catalog.location.clone(),
                    ReferenceTarget::Catalog(catalog.value.clone()),
                );
            }

            let ResolveNode::Resolve { default, rules, .. } = &variable.resolve else {
                continue;
            };

            if let ProjectField::Present(value) = default.as_ref()
                && let Some(target) = variable_value_target(variable, &value.value)
            {
                self.push_edge(
                    ReferenceSource::VariableResolveDefault {
                        variable: variable.id.clone(),
                    },
                    SemanticTarget::field(
                        SemanticEntity::Variable {
                            id: variable.id.clone(),
                        },
                        SemanticField::VariableResolveDefault,
                    ),
                    value.location.clone(),
                    target,
                );
            }

            let RuleCollection::Rules(rules) = rules else {
                continue;
            };
            for rule in rules {
                if rule.invalid_shape {
                    continue;
                }
                let entity = SemanticEntity::Rule {
                    variable: variable.id.clone(),
                    index: rule.index,
                };
                for (field, expression) in [
                    (SemanticField::VariableRuleWhen, &rule.when),
                    (SemanticField::VariableRuleQuery, &rule.query),
                ]
                .into_iter()
                .filter_map(|(field, expression)| expression.as_ref().map(|expr| (field, expr)))
                {
                    if let ProjectField::Present(expression) = expression {
                        for qualifier in &expression.value.references().qualifiers {
                            self.push_edge(
                                ReferenceSource::VariableRuleConditionQualifier {
                                    variable: variable.id.clone(),
                                    rule: rule.index,
                                },
                                SemanticTarget::field(entity.clone(), field.clone()),
                                expression.location.clone(),
                                ReferenceTarget::Qualifier(qualifier.clone()),
                            );
                        }
                        for referenced in &expression.value.references().variables {
                            self.push_edge(
                                ReferenceSource::VariableRuleConditionVariable {
                                    variable: variable.id.clone(),
                                    rule: rule.index,
                                },
                                SemanticTarget::field(entity.clone(), field.clone()),
                                expression.location.clone(),
                                ReferenceTarget::Variable(referenced.clone()),
                            );
                        }
                    }
                }
                if let ProjectField::Present(value) = &rule.value
                    && let Some(target) = variable_value_target(variable, &value.value)
                {
                    self.push_edge(
                        ReferenceSource::VariableRuleValue {
                            variable: variable.id.clone(),
                            rule: rule.index,
                        },
                        SemanticTarget::field(entity.clone(), SemanticField::VariableRuleValue),
                        value.location.clone(),
                        target,
                    );
                }
            }
        }
    }

    fn push_edge(
        &mut self,
        source: ReferenceSource,
        semantic_target: impl Into<SemanticTarget>,
        location: DiagnosticLocation,
        target: ReferenceTarget,
    ) {
        let semantic_target = semantic_target.into();
        let declaration = self.declarations.get(&target).cloned();
        self.edges.push(ReferenceEdge {
            source,
            semantic_target,
            location,
            target,
            declaration,
        });
    }
}

fn variable_catalog_id(variable: &VariableNode) -> Option<String> {
    variable_type_kind(&variable.type_source).and_then(|type_kind| match type_kind.value {
        VariableTypeKind::Catalog(catalog) => Some(catalog),
        _ => None,
    })
}

fn variable_value_target(
    variable: &VariableNode,
    value: &serde_json::Value,
) -> Option<ReferenceTarget> {
    let catalog = variable_catalog_id(variable)?;
    let value = value.as_str()?;
    Some(ReferenceTarget::CatalogEntry {
        catalog,
        value: value.to_owned(),
    })
}

impl ReferenceEdge {
    pub(super) fn is_resolved(&self) -> bool {
        self.declaration.is_some()
    }
}

struct ReferenceTargetCandidate {
    priority: u8,
    span_size: usize,
    target: ReferenceTarget,
}

fn location_contains_position(
    location: &DiagnosticLocation,
    path: &str,
    position: SourcePosition,
) -> bool {
    location.path == path
        && location
            .range
            .is_some_and(|range| source_range_contains_position(range, position))
}

fn source_range_contains_position(range: SourceRange, position: SourcePosition) -> bool {
    source_position_le(range.start, position) && source_position_lt(position, range.end)
}

fn source_position_le(left: SourcePosition, right: SourcePosition) -> bool {
    (left.line, left.character) <= (right.line, right.character)
}

fn source_position_lt(left: SourcePosition, right: SourcePosition) -> bool {
    (left.line, left.character) < (right.line, right.character)
}

fn source_range_size(range: SourceRange) -> usize {
    range
        .end
        .line
        .saturating_sub(range.start.line)
        .saturating_mul(10_000)
        .saturating_add(range.end.character.saturating_sub(range.start.character))
}
