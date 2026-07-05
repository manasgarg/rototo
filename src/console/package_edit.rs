use super::github::{package_repo_path, stable_package_key};
use super::store::{ActiveBranchRecord, PackageRecord};
use super::time::now_compact_stamp;

pub fn expected_variable_file_path(package: &PackageRecord, variable_id: &str) -> String {
    package_repo_path(&package.path, &format!("variables/{variable_id}.toml"))
}

pub fn variable_default_target_path() -> String {
    "/resolve/default".to_owned()
}

pub fn console_branch_name(login: &str, package: &PackageRecord) -> String {
    let login: String = {
        let mut cleaned = String::new();
        let mut pending_dash = false;
        for c in login.to_lowercase().chars() {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                if pending_dash && !cleaned.is_empty() {
                    cleaned.push('-');
                }
                cleaned.push(c);
                pending_dash = false;
            } else {
                pending_dash = true;
            }
        }
        cleaned
    };
    let key = stable_package_key(&package.source_tree_label, &package.path);
    format!(
        "rototo-console/{login}/{key}/{stamp}",
        stamp = now_compact_stamp()
    )
}

pub fn branch_pr_title(package: &PackageRecord) -> String {
    let path = if package.path == "." {
        "root package"
    } else {
        &package.path
    };
    format!("Update rototo package {path}")
}

pub fn branch_pr_body(
    package: &PackageRecord,
    branch: &ActiveBranchRecord,
    changed_paths: &[String],
    error_count: usize,
    warning_count: usize,
) -> String {
    let lint_status = if error_count > 0 {
        format!("{error_count} error(s)")
    } else if warning_count > 0 {
        format!("{warning_count} warning(s)")
    } else {
        "clean".to_owned()
    };
    let changed_paths = if changed_paths.is_empty() {
        vec!["- No changed files detected.".to_owned()]
    } else {
        changed_paths
            .iter()
            .map(|path| format!("- `{path}`"))
            .collect()
    };

    let mut body = vec![
        "## Rototo Console".to_owned(),
        String::new(),
        format!("Package: `{}:{}`", package.source_tree_label, package.path),
        format!("Base ref: `{}`", branch.base_ref),
        format!("Branch: `{}`", branch.branch),
        format!("Lint status: {lint_status}"),
        String::new(),
        "## Changed files".to_owned(),
        String::new(),
    ];
    body.extend(changed_paths);
    body.join("\n")
}

/// File paths must stay inside the package: no absolute paths, no `..`
/// segments, and (for non-root packages) the package path prefix.
pub fn belongs_to_package(package_path: &str, file_path: &str) -> bool {
    if file_path.starts_with('/') || file_path.split('/').any(|segment| segment == "..") {
        return false;
    }
    package_path == "." || file_path.starts_with(&format!("{package_path}/"))
}

/// Package entity kind the branch editor knows how to create.
///
/// The enum is parsed from request JSON and then used to select file templates.
/// It is intentionally tied to rototo's first-class nouns so new generic
/// package/document concepts do not leak into console creation paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Variables,
    Qualifiers,
    Catalogs,
    CatalogEntries,
    Context,
    Linters,
}

/// File planned for creation in a branch.
///
/// Template generation returns these before any write happens. The branch route
/// checks for conflicts first, writes each file through the selected backend,
/// and then serializes the same planned paths back to the UI.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PlannedFile {
    pub path: String,
    pub content: String,
}

pub fn parse_entity_id(value: Option<&str>) -> Option<String> {
    let id = value?.trim();
    (!id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')))
    .then(|| id.to_owned())
}

pub fn parse_variable_type(value: Option<&str>) -> String {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return "string".to_owned();
    };
    if is_supported_variable_type(value) {
        value.to_owned()
    } else {
        "string".to_owned()
    }
}

fn is_supported_variable_type(value: &str) -> bool {
    if matches!(value, "bool" | "int" | "number" | "string" | "list") {
        return true;
    }
    if let Some(catalog) = value.strip_prefix("catalog=") {
        return parse_entity_id(Some(catalog)).is_some();
    }
    let Some(inner) = value
        .strip_prefix("list<")
        .and_then(|value| value.strip_suffix('>'))
        .map(str::trim)
    else {
        return false;
    };
    if matches!(inner, "bool" | "int" | "number" | "string") {
        return true;
    }
    inner
        .strip_prefix("catalog=")
        .is_some_and(|catalog| parse_entity_id(Some(catalog)).is_some())
}

pub fn entity_template_files(
    kind: EntityKind,
    id: &str,
    catalog_id: Option<&str>,
    package_path: &str,
    variable_type: &str,
) -> Vec<PlannedFile> {
    let path = |relative: &str| package_repo_path(package_path, relative);
    match kind {
        EntityKind::Variables => vec![PlannedFile {
            path: path(&format!("variables/{id}.toml")),
            content: variable_template(id, variable_type),
        }],
        EntityKind::Qualifiers => vec![PlannedFile {
            path: path(&format!("qualifiers/{id}.toml")),
            content: qualifier_template(id),
        }],
        EntityKind::Catalogs => vec![
            PlannedFile {
                path: path(&format!("catalogs/{id}.schema.json")),
                content: catalog_schema_template(),
            },
            PlannedFile {
                path: path(&format!("catalogs/{id}-entries/default.toml")),
                content: catalog_entry_template().to_owned(),
            },
        ],
        EntityKind::CatalogEntries => {
            let catalog_id = catalog_id.expect("catalog value creation requires catalogId");
            vec![PlannedFile {
                path: path(&format!("catalogs/{catalog_id}-entries/{id}.toml")),
                content: catalog_entry_template().to_owned(),
            }]
        }
        EntityKind::Context => vec![
            PlannedFile {
                path: path(&format!("evaluation-contexts/{id}.schema.json")),
                content: evaluation_context_schema_template(),
            },
            PlannedFile {
                path: path(&format!("evaluation-contexts/{id}-samples/default.json")),
                content: "{\n}\n".to_owned(),
            },
        ],
        EntityKind::Linters => vec![PlannedFile {
            path: path(&format!("lint/{id}.lua")),
            content: linter_template().to_owned(),
        }],
    }
}

