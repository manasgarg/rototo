use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Serialize;

use rototo::model::{InspectSelection, PackageInspectRequest};
use rototo::{Result, RototoError, inspect_package_report};

use crate::style;
use crate::{InitArgs, path_exists};

pub(crate) async fn run_init(args: InitArgs, json: bool, quiet: bool) -> Result<ExitCode> {
    let package = local_init_package_path(&args.package)?;
    let target = init_target(&args)?;
    let plan = build_init_plan(&package, target).await?;
    let report = execute_init_plan(&package, &plan, args.force, args.dry_run).await?;
    print_init_report(&report, json, quiet)?;
    Ok(ExitCode::SUCCESS)
}

enum InitTarget {
    Package,
    Variable(String),
    Catalog(String),
    Context { id: String, update: bool },
}

fn init_target(args: &InitArgs) -> Result<InitTarget> {
    let mut count = 0;
    let mut target = InitTarget::Package;

    if let Some(id) = &args.variable {
        count += 1;
        validate_template_id("variable", id)?;
        target = InitTarget::Variable(id.clone());
    }
    if let Some(id) = &args.catalog {
        count += 1;
        validate_template_id("catalog", id)?;
        target = InitTarget::Catalog(id.clone());
    }
    if let Some(id) = &args.evaluation_context {
        count += 1;
        validate_template_id("evaluation context", id)?;
        target = InitTarget::Context {
            id: id.clone(),
            update: args.update,
        };
    }

    if count > 1 {
        return Err(RototoError::new(
            "init accepts one entity flag at a time: --variable, --catalog, or --evaluation-context",
        ));
    }

    Ok(target)
}

fn validate_template_id(kind: &str, id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(RototoError::new(format!("{kind} id must not be empty")));
    }
    if id.starts_with('.') || id.split('.').any(str::is_empty) {
        return Err(RototoError::new(format!(
            "{kind} id must not start with '.', end with '.', or contain empty '.' segments"
        )));
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(RototoError::new(format!(
            "{kind} id must use only ASCII letters, digits, '.', '_', or '-'"
        )));
    }
    Ok(())
}

fn local_init_package_path(path: &Path) -> Result<PathBuf> {
    let source = path.to_string_lossy();
    if source.contains("://") || source.starts_with("git+") {
        return Err(RototoError::new(
            "init requires a local package path, not a package source URI",
        ));
    }

    std::path::absolute(path)
        .map_err(|err| RototoError::new(format!("failed to resolve package path: {err}")))
}

async fn build_init_plan(package: &Path, target: InitTarget) -> Result<InitPlan> {
    let initialized = package_initialized(package).await?;
    match target {
        InitTarget::Package => Ok(InitPlan::from_entries(package_init_plan(package))),
        InitTarget::Variable(id) => {
            let mut plan = implicit_package_init_plan(package, initialized);
            if initialized {
                plan.push(InitPlanEntry::directory(package.join("variables")));
            }
            plan.push(InitPlanEntry::file(
                "variable",
                package.join("variables").join(format!("{id}.toml")),
                variable_template(&id),
            ));
            Ok(InitPlan::from_entries(plan))
        }
        InitTarget::Catalog(id) => {
            let mut plan = implicit_package_init_plan(package, initialized);
            if initialized {
                plan.push(InitPlanEntry::directory(package.join("model/catalogs")));
            }
            plan.extend([
                InitPlanEntry::directory(package.join("data/catalogs").join(&id)),
                InitPlanEntry::file(
                    "catalog",
                    package
                        .join("model/catalogs")
                        .join(format!("{id}.schema.json")),
                    catalog_schema_template()?,
                ),
                InitPlanEntry::file(
                    "catalog_entry",
                    package.join("data/catalogs").join(&id).join("default.toml"),
                    catalog_entry_template(),
                ),
            ]);
            Ok(InitPlan::from_entries(plan))
        }
        InitTarget::Context { id, update } => {
            let mut plan = implicit_package_init_plan(package, initialized);
            if initialized {
                plan.push(InitPlanEntry::directory(package.join("model/context")));
            }
            let context_path = package
                .join("model/context")
                .join(format!("{id}.schema.json"));
            let (content, schema_update) =
                context_schema_template(package, &context_path, &id, initialized, update).await?;
            let entry = if update {
                InitPlanEntry::file_update("evaluation_context", context_path, content)
            } else {
                InitPlanEntry::file("evaluation_context", context_path, content)
            };
            plan.push(entry);
            let mut init_plan = InitPlan::from_entries(plan);
            if let Some(schema_update) = schema_update {
                init_plan.schema_updates.push(schema_update);
            }
            Ok(init_plan)
        }
    }
}

