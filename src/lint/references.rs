use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{DiagnosticLocation, EntityId, SourcePosition, SourceRange};
use crate::workspace::workspace_environments;

use super::index::*;
use super::source::{
    DocumentKind, SourceStore, resolve_workspace_relative_path, resolve_workspace_root_path,
};
use super::syntax::SyntaxIndex;

#[derive(Default)]
pub(super) struct ReferenceIndex {
    declarations: BTreeMap<ReferenceTarget, DiagnosticLocation>,
    edges: Vec<ReferenceEdge>,
    qualifier_referenced_by: BTreeMap<QualifierId, Vec<ReferenceSite>>,
    value_referenced_by: BTreeMap<(VariableId, ValueKey), Vec<ReferenceSite>>,
}

#[derive(Clone)]
pub(super) struct ReferenceEdge {
    pub(super) source: ReferenceSource,
    pub(super) entity: EntityId,
    pub(super) location: DiagnosticLocation,
    pub(super) target: ReferenceTarget,
    declaration: Option<DiagnosticLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ReferenceSource {
    ManifestContextSchema,
    QualifierPredicateQualifier {
        qualifier: String,
        predicate: usize,
    },
    QualifierPredicateContextAttribute {
        qualifier: String,
        predicate: usize,
    },
    VariableSchema {
        variable: String,
    },
    VariableEnvironment {
        variable: String,
        environment: String,
    },
    VariableEnvironmentValue {
        variable: String,
        environment: String,
    },
    VariableRuleQualifier {
        variable: String,
        environment: String,
        rule: usize,
    },
    VariableRuleValue {
        variable: String,
        environment: String,
        rule: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ReferenceTarget {
    ContextAttribute(String),
    Environment(String),
    Qualifier(String),
    Schema(String),
    VariableValue { variable: String, value: String },
}

#[derive(Clone)]
pub(super) struct QualifierReferenceEdge {
    pub(super) from: String,
    pub(super) to: String,
    pub(super) location: DiagnosticLocation,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(super) struct ReferenceSite {
    pub(super) from: EntityId,
    pub(super) location: DiagnosticLocation,
}

impl ReferenceIndex {
    pub(super) fn build(index: &SemanticIndex, source: &SourceStore, syntax: &SyntaxIndex) -> Self {
        let mut references = Self::default();
        references.add_declarations(index, source);
        references.add_manifest_references(index);
        references.add_qualifier_references(index);
        references.add_variable_references(index, syntax);
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
                ReferenceTarget::Qualifier(_) if location.path == path => {
                    candidates.push(ReferenceTargetCandidate {
                        priority: 5,
                        span_size: usize::MAX,
                        target: target.clone(),
                    });
                }
                ReferenceTarget::Schema(_)
                    if location.path == path && self.has_references(target) =>
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
                ReferenceTarget::ContextAttribute(_) | ReferenceTarget::Environment(_) => {}
                ReferenceTarget::Qualifier(_)
                | ReferenceTarget::Schema(_)
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
            let ReferenceSource::QualifierPredicateQualifier { qualifier, .. } = &edge.source
            else {
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
            if !matches!(edge.source, ReferenceSource::VariableRuleQualifier { .. })
                || !edge.is_resolved()
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

    pub(super) fn referenced_variable_value_keys(&self, variable_id: &str) -> BTreeSet<String> {
        self.value_referenced_by
            .keys()
            .filter(|(variable, _value)| variable == variable_id)
            .map(|(_variable, value)| value.clone())
            .collect()
    }

    #[allow(dead_code)]
    pub(super) fn qualifier_reference_sites(&self, qualifier: &str) -> &[ReferenceSite] {
        self.qualifier_referenced_by
            .get(qualifier)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    #[allow(dead_code)]
    pub(super) fn variable_value_reference_sites(
        &self,
        variable: &str,
        value: &str,
    ) -> &[ReferenceSite] {
        self.value_referenced_by
            .get(&(variable.to_owned(), value.to_owned()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn add_declarations(&mut self, index: &SemanticIndex, source: &SourceStore) {
        for qualifier in index.qualifiers.values() {
            self.declarations.insert(
                ReferenceTarget::Qualifier(qualifier.id.clone()),
                qualifier.location.clone(),
            );
        }

        for variable in index.variables.values() {
            for value in variable.values.inline_values.values() {
                self.declarations.insert(
                    ReferenceTarget::VariableValue {
                        variable: variable.id.clone(),
                        value: value.key.clone(),
                    },
                    value.location.clone(),
                );
            }
            if let Some(external_values) = index.external_values.get(&variable.id) {
                for value in external_values.values() {
                    self.declarations
                        .entry(ReferenceTarget::VariableValue {
                            variable: variable.id.clone(),
                            value: value.key.clone(),
                        })
                        .or_insert_with(|| value.location.clone());
                }
            }
        }

        if let Some(manifest) = &index.manifest
            && let WorkspaceEnvironmentCollection::Environments { values, .. } =
                &manifest.environments
        {
            for environment in values {
                self.declarations.insert(
                    ReferenceTarget::Environment(environment.name.clone()),
                    environment.location.clone(),
                );
            }
        }

        for document in source.documents.values() {
            match &document.kind {
                DocumentKind::ExternalValue {
                    variable_id,
                    value_key,
                } => {
                    self.declarations
                        .entry(ReferenceTarget::VariableValue {
                            variable: variable_id.clone(),
                            value: value_key.clone(),
                        })
                        .or_insert_with(|| document.document_location());
                }
                DocumentKind::Schema => {
                    self.declarations.insert(
                        ReferenceTarget::Schema(document.path.clone()),
                        document.document_location(),
                    );
                }
                DocumentKind::Manifest
                | DocumentKind::Qualifier { .. }
                | DocumentKind::Variable { .. }
                | DocumentKind::CustomLint => {}
            }
        }
    }

    fn add_manifest_references(&mut self, index: &SemanticIndex) {
        let Some(manifest) = &index.manifest else {
            return;
        };
        let Some(context) = &manifest.context_schema else {
            return;
        };
        let ProjectField::Present(schema) = &context.schema else {
            return;
        };
        let Some(schema_path) = resolve_workspace_root_path(&schema.value) else {
            return;
        };

        self.push_edge(
            ReferenceSource::ManifestContextSchema,
            EntityId::Manifest,
            schema.location.clone(),
            ReferenceTarget::Schema(schema_path),
        );
    }

    fn add_qualifier_references(&mut self, index: &SemanticIndex) {
        for qualifier in index.qualifiers.values() {
            let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
                continue;
            };

            for predicate in predicates {
                let ProjectField::Present(attribute) = &predicate.attribute else {
                    continue;
                };
                if let Some(target) = qualifier_reference(&attribute.value) {
                    self.push_edge(
                        ReferenceSource::QualifierPredicateQualifier {
                            qualifier: qualifier.id.clone(),
                            predicate: predicate.index,
                        },
                        EntityId::Predicate {
                            qualifier: qualifier.id.clone(),
                            index: predicate.index,
                        },
                        attribute.location.clone(),
                        ReferenceTarget::Qualifier(target.to_owned()),
                    );
                } else {
                    self.push_edge(
                        ReferenceSource::QualifierPredicateContextAttribute {
                            qualifier: qualifier.id.clone(),
                            predicate: predicate.index,
                        },
                        EntityId::Predicate {
                            qualifier: qualifier.id.clone(),
                            index: predicate.index,
                        },
                        attribute.location.clone(),
                        ReferenceTarget::ContextAttribute(attribute.value.clone()),
                    );
                }
            }
        }
    }

    fn add_variable_references(&mut self, index: &SemanticIndex, syntax: &SyntaxIndex) {
        for variable in index.variables.values() {
            if let TypeSourceNode::Schema(schema) = &variable.type_source
                && let Some(schema_path) =
                    resolve_workspace_relative_path(&variable.location.path, &schema.value)
            {
                self.push_edge(
                    ReferenceSource::VariableSchema {
                        variable: variable.id.clone(),
                    },
                    EntityId::Variable {
                        id: variable.id.clone(),
                    },
                    schema.location.clone(),
                    ReferenceTarget::Schema(schema_path),
                );
            }

            let EnvironmentCollection::Environments(environments) = &variable.environments else {
                continue;
            };
            let declared_environments = declared_workspace_environments(index, syntax);
            for block in environments.values() {
                if block.environment != "_" && declared_environments.is_some() {
                    self.push_edge(
                        ReferenceSource::VariableEnvironment {
                            variable: variable.id.clone(),
                            environment: block.environment.clone(),
                        },
                        EntityId::EnvironmentBlock {
                            variable: variable.id.clone(),
                            environment: block.environment.clone(),
                        },
                        block.location.clone(),
                        ReferenceTarget::Environment(block.environment.clone()),
                    );
                }

                if let ProjectField::Present(value) = &block.value {
                    self.push_edge(
                        ReferenceSource::VariableEnvironmentValue {
                            variable: variable.id.clone(),
                            environment: block.environment.clone(),
                        },
                        EntityId::EnvironmentBlock {
                            variable: variable.id.clone(),
                            environment: block.environment.clone(),
                        },
                        value.location.clone(),
                        ReferenceTarget::VariableValue {
                            variable: variable.id.clone(),
                            value: value.value.clone(),
                        },
                    );
                }

                let RuleCollection::Rules(rules) = &block.rules else {
                    continue;
                };
                for rule in rules {
                    if rule.invalid_shape {
                        continue;
                    }
                    let entity = EntityId::Rule {
                        variable: variable.id.clone(),
                        environment: block.environment.clone(),
                        index: rule.index,
                    };
                    if let ProjectField::Present(qualifier) = &rule.qualifier {
                        self.push_edge(
                            ReferenceSource::VariableRuleQualifier {
                                variable: variable.id.clone(),
                                environment: block.environment.clone(),
                                rule: rule.index,
                            },
                            entity.clone(),
                            qualifier.location.clone(),
                            ReferenceTarget::Qualifier(qualifier.value.clone()),
                        );
                    }
                    if let ProjectField::Present(value) = &rule.value {
                        self.push_edge(
                            ReferenceSource::VariableRuleValue {
                                variable: variable.id.clone(),
                                environment: block.environment.clone(),
                                rule: rule.index,
                            },
                            entity.clone(),
                            value.location.clone(),
                            ReferenceTarget::VariableValue {
                                variable: variable.id.clone(),
                                value: value.value.clone(),
                            },
                        );
                    }
                }
            }
        }
    }

    fn push_edge(
        &mut self,
        source: ReferenceSource,
        entity: EntityId,
        location: DiagnosticLocation,
        target: ReferenceTarget,
    ) {
        let declaration = self.declarations.get(&target).cloned();
        if declaration.is_some() {
            let site = ReferenceSite {
                from: entity.clone(),
                location: location.clone(),
            };
            match &target {
                ReferenceTarget::Qualifier(qualifier) => {
                    self.qualifier_referenced_by
                        .entry(qualifier.clone())
                        .or_default()
                        .push(site);
                }
                ReferenceTarget::VariableValue { variable, value } => {
                    self.value_referenced_by
                        .entry((variable.clone(), value.clone()))
                        .or_default()
                        .push(site);
                }
                ReferenceTarget::ContextAttribute(_)
                | ReferenceTarget::Environment(_)
                | ReferenceTarget::Schema(_) => {}
            }
        }
        self.edges.push(ReferenceEdge {
            source,
            entity,
            location,
            target,
            declaration,
        });
    }

    fn has_references(&self, target: &ReferenceTarget) -> bool {
        self.edges.iter().any(|edge| &edge.target == target)
    }
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

pub(super) fn qualifier_reference(attribute: &str) -> Option<&str> {
    attribute.strip_prefix("qualifier.")
}

fn declared_workspace_environments(
    index: &SemanticIndex,
    syntax: &SyntaxIndex,
) -> Option<BTreeSet<String>> {
    let manifest = index.manifest.as_ref()?;
    let parsed = syntax.toml.get(&manifest.doc)?;
    workspace_environments(&parsed.to_plain_toml())
        .ok()
        .map(|environments| environments.into_iter().collect())
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
