use std::path::Path;

use crate::error::Result;
use crate::fixtures::FixtureAssertionReport;
use crate::sdk::Workspace;

pub async fn assert_fixtures(
    workspace: &Workspace,
    suite_path: impl AsRef<Path>,
) -> Result<FixtureAssertionReport> {
    crate::fixtures::assert_fixtures(workspace, suite_path).await
}
