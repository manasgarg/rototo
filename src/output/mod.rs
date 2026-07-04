use std::path::Path;

use serde::Serialize;
use toml::Value as TomlValue;

use crate::style;

use rototo::diagnostics::{
    DiagnosticCatalogEntry, DiagnosticEntity, DiagnosticLocation, LintDiagnostic, SemanticEntity,
    SemanticField, SemanticTarget, Severity,
};
use rototo::error::{Result, RototoError};
use rototo::model::{InspectRuntimeStatus, PackageDiff, PackageInspectReport};
use rototo::model::{PackageInspection, PackageLint};
use rototo::package::{catalog_for_id, read_catalog_json, read_variable_toml, variable_for_id};

mod diff;
mod inspect;
mod lint;

pub(crate) use diff::print_package_diff;
pub(crate) use inspect::{
    print_catalog_get, print_catalog_list, print_inspect_report, print_variable_get,
    print_variable_list,
};
pub(crate) use lint::{print_diagnostic_catalog_entry, print_package_lint};

pub(super) fn print_entity_separator(index: usize, count: usize) {
    if count > 1 && index > 0 {
        println!("{}", style::hairline());
    }
}

pub(super) fn compact_json(value: &serde_json::Value) -> Result<String> {
    serde_json::to_string(value).map_err(|err| RototoError::new(err.to_string()))
}

pub(super) fn plural_count(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}
