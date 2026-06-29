use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::console::time::now_iso;
use crate::error::{Result, RototoError};

use super::Store;
use super::rows::{package_from_row, source_tree_from_row};
use super::types::{
    DiscoveredPackageInput, PackageRecord, RegisterSourceTreeInput, RequestContextNames,
    SourceTreeWithPackages,
};
use super::util::{db_err, new_id};

/// Stable identity for a discovered package within one source tree.
///
/// Discovery can produce rows in any order, so cleanup compares by package
/// path and git ref instead of row id. The key lives only during one discovery
/// transaction.
#[derive(Hash, PartialEq, Eq)]
struct PackageKey {
    path: String,
    revision: String,
}

/// Existing package row paired with its discovery identity.
///
/// The cleanup step uses the row id for updates/deletes and the key for set
/// membership against the newly discovered packages.
struct PackageRowKey {
    id: String,
    key: PackageKey,
}

impl PackageKey {
    fn from_discovered(package: &DiscoveredPackageInput) -> Self {
        Self {
            path: package.path.clone(),
            revision: package.revision.clone(),
        }
    }
}

impl Store {
    pub async fn upsert_source_tree_with_packages(
        &self,
        input: RegisterSourceTreeInput,
    ) -> Result<SourceTreeWithPackages> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            let tx = conn.unchecked_transaction().map_err(db_err)?;

            let source_tree_id = upsert_source_tree_row(&tx, &input, &now)?;
            let discovered_keys = upsert_discovered_packages(
                &tx,
                &source_tree_id,
                &input.display_name,
                &input.packages,
                &now,
            )?;
            cleanup_missing_packages(&tx, &source_tree_id, &discovered_keys)?;

            tx.commit().map_err(db_err)?;

            source_tree_with_packages_by_id(conn, &source_tree_id, &input.principal_id)?
                .ok_or_else(|| RototoError::new("source tree registration failed"))
        })
        .await
    }

    pub async fn list_source_trees_for_user(
        &self,
        principal_id: &str,
    ) -> Result<Vec<SourceTreeWithPackages>> {
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| list_source_trees_for_user_sync(conn, &principal_id))
            .await
    }

    pub async fn get_source_tree_for_user(
        &self,
        source_tree_id: &str,
        principal_id: &str,
    ) -> Result<Option<SourceTreeWithPackages>> {
        let source_tree_id = source_tree_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            source_tree_with_packages_by_id(conn, &source_tree_id, &principal_id)
        })
        .await
    }

    pub async fn delete_source_tree_for_user(
        &self,
        source_tree_id: &str,
        principal_id: &str,
    ) -> Result<bool> {
        let source_tree_id = source_tree_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            if source_tree_with_packages_by_id(conn, &source_tree_id, &principal_id)?.is_none() {
                return Ok(false);
            }
            // ON DELETE CASCADE clears packages and active branch
            // selections transitively.
            conn.execute(
                "DELETE FROM source_trees WHERE id = ?1",
                params![source_tree_id],
            )
            .map_err(db_err)?;
            Ok(true)
        })
        .await
    }

    pub async fn list_packages_for_user(&self, principal_id: &str) -> Result<Vec<PackageRecord>> {
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| list_packages_for_user_sync(conn, &principal_id))
            .await
    }

    /// Accepts the row id or the derived slug, so friendly URLs and older id
    /// URLs both resolve.
    pub async fn get_package_for_user(
        &self,
        package_handle: &str,
        principal_id: &str,
    ) -> Result<Option<PackageRecord>> {
        let package_handle = package_handle.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let by_id = package_by_id_for_user(conn, &package_handle, &principal_id)?;
            if by_id.is_some() {
                return Ok(by_id);
            }
            package_by_slug_for_user(conn, &package_handle, &principal_id)
        })
        .await
    }

    pub async fn request_context_names(
        &self,
        package_id: Option<&str>,
        branch_id: Option<&str>,
    ) -> Result<RequestContextNames> {
        let package_id = package_id.map(str::to_owned);
        let branch_id = branch_id.map(str::to_owned);
        self.with_conn(move |conn, _| {
            let mut names = RequestContextNames::default();
            if let Some(package_id) = package_id.as_deref()
                && let Some((repo, package)) = package_context_names(conn, package_id)?
            {
                names.repo = Some(repo);
                names.package = Some(package);
            }
            if let Some(branch_id) = branch_id.as_deref()
                && let Some((repo, package, branch)) = branch_context_names(conn, branch_id)?
            {
                names.repo.get_or_insert(repo);
                if names.package.is_none() {
                    names.package = package;
                }
                names.branch = Some(branch);
            }
            Ok(names)
        })
        .await
    }
}

