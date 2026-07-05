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

    // The same rule holds one namespace down: the id is the relative path,
    // and the error points at the marker next to the nested file.
    let base2 = temp.path().join("base2");
    let overlay2 = temp.path().join("overlay2");
    write(&base2, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        &base2,
        "variables/acme/flag.toml",
        "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = false\n",
    )
    .await;
    write(
        &overlay2,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base2\"]\n",
    )
    .await;
    write(
        &overlay2,
        "variables/acme/flag.toml",
        "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = true\n",
    )
    .await;
    let err = Package::load(overlay2.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string().contains(
            "variable acme/flag is declared in the base packages; update it with \
             variables/acme/flag.update.toml"
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

#[tokio::test]
async fn directories_namespace_every_collection() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    // A base whose enum and catalog live under namespace directories: the
    // ids are acme/tier and acme/plans.
    write(&base, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        &base,
        "governance.toml",
        "[defaults]\nallowed_operations = [\"add\", \"update\", \"delete\"]\n",
    )
    .await;
    write(
        &base,
        "model/enums/acme/tier.toml",
        "schema_version = 1\ntype = \"string\"\n",
    )
    .await;
    write(
        &base,
        "data/enums/acme/tier.toml",
        "members = [\"standard\", \"premium\"]\n",
    )
    .await;
    write(
        &base,
        "model/catalogs/acme/plans.schema.json",
        r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "name": { "type": "string" },
    "tier": { "type": "string", "x-rototo-ref": "enum:acme/tier" }
  },
  "required": ["name", "tier"],
  "additionalProperties": false
}
"#,
    )
    .await;
    write(
        &base,
        "data/catalogs/acme/plans/basic.toml",
        "name = \"Basic\"\ntier = \"standard\"\n",
    )
    .await;
    write(
        &base,
        "variables/plan.toml",
        "schema_version = 1\ntype = \"catalog:acme/plans\"\n\n[resolve]\ndefault = \"basic\"\n",
    )
    .await;

    // The base stands on its own with namespaced ids throughout.
    let package = Package::load(base.to_string_lossy()).await.unwrap();
    let context = EvaluationContext::from_json(serde_json::json!({})).unwrap();
    let resolution = package.resolve_variable("plan", &context).unwrap();
    assert_eq!(resolution.value["name"], "Basic");

    // An overlay updates the namespaced catalog's entry through the marker
    // and unions a member into the namespaced enum.
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base\"]\n",
    )
    .await;
    write(
        &overlay,
        "data/catalogs/acme/plans/basic.update.toml",
        "name = \"Basic Plus\"\n",
    )
    .await;
    write(
        &overlay,
        "data/enums/acme/tier.toml",
        "members = [\"enterprise\"]\n",
    )
    .await;

    let package = Package::load(overlay.to_string_lossy()).await.unwrap();
    let resolution = package.resolve_variable("plan", &context).unwrap();
    assert_eq!(resolution.value["name"], "Basic Plus");
    let members = tokio::fs::read_to_string(package.root().join("data/enums/acme/tier.toml"))
        .await
        .unwrap();
    assert!(members.contains("enterprise"), "{members}");
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

// --- governance kind sweep -------------------------------------------------

async fn write_contract_base(root: &Path) {
    write(root, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        root,
        "model/enums/tier.toml",
        "schema_version = 1\ntype = \"string\"\n",
    )
    .await;
    write(root, "data/enums/tier.toml", "members = [\"standard\"]\n").await;
    write(
        root,
        "model/context/request.schema.json",
        r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": true,
  "properties": { "region": { "type": "string" } }
}
"#,
    )
    .await;
    write(
        root,
        "model/context/request-samples/eu.json",
        "{ \"region\": \"eu\" }\n",
    )
    .await;
}

fn extends_manifest() -> &'static str {
    "schema_version = 1\nextends = [\"../base\"]\n"
}

