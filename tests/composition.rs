//! Structured composition through `extends`: an overlay package unions,
//! tombstones, and patches catalog entries, replaces a variable's `[resolve]`
//! block while inheriting its type, and adds namespaced variables of its own.

use std::path::Path;

use rototo::{EvaluationContext, Package};

async fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.unwrap();
    }
    tokio::fs::write(path, contents).await.unwrap();
}

/// A base package with a `plans` catalog (free, growth) and an `active_plan`
/// variable resolved by rules.
async fn write_base(root: &Path) {
    write(root, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        root,
        "model/catalogs/plans.schema.json",
        r#"{
  "type": "object",
  "required": ["name", "monthly_price"],
  "properties": {
    "name": { "type": "string" },
    "monthly_price": { "type": "number" },
    "limits": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "seats": { "type": "integer" },
        "projects": { "type": "integer" }
      }
    }
  },
  "additionalProperties": false
}
"#,
    )
    .await;
    write(
        root,
        "model/context/request.schema.json",
        r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": true,
  "properties": {
    "account": {
      "type": "object",
      "additionalProperties": true,
      "properties": {
        "paid": { "type": "boolean" },
        "trial": { "type": "boolean" }
      }
    }
  }
}
"#,
    )
    .await;
    write(
        root,
        "data/catalogs/plans/free.toml",
        "name = \"Free\"\nmonthly_price = 0\n",
    )
    .await;
    write(
        root,
        "data/catalogs/plans/growth.toml",
        r#"name = "Growth"
monthly_price = 49

[limits]
seats = 10
projects = 20
"#,
    )
    .await;
    write(
        root,
        "variables/active_plan.toml",
        r#"schema_version = 1
type = "catalog:plans"

[resolve]
default = "free"

[[resolve.rule]]
when = 'context.account.paid == true'
value = "growth"
"#,
    )
    .await;
}

/// The tenant overlay: every membership mechanism once, exactly the shape the
/// design's worked example uses.
async fn write_overlay(root: &Path) {
    write(
        root,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    // UNION: a new entry only this tenant has.
    write(
        root,
        "data/catalogs/plans/acme_enterprise.toml",
        "name = \"Acme Enterprise\"\nmonthly_price = 500\n",
    )
    .await;
    // TOMBSTONE: the tenant does not offer the base free plan.
    write(
        root,
        "data/catalogs/plans/free.tombstone.toml",
        "tombstone = true\nreason = \"Acme does not offer a free plan\"\n",
    )
    .await;
    // PATCH: negotiated price; other fields (name, limits) inherited.
    write(
        root,
        "data/catalogs/plans/growth.patch.toml",
        "monthly_price = 59\n\n[limits]\nseats = 25\n",
    )
    .await;
    // OVERRIDE: a replacement [resolve] block; the type stays with the base.
    write(
        root,
        "variables/active_plan.toml",
        r#"[resolve]
default = "acme_enterprise"

[[resolve.rule]]
when = 'variables["acme/in_trial"]'
value = "growth"
"#,
    )
    .await;
    // ADD: a namespaced tenant-internal condition.
    write(
        root,
        "variables/acme/in_trial.toml",
        r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = 'context.account.trial == true'
value = true
"#,
    )
    .await;
}

#[tokio::test]
async fn overlay_composes_membership_values_and_additions() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write_overlay(&overlay).await;

    let package = Package::load(overlay.to_string_lossy()).await.unwrap();

    // OVERRIDE: the overlay's default wins, and its type came from the base.
    let context = EvaluationContext::from_json(serde_json::json!({
        "account": { "trial": false }
    }))
    .unwrap();
    let resolution = package.resolve_variable("active_plan", &context).unwrap();
    assert_eq!(resolution.value["name"], "Acme Enterprise");

    // OVERRIDE + ADD: the overlay rule leans on the namespaced condition.
    let context = EvaluationContext::from_json(serde_json::json!({
        "account": { "trial": true }
    }))
    .unwrap();
    let resolution = package.resolve_variable("active_plan", &context).unwrap();
    // PATCH: negotiated price and seat limit override the base fields,
    // unpatched fields are inherited.
    assert_eq!(resolution.value["name"], "Growth");
    assert_eq!(resolution.value["monthly_price"], 59);
    assert_eq!(resolution.value["limits"]["seats"], 25);
    assert_eq!(resolution.value["limits"]["projects"], 20);
}