fn variable_template(id: &str, variable_type: &str) -> String {
    let default_literal = match variable_type {
        "bool" => "false",
        "int" | "number" => "0",
        "list" => "[]",
        value if value.starts_with("list<") => "[]",
        value if value.starts_with("catalog=") => "\"\"",
        _ => "\"control\"",
    };
    format!(
        "schema_version = 1\n\n\
         description = {description}\n\
         type = {variable_type}\n\n\
         [resolve]\n\
         default = {default_literal}\n",
        description = toml_string(&format!(
            "Edit this description to explain what {id} controls"
        )),
        variable_type = toml_string(variable_type),
    )
}

fn qualifier_template(id: &str) -> String {
    format!(
        "schema_version = 1\n\n\
         description = {description}\n\n\
         when = \"context.user.tier == \\\"premium\\\"\"\n",
        description = toml_string(&format!(
            "Edit this description to explain when {id} should match"
        )),
    )
}

fn catalog_schema_template() -> String {
    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "description": "Edit this description to explain the catalog values",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "heading": { "type": "string" },
            "enabled": { "type": "boolean" },
        },
        "required": ["heading", "enabled"],
    });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&schema).expect("static schema serializes")
    )
}

fn catalog_entry_template() -> &'static str {
    "heading = \"Edit this heading\"\nenabled = false\n"
}

fn evaluation_context_schema_template() -> String {
    "{\n  \"$schema\": \"https://json-schema.org/draft/2020-12/schema\",\n  \"type\": \"object\",\n  \"additionalProperties\": true,\n  \"properties\": {}\n}\n"
        .to_owned()
}

fn linter_template() -> &'static str {
    "function register(lint)\n  -- Register custom lint handlers here.\nend\n"
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("strings serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(path: &str, source: &str) -> PackageRecord {
        PackageRecord {
            id: "w1".to_owned(),
            slug: "configs".to_owned(),
            source_tree_id: "r1".to_owned(),
            source_tree_label: "octo/configs".to_owned(),
            display_path: path.to_owned(),
            path: path.to_owned(),
            revision: "main".to_owned(),
            source: source.to_owned(),
            discovered_at: "2026-01-01T00:00:00.000Z".to_owned(),
        }
    }

    fn branch(name: &str) -> ActiveBranchRecord {
        ActiveBranchRecord {
            id: "b1".to_owned(),
            source_tree_id: "r1".to_owned(),
            principal_id: "42".to_owned(),
            branch: name.to_owned(),
            base_ref: "main".to_owned(),
            base_commit: None,
            pr_url: None,
            pr_number: None,
            pr_state: None,
            pr_merged_at: None,
            pr_synced_at: None,
            last_selected_package_path: Some(".".to_owned()),
            last_seen_commit: None,
            status: super::super::store::ActiveBranchStatus::Active,
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            last_opened_at: "2026-01-01T00:00:00.000Z".to_owned(),
            last_edited_at: None,
            archived_at: None,
        }
    }

    #[test]
    fn package_path_guard_blocks_escapes() {
        assert!(belongs_to_package(".", "variables/x.toml"));
        assert!(!belongs_to_package(".", "/etc/passwd"));
        assert!(!belongs_to_package(".", "variables/../../x"));
        assert!(belongs_to_package("payments", "payments/variables/x.toml"));
        assert!(!belongs_to_package("payments", "variables/x.toml"));
    }

    #[test]
    fn entity_templates_cover_catalog_bundle() {
        let files = entity_template_files(EntityKind::Catalogs, "banner", None, ".", "string");
        let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "catalogs/banner.schema.json",
                "catalogs/banner-entries/default.toml",
            ]
        );
        let nested = entity_template_files(
            EntityKind::CatalogEntries,
            "summer",
            Some("banner"),
            "payments",
            "string",
        );
        assert_eq!(
            nested[0].path,
            "payments/catalogs/banner-entries/summer.toml"
        );
    }

    #[test]
    fn variable_type_parser_keeps_list_and_catalog_types() {
        assert_eq!(parse_variable_type(Some("list<string>")), "list<string>");
        assert_eq!(
            parse_variable_type(Some("list<catalog=banner>")),
            "list<catalog=banner>"
        );
        assert_eq!(
            parse_variable_type(Some("catalog=banner")),
            "catalog=banner"
        );
        assert_eq!(parse_variable_type(Some("list<list<string>>")), "string");
        assert_eq!(parse_variable_type(Some("catalog=bad id")), "string");
    }

    #[test]
    fn branch_names_carry_login_key_and_stamp() {
        let name = console_branch_name("Octo Cat!", &package(".", "src"));
        let parts: Vec<&str> = name.split('/').collect();
        assert_eq!(parts[0], "rototo-console");
        assert_eq!(parts[1], "octo-cat");
        assert_eq!(parts[2].len(), 12);
        assert_eq!(parts[3].len(), 14);
    }

    #[test]
    fn pr_body_lists_changed_files_and_lint_status() {
        let body = branch_pr_body(
            &package(".", "src"),
            &branch("feature"),
            &["variables/banner.toml".to_owned()],
            0,
            2,
        );
        assert!(body.starts_with("## Rototo Console"));
        assert!(body.contains("Lint status: 2 warning(s)"));
        assert!(body.contains("- `variables/banner.toml`"));
    }
}