fn implicit_package_init_plan(package: &Path, initialized: bool) -> Vec<InitPlanEntry> {
    if initialized {
        Vec::new()
    } else {
        package_init_plan(package)
    }
}

fn package_init_plan(package: &Path) -> Vec<InitPlanEntry> {
    vec![
        InitPlanEntry::directory(package.to_path_buf()),
        InitPlanEntry::file(
            "package_manifest",
            package.join("rototo-package.toml"),
            package_manifest_template(),
        ),
        InitPlanEntry::directory(package.join("variables")),
        InitPlanEntry::directory(package.join("model/catalogs")),
        InitPlanEntry::directory(package.join("data/catalogs")),
        InitPlanEntry::directory(package.join("model/context")),
        InitPlanEntry::directory(package.join("lint")),
    ]
}

async fn package_initialized(package: &Path) -> Result<bool> {
    path_exists(&package.join("rototo-package.toml")).await
}

#[derive(Debug)]
struct InitPlan {
    entries: Vec<InitPlanEntry>,
    schema_updates: Vec<InitSchemaUpdate>,
}

impl InitPlan {
    fn from_entries(entries: Vec<InitPlanEntry>) -> Self {
        Self {
            entries,
            schema_updates: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct InitPlanEntry {
    kind: &'static str,
    path: PathBuf,
    content: Option<String>,
    update_existing: bool,
}

impl InitPlanEntry {
    fn directory(path: PathBuf) -> Self {
        Self {
            kind: "directory",
            path,
            content: None,
            update_existing: false,
        }
    }

    fn file(kind: &'static str, path: PathBuf, content: String) -> Self {
        Self {
            kind,
            path,
            content: Some(content),
            update_existing: false,
        }
    }

    fn file_update(kind: &'static str, path: PathBuf, content: String) -> Self {
        Self {
            kind,
            path,
            content: Some(content),
            update_existing: true,
        }
    }

    fn is_directory(&self) -> bool {
        self.content.is_none()
    }
}

#[derive(Debug)]
struct InitSchemaUpdate {
    context_id: String,
    path: PathBuf,
    added: Vec<InitSchemaPathChange>,
    unchanged: Vec<InitSchemaPathChange>,
    conflicts: Vec<InitSchemaConflict>,
}

impl InitSchemaUpdate {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.unchanged.is_empty() && self.conflicts.is_empty()
    }
}

#[derive(Clone, Debug, Serialize)]
struct InitSchemaPathChange {
    path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    types: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct InitSchemaConflict {
    path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    existing_types: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    inferred_types: Vec<String>,
}

#[derive(Debug, Serialize)]
struct InitReport {
    command: &'static str,
    package: String,
    dry_run: bool,
    files: Vec<InitFileReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    schema_updates: Vec<InitSchemaUpdateReport>,
}

#[derive(Debug, Serialize)]
struct InitFileReport {
    kind: &'static str,
    path: String,
    action: InitAction,
}

#[derive(Debug, Serialize)]
struct InitSchemaUpdateReport {
    context_id: String,
    path: String,
    added: Vec<InitSchemaPathChange>,
    unchanged: Vec<InitSchemaPathChange>,
    conflicts: Vec<InitSchemaConflict>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum InitAction {
    Exists,
    Created,
    Overwritten,
    Updated,
    WouldCreate,
    WouldOverwrite,
    WouldUpdate,
}

impl InitAction {
    fn label(self) -> &'static str {
        match self {
            Self::Exists => "exists",
            Self::Created => "created",
            Self::Overwritten => "overwritten",
            Self::Updated => "updated",
            Self::WouldCreate => "would create",
            Self::WouldOverwrite => "would overwrite",
            Self::WouldUpdate => "would update",
        }
    }
}

async fn execute_init_plan(
    package: &Path,
    plan: &InitPlan,
    force: bool,
    dry_run: bool,
) -> Result<InitReport> {
    let mut actions = Vec::with_capacity(plan.entries.len());
    for entry in &plan.entries {
        actions.push(planned_init_action(entry, force, dry_run).await?);
    }

    if !dry_run {
        for entry in &plan.entries {
            if entry.is_directory() {
                tokio::fs::create_dir_all(&entry.path)
                    .await
                    .map_err(|err| {
                        RototoError::new(format!(
                            "failed to create directory {}: {err}",
                            entry.path.display()
                        ))
                    })?;
            } else if let Some(content) = &entry.content {
                if let Some(parent) = entry.path.parent() {
                    tokio::fs::create_dir_all(parent).await.map_err(|err| {
                        RototoError::new(format!(
                            "failed to create directory {}: {err}",
                            parent.display()
                        ))
                    })?;
                }
                tokio::fs::write(&entry.path, content)
                    .await
                    .map_err(|err| {
                        RototoError::new(format!("failed to write {}: {err}", entry.path.display()))
                    })?;
            }
        }
    }

    Ok(InitReport {
        command: "init",
        package: package.display().to_string(),
        dry_run,
        files: plan
            .entries
            .iter()
            .zip(actions)
            .map(|(entry, action)| InitFileReport {
                kind: entry.kind,
                path: init_report_path(package, &entry.path),
                action,
            })
            .collect(),
        schema_updates: plan
            .schema_updates
            .iter()
            .map(|update| InitSchemaUpdateReport {
                context_id: update.context_id.clone(),
                path: init_report_path(package, &update.path),
                added: update.added.clone(),
                unchanged: update.unchanged.clone(),
                conflicts: update.conflicts.clone(),
            })
            .collect(),
    })
}

async fn planned_init_action(
    entry: &InitPlanEntry,
    force: bool,
    dry_run: bool,
) -> Result<InitAction> {
    let metadata = match tokio::fs::metadata(&entry.path).await {
        Ok(metadata) => Some(metadata),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(RototoError::new(format!(
                "failed to inspect {}: {err}",
                entry.path.display()
            )));
        }
    };

    if entry.is_directory() {
        if let Some(metadata) = metadata {
            if !metadata.is_dir() {
                return Err(RototoError::new(format!(
                    "path exists and is not a directory: {}",
                    entry.path.display()
                )));
            }
            return Ok(InitAction::Exists);
        }
        return Ok(if dry_run {
            InitAction::WouldCreate
        } else {
            InitAction::Created
        });
    }

    if let Some(metadata) = metadata {
        if metadata.is_dir() {
            return Err(RototoError::new(format!(
                "path exists and is a directory: {}",
                entry.path.display()
            )));
        }
        if !force {
            if entry.update_existing {
                return Ok(if dry_run {
                    InitAction::WouldUpdate
                } else {
                    InitAction::Updated
                });
            }
            return Err(RototoError::new(format!(
                "file already exists: {} (use --force to overwrite)",
                entry.path.display()
            )));
        }
        return Ok(if dry_run {
            InitAction::WouldOverwrite
        } else {
            InitAction::Overwritten
        });
    }

    Ok(if dry_run {
        InitAction::WouldCreate
    } else {
        InitAction::Created
    })
}

fn init_report_path(package: &Path, path: &Path) -> String {
    match path.strip_prefix(package) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
        Ok(relative) => relative.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

fn print_init_report(report: &InitReport, json: bool, quiet: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report)
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    if quiet {
        return Ok(());
    }

    println!("package: {}", report.package);
    for file in &report.files {
        println!(
            "  {:<15} {}",
            format!("{}:", file.action.label()),
            file.path
        );
    }
    for update in &report.schema_updates {
        if update.added.is_empty() && update.unchanged.is_empty() && update.conflicts.is_empty() {
            continue;
        }
        println!("  {} {}", style::label("schema"), style::sea(&update.path));
        for change in &update.added {
            println!(
                "    {:<11} {}{}",
                format!("{}:", style::ok("added")),
                style::info(&change.path),
                init_schema_types_suffix(&change.types)
            );
        }
        for change in &update.unchanged {
            println!(
                "    {:<11} {}{}",
                format!("{}:", style::dim("unchanged")),
                style::info(&change.path),
                init_schema_types_suffix(&change.types)
            );
        }
        for conflict in &update.conflicts {
            println!(
                "    {:<11} {} {}",
                format!("{}:", style::warn("conflict")),
                style::info(&conflict.path),
                style::dim(&format!(
                    "existing {}, inferred {}",
                    init_schema_types_label(&conflict.existing_types),
                    init_schema_types_label(&conflict.inferred_types)
                ))
            );
        }
    }
    Ok(())
}

fn init_schema_types_suffix(types: &[String]) -> String {
    if types.is_empty() {
        String::new()
    } else {
        format!(" {}", style::dim(&types.join(" or ")))
    }
}

fn init_schema_types_label(types: &[String]) -> String {
    if types.is_empty() {
        "untyped".to_owned()
    } else {
        types.join(" or ")
    }
}

fn package_manifest_template() -> String {
    r#"schema_version = 1

# Optional package layering:
#
# extends = ["../shared-config"]
#
# Custom lint handlers live in lint/*.lua and register their rule metadata there.

# Optional resolution tracing. Each [[trace]] policy emits a full resolution
# trace to the SDK trace stream whenever its `when` matches, with no app
# redeploy. Use it to debug a specific production resolution from reviewed
# config. `when` reads context.* and variables["<id>"] like any expression,
# and may additionally read env.resolving.variable, the variable currently
# being resolved.
#
# [[trace]]
# when = 'env.resolving.variable == "checkout_redesign" && context.user.id == "tester-123"'
"#
    .to_owned()
}

fn variable_template(id: &str) -> String {
    let description = toml_string(&format!(
        "Edit this description to explain what {id} controls"
    ));
    format!(
        r#"schema_version = 1

# Explain what runtime behavior this variable controls.
description = {description}

# Required. Supported types:
# bool, int, number, string, list, list<string>, list<int>, list<number>,
# list<bool>, catalog:<catalog-id>, list<catalog:<catalog-id>>
type = "string"

[resolve]
# Required. Used when no rule matches. The value must match `type`.
default = "control"

# Literal defaults by type:
# default = true
# default = 10
# default = 0.25
# default = "control"
# default = ["email", "card"]
# default = "catalog-entry-id"
# default = ["catalog-entry-a", "catalog-entry-b"]

# Rules are evaluated top to bottom. The first matching rule selects its value.
#
# [[resolve.rule]]
# when = 'context.account.plan == "enterprise" && context.account.seats >= 100'
# value = "enterprise"

# Rule conditions can also read other variables' resolved values, so a bool
# "condition" variable can name a runtime condition other variables share.
#
# [[resolve.rule]]
# when = 'variables["premium_users"]'
# value = "treatment"

# For catalog-backed variables, set:
#
# type = "catalog:{id}"
#
# Then `default` and rule `value` select catalog entry ids:
#
# [resolve]
# default = "control"
#
# [[resolve.rule]]
# when = 'variables["premium_users"]'
# value = "premium"

# For list<catalog:...> variables, rules may select entries with `query`
# instead of a fixed value. `entry.*` reads each catalog entry.
#
# type = "list<catalog:{id}>"
#
# [resolve]
# default = []
#
# [[resolve.rule]]
# query = 'entry.enabled == true && variables["premium_users"]'
"#
    )
}

fn catalog_schema_template() -> Result<String> {
    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "description": "Edit this description to explain the catalog values",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "heading": { "type": "string" },
            "enabled": { "type": "boolean" }
        },
        "required": ["heading", "enabled"]
    });
    pretty_json(&schema)
}

