//! The one addressing grammar for everything in a package
//! (`design/addressing.md`):
//!
//! ```text
//! address      = step *( ":" step ) [ "#" json-pointer ]
//! step         = class "=" [ id ]
//! ```
//!
//! `=` binds a class to an id, `:` separates containment steps, `/` inside
//! an id means only namespacing, and everything after `#` is an RFC 6901
//! pointer into the entity's logical projection. Ids can never contain
//! `=`, `:`, `.` or `#`, so parsing is purely lexical.
//!
//! This module is the source of truth for parsing, validation, relative
//! resolution, and canonical rendering. Consumers (custom lint targets,
//! `x-rototo-ref`, diagnostics rendering) port onto it one at a time; none
//! are ported yet.

// No consumer is ported yet; drop this with the first port (the custom
// lint target migration).
#![allow(dead_code)]

use std::fmt;

use crate::error::{Result, RototoError};

/// The entity classes an address step may name. Singular, matching the
/// class names used by `x-rototo-ref` and type declarations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum EntityClass {
    Package,
    Manifest,
    Governance,
    Variable,
    Catalog,
    Entry,
    Enum,
    EvaluationContext,
    Sample,
    Layer,
    Linter,
}

impl EntityClass {
    const ALL: &'static [Self] = &[
        Self::Package,
        Self::Manifest,
        Self::Governance,
        Self::Variable,
        Self::Catalog,
        Self::Entry,
        Self::Enum,
        Self::EvaluationContext,
        Self::Sample,
        Self::Layer,
        Self::Linter,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Package => "package",
            Self::Manifest => "manifest",
            Self::Governance => "governance",
            Self::Variable => "variable",
            Self::Catalog => "catalog",
            Self::Entry => "entry",
            Self::Enum => "enum",
            Self::EvaluationContext => "evaluation-context",
            Self::Sample => "sample",
            Self::Layer => "layer",
            Self::Linter => "linter",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|class| class.as_str() == name)
    }

    /// Singletons take an empty id: the step names the one entity.
    pub(crate) fn is_singleton(self) -> bool {
        matches!(self, Self::Package | Self::Manifest | Self::Governance)
    }

    /// The class this one nests under, or `None` for a root class. Nesting
    /// exists only where the child is its own document with its own id.
    pub(crate) fn parent(self) -> Option<Self> {
        match self {
            Self::Entry => Some(Self::Catalog),
            Self::Sample => Some(Self::EvaluationContext),
            _ => None,
        }
    }

    /// Whether an entity of this class has a document a `#` pointer can
    /// walk. The package is a collection, and Lua lint files are not JSON
    /// documents.
    pub(crate) fn has_document(self) -> bool {
        !matches!(self, Self::Package | Self::Linter)
    }
}

/// What one step's id slot holds.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum StepId {
    /// Empty id. For a singleton class this names the one entity; for any
    /// other class it is the collective (all members).
    Empty,
    /// A trailing-slash prefix: the namespace subtree under it.
    Subtree(String),
    /// A concrete entity id, possibly namespaced (`payments/max_tokens`).
    Entity(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Step {
    pub(crate) class: EntityClass,
    pub(crate) id: StepId,
}

/// How deep an address reaches. Consumers use this to enforce their
/// acceptance tables (a lint target takes any depth; an `x-rototo-ref`
/// demands `Collective`; and so on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AddressDepth {
    /// `package=`: the whole package.
    Package,
    /// A class with an empty id on a non-singleton class: all members.
    Collective,
    /// A namespace subtree (`variable=payments/`).
    Subtree,
    /// A concrete entity, nested or not, singleton documents included.
    Entity,
}

/// A parsed, validated address.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Address {
    steps: Vec<Step>,
    pointer: Option<String>,
}

