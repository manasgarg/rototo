//! Where each entity lives in the package tree. The file layout is part of
//! the package format (`docs/src/package-format.md`); the engine and lint
//! discovery must agree on it.

use crate::address::is_valid_entity_id;
use crate::error::{Result, RototoError};

/// Validates a bare entity id the way the addressing grammar does.
pub(super) fn checked_id(kind: &str, id: &str) -> Result<()> {
    if is_valid_entity_id(id) {
        Ok(())
    } else {
        Err(RototoError::new(format!(
            "{kind} id `{id}` must be lowercase snake_case segments separated by `/`"
        )))
    }
}

pub(super) fn variable_path(id: &str) -> String {
    format!("variables/{id}.toml")
}

pub(super) fn enum_path(id: &str) -> String {
    format!("enums/{id}.toml")
}

pub(super) fn layer_path(id: &str) -> String {
    format!("layers/{id}.toml")
}

pub(super) fn catalog_schema_path(id: &str) -> String {
    format!("model/catalogs/{id}.schema.json")
}

pub(super) fn catalog_data_dir(id: &str) -> String {
    format!("data/catalogs/{id}/")
}

pub(super) fn entry_path(catalog: &str, key: &str) -> String {
    format!("data/catalogs/{catalog}/{key}.toml")
}

pub(super) fn context_schema_path(id: &str) -> String {
    format!("model/context/{id}.schema.json")
}

pub(super) fn samples_dir(context: &str) -> String {
    format!("model/context/{context}-samples/")
}

pub(super) fn sample_path(context: &str, key: &str) -> String {
    format!("model/context/{context}-samples/{key}.json")
}
