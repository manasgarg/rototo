//! Upcoming changes: behavior the package has scheduled to change on its own.
//!
//! A rule or query expression that compares `env.now` against a literal
//! instant flips when that instant passes, with no commit and no deploy. This
//! surface lists every such boundary that is still in the future, so consoles
//! and reviews can show "the October increase" before October.

use serde::Serialize;

use crate::error::{Result, RototoError};
use crate::expression::TimeBoundary;
use crate::predicate::parse_rfc3339_timestamp;

use super::PackageLintSnapshot;
use super::index::{ProjectField, ResolveNode, RuleCollection};
use super::semantic_model::ModelLocation;

/// One scheduled behavior change: an `env.now` boundary that has not passed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingChange {
    pub variable: String,
    pub site: UpcomingChangeSite,
    /// The instant, exactly as the expression spells it.
    pub boundary: String,
    /// The function or operator testing `env.now`
    /// (`timeAtOrAfter`, `timeBetween`, `>=`, ...).
    pub comparison: String,
    /// The full expression the boundary appears in.
    pub expression: String,
    pub location: ModelLocation,
}

/// Where in the variable's resolution the boundary sits.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum UpcomingChangeSite {
    Rule { index: usize },
    QueryFilter,
    QuerySort,
}

pub(super) fn upcoming_changes(
    snapshot: &PackageLintSnapshot,
    now: &str,
) -> Result<Vec<UpcomingChange>> {
    let Some(now) = parse_rfc3339_timestamp(now) else {
        return Err(RototoError::new(format!(
            "now must be an RFC3339 timestamp, got: {now}"
        )));
    };

    let mut changes = Vec::new();
    for node in snapshot.index.variables.values() {
        let ResolveNode::Resolve { rules, query, .. } = &node.resolve else {
            continue;
        };
        if let RuleCollection::Rules(rules) = rules {
            for rule in rules {
                if let Some(ProjectField::Present(when)) = &rule.when {
                    push_boundaries(
                        &mut changes,
                        &node.id,
                        UpcomingChangeSite::Rule { index: rule.index },
                        when.value.time_boundaries(),
                        when.value.source(),
                        &model_location(&when.location),
                        now,
                    );
                }
            }
        }
        if let Some(query) = query {
            for (field, site) in [
                (&query.filter, UpcomingChangeSite::QueryFilter),
                (&query.sort, UpcomingChangeSite::QuerySort),
            ] {
                if let Some(ProjectField::Present(expression)) = field {
                    push_boundaries(
                        &mut changes,
                        &node.id,
                        site,
                        expression.value.time_boundaries(),
                        expression.value.source(),
                        &model_location(&expression.location),
                        now,
                    );
                }
            }
        }
    }

    // Soonest first: the next thing that will happen leads the list.
    changes.sort_by(|left, right| {
        let left_key = parse_rfc3339_timestamp(&left.boundary);
        let right_key = parse_rfc3339_timestamp(&right.boundary);
        left_key
            .cmp(&right_key)
            .then_with(|| left.variable.cmp(&right.variable))
    });
    Ok(changes)
}

fn push_boundaries(
    changes: &mut Vec<UpcomingChange>,
    variable: &str,
    site: UpcomingChangeSite,
    boundaries: Vec<TimeBoundary>,
    expression: &str,
    location: &ModelLocation,
    now: crate::predicate::Rfc3339Timestamp,
) {
    for boundary in boundaries {
        if boundary.timestamp <= now {
            continue;
        }
        changes.push(UpcomingChange {
            variable: variable.to_owned(),
            site: site.clone(),
            boundary: boundary.instant,
            comparison: boundary.comparison,
            expression: expression.to_owned(),
            location: location.clone(),
        });
    }
}

fn model_location(location: &crate::diagnostics::DiagnosticLocation) -> ModelLocation {
    ModelLocation {
        path: location.path.clone(),
        range: location.range,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{LintInput, lint_package_snapshot};
    use super::*;

    async fn write_package(root: &std::path::Path, when: &str) {
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(root.join("rototo-package.toml"), "schema_version = 1\n")
            .await
            .unwrap();
        tokio::fs::write(
            root.join("variables/holiday_banner.toml"),
            format!(
                "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = false\n\n[[resolve.rule]]\nwhen = '{when}'\nvalue = true\n"
            ),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn reports_future_boundaries_and_skips_passed_ones() {
        let tempdir = tempfile::tempdir().unwrap();
        write_package(
            tempdir.path(),
            r#"timeBetween(env.now, "2026-12-20T00:00:00Z", "2027-01-05T00:00:00Z")"#,
        )
        .await;
        let snapshot = lint_package_snapshot(LintInput::new(tempdir.path().to_path_buf()))
            .await
            .unwrap();

        // Before the window: both edges are upcoming.
        let changes = upcoming_changes(&snapshot, "2026-07-01T00:00:00Z").unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].variable, "holiday_banner");
        assert_eq!(changes[0].boundary, "2026-12-20T00:00:00Z");
        assert_eq!(changes[0].comparison, "timeBetween");
        assert!(matches!(
            changes[0].site,
            UpcomingChangeSite::Rule { index: 0 }
        ));
        assert_eq!(changes[1].boundary, "2027-01-05T00:00:00Z");

        // Inside the window: only the closing edge remains upcoming.
        let changes = upcoming_changes(&snapshot, "2026-12-25T00:00:00Z").unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].boundary, "2027-01-05T00:00:00Z");

        // After the window: nothing is scheduled anymore.
        let changes = upcoming_changes(&snapshot, "2027-02-01T00:00:00Z").unwrap();
        assert!(changes.is_empty());
    }

    #[tokio::test]
    async fn rejects_a_malformed_now() {
        let tempdir = tempfile::tempdir().unwrap();
        write_package(tempdir.path(), r#"env.now >= "2027-06-01T00:00:00Z""#).await;
        let snapshot = lint_package_snapshot(LintInput::new(tempdir.path().to_path_buf()))
            .await
            .unwrap();
        let err = upcoming_changes(&snapshot, "sometime").unwrap_err();
        assert!(err.to_string().contains("RFC3339"));
    }
}
