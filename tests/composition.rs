//! Structured composition through `extends`: an overlay package unions,
//! deletes, and updates catalog entries, replaces a variable's `[resolve]`
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
    // Deny-by-default is unconditional; this base opens itself to its own
    // overlays with a broad [defaults] grant.
    write(
        root,
        "governance.toml",
        "[defaults]\nallowed_operations = [\"add\", \"update\", \"delete\"]\n",
    )
    .await;

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
    // DELETED: the tenant does not offer the base free plan.
    write(
        root,
        "data/catalogs/plans/free.deleted.toml",
        "deleted = true\nreason = \"Acme does not offer a free plan\"\n",
    )
    .await;
    // UPDATE: negotiated price; other fields (name, limits) inherited.
    write(
        root,
        "data/catalogs/plans/growth.update.toml",
        "monthly_price = 59\n\n[limits]\nseats = 25\n",
    )
    .await;
    // UPDATE: a replacement [resolve] block; the type stays with the base.
    write(
        root,
        "variables/active_plan.update.toml",
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
    // UPDATE: negotiated price and seat limit override the base fields,
    // fields the update does not mention are inherited.
    assert_eq!(resolution.value["name"], "Growth");
    assert_eq!(resolution.value["monthly_price"], 59);
    assert_eq!(resolution.value["limits"]["seats"], 25);
    assert_eq!(resolution.value["limits"]["projects"], 20);
}

#[tokio::test]
async fn overlay_deleted_marker_removes_the_base_entry() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write_overlay(&overlay).await;

    // The base default was "free"; the overlay replaced the resolve block, so
    // referencing the deleted entry from the overlay is a lint failure.
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
async fn orphan_deleted_and_update_markers_fail_loudly() {
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
        "data/catalogs/plans/nonexistent.deleted.toml",
        "deleted = true\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("deleted marker has no catalog entry to remove"),
        "{err}"
    );

    tokio::fs::remove_file(overlay.join("data/catalogs/plans/nonexistent.deleted.toml"))
        .await
        .unwrap();
    write(
        &overlay,
        "data/catalogs/plans/nonexistent.update.toml",
        "monthly_price = 1\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("update has no catalog entry to update"),
        "{err}"
    );
}

#[tokio::test]
async fn same_layer_entry_and_deleted_marker_conflict() {
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
        "data/catalogs/plans/growth.deleted.toml",
        "deleted = true\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("both provides catalog entry growth and declares a deleted marker"),
        "{err}"
    );
}

#[tokio::test]
async fn variable_restatement_requires_the_update_marker() {
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
        err.to_string().contains(
            "variable active_plan is declared in the base packages; update it with \
             variables/active_plan.update.toml"
        ),
        "{err}"
    );
}

#[tokio::test]
async fn byte_identical_variable_restatement_is_a_noop() {
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
    // Restating the base's file byte for byte is how diamond ancestry looks;
    // it composes as a no-op instead of demanding an update marker.
    let original = tokio::fs::read(base.join("variables/active_plan.toml"))
        .await
        .unwrap();
    tokio::fs::create_dir_all(overlay.join("variables"))
        .await
        .unwrap();
    tokio::fs::write(overlay.join("variables/active_plan.toml"), original)
        .await
        .unwrap();

    Package::load(overlay.to_string_lossy()).await.unwrap();
}

#[tokio::test]
async fn variable_update_may_only_carry_resolve_and_description() {
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
    // Even restating the type the base already declares is an error: an
    // update file carries only what it changes.
    write(
        &overlay,
        "variables/active_plan.update.toml",
        "type = \"catalog:plans\"\n\n[resolve]\ndefault = \"growth\"\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("a variable update may only update [resolve] and description"),
        "{err}"
    );

    // The permitted keys compose: resolve swaps whole, description replaces.
    write(
        &overlay,
        "variables/active_plan.update.toml",
        "description = \"Acme's plan selection\"\n\n[resolve]\ndefault = \"growth\"\n",
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

#[tokio::test]
async fn orphan_variable_updates_fail_loudly() {
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
        "variables/nonexistent.update.toml",
        "[resolve]\ndefault = true\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("variable update has no base variable to update"),
        "{err}"
    );
}

#[tokio::test]
async fn same_layer_variable_add_and_update_conflict() {
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
        "variables/brand_new.toml",
        "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = false\n",
    )
    .await;
    write(
        &overlay,
        "variables/brand_new.update.toml",
        "[resolve]\ndefault = true\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("package both provides variable brand_new and declares an update for it"),
        "{err}"
    );
}

