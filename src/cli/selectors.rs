#![allow(clippy::wildcard_imports)]

use crate::*;

use crate::cli::lint::catalog_authorities;

#[derive(Clone, Debug, Default)]
pub(crate) struct TargetSelectors {
    pub(crate) variables: Selection<String>,
    pub(crate) catalogs: Selection<String>,
    pub(crate) lint_rules: Selection<String>,
    pub(crate) lint_authorities: Selection<String>,
    pub(crate) linters: Selection<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) enum Selection<T> {
    #[default]
    None,
    Some(BTreeSet<T>),
    All,
}

impl<T> Selection<T> {
    pub(crate) fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub(crate) fn is_some_or_all(&self) -> bool {
        !self.is_none()
    }

    pub(crate) fn explicit_values(&self) -> Box<dyn Iterator<Item = &T> + '_> {
        match self {
            Self::Some(values) => Box::new(values.iter()),
            Self::None | Self::All => Box::new(std::iter::empty()),
        }
    }
}

impl TargetSelectors {
    pub(crate) fn from_args(args: &SelectorArgs) -> Self {
        Self {
            variables: selection(args.all_variables, &args.variables),
            catalogs: selection(args.all_catalogs, &args.catalogs),
            lint_rules: selection(args.all_lint_rules, &args.lint_rules),
            lint_authorities: selection(args.all_lint_authorities, &args.lint_authorities),
            linters: selection(args.all_linters, &args.linters),
        }
    }

    pub(crate) fn from_resolve_args(args: &ResolveSelectorArgs) -> Self {
        Self {
            variables: selection(args.all_variables, &args.variables),
            catalogs: Selection::None,
            lint_rules: Selection::None,
            lint_authorities: Selection::None,
            linters: Selection::None,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.variables.is_none()
            && self.catalogs.is_none()
            && self.lint_rules.is_none()
            && self.lint_authorities.is_none()
            && self.linters.is_none()
    }

    pub(crate) fn has_resolvable_targets(&self) -> bool {
        self.variables.is_some_or_all()
    }

    pub(crate) fn is_global_catalog_query(&self) -> bool {
        self.variables.is_none()
            && self.catalogs.is_none()
            && self.linters.is_none()
            && (self.lint_rules.is_some_or_all() || self.lint_authorities.is_some_or_all())
    }
}

pub(crate) fn selection(all: bool, values: &[String]) -> Selection<String> {
    if all {
        Selection::All
    } else if values.is_empty() {
        Selection::None
    } else {
        Selection::Some(values.iter().cloned().collect())
    }
}

pub(crate) fn inspect_selection(selection: &Selection<String>) -> InspectSelection {
    match selection {
        Selection::None => InspectSelection::None,
        Selection::All => InspectSelection::All,
        Selection::Some(values) => InspectSelection::Some(values.iter().cloned().collect()),
    }
}

pub(crate) fn validate_global_catalog_selectors(
    selectors: &TargetSelectors,
    catalog: &DiagnosticCatalog,
) -> Result<()> {
    for rule in selectors.lint_rules.explicit_values() {
        diagnostic_for_rule(catalog, rule)?;
    }
    let authorities = catalog_authorities(catalog);
    for authority in selectors.lint_authorities.explicit_values() {
        if !authorities.contains(authority) {
            return Err(RototoError::new(format!(
                "lint authority not found: {authority}"
            )));
        }
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) enum SelectedIds {
    None,
    Some(Vec<String>),
    All,
}

pub(crate) fn selected_variable_ids(
    inspection: &PackageInspection,
    selection: &Selection<String>,
) -> SelectedIds {
    match selection {
        Selection::None => SelectedIds::None,
        Selection::All => SelectedIds::All,
        Selection::Some(ids) => SelectedIds::Some(ordered_selected_ids(
            ids,
            inspection
                .variables
                .iter()
                .map(|variable| variable.id.as_str()),
        )),
    }
}

pub(crate) fn ordered_selected_ids<'a>(
    ids: &BTreeSet<String>,
    package_order: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    let mut ordered = Vec::new();
    for id in package_order {
        if ids.contains(id) {
            ordered.push(id.to_owned());
        }
    }
    for id in ids {
        if !ordered.iter().any(|ordered_id| ordered_id == id) {
            ordered.push(id.clone());
        }
    }
    ordered
}

pub(crate) fn validate_package_selectors(
    selectors: &TargetSelectors,
    inspection: &PackageInspection,
    catalog: &DiagnosticCatalog,
) -> Result<()> {
    for id in selectors.variables.explicit_values() {
        if !inspection
            .variables
            .iter()
            .any(|variable| variable.id == *id)
        {
            return Err(RototoError::new(format!(
                "variable not found: variable://{id}"
            )));
        }
    }
    for id in selectors.catalogs.explicit_values() {
        if !inspection.catalogs.iter().any(|catalog| catalog.id == *id) {
            return Err(RototoError::new(format!(
                "catalog not found: catalog://{id}"
            )));
        }
    }
    for rule in selectors.lint_rules.explicit_values() {
        diagnostic_for_rule(catalog, rule)?;
    }
    let authorities = catalog_authorities(catalog);
    for authority in selectors.lint_authorities.explicit_values() {
        if !authorities.contains(authority) {
            return Err(RototoError::new(format!(
                "lint authority not found: {authority}"
            )));
        }
    }
    for id in selectors.linters.explicit_values() {
        if !inspection.linters.iter().any(|linter| linter.id == *id) {
            return Err(RototoError::new(format!("linter not found: {id}")));
        }
    }
    Ok(())
}
