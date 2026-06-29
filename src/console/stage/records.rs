use std::sync::Arc;

use super::{PackagePath, RepoRelativePath};
use crate::lint::PackageSemanticModel;
use crate::sdk::Package;

#[derive(Clone, Debug)]
pub struct DiscoveredPackages {
    pub paths: Vec<PackagePath>,
}

#[derive(Clone, Debug)]
pub struct BranchChanges {
    pub changed_files: Vec<RepoRelativePath>,
}

#[derive(Clone, Debug)]
pub struct SemanticPackage {
    pub package: Arc<Package>,
    pub model: Arc<PackageSemanticModel>,
}
