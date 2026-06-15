use serde_json::Value as JsonValue;

use super::github::{stable_workspace_key, workspace_archive_source, workspace_repo_path};
use super::store::{DraftChangeRecord, DraftSessionRecord, WorkspaceRecord};
use super::time::now_compact_stamp;

/// The workspace source to stage for a draft: the draft branch of the remote
/// archive, or the local path unchanged for dev/test registrations.
pub fn draft_source(workspace: &WorkspaceRecord, draft: &DraftSessionRecord) -> String {
    if !workspace.source.contains("://") {
        return workspace.source.clone();
    }
    if let Some(source) = replace_git_source_ref(&workspace.source, &draft.branch, &workspace.path)
    {
        return source;
    }
    if workspace
        .source
        .starts_with("https://api.github.com/repos/")
    {
        return workspace_archive_source(
            &workspace.owner,
            &workspace.name,
            &draft.branch,
            &workspace.path,
        );
    }
    workspace_archive_source(
        &workspace.owner,
        &workspace.name,
        &draft.branch,
        &workspace.path,
    )
}

fn replace_git_source_ref(source: &str, git_ref: &str, workspace_path: &str) -> Option<String> {
    if !source.starts_with("git+") {
        return None;
    }
    let (base, fragment) = source
        .split_once('#')
        .map(|(base, fragment)| (base, Some(fragment)))
        .unwrap_or((source, None));
    let subdir = fragment
        .and_then(|fragment| fragment.split_once(':').map(|(_, subdir)| subdir))
        .filter(|subdir| !subdir.is_empty())
        .unwrap_or(workspace_path);
    if subdir == "." {
        Some(format!("{base}#{git_ref}"))
    } else {
        Some(format!("{base}#{git_ref}:{subdir}"))
    }
}

pub fn expected_variable_file_path(workspace: &WorkspaceRecord, variable_id: &str) -> String {
    workspace_repo_path(&workspace.path, &format!("variables/{variable_id}.toml"))
}

pub fn variable_value_target_path(value_key: &str) -> String {
    format!("/values/{}", json_pointer_escape_segment(value_key))
}

pub fn draft_branch_name(login: &str, workspace: &WorkspaceRecord) -> String {
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
    let key = stable_workspace_key(&workspace.owner, &workspace.name, &workspace.path);
    format!(
        "rototo-console/{login}/{key}/{stamp}",
        stamp = now_compact_stamp()
    )
}

pub fn draft_pr_title(workspace: &WorkspaceRecord) -> String {
    let path = if workspace.path == "." {
        "root workspace"
    } else {
        &workspace.path
    };
    format!("Update rototo workspace {path}")
}

pub fn draft_pr_body(
    workspace: &WorkspaceRecord,
    draft: &DraftSessionRecord,
    changes: &[DraftChangeRecord],
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
    let change_lines: Vec<String> = if changes.is_empty() {
        vec!["- No tracked semantic changes.".to_owned()]
    } else {
        changes
            .iter()
            .map(|change| {
                let target = match change.target_path.as_deref() {
                    Some(target_path) if !target_path.is_empty() => {
                        format!("`{}` `{}`", change.file_path, target_path)
                    }
                    _ => format!("`{}`", change.file_path),
                };
                format!(
                    "- {target}: `{}` -> `{}`",
                    json_summary(&change.before_json),
                    json_summary(&change.after_json),
                )
            })
            .collect()
    };

    let mut body = vec![
        "## Rototo Console".to_owned(),
        String::new(),
        format!(
            "Workspace: `{}/{}:{}`",
            workspace.owner, workspace.name, workspace.path
        ),
        format!("Base ref: `{}`", draft.base_ref),
        format!("Draft branch: `{}`", draft.branch),
        format!("Lint status: {lint_status}"),
        String::new(),
        "## Semantic changes".to_owned(),
        String::new(),
    ];
    body.extend(change_lines);
    body.join("\n")
}

