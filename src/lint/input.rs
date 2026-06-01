use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct LintInput {
    pub(crate) root: PathBuf,
    pub(crate) overlays: BTreeMap<String, OverlayDocument>,
}

impl LintInput {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            overlays: BTreeMap::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct OverlayDocument {
    pub(crate) text: String,
    pub(crate) version: Option<i32>,
}