#[tokio::test]
async fn overlay_tombstone_disables_the_base_entry() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write_overlay(&overlay).await;

    // The base default was "free"; the overlay replaced the resolve block, so
    // referencing the tombstoned entry from the overlay is a lint failure.
    write(
        &overlay,
        "variables/wants_free.toml",
        r#"schema_version = 1
type = "catalog:plans"

[resolve]
default = "free"
"#,
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string().contains("lint failed"),
        "loading should fail lint because the free entry is disabled: {err}"
    );

    // Inspecting without the lint gate shows exactly which reference broke.
    let staged = Package::inspect(overlay.to_string_lossy()).await.unwrap();
    let lint = staged.lint().await.unwrap();
    assert!(
        lint.diagnostics.iter().any(|diagnostic| {
            diagnostic.rule.as_string() == "rototo/variable-unknown-value"
                && diagnostic.message.contains("free")
        }),
        "{:#?}",
        lint.diagnostics
    );

    // The base itself still lints clean and still has the entry.
    let base_package = Package::load(base.to_string_lossy()).await.unwrap();
    let context = EvaluationContext::from_json(serde_json::json!({
        "account": { "paid": false }
    }))
    .unwrap();
    let resolution = base_package
        .resolve_variable("active_plan", &context)
        .unwrap();
    assert_eq!(resolution.value["name"], "Free");
}

#[tokio::test]
async fn orphan_tombstones_and_patches_fail_loudly() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(
        &overlay,
        "data/catalogs/plans/nonexistent.tombstone.toml",
        "tombstone = true\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("tombstone has no catalog entry to disable"),
        "{err}"
    );

    tokio::fs::remove_file(overlay.join("data/catalogs/plans/nonexistent.tombstone.toml"))
        .await
        .unwrap();
    write(
        &overlay,
        "data/catalogs/plans/nonexistent.patch.toml",
        "monthly_price = 1\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("patch has no catalog entry to override"),
        "{err}"
    );
}

#[tokio::test]
async fn same_layer_entry_and_tombstone_conflict() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(
        &overlay,
        "data/catalogs/plans/growth.toml",
        "name = \"Growth\"\nmonthly_price = 59\n",
    )
    .await;
    write(
        &overlay,
        "data/catalogs/plans/growth.tombstone.toml",
        "tombstone = true\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("both provides catalog entry growth and declares a tombstone"),
        "{err}"
    );
}

#[tokio::test]
async fn overlay_cannot_change_a_variable_type() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(
        &overlay,
        "variables/active_plan.toml",
        "type = \"string\"\n\n[resolve]\ndefault = \"growth\"\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("overlay changes the variable's type from catalog:plans to string"),
        "{err}"
    );

    // Restating the same type is allowed; narrowing a type means agreeing
    // with it.
    write(
        &overlay,
        "variables/active_plan.toml",
        "type = \"catalog:plans\"\n\n[resolve]\ndefault = \"growth\"\n",
    )
    .await;
    let package = Package::load(overlay.to_string_lossy()).await.unwrap();
    let context = EvaluationContext::from_json(serde_json::json!({
        "account": { "paid": false }
    }))
    .unwrap();
    let resolution = package.resolve_variable("active_plan", &context).unwrap();
    assert_eq!(resolution.value["name"], "Growth");
}

/// The base's layering contract for the governed tests: entries may be
/// added, prices patched (never on free), any plan but free disabled, and
/// active_plan's resolution overridden.
const PLANS_GOVERNANCE: &str = r#"[catalog.plans]
allowed_operations = ["add", "update", "delete"]

[catalog.plans.update_policy]
allowed_fields = ["monthly_price", "limits"]
denied_entries = ["free"]

[catalog.plans.delete_policy]
allowed_entries = ["*"]
denied_entries = ["free"]

[variable.active_plan]
allowed_operations = ["override"]
"#;

#[tokio::test]
async fn governed_base_admits_the_granted_overlay() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write(&base, "governance.toml", PLANS_GOVERNANCE).await;
    write_overlay(&overlay).await;
    // The stock overlay tombstones free, which the contract denies; point the
    // tombstone at growth instead and drop the growth patch.
    tokio::fs::remove_file(overlay.join("data/catalogs/plans/free.tombstone.toml"))
        .await
        .unwrap();
    tokio::fs::remove_file(overlay.join("data/catalogs/plans/growth.patch.toml"))
        .await
        .unwrap();
    write(
        &overlay,
        "variables/active_plan.toml",
        "[resolve]\ndefault = \"acme_enterprise\"\n",
    )
    .await;
    tokio::fs::remove_file(overlay.join("variables/acme/in_trial.toml"))
        .await
        .unwrap();

    let package = Package::load(overlay.to_string_lossy()).await.unwrap();
    let context = EvaluationContext::from_json(serde_json::json!({})).unwrap();
    let resolution = package.resolve_variable("active_plan", &context).unwrap();
    assert_eq!(resolution.value["name"], "Acme Enterprise");
}