impl Address {
    pub(crate) fn parse(source: &str) -> Result<Self> {
        let (entity_path, pointer) = match source.split_once('#') {
            Some((path, pointer)) => (path, Some(pointer)),
            None => (source, None),
        };
        if entity_path.is_empty() {
            return Err(invalid(
                source,
                "an address needs at least one class=id step before any # pointer",
            ));
        }
        let mut steps = Vec::new();
        for segment in entity_path.split(':') {
            let Some((class_name, id)) = segment.split_once('=') else {
                return Err(invalid(
                    source,
                    format!("step `{segment}` is missing the `=` between class and id"),
                ));
            };
            let Some(class) = EntityClass::parse(class_name) else {
                return Err(invalid(
                    source,
                    format!(
                        "unknown entity class `{class_name}`; expected one of {}",
                        class_names()
                    ),
                ));
            };
            let id = parse_step_id(source, class, id)?;
            steps.push(Step { class, id });
        }
        let pointer = match pointer {
            Some(pointer) => Some(validate_pointer(source, pointer)?),
            None => None,
        };
        let address = Self { steps, pointer };
        address.validate(source)?;
        Ok(address)
    }

    fn validate(&self, source: &str) -> Result<()> {
        for (index, step) in self.steps.iter().enumerate() {
            let expected_parent = step.class.parent();
            match (index, expected_parent) {
                (0, None) => {}
                (0, Some(parent)) => {
                    return Err(invalid(
                        source,
                        format!(
                            "`{}=` only nests under `{}=`; it cannot start an address",
                            step.class.as_str(),
                            parent.as_str()
                        ),
                    ));
                }
                (_, None) => {
                    return Err(invalid(
                        source,
                        format!("`{}=` is a root class; it cannot nest", step.class.as_str()),
                    ));
                }
                (_, Some(parent)) => {
                    let previous = &self.steps[index - 1];
                    if previous.class != parent {
                        return Err(invalid(
                            source,
                            format!(
                                "`{}=` nests under `{}=`, not `{}=`",
                                step.class.as_str(),
                                parent.as_str(),
                                previous.class.as_str()
                            ),
                        ));
                    }
                    if !matches!(previous.id, StepId::Entity(_)) {
                        return Err(invalid(
                            source,
                            format!(
                                "nesting needs a concrete parent: give `{}=` an id before `{}=`",
                                previous.class.as_str(),
                                step.class.as_str()
                            ),
                        ));
                    }
                }
            }
        }
        if let Some(_pointer) = &self.pointer {
            let last = self.last_step();
            if !self.is_entity() {
                return Err(invalid(
                    source,
                    "a # pointer needs a concrete entity, not a collective or subtree",
                ));
            }
            if !last.class.has_document() {
                return Err(invalid(
                    source,
                    format!(
                        "`{}=` entities have no document for a # pointer to walk",
                        last.class.as_str()
                    ),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub(crate) fn last_step(&self) -> &Step {
        self.steps.last().expect("an address has at least one step")
    }

    pub(crate) fn pointer(&self) -> Option<&str> {
        self.pointer.as_deref()
    }

    fn is_entity(&self) -> bool {
        let last = self.last_step();
        matches!(last.id, StepId::Entity(_))
            || (last.class.is_singleton() && last.class != EntityClass::Package)
    }

    /// How deep the entity path reaches, ignoring any pointer.
    pub(crate) fn depth(&self) -> AddressDepth {
        let last = self.last_step();
        match (&last.id, last.class) {
            (StepId::Empty, EntityClass::Package) => AddressDepth::Package,
            (StepId::Empty, class) if class.is_singleton() => AddressDepth::Entity,
            (StepId::Empty, _) => AddressDepth::Collective,
            (StepId::Subtree(_), _) => AddressDepth::Subtree,
            (StepId::Entity(_), _) => AddressDepth::Entity,
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, step) in self.steps.iter().enumerate() {
            if index > 0 {
                write!(formatter, ":")?;
            }
            write!(formatter, "{}=", step.class.as_str())?;
            match &step.id {
                StepId::Empty => {}
                StepId::Subtree(prefix) => write!(formatter, "{prefix}/")?,
                StepId::Entity(id) => write!(formatter, "{id}")?,
            }
        }
        if let Some(pointer) = &self.pointer {
            write!(formatter, "#{pointer}")?;
        }
        Ok(())
    }
}

/// A reference: an address string that may be relative, resolved against a
/// base the context supplies (RFC 3986 style).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reference {
    /// A class-marked path: package-absolute, the base is ignored.
    Absolute(Address),
    /// `#/a/b`: a pointer into the base entity.
    FragmentOnly(String),
    /// `welcome#/body`: a bare id (plus optional pointer) that fills the
    /// open id slot a base ends with.
    BareId { id: String, pointer: Option<String> },
}

impl Reference {
    pub(crate) fn parse(source: &str) -> Result<Self> {
        if let Some(pointer) = source.strip_prefix('#') {
            return Ok(Self::FragmentOnly(validate_pointer(source, pointer)?));
        }
        let (entity_part, pointer) = match source.split_once('#') {
            Some((entity, pointer)) => (entity, Some(pointer)),
            None => (source, None),
        };
        if entity_part.contains('=') || entity_part.contains(':') {
            return Ok(Self::Absolute(Address::parse(source)?));
        }
        validate_id(source, entity_part)?;
        let pointer = match pointer {
            Some(pointer) => Some(validate_pointer(source, pointer)?),
            None => None,
        };
        Ok(Self::BareId {
            id: entity_part.to_owned(),
            pointer,
        })
    }

    /// Resolves this reference against a base address.
    pub(crate) fn resolve(&self, base: &Address) -> Result<Address> {
        match self {
            Self::Absolute(address) => Ok(address.clone()),
            Self::FragmentOnly(pointer) => {
                if !base.is_entity() {
                    return Err(RototoError::new(format!(
                        "a fragment-only reference needs an entity base, got `{base}`"
                    )));
                }
                if !base.last_step().class.has_document() {
                    return Err(RototoError::new(format!(
                        "the base `{base}` has no document for a # pointer to walk"
                    )));
                }
                Ok(Address {
                    steps: base.steps.clone(),
                    pointer: Some(pointer.clone()),
                })
            }
            Self::BareId { id, pointer } => {
                let last = base.last_step();
                if last.class.is_singleton() || !matches!(last.id, StepId::Empty) {
                    return Err(RototoError::new(format!(
                        "a bare id resolves against a base ending in an open id slot \
                         (for example `catalog=x:entry=`), got `{base}`"
                    )));
                }
                let mut steps = base.steps.clone();
                let slot = steps.last_mut().expect("base has at least one step");
                slot.id = StepId::Entity(id.clone());
                let resolved = Address {
                    steps,
                    pointer: pointer.clone(),
                };
                resolved.validate(&resolved.to_string())?;
                Ok(resolved)
            }
        }
    }
}

fn parse_step_id(source: &str, class: EntityClass, id: &str) -> Result<StepId> {
    if id.is_empty() {
        return Ok(StepId::Empty);
    }
    if class.is_singleton() {
        return Err(invalid(
            source,
            format!("`{}=` is a singleton and takes no id", class.as_str()),
        ));
    }
    if let Some(prefix) = id.strip_suffix('/') {
        if prefix.is_empty() {
            return Err(invalid(
                source,
                format!(
                    "`{}=/` is not a subtree; the empty id is already the collective",
                    class.as_str()
                ),
            ));
        }
        validate_id(source, prefix)?;
        return Ok(StepId::Subtree(prefix.to_owned()));
    }
    validate_id(source, id)?;
    Ok(StepId::Entity(id.to_owned()))
}

fn validate_id(source: &str, id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(invalid(source, "an id cannot be empty"));
    }
    let valid_segments = id.split('/').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    });
    if !valid_segments {
        return Err(invalid(
            source,
            format!("id `{id}` must be lowercase snake_case segments separated by `/`"),
        ));
    }
    Ok(())
}

fn validate_pointer(source: &str, pointer: &str) -> Result<String> {
    if pointer.is_empty() {
        return Ok(String::new());
    }
    if !pointer.starts_with('/') {
        return Err(invalid(
            source,
            format!("JSON pointer `{pointer}` must be empty or start with `/`"),
        ));
    }
    let mut bytes = pointer.bytes().peekable();
    while let Some(byte) = bytes.next() {
        if byte == b'~' && !matches!(bytes.peek(), Some(b'0') | Some(b'1')) {
            return Err(invalid(
                source,
                format!("JSON pointer `{pointer}` has a `~` not followed by 0 or 1"),
            ));
        }
    }
    Ok(pointer.to_owned())
}

fn class_names() -> String {
    EntityClass::ALL
        .iter()
        .map(|class| class.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn invalid(source: &str, message: impl fmt::Display) -> RototoError {
    RototoError::new(format!("invalid address `{source}`: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Address {
        Address::parse(source).unwrap_or_else(|err| panic!("{source} should parse: {err}"))
    }

    #[test]
    fn every_design_doc_example_round_trips() {
        // The example table from design/addressing.md, byte for byte.
        for source in [
            "package=",
            "manifest=",
            "manifest=#/trace/0/when",
            "governance=",
            "governance=#/variable/payments~1max_tokens/allowed_operations",
            "variable=",
            "variable=payments/",
            "variable=checkout_redesign",
            "variable=payments/max_tokens",
            "variable=payments/max_tokens#/type",
            "variable=payments/max_tokens#/resolve/default",
            "variable=payments/max_tokens#/resolve/rule/0",
            "variable=payments/max_tokens#/resolve/rule/0/when",
            "catalog=",
            "catalog=support_banner",
            "catalog=support_banner#/properties/message",
            "catalog=acme/banner",
            "catalog=acme/banner:entry=",
            "catalog=acme/banner:entry=default",
            "catalog=acme/banner:entry=promo/summer",
            "catalog=acme/banner:entry=default#/message",
            "enum=tier",
            "enum=tier#/type",
            "enum=tier#/members",
            "enum=tier#/members/1",
            "evaluation-context=request",
            "evaluation-context=request#/properties/user/properties/tier",
            "evaluation-context=request:sample=",
            "evaluation-context=request:sample=premium",
            "evaluation-context=request:sample=premium#/user/tier",
            "layer=rollout",
            "layer=rollout#/allocation/0/arm/1/buckets",
            "linter=budget",
        ] {
            assert_eq!(parse(source).to_string(), source, "{source}");
        }
    }

    #[test]
    fn depths_follow_the_acceptance_model() {
        assert_eq!(parse("package=").depth(), AddressDepth::Package);
        assert_eq!(parse("manifest=").depth(), AddressDepth::Entity);
        assert_eq!(parse("governance=").depth(), AddressDepth::Entity);
        assert_eq!(parse("variable=").depth(), AddressDepth::Collective);
        assert_eq!(parse("variable=payments/").depth(), AddressDepth::Subtree);
        assert_eq!(
            parse("variable=payments/max_tokens").depth(),
            AddressDepth::Entity
        );
        assert_eq!(
            parse("catalog=acme/banner:entry=").depth(),
            AddressDepth::Collective
        );
        assert_eq!(
            parse("catalog=acme/banner:entry=default").depth(),
            AddressDepth::Entity
        );
    }

    #[test]
    fn the_worked_parse_from_the_design_doc() {
        let address = parse("catalog=acme/banner:entry=promo/summer#/message");
        assert_eq!(address.steps().len(), 2);
        assert_eq!(address.steps()[0].class, EntityClass::Catalog);
        assert_eq!(
            address.steps()[0].id,
            StepId::Entity("acme/banner".to_owned())
        );
        assert_eq!(address.steps()[1].class, EntityClass::Entry);
        assert_eq!(
            address.steps()[1].id,
            StepId::Entity("promo/summer".to_owned())
        );
        assert_eq!(address.pointer(), Some("/message"));

        // Namespaced ids own everything between markers: this is the
        // variable named payments/rules, not a rules collection.
        let address = parse("variable=payments/rules");
        assert_eq!(address.depth(), AddressDepth::Entity);
        assert_eq!(
            address.last_step().id,
            StepId::Entity("payments/rules".to_owned())
        );
    }

    #[test]
    fn malformed_addresses_are_rejected_with_the_reason() {
        for (source, expected) in [
            ("", "at least one class=id step"),
            ("#/a", "at least one class=id step"),
            ("variables=x", "unknown entity class `variables`"),
            ("variable", "missing the `=`"),
            ("=x", "unknown entity class ``"),
            ("variable=/", "the empty id is already the collective"),
            ("package=x", "singleton and takes no id"),
            ("manifest=x", "singleton and takes no id"),
            ("entry=default", "cannot start an address"),
            ("variable=x:entry=y", "nests under `catalog=`"),
            ("catalog=:entry=x", "nesting needs a concrete parent"),
            ("catalog=a/:entry=x", "nesting needs a concrete parent"),
            ("variable=x:variable=y", "root class; it cannot nest"),
            ("variable=Payments", "lowercase snake_case"),
            ("variable=a//b", "lowercase snake_case"),
            ("variable=/a", "lowercase snake_case"),
            ("variable=a-b", "lowercase snake_case"),
            ("variable=#/type", "concrete entity, not a collective"),
            (
                "variable=payments/#/type",
                "concrete entity, not a collective",
            ),
            ("package=#/x", "concrete entity, not a collective"),
            ("linter=budget#/x", "no document"),
            ("variable=x#type", "must be empty or start with `/`"),
            ("variable=x#/a~2b", "`~` not followed by 0 or 1"),
        ] {
            let err = Address::parse(source)
                .expect_err(&format!("{source} should be rejected"))
                .to_string();
            assert!(err.contains(expected), "{source}: {err}");
        }
    }

    #[test]
    fn fragment_only_references_resolve_against_an_entity_base() {
        let base = parse("variable=payments/max_tokens");
        let reference = Reference::parse("#/resolve/default").unwrap();
        let resolved = reference.resolve(&base).unwrap();
        assert_eq!(
            resolved.to_string(),
            "variable=payments/max_tokens#/resolve/default"
        );

        // A collective base has no document to point into.
        let collective = parse("variable=");
        assert!(reference.resolve(&collective).is_err());
        // A linter has no document either.
        let linter = parse("linter=budget");
        assert!(reference.resolve(&linter).is_err());
    }

    #[test]
    fn bare_id_references_fill_the_open_slot() {
        // Today's entry references, restated as the general rule: the
        // x-rototo-ref pin catalog=email_template gives values the base
        // catalog=email_template:entry= and the value fills the slot.
        let base = parse("catalog=email_template:entry=");
        let reference = Reference::parse("welcome#/body").unwrap();
        let resolved = reference.resolve(&base).unwrap();
        assert_eq!(
            resolved.to_string(),
            "catalog=email_template:entry=welcome#/body"
        );

        let plain = Reference::parse("welcome").unwrap();
        assert_eq!(
            plain.resolve(&base).unwrap().to_string(),
            "catalog=email_template:entry=welcome"
        );

        // Namespaced ids fill slots too.
        let namespaced = Reference::parse("promo/summer").unwrap();
        assert_eq!(
            namespaced.resolve(&base).unwrap().to_string(),
            "catalog=email_template:entry=promo/summer"
        );

        // No open slot, no bare id.
        let entity = parse("catalog=email_template:entry=welcome");
        assert!(reference.resolve(&entity).is_err());
        let singleton = parse("manifest=");
        assert!(reference.resolve(&singleton).is_err());
    }

    #[test]
    fn class_marked_references_are_package_absolute() {
        let base = parse("catalog=email_template:entry=");
        let reference = Reference::parse("variable=eu_users").unwrap();
        assert_eq!(
            reference.resolve(&base).unwrap().to_string(),
            "variable=eu_users"
        );
    }

    #[test]
    fn malformed_references_are_rejected() {
        assert!(Reference::parse("Welcome#/body").is_err());
        assert!(Reference::parse("welcome#body").is_err());
        assert!(Reference::parse("unknown=x").is_err());
    }
}