#[tokio::test]
async fn governed_model_files_are_never_editable() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    write_contract_base(&base).await;

    // G9: a base enum declaration.
    let overlay = temp.path().join("overlay-enum");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "model/enums/tier.toml",
        "schema_version = 1\ntype = \"int\"\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance does not allow an overlay to change a base enum declaration"),
        "{err}"
    );

    // G10: a base evaluation context schema.
    let overlay = temp.path().join("overlay-context");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(&overlay, "model/context/request.schema.json", "{}\n").await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string().contains(
            "governance does not allow an overlay to change a base evaluation context schema"
        ),
        "{err}"
    );
}

#[tokio::test]
async fn governed_samples_reject_edits_but_admit_additions() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    write_contract_base(&base).await;

    // G11a: restating a base sample with different content.
    let overlay = temp.path().join("overlay-edit");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "model/context/request-samples/eu.json",
        "{ \"region\": \"eu-west\" }\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("change a base sample for evaluation context request"),
        "{err}"
    );

    // G11b: a new sample file needs no grant at all.
    let overlay = temp.path().join("overlay-add");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "model/context/request-samples/us.json",
        "{ \"region\": \"us\" }\n",
    )
    .await;
    Package::load(overlay.to_string_lossy()).await.unwrap();
}

#[tokio::test]
async fn governed_enum_members_check_update_and_add() {
    let temp = tempfile::TempDir::new().unwrap();

    // G12: the base has a member set; providing another is an update.
    let base = temp.path().join("base");
    write_contract_base(&base).await;
    let overlay = temp.path().join("overlay-update");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(&overlay, "data/enums/tier.toml", "members = [\"gold\"]\n").await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies update on enum.tier"),
        "{err}"
    );
    write(
        &base,
        "governance.toml",
        "[enum.tier]\nallowed_operations = [\"update\"]\n",
    )
    .await;
    Package::load(overlay.to_string_lossy()).await.unwrap();

    // G13: the base declares the enum with no member file; members are an add.
    let base2 = temp.path().join("base2");
    write(&base2, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        &base2,
        "model/enums/size.toml",
        "schema_version = 1\ntype = \"string\"\n",
    )
    .await;
    let overlay = temp.path().join("overlay-add");
    write(
        &overlay,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../base2\"]\n",
    )
    .await;
    write(
        &overlay,
        "data/enums/size.toml",
        "members = [\"s\", \"m\"]\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies add on enum.size"),
        "{err}"
    );
    write(
        &base2,
        "governance.toml",
        "[enum.size]\nallowed_operations = [\"add\"]\n",
    )
    .await;
    Package::load(overlay.to_string_lossy()).await.unwrap();
}

#[tokio::test]
async fn governed_namespaced_variable_targets() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    write(&base, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        &base,
        "variables/acme/flag.toml",
        "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = false\n",
    )
    .await;
    let overlay = temp.path().join("overlay");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "variables/acme/flag.update.toml",
        "[resolve]\ndefault = true\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies update on variable.acme/flag"),
        "{err}"
    );

    // The governance target quotes the namespaced id.
    write(
        &base,
        "governance.toml",
        "[variable.\"acme/flag\"]\nallowed_operations = [\"update\"]\n",
    )
    .await;
    Package::load(overlay.to_string_lossy()).await.unwrap();
}

#[tokio::test]
async fn governed_layer_updates_need_the_update_grant() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    write(&base, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        &base,
        "layers/checkout.toml",
        "schema_version = 1\nunit = \"context.user.id\"\nbuckets = 1000\n",
    )
    .await;
    let overlay = temp.path().join("overlay");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    // Changing buckets silently reassigns enrolled units; that is exactly
    // what the update grant gates.
    write(
        &overlay,
        "layers/checkout.toml",
        "schema_version = 1\nunit = \"context.user.id\"\nbuckets = 500\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies update on layer.checkout"),
        "{err}"
    );

    write(
        &base,
        "governance.toml",
        "[layer.checkout]\nallowed_operations = [\"update\"]\n",
    )
    .await;
    Package::load(overlay.to_string_lossy()).await.unwrap();
}