fn json_summary(value: &str) -> String {
    serde_json::from_str::<JsonValue>(value)
        .map(|parsed| parsed.to_string())
        .unwrap_or_else(|_| value.to_owned())
}

fn json_pointer_escape_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// File paths must stay inside the workspace: no absolute paths, no `..`
/// segments, and (for non-root workspaces) the workspace path prefix.
pub fn belongs_to_workspace(workspace_path: &str, file_path: &str) -> bool {
    if file_path.starts_with('/') || file_path.split('/').any(|segment| segment == "..") {
        return false;
    }
    workspace_path == "." || file_path.starts_with(&format!("{workspace_path}/"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Variables,
    Qualifiers,
    Catalogs,
    CatalogEntries,
    Schemas,
    Context,
    Linters,
}

impl EntityKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Variables => "variable",
            Self::Qualifiers => "qualifier",
            Self::Catalogs => "catalog",
            Self::CatalogEntries => "catalog entry",
            Self::Schemas => "schema",
            Self::Context => "context example",
            Self::Linters => "linter",
        }
    }
}

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

pub fn parse_variable_type(value: Option<&str>) -> &'static str {
    match value {
        Some("bool") => "bool",
        Some("int") => "int",
        Some("number") => "number",
        Some("list") => "list",
        _ => "string",
    }
}

pub fn entity_template_files(
    kind: EntityKind,
    id: &str,
    catalog_id: Option<&str>,
    workspace_path: &str,
    variable_type: &str,
) -> Vec<PlannedFile> {
    let path = |relative: &str| workspace_repo_path(workspace_path, relative);
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
                path: path(&format!("catalogs/{id}.toml")),
                content: catalog_template(id),
            },
            PlannedFile {
                path: path(&format!("schemas/{id}.schema.json")),
                content: catalog_schema_template(),
            },
            PlannedFile {
                path: path(&format!("catalogs/{id}-entries/default.toml")),
                content: catalog_entry_template().to_owned(),
            },
        ],
        EntityKind::CatalogEntries => {
            let catalog_id = catalog_id.expect("catalog entry creation requires catalogId");
            vec![PlannedFile {
                path: path(&format!("catalogs/{catalog_id}-entries/{id}.toml")),
                content: catalog_entry_template().to_owned(),
            }]
        }
        EntityKind::Schemas => vec![PlannedFile {
            path: path(&format!("schemas/{}", json_file_name(id))),
            content: schema_template(id),
        }],
        EntityKind::Context => vec![PlannedFile {
            path: path(&format!("contexts/{}", json_file_name(id))),
            content: "{\n}\n".to_owned(),
        }],
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
        _ => "\"control\"",
    };
    format!(
        "schema_version = 1\n\n\
         description = {description}\n\
         type = {variable_type}\n\n\
         [values]\n\
         control = {default_literal}\n\n\
         [resolve]\n\
         default = \"control\"\n",
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
         [[predicate]]\n\
         attribute = \"user.tier\"\n\
         op = \"eq\"\n\
         value = \"premium\"\n",
        description = toml_string(&format!(
            "Edit this description to explain when {id} should match"
        )),
    )
}

fn catalog_template(id: &str) -> String {
    format!(
        "schema_version = 1\n\n\
         description = {description}\n\
         schema = {schema}\n",
        description = toml_string(&format!(
            "Edit this description to explain the {id} catalog entries"
        )),
        schema = toml_string(&format!("../schemas/{id}.schema.json")),
    )
}

fn catalog_schema_template() -> String {
    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
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

fn schema_template(id: &str) -> String {
    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": id,
        "type": "object",
        "additionalProperties": true,
    });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&schema).expect("static schema serializes")
    )
}

fn linter_template() -> &'static str {
    "function register(lint)\n  -- Register custom lint handlers here.\nend\n"
}