#[tokio::test]
async fn governed_base_denies_ungranted_operations() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    write_base(&base).await;
    write(&base, "governance.toml", PLANS_GOVERNANCE).await;

    // Tombstoning the protected entry.
    let overlay = temp.path().join("overlay-delete");
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(
        &overlay,
        "data/catalogs/plans/free.tombstone.toml",
        "tombstone = true\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies delete of entry free on catalog.plans"),
        "{err}"
    );

    // Patching a field outside the allowlist.
    let overlay = temp.path().join("overlay-field");
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(
        &overlay,
        "data/catalogs/plans/growth.patch.toml",
        "name = \"Renamed\"\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies update of field name on catalog.plans"),
        "{err}"
    );

    // Replacing a whole entry file is not a governed operation.
    let overlay = temp.path().join("overlay-replace");
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(
        &overlay,
        "data/catalogs/plans/growth.toml",
        "name = \"Growth\"\nmonthly_price = 1\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(err.to_string().contains("use growth.patch.toml"), "{err}");

    // Touching the schema needs constrain, which the contract does not grant.
    let overlay = temp.path().join("overlay-schema");
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(&overlay, "model/catalogs/plans.schema.json", "{}\n").await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies constrain on catalog.plans"),
        "{err}"
    );

    // Overriding a variable the contract never opened.
    let overlay = temp.path().join("overlay-variable");
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(
        &overlay,
        "variables/wants_free.toml",
        "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = false\n",
    )
    .await;
    // Adding a brand-new variable mints an id and is fine...
    Package::load(overlay.to_string_lossy()).await.unwrap();
    // ...but replacing a base variable's resolution needs the override grant.
    write(
        &overlay,
        "variables/is_enterprise.toml",
        "[resolve]\ndefault = true\n",
    )
    .await;
    write(
        &base,
        "variables/is_enterprise.toml",
        "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = false\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies override on variable.is_enterprise"),
        "{err}"
    );
}

#[tokio::test]
async fn governance_grants_cannot_exceed_the_inherited_ceiling() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write(&base, "governance.toml", PLANS_GOVERNANCE).await;
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    // The overlay tries to hand its own sub-layers constrain, which the base
    // never granted the overlay itself.
    write(
        &overlay,
        "governance.toml",
        "[catalog.plans]\nallowed_operations = [\"constrain\"]\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance grant exceeds the inherited ceiling"),
        "{err}"
    );

    // Narrowing is legal: re-granting a subset with more denies.
    write(
        &overlay,
        "governance.toml",
        "[catalog.plans]\nallowed_operations = [\"add\"]\n",
    )
    .await;
    Package::load(overlay.to_string_lossy()).await.unwrap();
}

#[tokio::test]
async fn trace_provenance_names_the_layer_that_owns_the_resolution() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write_overlay(&overlay).await;

    // Load once through the SDK to prove the composed package lints clean.
    Package::load(overlay.to_string_lossy()).await.unwrap();

    let staged = Package::inspect(overlay.to_string_lossy()).await.unwrap();
    let context = serde_json::json!({ "account": { "trial": false } });

    // The overlay replaced active_plan's resolve block; the trace says so.
    let trace = rototo::trace_variable_resolution(staged.root(), "active_plan", &context)
        .await
        .unwrap();
    assert_eq!(trace.provenance.as_deref(), Some(overlay.to_str().unwrap()));

    // acme/in_trial is the overlay's own variable.
    let trace = rototo::trace_variable_resolution(staged.root(), "acme/in_trial", &context)
        .await
        .unwrap();
    assert_eq!(trace.provenance.as_deref(), Some(overlay.to_str().unwrap()));
}

#[tokio::test]
async fn overlay_enum_members_union_with_the_base() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write(
        &base,
        "model/enums/regions.toml",
        "schema_version = 1\ntype = \"string\"\n",
    )
    .await;
    write(
        &base,
        "data/enums/regions.toml",
        "members = [\"us\", \"eu\"]\n",
    )
    .await;
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    // The overlay declares only what it adds; the base's members stay.
    write(
        &overlay,
        "data/enums/regions.toml",
        "members = [\"apac\", \"eu\"]\n",
    )
    .await;
    write(
        &overlay,
        "variables/home_region.toml",
        r#"schema_version = 1
type = "enum:regions"

[resolve]
default = "apac"

[[resolve.rule]]
when = 'context.account.paid == true'
value = "us"
"#,
    )
    .await;

    let package = Package::load(overlay.to_string_lossy()).await.unwrap();
    let context = EvaluationContext::from_json(serde_json::json!({
        "account": { "paid": true }
    }))
    .unwrap();
    // "us" is a base member the overlay never restated; the union kept it.
    let resolution = package.resolve_variable("home_region", &context).unwrap();
    assert_eq!(resolution.value, "us");
}