#[tokio::test]
async fn governed_lint_files_cannot_be_replaced() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    write(&base, "rototo-package.toml", "schema_version = 1\n").await;
    write(&base, "lint/checks.lua", "function register(lint)\nend\n").await;
    let overlay = temp.path().join("overlay");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "lint/checks.lua",
        "function register(lint)\n  -- reworded\nend\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance does not model replacing a lint file the base owns"),
        "{err}"
    );
}

#[tokio::test]
async fn overlay_minted_catalogs_are_ungoverned() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    write_base(&base).await;
    write(&base, "governance.toml", PLANS_GOVERNANCE).await;

    // The overlay introduces its own catalog next to the strictly governed
    // base: schema, entry, everything, without any grant.
    let overlay = temp.path().join("overlay");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "model/catalogs/acme_addons.schema.json",
        r#"{
  "type": "object",
  "required": ["name"],
  "properties": { "name": { "type": "string" } },
  "additionalProperties": false
}
"#,
    )
    .await;
    write(
        &overlay,
        "data/catalogs/acme_addons/sso.toml",
        "name = \"SSO\"\n",
    )
    .await;

    Package::load(overlay.to_string_lossy()).await.unwrap();
}

#[tokio::test]
async fn unparseable_overlay_governance_fails_the_load() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    write_base(&base).await;
    let overlay = temp.path().join("overlay");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(&overlay, "governance.toml", "not toml ((\n").await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("failed to parse the overlay governance.toml"),
        "{err}"
    );
}

#[tokio::test]
async fn granted_deletes_and_updates_walk_the_allowed_side() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    write_base(&base).await;
    write(&base, "governance.toml", PLANS_GOVERNANCE).await;

    // G3: deleting growth is inside the delete policy. The base rule that
    // selected growth has to go with it, through the granted variable update.
    let overlay = temp.path().join("overlay-delete");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "data/catalogs/plans/growth.deleted.toml",
        "deleted = true\n",
    )
    .await;
    write(
        &overlay,
        "variables/active_plan.update.toml",
        "[resolve]\ndefault = \"free\"\n",
    )
    .await;
    let package = Package::load(overlay.to_string_lossy()).await.unwrap();
    assert!(
        !package
            .root()
            .join("data/catalogs/plans/growth.toml")
            .is_file()
    );

    // G4: updating a field inside allowed_fields on a permitted entry.
    let overlay = temp.path().join("overlay-update");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "data/catalogs/plans/growth.update.toml",
        "monthly_price = 79\n",
    )
    .await;
    let package = Package::load(overlay.to_string_lossy()).await.unwrap();
    let context = EvaluationContext::from_json(serde_json::json!({
        "account": { "paid": true }
    }))
    .unwrap();
    let resolution = package.resolve_variable("active_plan", &context).unwrap();
    assert_eq!(resolution.value["monthly_price"], 79);

    // G6: the update policy's entry denylist wins over the field allowlist.
    let overlay = temp.path().join("overlay-denied-entry");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "data/catalogs/plans/free.update.toml",
        "monthly_price = 1\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies update of entry free on catalog.plans"),
        "{err}"
    );
}

#[tokio::test]
async fn defaults_grants_yield_to_entity_denies() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    write_base(&base).await;
    write(
        &base,
        "governance.toml",
        "[defaults]\nallowed_operations = [\"add\", \"update\", \"delete\"]\n\n\
         [catalog.plans]\ndenied_operations = [\"delete\"]\n",
    )
    .await;

    // The defaults open update everywhere...
    let overlay = temp.path().join("overlay-update");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "data/catalogs/plans/growth.update.toml",
        "monthly_price = 59\n",
    )
    .await;
    Package::load(overlay.to_string_lossy()).await.unwrap();

    // ...but the entity's own deny wins over the defaults grant.
    let overlay = temp.path().join("overlay-delete");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "data/catalogs/plans/free.deleted.toml",
        "deleted = true\n",
    )
    .await;
    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies delete on catalog.plans"),
        "{err}"
    );
}