fn catalog_entry_template() -> String {
    r#"heading = "Edit this heading"
enabled = false
"#
    .to_owned()
}

async fn context_schema_template(
    package: &Path,
    context_path: &Path,
    context_id: &str,
    initialized: bool,
    update: bool,
) -> Result<(String, Option<InitSchemaUpdate>)> {
    let attributes = if initialized {
        inferred_context_attributes(package).await?
    } else {
        BTreeMap::new()
    };

    if update {
        return update_context_schema_template(context_path, context_id, &attributes).await;
    }

    Ok((
        pretty_json(&context_schema_from_attributes(&attributes)?)?,
        None,
    ))
}

async fn inferred_context_attributes(
    package: &Path,
) -> Result<BTreeMap<String, BTreeSet<ContextSchemaType>>> {
    let report = inspect_package_report(
        package,
        PackageInspectRequest {
            variables: InspectSelection::All,
            ..PackageInspectRequest::default()
        },
    )
    .await?;

    let mut attributes = BTreeMap::new();
    for variable in &report.variables {
        collect_inferred_context_attributes(&variable.context_attributes, &mut attributes);
    }
    Ok(attributes)
}

fn collect_inferred_context_attributes(
    reports: &[rototo::model::ContextAttributeInspectReport],
    attributes: &mut BTreeMap<String, BTreeSet<ContextSchemaType>>,
) {
    for report in reports {
        let types = attributes.entry(report.path.clone()).or_default();
        for expected in &report.expected_types {
            if let Some(ty) = context_schema_type_from_expected(expected) {
                types.insert(ty);
            }
        }
    }
}