#[tokio::test]
async fn a_base_without_a_contract_denies_modification() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    // A bare base: no governance.toml at all. Deny-by-default is
    // unconditional, so the overlay may add next to it but not modify it.
    write(&base, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        &base,
        "variables/greeting.toml",
        "schema_version = 1\ntype = \"string\"\n\n[resolve]\ndefault = \"hello\"\n",
    )
    .await;
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(
        &overlay,
        "variables/farewell.toml",
        "schema_version = 1\ntype = \"string\"\n\n[resolve]\ndefault = \"bye\"\n",
    )
    .await;

    // Adding a new id is free.
    Package::load(overlay.to_string_lossy()).await.unwrap();

    // Updating the base's variable is not: no contract means no grants.
    write(
        &overlay,
        "variables/greeting.update.toml",
        "[resolve]\ndefault = \"hi\"\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies update on variable.greeting"),
        "{err}"
    );
}

/// The base's layering contract for the governed tests: entries may be
/// added, prices updated (never on free), any plan but free disabled, and
/// active_plan's resolution updatable.
const PLANS_GOVERNANCE: &str = r#"[catalog.plans]
allowed_operations = ["add", "update", "delete"]

[catalog.plans.update_policy]
allowed_fields = ["monthly_price", "limits"]
denied_entries = ["free"]

[catalog.plans.delete_policy]
allowed_entries = ["*"]
denied_entries = ["free"]

[variable.active_plan]
allowed_operations = ["update"]
"#;