#[tokio::test]
async fn defaults_ceiling_is_enforced() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    write_base(&base).await;
    write(&base, "governance.toml", PLANS_GOVERNANCE).await;

    // The base granted per-entity operations, never a default; an overlay
    // handing its own sub-overlays a broad [defaults] exceeds the ceiling.
    let overlay = temp.path().join("overlay");
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "governance.toml",
        "[defaults]\nallowed_operations = [\"add\"]\n",
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance grant exceeds the inherited ceiling: [defaults] allows add"),
        "{err}"
    );
}

// --- sibling symmetry, depth, and merge details ------------------------------

#[tokio::test]
async fn same_layer_update_and_deleted_marker_conflict() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "data/catalogs/plans/growth.update.toml",
        "monthly_price = 59\n",
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
        err.to_string().contains(
            "package both declares an update and a deleted marker for catalog entry growth"
        ),
        "{err}"
    );
}

#[tokio::test]
async fn sibling_base_may_not_update_another_siblings_entry() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let rogue = temp.path().join("rogue");
    let app = temp.path().join("app");
    write_base(&base).await;
    write(&rogue, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        &rogue,
        "data/catalogs/plans/growth.update.toml",
        "monthly_price = 1\n",
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
            .contains("extends bases conflict on catalog plans entry growth"),
        "{err}"
    );
}

#[tokio::test]
async fn sibling_bases_conflict_on_diverging_catalog_schemas() {
    let temp = tempfile::TempDir::new().unwrap();
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    let app = temp.path().join("app");
    write_base(&left).await;
    write_base(&right).await;
    // Same catalog, one field renamed: the schemas diverge while every other
    // file rides through byte-identical.
    let schema = tokio::fs::read_to_string(left.join("model/catalogs/plans.schema.json"))
        .await
        .unwrap();
    tokio::fs::write(
        right.join("model/catalogs/plans.schema.json"),
        schema.replace("monthly_price", "price_per_month"),
    )
    .await
    .unwrap();
    write(
        &app,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../left\", \"../right\"]\n",
    )
    .await;

    let err = Package::load(app.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("extends bases conflict on catalog plans schema"),
        "{err}"
    );
}

