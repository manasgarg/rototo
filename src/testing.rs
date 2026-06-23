use std::path::Path;

use crate::error::Result;
use crate::fixtures::FixtureAssertionReport;
use crate::sdk::Package;

pub async fn assert_fixtures(
    package: &Package,
    suite_path: impl AsRef<Path>,
) -> Result<FixtureAssertionReport> {
    crate::fixtures::assert_fixtures(package, suite_path).await
}
