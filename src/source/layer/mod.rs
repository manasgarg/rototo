use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use tempfile::TempDir;

use crate::error::{Result, RototoError};
use crate::package::package_extends_sources;

use super::PACKAGE_MANIFEST;
use super::governance::{Operation, read_governance_contract};
use super::load::{load_single_package_source, load_single_package_source_snapshot};
use super::path::relative_path_is_safe;
use super::types::{
    ExtendSourceBase, LoadedPackageSource, LocalStageMode, ResolvedExtendSource, SourceFingerprint,
    SourceLayer, SourceOptions, StagedPackage,
};
use super::uri::SourceUri;

mod compose;
mod enforce;
mod graph;
mod provenance;

use compose::*;
use enforce::*;
use provenance::*;

pub(crate) use graph::load_package_source_graph;
pub(crate) use provenance::{RESOLVE_PROVENANCE_FILE, read_resolve_provenance};

#[cfg(test)]
mod tests {
    use super::graph::{read_package_extends, resolve_extend_source};
    use super::*;

    #[test]
    fn staged_extend_base_rejects_local_filesystem_escape_sources() {
        let staged = tempfile::TempDir::new().unwrap();
        let base = ExtendSourceBase {
            path: staged.path(),
            temporary: true,
        };

        for source in [
            "/tmp/outside",
            "../outside",
            "file:///tmp/outside",
            "git+file:///tmp/outside.git",
        ] {
            let err = resolve_extend_source(source, Some(base)).unwrap_err();
            assert!(err.to_string().contains("escapes a staged package"));
        }

        let resolved = resolve_extend_source("parent", Some(base)).unwrap();
        assert_eq!(
            resolved.source,
            staged.path().join("parent").display().to_string()
        );
        assert!(resolved.inherited_temporary_base);
    }

    #[tokio::test]
    async fn read_package_extends_rejects_blank_sources() {
        let temp = tempfile::TempDir::new().unwrap();
        tokio::fs::write(
            temp.path().join(PACKAGE_MANIFEST),
            r#"schema_version = 1
extends = ["../base", "  "]
"#,
        )
        .await
        .unwrap();

        let err = read_package_extends(temp.path()).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("package extends source must not be blank")
        );
    }

    #[tokio::test]
    async fn parent_layer_copy_skips_only_root_manifest() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        tokio::fs::create_dir_all(source.join("data/catalogs/config"))
            .await
            .unwrap();
        tokio::fs::write(source.join(PACKAGE_MANIFEST), "schema_version = 1\n")
            .await
            .unwrap();
        tokio::fs::write(
            source.join("data/catalogs/config").join(PACKAGE_MANIFEST),
            "value = true\n",
        )
        .await
        .unwrap();

        copy_package_layer(&source, &target, false, "test-layer".to_owned(), None)
            .await
            .unwrap();

        assert!(!target.join(PACKAGE_MANIFEST).exists());
        assert!(
            target
                .join("data/catalogs/config")
                .join(PACKAGE_MANIFEST)
                .is_file()
        );
    }
}