#[tokio::test]
async fn sibling_bases_conflict_on_the_same_layer_id() {
    let temp = tempfile::TempDir::new().unwrap();
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    let app = temp.path().join("app");
    for (root, buckets) in [(&left, "1000"), (&right, "500")] {
        write(root, "rototo-package.toml", "schema_version = 1\n").await;
        write(
            root,
            "layers/checkout.toml",
            &format!("schema_version = 1\nunit = \"context.user.id\"\nbuckets = {buckets}\n"),
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
            .contains("extends bases conflict on layer checkout"),
        "{err}"
    );
}

#[tokio::test]
async fn sibling_bases_conflict_on_the_same_lint_file() {
    let temp = tempfile::TempDir::new().unwrap();
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    let app = temp.path().join("app");
    for (root, body) in [(&left, "-- left\n"), (&right, "-- right\n")] {
        write(root, "rototo-package.toml", "schema_version = 1\n").await;
        write(
            root,
            "lint/checks.lua",
            &format!("function register(lint)\n{body}end\n"),
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
            .contains("extends bases conflict on file lint/checks.lua"),
        "{err}"
    );
}

#[tokio::test]
async fn sibling_enum_declaration_and_members_conflict() {
    // One base declares the enum, the other provides its members. The two
    // halves of one enum belong to one owner: siblings may not split an
    // entity between them, so this is a conflict, deliberately.
    let temp = tempfile::TempDir::new().unwrap();
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    let app = temp.path().join("app");
    write(&left, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        &left,
        "model/enums/tier.toml",
        "schema_version = 1\ntype = \"string\"\n",
    )
    .await;
    write(&right, "rototo-package.toml", "schema_version = 1\n").await;
    write(&right, "data/enums/tier.toml", "members = [\"gold\"]\n").await;
    write(
        &app,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../left\", \"../right\"]\n",
    )
    .await;

    let err = Package::load(app.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("extends bases conflict on enum tier"),
        "{err}"
    );
}

#[tokio::test]
async fn sibling_bases_add_disjoint_samples_to_a_shared_context() {
    let temp = tempfile::TempDir::new().unwrap();
    let core = temp.path().join("core");
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    let app = temp.path().join("app");
    write_contract_base(&core).await;
    // Each sibling adds its own sample; the shared schema and the core's own
    // sample ride through byte-identical.
    write(
        &left,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../core\"]\n",
    )
    .await;
    write(
        &left,
        "model/context/request-samples/left.json",
        "{ \"region\": \"left\" }\n",
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
        "model/context/request-samples/right.json",
        "{ \"region\": \"right\" }\n",
    )
    .await;
    write(
        &app,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../left\", \"../right\"]\n",
    )
    .await;

    let package = Package::load(app.to_string_lossy()).await.unwrap();
    for sample in ["eu", "left", "right"] {
        assert!(
            package
                .root()
                .join(format!("model/context/request-samples/{sample}.json"))
                .is_file(),
            "{sample}"
        );
    }

    // The same sample id with different content still conflicts.
    write(
        &right,
        "model/context/request-samples/left.json",
        "{ \"region\": \"other\" }\n",
    )
    .await;
    let err = Package::load(app.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("extends bases conflict on evaluation context request sample left.json"),
        "{err}"
    );
}

#[tokio::test]
async fn a_three_deep_chain_composes_bottom_up() {
    let temp = tempfile::TempDir::new().unwrap();
    let core = temp.path().join("core");
    let mid = temp.path().join("mid");
    let app = temp.path().join("app");
    write_base(&core).await;

    // The middle package updates the entry and the variable; the app updates
    // the entry again on top of the middle's result.
    write(
        &mid,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../core\"]\n",
    )
    .await;
    write(
        &mid,
        "data/catalogs/plans/growth.update.toml",
        "monthly_price = 59\n\n[limits]\nseats = 25\n",
    )
    .await;
    write(
        &mid,
        "variables/active_plan.update.toml",
        "[resolve]\ndefault = \"growth\"\n",
    )
    .await;
    write(
        &app,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../mid\"]\n",
    )
    .await;
    write(
        &app,
        "data/catalogs/plans/growth.update.toml",
        "monthly_price = 99\n",
    )
    .await;

    let package = Package::load(app.to_string_lossy()).await.unwrap();
    let context = EvaluationContext::from_json(serde_json::json!({})).unwrap();
    let resolution = package.resolve_variable("active_plan", &context).unwrap();
    // C9/D1: the app's update landed on the middle's result - its own field
    // won, the middle's nested limits update survived, the core's name rode
    // through.
    assert_eq!(resolution.value["monthly_price"], 99);
    assert_eq!(resolution.value["limits"]["seats"], 25);
    assert_eq!(resolution.value["name"], "Growth");

    // D2: the resolve provenance survives the second flatten - the middle
    // package's [resolve] won, and the trace says so even when the middle
    // arrived at the app pre-flattened.
    let staged = Package::inspect(app.to_string_lossy()).await.unwrap();
    let trace =
        rototo::trace_variable_resolution(staged.root(), "active_plan", &serde_json::json!({}))
            .await
            .unwrap();
    let provenance = trace.provenance.expect("composed package has provenance");
    assert!(provenance.contains("mid"), "{provenance}");
}

#[tokio::test]
async fn governance_binds_through_a_three_deep_chain() {
    let temp = tempfile::TempDir::new().unwrap();
    let core = temp.path().join("core");
    let mid = temp.path().join("mid");
    let app = temp.path().join("app");
    // The core grants nothing beyond adds on its catalog; the middle package
    // passes through untouched; the app two levels up is still bound.
    write_base(&core).await;
    write(
        &core,
        "governance.toml",
        "[catalog.plans]\nallowed_operations = [\"add\"]\n",
    )
    .await;
    write(
        &mid,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../core\"]\n",
    )
    .await;
    write(
        &mid,
        "data/catalogs/plans/team.toml",
        "name = \"Team\"\nmonthly_price = 99\n",
    )
    .await;
    write(
        &app,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../mid\"]\n",
    )
    .await;
    write(
        &app,
        "variables/active_plan.update.toml",
        "[resolve]\ndefault = \"team\"\n",
    )
    .await;

    let err = Package::load(app.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies update on variable.active_plan"),
        "{err}"
    );
}

#[tokio::test]
async fn catalog_update_replaces_arrays_wholesale() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write(&base, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        &base,
        "governance.toml",
        "[defaults]\nallowed_operations = [\"add\", \"update\", \"delete\"]\n",
    )
    .await;
    write(
        &base,
        "model/catalogs/gates.schema.json",
        r#"{
  "type": "object",
  "required": ["name", "regions"],
  "properties": {
    "name": { "type": "string" },
    "regions": { "type": "array", "items": { "type": "string" } }
  },
  "additionalProperties": false
}
"#,
    )
    .await;
    write(
        &base,
        "data/catalogs/gates/rollout.toml",
        "name = \"Rollout\"\nregions = [\"eu\", \"us\"]\n",
    )
    .await;
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    write(
        &overlay,
        "data/catalogs/gates/rollout.update.toml",
        "regions = [\"apac\"]\n",
    )
    .await;

    let package = Package::load(overlay.to_string_lossy()).await.unwrap();
    let entry = tokio::fs::read_to_string(package.root().join("data/catalogs/gates/rollout.toml"))
        .await
        .unwrap();
    // No concatenation: the overlay's array replaced the base's whole.
    assert!(entry.contains("apac"), "{entry}");
    assert!(!entry.contains("eu"), "{entry}");
    assert!(entry.contains("Rollout"), "{entry}");
}