async fn update_context_schema_template(
    context_path: &Path,
    context_id: &str,
    attributes: &BTreeMap<String, BTreeSet<ContextSchemaType>>,
) -> Result<(String, Option<InitSchemaUpdate>)> {
    let mut update = InitSchemaUpdate {
        context_id: context_id.to_owned(),
        path: context_path.to_path_buf(),
        added: Vec::new(),
        unchanged: Vec::new(),
        conflicts: Vec::new(),
    };

    let existing = match tokio::fs::read_to_string(context_path).await {
        Ok(text) => Some(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(RototoError::new(format!(
                "failed to read {}: {err}",
                context_path.display()
            )));
        }
    };

    let Some(existing_text) = existing else {
        update.added = attributes
            .iter()
            .map(|(path, types)| InitSchemaPathChange {
                path: context_path_label(path),
                types: context_schema_type_names(types),
            })
            .collect();
        let schema = context_schema_from_attributes(attributes)?;
        return Ok((
            pretty_json(&schema)?,
            (!update.is_empty()).then_some(update),
        ));
    };

    let mut schema = serde_json::from_str::<serde_json::Value>(&existing_text).map_err(|err| {
        RototoError::new(format!(
            "failed to parse {} as JSON: {err}",
            context_path.display()
        ))
    })?;
    if !schema.is_object() {
        return Err(RototoError::new(format!(
            "evaluation context schema must be a JSON object to update: {}",
            context_path.display()
        )));
    }

    for (path, types) in attributes {
        add_inferred_context_path(&mut schema, path, types, &mut update)?;
    }

    let content = if update.added.is_empty() {
        existing_text
    } else {
        pretty_json(&schema)?
    };
    Ok((content, (!update.is_empty()).then_some(update)))
}