fn upsert_source_tree_row(
    tx: &Transaction<'_>,
    input: &RegisterSourceTreeInput,
    now: &str,
) -> Result<String> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM source_trees WHERE principal_id = ?1 AND source = ?2",
            params![input.principal_id, input.source],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_err)?;

    if let Some(source_tree_id) = existing {
        tx.execute(
            "UPDATE source_trees
             SET kind = ?1,
                 display_name = ?2,
                 default_revision = ?3,
                 updated_at = ?4,
                 last_discovered_at = ?5
             WHERE id = ?6",
            params![
                input.kind.as_str(),
                input.display_name,
                input.default_revision,
                now,
                now,
                source_tree_id.as_str()
            ],
        )
        .map_err(db_err)?;
        return Ok(source_tree_id);
    }

    let source_tree_id = new_id();
    tx.execute(
        "INSERT INTO source_trees (
           id, principal_id, kind, source, display_name, default_revision,
           created_at, updated_at, last_discovered_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            source_tree_id,
            input.principal_id,
            input.kind.as_str(),
            input.source,
            input.display_name,
            input.default_revision,
            now,
            now,
            now
        ],
    )
    .map_err(db_err)?;
    Ok(source_tree_id)
}

fn upsert_discovered_packages(
    tx: &Transaction<'_>,
    source_tree_id: &str,
    source_tree_label: &str,
    packages: &[DiscoveredPackageInput],
    now: &str,
) -> Result<HashSet<PackageKey>> {
    let mut discovered_keys = HashSet::with_capacity(packages.len());

    for package in packages {
        discovered_keys.insert(PackageKey::from_discovered(package));
        upsert_package_row(tx, source_tree_id, source_tree_label, package, now)?;
    }

    Ok(discovered_keys)
}

fn upsert_package_row(
    tx: &Transaction<'_>,
    source_tree_id: &str,
    source_tree_label: &str,
    package: &DiscoveredPackageInput,
    now: &str,
) -> Result<()> {
    let updated = tx
        .execute(
            "UPDATE source_tree_packages
             SET source_tree_label = ?1, source = ?2, discovered_at = ?3, active = 1
             WHERE source_tree_id = ?4 AND path = ?5 AND revision = ?6",
            params![
                source_tree_label,
                package.source.as_str(),
                now,
                source_tree_id,
                package.path.as_str(),
                package.revision.as_str(),
            ],
        )
        .map_err(db_err)?;

    if updated != 0 {
        return Ok(());
    }

    let package_id = new_id();
    tx.execute(
        "INSERT INTO source_tree_packages (
           id, source_tree_id, path, revision, source_tree_label, source, discovered_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            package_id,
            source_tree_id,
            package.path.as_str(),
            package.revision.as_str(),
            source_tree_label,
            package.source.as_str(),
            now,
        ],
    )
    .map_err(db_err)?;
    Ok(())
}

fn cleanup_missing_packages(
    tx: &Transaction<'_>,
    source_tree_id: &str,
    discovered_keys: &HashSet<PackageKey>,
) -> Result<()> {
    for package in package_keys_for_source_tree(tx, source_tree_id)? {
        if discovered_keys.contains(&package.key) {
            continue;
        }
        delete_or_deactivate_package(tx, &package.id)?;
    }
    Ok(())
}

fn package_keys_for_source_tree(
    tx: &Transaction<'_>,
    source_tree_id: &str,
) -> Result<Vec<PackageRowKey>> {
    let mut statement = tx
        .prepare("SELECT id, path, revision FROM source_tree_packages WHERE source_tree_id = ?1")
        .map_err(db_err)?;
    statement
        .query_map(params![source_tree_id], |row| {
            Ok(PackageRowKey {
                id: row.get(0)?,
                key: PackageKey {
                    path: row.get(1)?,
                    revision: row.get(2)?,
                },
            })
        })
        .map_err(db_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_err)
}

fn delete_or_deactivate_package(tx: &Transaction<'_>, package_id: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM source_tree_packages
         WHERE id = ?1
           AND NOT EXISTS (
             SELECT 1
             FROM active_branch_packages abw
             INNER JOIN active_branches b ON b.id = abw.branch_id
             WHERE b.source_tree_id = source_tree_packages.source_tree_id
               AND abw.package_path = source_tree_packages.path
           )",
        params![package_id],
    )
    .map_err(db_err)?;
    tx.execute(
        "UPDATE source_tree_packages SET active = 0 WHERE id = ?1",
        params![package_id],
    )
    .map_err(db_err)?;
    Ok(())
}

fn list_source_trees_for_user_sync(
    conn: &Connection,
    principal_id: &str,
) -> Result<Vec<SourceTreeWithPackages>> {
    list_source_tree_ids_for_user(conn, principal_id)?
        .into_iter()
        .map(|id| {
            source_tree_with_packages_by_id(conn, &id, principal_id)?
                .ok_or_else(|| RototoError::new("source tree listing failed"))
        })
        .collect()
}

fn list_source_tree_ids_for_user(conn: &Connection, principal_id: &str) -> Result<Vec<String>> {
    let mut statement = conn
        .prepare(
            "SELECT id FROM source_trees WHERE principal_id = ?1
             ORDER BY updated_at DESC, display_name ASC, source ASC",
        )
        .map_err(db_err)?;
    statement
        .query_map(params![principal_id], |row| row.get(0))
        .map_err(db_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_err)
}