#[tokio::test]
async fn overlay_lint_rules_run_against_the_composed_package() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let overlay = temp.path().join("overlay");
    write_base(&base).await;
    write(&overlay, "rototo-package.toml", extends_manifest()).await;
    // The overlay's own rule judges a base-provided entry.
    write(
        &overlay,
        "lint/pricing.lua",
        r#"function register(lint)
  lint:rule({
    id = "acme/no-free-plans",
    title = "Free plans are not offered",
    help = "Every plan must carry a price.",
    target = "catalog=plans:entry=",
    handler = "check_price",
  })
end

function check_price(package, entry)
  if entry.value.monthly_price == 0 then
    return {
      { message = "plan " .. entry.key .. " has no price" }
    }
  end
  return {}
end
"#,
    )
    .await;

    let err = Package::load(overlay.to_string_lossy()).await.unwrap_err();
    assert!(err.to_string().contains("lint failed"), "{err}");
    let staged = Package::inspect(overlay.to_string_lossy()).await.unwrap();
    let lint = staged.lint().await.unwrap();
    assert!(
        lint.diagnostics.iter().any(|diagnostic| {
            diagnostic.rule.as_string() == "acme/no-free-plans"
                && diagnostic.message.contains("free")
        }),
        "{:#?}",
        lint.diagnostics
    );
}

/// Two packages extending each other must fail the load with the cycle
/// spelled out, not recurse forever.
#[tokio::test]
async fn extends_cycles_fail_the_load() {
    let tempdir = tempfile::tempdir().unwrap();
    let left = tempdir.path().join("left");
    let right = tempdir.path().join("right");
    write(
        &left,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../right\"]\n",
    )
    .await;
    write(
        &right,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\"../left\"]\n",
    )
    .await;

    let err = Package::load(left.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string().contains("package extends cycle detected"),
        "unexpected error: {err}"
    );
}

/// A package extending itself is the smallest cycle.
#[tokio::test]
async fn a_package_extending_itself_fails_the_load() {
    let tempdir = tempfile::tempdir().unwrap();
    let root = tempdir.path().join("selfish");
    write(
        &root,
        "rototo-package.toml",
        "schema_version = 1\nextends = [\".\"]\n",
    )
    .await;

    let err = Package::load(root.to_string_lossy()).await.unwrap_err();
    assert!(
        err.to_string().contains("package extends cycle detected"),
        "unexpected error: {err}"
    );
}