fn context_schema_from_attributes(
    attributes: &BTreeMap<String, BTreeSet<ContextSchemaType>>,
) -> Result<serde_json::Value> {
    if attributes.is_empty() {
        return starter_context_schema();
    }

    let mut builder = ContextSchemaBuilder::default();
    for (path, types) in attributes {
        builder.add_context_path(path, types);
    }
    Ok(builder.into_schema())
}

fn starter_context_schema() -> Result<serde_json::Value> {
    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "user": {
                "type": "object",
                "additionalProperties": true,
                "properties": {
                    "tier": { "type": "string" },
                    "id": { "type": ["string", "number"] }
                }
            }
        }
    });
    Ok(schema)
}

fn add_inferred_context_path(
    schema: &mut serde_json::Value,
    path: &str,
    types: &BTreeSet<ContextSchemaType>,
    update: &mut InitSchemaUpdate,
) -> Result<()> {
    let segments = path.split('.').collect::<Vec<_>>();
    if segments.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
        return Ok(());
    }
    merge_inferred_context_path(schema, &segments, path, types, update)
}

fn merge_inferred_context_path(
    schema: &mut serde_json::Value,
    segments: &[&str],
    full_path: &str,
    inferred_types: &BTreeSet<ContextSchemaType>,
    update: &mut InitSchemaUpdate,
) -> Result<()> {
    let segment = segments[0];
    let Some(object) = schema.as_object_mut() else {
        update.conflicts.push(InitSchemaConflict {
            path: context_path_label(full_path),
            existing_types: context_schema_type_names(&BTreeSet::new()),
            inferred_types: context_schema_type_names(inferred_types),
        });
        return Ok(());
    };
    let properties = object
        .entry("properties".to_owned())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    let Some(properties) = properties.as_object_mut() else {
        update.conflicts.push(InitSchemaConflict {
            path: context_path_label(full_path),
            existing_types: context_schema_type_names(&BTreeSet::new()),
            inferred_types: context_schema_type_names(inferred_types),
        });
        return Ok(());
    };

    if segments.len() == 1 {
        match properties.get(segment) {
            Some(existing) => {
                if context_schema_field_satisfies(existing, inferred_types) {
                    update.unchanged.push(InitSchemaPathChange {
                        path: context_path_label(full_path),
                        types: context_schema_type_names(inferred_types),
                    });
                } else {
                    update.conflicts.push(InitSchemaConflict {
                        path: context_path_label(full_path),
                        existing_types: context_schema_type_names(&context_schema_declared_types(
                            existing,
                        )),
                        inferred_types: context_schema_type_names(inferred_types),
                    });
                }
            }
            None => {
                properties.insert(segment.to_owned(), context_schema_leaf(inferred_types));
                update.added.push(InitSchemaPathChange {
                    path: context_path_label(full_path),
                    types: context_schema_type_names(inferred_types),
                });
            }
        }
        return Ok(());
    }

    if !properties.contains_key(segment) {
        properties.insert(segment.to_owned(), empty_context_object_schema());
    }

    let child = properties
        .get_mut(segment)
        .expect("context schema child inserted above");
    if !ensure_context_schema_can_contain_child(child) {
        update.conflicts.push(InitSchemaConflict {
            path: context_path_label(full_path),
            existing_types: context_schema_type_names(&context_schema_declared_types(child)),
            inferred_types: context_schema_type_names(inferred_types),
        });
        return Ok(());
    }

    merge_inferred_context_path(child, &segments[1..], full_path, inferred_types, update)
}