fn package_by_id_for_user(
    conn: &Connection,
    package_id: &str,
    principal_id: &str,
) -> Result<Option<PackageRecord>> {
    conn.query_row(
        "SELECT w.id, w.source_tree_id, w.path, w.revision, w.source_tree_label, w.source, w.discovered_at
         FROM source_tree_packages w
         INNER JOIN source_trees r ON r.id = w.source_tree_id
         WHERE w.id = ?1 AND r.principal_id = ?2",
        params![package_id, principal_id],
        package_from_row,
    )
    .optional()
    .map_err(db_err)
}

fn package_by_slug_for_user(
    conn: &Connection,
    slug: &str,
    principal_id: &str,
) -> Result<Option<PackageRecord>> {
    let active_match = find_package_by_slug(list_packages_for_user_sync(conn, principal_id)?, slug);
    if active_match.is_some() {
        return Ok(active_match);
    }

    Ok(find_package_by_slug(
        list_all_packages_for_user_sync(conn, principal_id)?,
        slug,
    ))
}

fn find_package_by_slug(packages: Vec<PackageRecord>, slug: &str) -> Option<PackageRecord> {
    packages.into_iter().find(|package| package.slug == slug)
}

pub(super) fn package_slug(name: &str, path: &str) -> String {
    let base = if path == "." {
        name.to_owned()
    } else {
        format!("{name}-{path}")
    };
    let mut slug = String::new();
    let mut pending_dash = false;
    for c in base.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(c);
            pending_dash = false;
        } else {
            pending_dash = true;
        }
    }
    slug
}

pub(super) fn list_packages_for_user_sync(
    conn: &Connection,
    principal_id: &str,
) -> Result<Vec<PackageRecord>> {
    let mut statement = conn
        .prepare(
            "SELECT w.id, w.source_tree_id, w.path, w.revision, w.source_tree_label, w.source, w.discovered_at
             FROM source_tree_packages w
             INNER JOIN source_trees r ON r.id = w.source_tree_id
             WHERE r.principal_id = ?1
               AND w.active = 1
             ORDER BY w.source_tree_label ASC, w.path ASC",
        )
        .map_err(db_err)?;
    let packages = statement
        .query_map(params![principal_id], package_from_row)
        .map_err(db_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db_err)?;
    Ok(packages)
}

fn list_all_packages_for_user_sync(
    conn: &Connection,
    principal_id: &str,
) -> Result<Vec<PackageRecord>> {
    let mut statement = conn
        .prepare(
            "SELECT w.id, w.source_tree_id, w.path, w.revision, w.source_tree_label, w.source, w.discovered_at
             FROM source_tree_packages w
             INNER JOIN source_trees r ON r.id = w.source_tree_id
             WHERE r.principal_id = ?1
             ORDER BY w.source_tree_label ASC, w.path ASC",
        )
        .map_err(db_err)?;
    let packages = statement
        .query_map(params![principal_id], package_from_row)
        .map_err(db_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db_err)?;
    Ok(packages)
}

fn source_tree_with_packages_by_id(
    conn: &Connection,
    source_tree_id: &str,
    principal_id: &str,
) -> Result<Option<SourceTreeWithPackages>> {
    let source_tree = conn
        .query_row(
            "SELECT id, principal_id, kind, source, display_name, default_revision,
                    created_at, updated_at, last_discovered_at
             FROM source_trees WHERE id = ?1 AND principal_id = ?2",
            params![source_tree_id, principal_id],
            source_tree_from_row,
        )
        .optional()
        .map_err(db_err)?;
    let Some(source_tree) = source_tree else {
        return Ok(None);
    };
    let packages = active_packages_for_source_tree(conn, &source_tree.id)?;
    Ok(Some(SourceTreeWithPackages {
        source_tree,
        packages,
    }))
}

fn package_context_names(conn: &Connection, package_id: &str) -> Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT r.display_name, w.path
         FROM source_tree_packages w
         INNER JOIN source_trees r ON r.id = w.source_tree_id
         WHERE w.id = ?1",
        params![package_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(db_err)
}

fn branch_context_names(
    conn: &Connection,
    branch_id: &str,
) -> Result<Option<(String, Option<String>, String)>> {
    conn.query_row(
        "SELECT r.display_name, b.last_selected_package_path, b.branch
         FROM active_branches b
         INNER JOIN source_trees r ON r.id = b.source_tree_id
         WHERE b.id = ?1",
        params![branch_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
    .map_err(db_err)
}

fn active_packages_for_source_tree(
    conn: &Connection,
    source_tree_id: &str,
) -> Result<Vec<PackageRecord>> {
    let mut statement = conn
        .prepare(
            "SELECT id, source_tree_id, path, revision, source_tree_label, source, discovered_at
             FROM source_tree_packages
             WHERE source_tree_id = ?1 AND active = 1
             ORDER BY path ASC",
        )
        .map_err(db_err)?;
    statement
        .query_map(params![source_tree_id], package_from_row)
        .map_err(db_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db_err)
}