/// Chains deeper than 32 fail with the depth named, so a runaway graph is an
/// error instead of unbounded staging work.
#[tokio::test]
async fn extends_chains_deeper_than_the_limit_fail_the_load() {
    let tempdir = tempfile::tempdir().unwrap();
    // Package 0 is the root; package i extends package i+1; the last one is
    // a plain base. 33 packages exceed the depth limit of 32.
    let count = 34;
    for i in 0..count {
        let root = tempdir.path().join(format!("pkg{i}"));
        let manifest = if i + 1 < count {
            format!("schema_version = 1\nextends = [\"../pkg{}\"]\n", i + 1)
        } else {
            "schema_version = 1\n".to_owned()
        };
        write(&root, "rototo-package.toml", &manifest).await;
    }

    let err = Package::load(tempdir.path().join("pkg0").to_string_lossy())
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("package extends depth exceeded 32"),
        "unexpected error: {err}"
    );
}

async fn run_git(repo: &Path, args: &[&str]) {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn commit_git_package(repo: &Path) {
    run_git(repo, &["init"]).await;
    run_git(repo, &["config", "user.email", "rototo@example.com"]).await;
    run_git(repo, &["config", "user.name", "Rototo Tests"]).await;
    run_git(repo, &["add", "."]).await;
    run_git(repo, &["commit", "-m", "initial"]).await;
}

/// A minimal base package granting its overlays broad rights, with one
/// string variable the overlay can see.
async fn write_remote_base(root: &Path) {
    write(root, "rototo-package.toml", "schema_version = 1\n").await;
    write(
        root,
        "governance.toml",
        "[defaults]\nallowed_operations = [\"add\", \"update\", \"delete\"]\n",
    )
    .await;
    write(
        root,
        "variables/greeting.toml",
        "schema_version = 1\ntype = \"string\"\n\n[resolve]\ndefault = \"hello\"\n",
    )
    .await;
}

/// A local package can extend a git source: the extends list takes the same
/// source grammar as a package argument.
#[tokio::test]
async fn a_local_package_can_extend_a_git_source() {
    let tempdir = tempfile::tempdir().unwrap();
    let base_repo = tempdir.path().join("base-repo");
    write_remote_base(&base_repo).await;
    commit_git_package(&base_repo).await;

    let overlay = tempdir.path().join("overlay");
    write(
        &overlay,
        "rototo-package.toml",
        &format!(
            "schema_version = 1\nextends = [\"git+file://{}\"]\n",
            base_repo.display()
        ),
    )
    .await;

    let package = Package::load(overlay.to_string_lossy()).await.unwrap();
    let context = EvaluationContext::from_json(serde_json::json!({})).unwrap();
    let value = package.resolve_variable("greeting", &context).unwrap();
    assert_eq!(value.value.as_str(), Some("hello"));
}

/// A staged (fetched) package may not extend `git+file://`: from a remote
/// package that is a read of the loading machine's disk, exactly like
/// `file://`. Truly remote parents (`git+https://`, `git+ssh://`, `https://`)
/// pass through; the local-filesystem schemes are the escape.
#[tokio::test]
async fn a_staged_package_may_not_extend_a_local_git_source() {
    let tempdir = tempfile::tempdir().unwrap();
    let base_repo = tempdir.path().join("base-repo");
    write_remote_base(&base_repo).await;
    commit_git_package(&base_repo).await;

    let overlay_repo = tempdir.path().join("overlay-repo");
    write(
        &overlay_repo,
        "rototo-package.toml",
        &format!(
            "schema_version = 1\nextends = [\"git+file://{}\"]\n",
            base_repo.display()
        ),
    )
    .await;
    commit_git_package(&overlay_repo).await;

    let err = Package::load(format!("git+file://{}", overlay_repo.display()))
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("package extends source escapes a staged package"),
        "unexpected error: {err}"
    );
}