fn ensure_context_schema_can_contain_child(schema: &mut serde_json::Value) -> bool {
    if !schema.is_object() {
        return false;
    }
    let types = context_schema_declared_types(schema);
    if !types.is_empty() && !types.contains(&ContextSchemaType::Object) {
        return false;
    }

    let object = schema.as_object_mut().expect("object checked above");
    let properties = object
        .entry("properties".to_owned())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    properties.is_object()
}

fn context_schema_field_satisfies(
    schema: &serde_json::Value,
    inferred_types: &BTreeSet<ContextSchemaType>,
) -> bool {
    if inferred_types.is_empty() {
        return true;
    }
    let declared = context_schema_declared_types(schema);
    if declared.is_empty() {
        return true;
    }
    inferred_types.iter().all(|inferred| {
        declared
            .iter()
            .any(|declared| declared.satisfies(*inferred))
    })
}

fn context_path_label(path: &str) -> String {
    format!("context.{path}")
}

fn context_schema_type_names(types: &BTreeSet<ContextSchemaType>) -> Vec<String> {
    let mut types = types.clone();
    normalize_context_schema_types(&mut types);
    types.iter().map(|ty| ty.as_str().to_owned()).collect()
}

#[derive(Default)]
struct ContextSchemaBuilder {
    properties: serde_json::Map<String, serde_json::Value>,
}

impl ContextSchemaBuilder {
    fn add_context_path(&mut self, path: &str, types: &BTreeSet<ContextSchemaType>) {
        let segments = path.split('.').collect::<Vec<_>>();
        if segments.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
            return;
        }

        insert_context_schema_path(&mut self.properties, &segments, types);
    }

    fn into_schema(self) -> serde_json::Value {
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": true,
            "properties": self.properties
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ContextSchemaType {
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array,
    Object,
}

impl ContextSchemaType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::String => "string",
            Self::Array => "array",
            Self::Object => "object",
        }
    }

    fn satisfies(self, inferred: Self) -> bool {
        matches!((self, inferred), (Self::Integer, Self::Number)) || self == inferred
    }
}

fn insert_context_schema_path(
    properties: &mut serde_json::Map<String, serde_json::Value>,
    segments: &[&str],
    types: &BTreeSet<ContextSchemaType>,
) {
    let segment = segments[0];
    if segments.len() == 1 {
        merge_context_schema_leaf(properties, segment, types);
        return;
    }

    let entry = properties
        .entry(segment.to_owned())
        .or_insert_with(empty_context_object_schema);
    ensure_context_object_schema(entry);
    let child_properties = entry
        .as_object_mut()
        .expect("object schema ensured above")
        .entry("properties")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .expect("properties object inserted above");
    insert_context_schema_path(child_properties, &segments[1..], types);
}