fn json_file_name(id: &str) -> String {
    if id.ends_with(".json") {
        id.to_owned()
    } else {
        format!("{id}.json")
    }
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("strings serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(path: &str, source: &str) -> WorkspaceRecord {
        WorkspaceRecord {
            id: "w1".to_owned(),
            slug: "configs".to_owned(),
            repo_id: "r1".to_owned(),
            owner: "octo".to_owned(),
            name: "configs".to_owned(),
            path: path.to_owned(),
            git_ref: "main".to_owned(),
            source: source.to_owned(),
            discovered_at: "2026-01-01T00:00:00.000Z".to_owned(),
        }
    }

    fn draft(branch: &str) -> DraftSessionRecord {
        DraftSessionRecord {
            id: "d1".to_owned(),
            workspace_id: "w1".to_owned(),
            principal_id: "42".to_owned(),
            branch: branch.to_owned(),
            base_ref: "main".to_owned(),
            status: super::super::store::DraftStatus::Open,
            pr_url: None,
            pr_number: None,
            pr_state: None,
            pr_merged_at: None,
            pr_synced_at: None,
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            updated_at: "2026-01-01T00:00:00.000Z".to_owned(),
            published_at: None,
        }
    }

    #[test]
    fn draft_source_swaps_ref_for_remote_sources_only() {
        let remote = workspace(
            ".",
            "https://api.github.com/repos/octo/configs/tarball/main",
        );
        assert_eq!(
            draft_source(&remote, &draft("feature")),
            "https://api.github.com/repos/octo/configs/tarball/feature"
        );
        let local = workspace(".", "/tmp/local-workspace");
        assert_eq!(
            draft_source(&local, &draft("feature")),
            "/tmp/local-workspace"
        );
    }

    #[test]
    fn draft_source_preserves_git_source_shape() {
        let remote = workspace(
            "payments",
            "git+https://github.com/octo/configs.git#main:payments",
        );
        assert_eq!(
            draft_source(&remote, &draft("feature")),
            "git+https://github.com/octo/configs.git#feature:payments"
        );
    }

    #[test]
    fn workspace_path_guard_blocks_escapes() {
        assert!(belongs_to_workspace(".", "variables/x.toml"));
        assert!(!belongs_to_workspace(".", "/etc/passwd"));
        assert!(!belongs_to_workspace(".", "variables/../../x"));
        assert!(belongs_to_workspace(
            "payments",
            "payments/variables/x.toml"
        ));
        assert!(!belongs_to_workspace("payments", "variables/x.toml"));
    }

    #[test]
    fn entity_templates_cover_catalog_bundle() {
        let files = entity_template_files(EntityKind::Catalogs, "banner", None, ".", "string");
        let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "catalogs/banner.toml",
                "schemas/banner.schema.json",
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
    fn branch_names_carry_login_key_and_stamp() {
        let branch = draft_branch_name("Octo Cat!", &workspace(".", "src"));
        let parts: Vec<&str> = branch.split('/').collect();
        assert_eq!(parts[0], "rototo-console");
        assert_eq!(parts[1], "octo-cat");
        assert_eq!(parts[2].len(), 12);
        assert_eq!(parts[3].len(), 14);
    }

    #[test]
    fn pr_body_lists_changes_and_lint_status() {
        let body = draft_pr_body(
            &workspace(".", "src"),
            &draft("feature"),
            &[DraftChangeRecord {
                id: "c1".to_owned(),
                draft_id: "d1".to_owned(),
                file_path: "variables/banner.toml".to_owned(),
                target_path: Some("/values/control".to_owned()),
                before_json: "false".to_owned(),
                after_json: "true".to_owned(),
                updated_at: "2026-01-01T00:00:00.000Z".to_owned(),
            }],
            0,
            2,
        );
        assert!(body.starts_with("## Rototo Console"));
        assert!(body.contains("Lint status: 2 warning(s)"));
        assert!(body.contains("- `variables/banner.toml` `/values/control`: `false` -> `true`"));
    }
}