#[tokio::test]
async fn governed_base_admits_the_granted_overlay() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write(&base, "governance.toml", PLANS_GOVERNANCE).await;
    write_overlay(&overlay).await;
    // The stock overlay deletes free, which the contract denies; point the
    // deleted marker at growth instead and drop the growth update.
    tokio::fs::remove_file(overlay.join("data/catalogs/plans/free.deleted.toml"))
        .await
        .unwrap();
    tokio::fs::remove_file(overlay.join("data/catalogs/plans/growth.update.toml"))
        .await
        .unwrap();
    write(
        &overlay,
        "variables/active_plan.update.toml",
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
        "data/catalogs/plans/free.deleted.toml",
        "deleted = true\n",
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
        "data/catalogs/plans/growth.update.toml",
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
    assert!(err.to_string().contains("use growth.update.toml"), "{err}");

    // Touching a base schema is never grantable; narrowing is custom lint.
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
            .contains("governance does not allow an overlay to change a base catalog schema"),
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
    // ...but replacing a base variable's resolution needs the update grant.
    write(
        &overlay,
        "variables/is_enterprise.update.toml",
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
            .contains("governance denies update on variable.is_enterprise"),
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
    // The overlay tries to hand its own sub-layers delete on the variable,
    // which the base never granted the overlay itself.
    write(
        &overlay,
        "governance.toml",
        "[variable.active_plan]\nallowed_operations = [\"delete\"]\n",
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

/// A base with an enum both halves declared, ready for an overlay to compose
/// member deletes against.
async fn write_enum_base(root: &Path) {
    // Deny-by-default is unconditional; this base opens itself to its own
    // overlays with a broad [defaults] grant.
    write(
        root,
        "governance.toml",
        "[defaults]\nallowed_operations = [\"add\", \"update\", \"delete\"]\n",
    )
    .await;

    write_base(root).await;
    write(
        root,
        "model/enums/regions.toml",
        "schema_version = 1\ntype = \"string\"\n",
    )
    .await;
    write(
        root,
        "data/enums/regions.toml",
        "members = [\"us\", \"eu\", \"legacy\"]\n",
    )
    .await;
}

#[tokio::test]
async fn overlay_deletes_enum_members_from_the_base() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_enum_base(&base).await;
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    // The overlay adds a member of its own and deletes a base member in the
    // same file; the two keys compose independently.
    write(
        &overlay,
        "data/enums/regions.toml",
        "members = [\"apac\"]\ndeleted = [\"legacy\"]\n",
    )
    .await;
    write(
        &overlay,
        "variables/home_region.toml",
        r#"schema_version = 1
type = "enum:regions"

[resolve]
default = "apac"
"#,
    )
    .await;

    Package::load(overlay.to_string_lossy()).await.unwrap();

    // The deleted member is really gone: a variable naming it fails lint on
    // the flattened package, the same failure a never-declared member gets.
    write(
        &overlay,
        "variables/home_region.toml",
        r#"schema_version = 1
type = "enum:regions"

[resolve]
default = "legacy"
"#,
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string().contains("lint failed"),
        "loading should fail lint because the legacy member is deleted: {err}"
    );

    // Inspecting without the lint gate shows the member really left the set.
    let staged = Package::inspect(overlay.to_string_lossy()).await.unwrap();
    let flattened = tokio::fs::read_to_string(staged.root().join("data/enums/regions.toml"))
        .await
        .unwrap();
    assert!(
        !flattened.contains("legacy") && flattened.contains("apac"),
        "unexpected flattened member file: {flattened}"
    );
    assert!(
        !flattened.contains("deleted"),
        "the deleted key must not land in the flattened package: {flattened}"
    );
}

#[tokio::test]
async fn orphan_enum_member_deletes_fail_loudly() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_enum_base(&base).await;
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    // Deleting a member no base package provides means the author is confused
    // about the base; fail the load instead of ignoring it.
    write(
        &overlay,
        "data/enums/regions.toml",
        "deleted = [\"atlantis\"]\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("deleted enum member is not in the base packages"),
        "unexpected error: {err}"
    );

    // A delete pointed at an enum data file no base package has at all fails
    // the same way.
    tokio::fs::remove_file(overlay.join("data/enums/regions.toml"))
        .await
        .unwrap();
    write(
        &overlay,
        "data/enums/channels.toml",
        "deleted = [\"email\"]\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("deleted enum members have no member set to remove"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn same_layer_enum_member_add_and_delete_conflict() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_enum_base(&base).await;
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(
        &overlay,
        "data/enums/regions.toml",
        "members = [\"apac\"]\ndeleted = [\"apac\"]\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("package both adds enum member \"apac\" and deletes it"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn deleting_every_enum_member_fails_the_load() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_enum_base(&base).await;
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(
        &overlay,
        "data/enums/regions.toml",
        "deleted = [\"us\", \"eu\", \"legacy\"]\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string().contains("leaves the enum with no members"),
        "unexpected error: {err}"
    );
}

/// A small standalone base with one bool variable, for multi-base extends
/// tests.
async fn write_named_base(root: &Path, variable: &str) {
    // Deny-by-default is unconditional; this base opens itself to its own
    // overlays with a broad [defaults] grant.
    write(
        root,
        "governance.toml",
        "[defaults]\nallowed_operations = [\"add\", \"update\", \"delete\"]\n",
    )
    .await;

    write(root, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        root,
        &format!("variables/{variable}.toml"),
        r#"schema_version = 1
type = "bool"

[resolve]
default = true
"#,
    )
    .await;
}

#[tokio::test]
async fn extends_composes_disjoint_sibling_bases() {
    let temp = tempfile::TempDir::new().unwrap();
    let billing = temp.path().join("billing");
    let regional = temp.path().join("regional");
    let app = temp.path().join("app");
    write_named_base(&billing, "billing_enabled").await;
    write_named_base(&regional, "regional_enabled").await;
    write(
        &app,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../billing\", \"../regional\"]\n",
    )
    .await;

    let package = Package::load(app.to_string_lossy()).await.unwrap();
    let context = EvaluationContext::from_json(serde_json::json!({})).unwrap();
    assert_eq!(
        package
            .resolve_variable("billing_enabled", &context)
            .unwrap()
            .value,
        true
    );
    assert_eq!(
        package
            .resolve_variable("regional_enabled", &context)
            .unwrap()
            .value,
        true
    );
}

#[tokio::test]
async fn sibling_bases_conflict_on_the_same_file() {
    let temp = tempfile::TempDir::new().unwrap();
    let one = temp.path().join("one");
    let two = temp.path().join("two");
    let app = temp.path().join("app");
    write_named_base(&one, "shared").await;
    write_named_base(&two, "shared").await;
    // Same variable id, different default: neither base was authored as an
    // overlay of the other, so this must not silently merge.
    write(
        &two,
        "variables/shared.toml",
        r#"schema_version = 1
type = "bool"

[resolve]
default = false
"#,
    )
    .await;
    write(
        &app,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../one\", \"../two\"]\n",
    )
    .await;

    let err = Package::load(app.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string().contains("extends bases conflict on"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn diamond_ancestry_composes_the_shared_base_once() {
    let temp = tempfile::TempDir::new().unwrap();
    let core = temp.path().join("core");
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    let app = temp.path().join("app");
    write_named_base(&core, "core_enabled").await;
    // The shared ancestor carries governance; identical restatements of its
    // files through both branches must not read as governed updates.
    write(
        &core,
        "governance.toml",
        "[variable.core_enabled]\nallowed_operations = []\n",
    )
    .await;
    write(
        &left,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../core\"]\n",
    )
    .await;
    write(
        &left,
        "variables/left_enabled.toml",
        "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = true\n",
    )
    .await;
    write(
        &right,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../core\"]\n",
    )
    .await;
    write(
        &right,
        "variables/right_enabled.toml",
        "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = true\n",
    )
    .await;
    write(
        &app,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../left\", \"../right\"]\n",
    )
    .await;

    let package = Package::load(app.to_string_lossy()).await.unwrap();
    let context = EvaluationContext::from_json(serde_json::json!({})).unwrap();
    for id in ["core_enabled", "left_enabled", "right_enabled"] {
        assert_eq!(
            package.resolve_variable(id, &context).unwrap().value,
            true,
            "{id}"
        );
    }
}

#[tokio::test]
async fn sibling_bases_add_disjoint_entries_to_a_shared_catalog() {
    let temp = tempfile::TempDir::new().unwrap();
    let core = temp.path().join("core");
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    let app = temp.path().join("app");
    write_base(&core).await;
    // Two siblings each extend the shared ancestor and add their own plan
    // entry. The catalog is shared additively: distinct entries compose,
    // and the schema rides through both branches unchanged.
    write(
        &left,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../core\"]\n",
    )
    .await;
    write(
        &left,
        "data/catalogs/plans/team.toml",
        "name = \"Team\"\nmonthly_price = 99\n",
    )
    .await;
    write(
        &right,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../core\"]\n",
    )
    .await;
    write(
        &right,
        "data/catalogs/plans/scale.toml",
        "name = \"Scale\"\nmonthly_price = 299\n",
    )
    .await;
    write(
        &app,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../left\", \"../right\"]\n",
    )
    .await;

    let package = Package::load(app.to_string_lossy()).await.unwrap();
    for entry in ["free", "growth", "team", "scale"] {
        assert!(
            package
                .root()
                .join(format!("data/catalogs/plans/{entry}.toml"))
                .is_file(),
            "{entry}"
        );
    }
}

#[tokio::test]
async fn sibling_bases_conflict_on_the_same_catalog_entry() {
    let temp = tempfile::TempDir::new().unwrap();
    let core = temp.path().join("core");
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    let app = temp.path().join("app");
    write_base(&core).await;
    for (root, price) in [(&left, "99"), (&right, "89")] {
        write(
            root,
            "rototo-package.toml",
            "schema_version = 1\nextends = [\"../core\"]\n",
        )
        .await;
        write(
            root,
            "data/catalogs/plans/team.toml",
            &format!("name = \"Team\"\nmonthly_price = {price}\n"),
        )
        .await;
    }
    write(
        &app,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../left\", \"../right\"]\n",
    )
    .await;

    let err = Package::load(app.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("extends bases conflict on catalog plans entry team"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn sibling_base_may_not_touch_another_siblings_catalog() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let rogue = temp.path().join("rogue");
    let app = temp.path().join("app");
    write_base(&base).await;
    // A package with no extends of its own carrying a raw deleted marker: as
    // a sibling base it would reach across and remove another base's entry,
    // so it conflicts on that entry even though catalogs share additively.
    write(&rogue, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        &rogue,
        "data/catalogs/plans/free.deleted.toml",
        "deleted = true\n",
    )
    .await;
    write(
        &app,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\", \"../rogue\"]\n",
    )
    .await;

    let err = Package::load(app.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("extends bases conflict on catalog plans entry free"),
        "unexpected error: {err}"
    );
}