fn merge_context_schema_leaf(
    properties: &mut serde_json::Map<String, serde_json::Value>,
    segment: &str,
    types: &BTreeSet<ContextSchemaType>,
) {
    let entry = properties
        .entry(segment.to_owned())
        .or_insert_with(|| context_schema_leaf(types));
    if entry
        .as_object()
        .is_some_and(|object| object.contains_key("properties"))
    {
        return;
    }

    let mut merged = context_schema_types_from_schema(entry);
    merged.extend(types.iter().copied());
    normalize_context_schema_types(&mut merged);
    *entry = context_schema_leaf(&merged);
}

fn empty_context_object_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": true,
        "properties": {}
    })
}

fn ensure_context_object_schema(value: &mut serde_json::Value) {
    if !value.is_object() {
        *value = empty_context_object_schema();
        return;
    }

    let object = value.as_object_mut().expect("object checked above");
    object.insert(
        "type".to_owned(),
        serde_json::Value::String("object".to_owned()),
    );
    object.insert(
        "additionalProperties".to_owned(),
        serde_json::Value::Bool(true),
    );
    let properties = object
        .entry("properties")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !properties.is_object() {
        *properties = serde_json::Value::Object(serde_json::Map::new());
    }
}

fn context_schema_leaf(types: &BTreeSet<ContextSchemaType>) -> serde_json::Value {
    if types.is_empty() {
        return serde_json::Value::Object(serde_json::Map::new());
    }
    let mut object = serde_json::Map::new();
    object.insert("type".to_owned(), context_schema_type_value(types));
    serde_json::Value::Object(object)
}

fn context_schema_type_value(types: &BTreeSet<ContextSchemaType>) -> serde_json::Value {
    let mut types = types.clone();
    normalize_context_schema_types(&mut types);
    if types.len() == 1 {
        return serde_json::Value::String(
            types.iter().next().expect("one type").as_str().to_owned(),
        );
    }
    serde_json::Value::Array(
        types
            .iter()
            .map(|ty| serde_json::Value::String(ty.as_str().to_owned()))
            .collect(),
    )
}

fn context_schema_types_from_schema(schema: &serde_json::Value) -> BTreeSet<ContextSchemaType> {
    let mut types = BTreeSet::new();
    match schema.as_object().and_then(|object| object.get("type")) {
        Some(serde_json::Value::String(value)) => {
            if let Some(ty) = context_schema_type_from_str(value) {
                types.insert(ty);
            }
        }
        Some(serde_json::Value::Array(values)) => {
            for value in values {
                if let Some(ty) = value.as_str().and_then(context_schema_type_from_str) {
                    types.insert(ty);
                }
            }
        }
        _ => {}
    }
    types
}

fn context_schema_declared_types(schema: &serde_json::Value) -> BTreeSet<ContextSchemaType> {
    let mut types = context_schema_types_from_schema(schema);
    if schema
        .as_object()
        .and_then(|object| object.get("properties"))
        .is_some()
    {
        types.insert(ContextSchemaType::Object);
    }
    types
}

fn context_schema_type_from_str(value: &str) -> Option<ContextSchemaType> {
    match value {
        "boolean" => Some(ContextSchemaType::Boolean),
        "integer" => Some(ContextSchemaType::Integer),
        "number" => Some(ContextSchemaType::Number),
        "string" => Some(ContextSchemaType::String),
        "array" => Some(ContextSchemaType::Array),
        "object" => Some(ContextSchemaType::Object),
        "null" => Some(ContextSchemaType::Null),
        _ => None,
    }
}

fn context_schema_type_from_expected(value: &str) -> Option<ContextSchemaType> {
    match value {
        "boolean" => Some(ContextSchemaType::Boolean),
        "number" => Some(ContextSchemaType::Number),
        "string" => Some(ContextSchemaType::String),
        _ => None,
    }
}

fn normalize_context_schema_types(types: &mut BTreeSet<ContextSchemaType>) {
    if types.contains(&ContextSchemaType::Number) {
        types.remove(&ContextSchemaType::Integer);
    }
}

fn pretty_json(value: &serde_json::Value) -> Result<String> {
    let mut text =
        serde_json::to_string_pretty(value).map_err(|err| RototoError::new(err.to_string()))?;
    text.push('\n');
    Ok(text)
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}
