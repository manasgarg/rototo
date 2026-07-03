//! Resolution expectations for the graduated demonstration packages under
//! `examples/`. Each package's README describes the scenario these tests pin
//! down: representative variables resolved through the public SDK with the
//! contexts the scenario cares about, plus the governance denials the
//! governed packages promise.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rototo::{EvaluationContext, Package};
use serde_json::json;

async fn load(package: &str) -> Package {
    let path = std::path::absolute(package).unwrap();
    Package::load(path.to_string_lossy()).await.unwrap()
}

fn context(value: serde_json::Value) -> EvaluationContext {
    EvaluationContext::from_json(value).unwrap()
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.unwrap();
    }
    tokio::fs::write(path, contents).await.unwrap();
}

/// A minimal overlay extending one of the example packages, used to check
/// that the base's governance actually bites.
async fn overlay_extending(base: &str) -> tempfile::TempDir {
    let base = std::path::absolute(base).unwrap();
    let temp = tempfile::TempDir::new().unwrap();
    write(
        temp.path(),
        "rototo-package.toml",
        &format!(
            "schema_version = 1\nname = \"overlay\"\nextends = [\"{}\"]\n",
            base.display()
        ),
    )
    .await;
    temp
}

// --- release-ops -----------------------------------------------------------

#[tokio::test]
async fn release_ops_resolves_ring_and_ops_variables_for_an_employee() {
    let package = load("examples/release-ops").await;
    let context = context(json!({
        "user": { "id": "u_1041", "is_employee": true, "beta_opt_in": false },
        "region": "us"
    }));

    let preview = package
        .resolve_variable("enable_realtime_preview", &context)
        .unwrap();
    assert_eq!(preview.value, json!(true));

    let log_level = package.resolve_variable("log_level", &context).unwrap();
    assert_eq!(log_level.value, json!("debug"));

    // The deprecation gate peels employees off the old export path first.
    let legacy = package
        .resolve_variable("enable_legacy_export", &context)
        .unwrap();
    assert_eq!(legacy.value, json!(false));
}

#[tokio::test]
async fn release_ops_resolves_customer_defaults_and_knobs() {
    let package = load("examples/release-ops").await;
    let context = context(json!({
        "user": { "id": "u_88213", "is_employee": false, "beta_opt_in": false },
        "region": "apac"
    }));

    let preview = package
        .resolve_variable("enable_realtime_preview", &context)
        .unwrap();
    assert_eq!(preview.value, json!(false));

    // Far regions save less often.
    let autosave = package
        .resolve_variable("autosave_interval_ms", &context)
        .unwrap();
    assert_eq!(autosave.value, json!(5000));

    // Pure time gate: false until the announcement instant, no context read.
    let v3_epoch = 1_789_480_800; // 2026-09-15T14:00:00Z
    let branding = package
        .resolve_variable("enable_v3_branding", &context)
        .unwrap();
    assert_eq!(branding.value, json!(now_epoch() >= v3_epoch));
}

#[tokio::test]
async fn release_ops_layer_keeps_rollout_and_experiment_mutually_exclusive() {
    let package = load("examples/release-ops").await;

    // u_1 hashes into the rollout's buckets (0-19): the new editor is on and
    // the toolbar experiment leaves the user unenrolled, on the baseline.
    let in_rollout = context(json!({ "user": { "id": "u_1" }, "region": "us" }));
    let editor = package
        .resolve_variable("enable_new_editor", &in_rollout)
        .unwrap();
    assert_eq!(editor.value, json!(true));
    let toolbar = package
        .resolve_variable("toolbar_layout", &in_rollout)
        .unwrap();
    assert_eq!(toolbar.value, json!("classic"));

    // u_2 hashes into the experiment's compact arm (55-89): old editor.
    let in_experiment = context(json!({ "user": { "id": "u_2" }, "region": "us" }));
    let editor = package
        .resolve_variable("enable_new_editor", &in_experiment)
        .unwrap();
    assert_eq!(editor.value, json!(false));
    let toolbar = package
        .resolve_variable("toolbar_layout", &in_experiment)
        .unwrap();
    assert_eq!(toolbar.value, json!("compact"));

    // Deterministic assignment: the same unit gets the same arm every time.
    let again = package
        .resolve_variable("toolbar_layout", &in_experiment)
        .unwrap();
    assert_eq!(again.value, json!("compact"));
}

// --- billing ----------------------------------------------------------------

#[tokio::test]
async fn billing_resolves_the_in_force_price_per_tier_and_currency() {
    let package = load("examples/billing").await;

    let team_eur = context(json!({ "account": { "plan_tier": "team", "currency": "eur" } }));
    let price = package.resolve_variable("active_price", &team_eur).unwrap();
    assert_eq!(price.value["id"], json!("team_eur_2025"));
    assert_eq!(price.value["monthly_amount"], json!(27));

    // (team, usd) has a future-dated increase: the newest in-force entry wins,
    // and the flip at 2026-10-01 needs no edit to the package.
    let increase_epoch = 1_790_812_800; // 2026-10-01T00:00:00Z
    let expected = if now_epoch() >= increase_epoch {
        "team_usd_2026_10"
    } else {
        "team_usd_2025"
    };
    let team_usd = context(json!({ "account": { "plan_tier": "team", "currency": "usd" } }));
    let price = package.resolve_variable("active_price", &team_usd).unwrap();
    assert_eq!(price.value["id"], json!(expected));
}

#[tokio::test]
async fn billing_resolves_the_plan_with_hydrated_entitlements() {
    let package = load("examples/billing").await;

    let business = context(json!({ "account": { "plan_tier": "business", "currency": "usd" } }));
    let plan = package.resolve_variable("active_plan", &business).unwrap();
    let features = plan.value["features"].as_array().unwrap();
    assert!(
        features.iter().any(|feature| feature["id"] == json!("sso")),
        "business entitlements should include sso: {features:?}"
    );

    let free = context(json!({ "account": { "plan_tier": "free", "currency": "usd" } }));
    let plan = package.resolve_variable("active_plan", &free).unwrap();
    assert_eq!(plan.value["seat_limit"], json!(3));
    let features = plan.value["features"].as_array().unwrap();
    assert_eq!(features.len(), 1);
    assert_eq!(features[0]["id"], json!("api_access"));

    let quota = package
        .resolve_variable("api_rate_limit_per_min", &business)
        .unwrap();
    assert_eq!(quota.value, json!(6000));
}

#[tokio::test]
async fn billing_governance_locks_pricing_but_grants_the_rate_limit_override() {
    // Minting a price from an overlay is denied: pricing is append-only and
    // owned by the base team.
    let overlay = overlay_extending("examples/billing").await;
    write(
        overlay.path(),
        "data/catalogs/prices/team_usd_cheap.toml",
        "plan_tier = \"team\"\ncurrency = \"usd\"\nmonthly_amount = 1\neffective_from = \"2025-01-01T00:00:00Z\"\n",
    )
    .await;
    let err = Package::load(overlay.path().to_string_lossy())
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies add on catalog.prices"),
        "unexpected error: {err}"
    );

    // The one granted door: a negotiated contract overrides the rate limit.
    let overlay = overlay_extending("examples/billing").await;
    write(
        overlay.path(),
        "variables/api_rate_limit_per_min.toml",
        "[resolve]\nmethod = \"rules\"\ndefault = 20000\n",
    )
    .await;
    let package = Package::load(overlay.path().to_string_lossy())
        .await
        .unwrap();
    let context = context(json!({ "account": { "plan_tier": "team", "currency": "usd" } }));
    let quota = package
        .resolve_variable("api_rate_limit_per_min", &context)
        .unwrap();
    assert_eq!(quota.value, json!(20000));
}

// --- tenancy-decisioning ----------------------------------------------------

#[tokio::test]
async fn tenancy_base_ranks_banners_by_audience_and_priority() {
    let package = load("examples/tenancy-decisioning/base").await;

    let first_visit = context(json!({
        "visitor": { "id": "v1", "visits": 1, "lifetime_spend": 0 }
    }));
    let banner = package
        .resolve_variable("homepage_banner", &first_visit)
        .unwrap();
    assert_eq!(banner.value["id"], json!("welcome"));

    let high_value = context(json!({
        "visitor": { "id": "v2", "visits": 9, "lifetime_spend": 900 }
    }));
    let banner = package
        .resolve_variable("homepage_banner", &high_value)
        .unwrap();
    assert_eq!(banner.value["id"], json!("loyalty_thanks"));

    // No audience matches and no campaign is live: the default floor.
    let plain = context(json!({
        "visitor": { "id": "v5", "visits": 5, "lifetime_spend": 10 }
    }));
    let banner = package.resolve_variable("homepage_banner", &plain).unwrap();
    assert_eq!(banner.value["id"], json!("default_banner"));
}

#[tokio::test]
async fn tenancy_overlay_composes_add_patch_and_tombstone() {
    let package = load("examples/tenancy-decisioning/acme-tenant").await;

    // Acme's added banner wins for its returning-visitor audience.
    let returning = context(json!({
        "visitor": { "id": "v3", "visits": 5, "lifetime_spend": 0 }
    }));
    let banner = package
        .resolve_variable("homepage_banner", &returning)
        .unwrap();
    assert_eq!(banner.value["id"], json!("acme_flash_sale"));

    // The base welcome banner still fires for new visitors, re-worded by
    // Acme's patch; only the patched fields changed.
    let first_visit = context(json!({
        "visitor": { "id": "v4", "visits": 1, "lifetime_spend": 0 }
    }));
    let banner = package
        .resolve_variable("homepage_banner", &first_visit)
        .unwrap();
    assert_eq!(banner.value["id"], json!("welcome"));
    assert_eq!(
        banner.value["headline"],
        json!("Welcome to Acme. See what our customers build.")
    );
    assert_eq!(banner.value["cta_url"], json!("/acme/showcase"));
}

#[tokio::test]
async fn tenancy_base_governance_scopes_updates_and_protects_the_floor() {
    // Patching a field outside the base's update_policy is denied.
    let overlay = overlay_extending("examples/tenancy-decisioning/base").await;
    write(
        overlay.path(),
        "data/catalogs/banners/welcome.patch.toml",
        "priority = 99\n",
    )
    .await;
    let err = Package::load(overlay.path().to_string_lossy())
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies update of field priority on catalog.banners"),
        "unexpected error: {err}"
    );

    // The default banner is the floor every site keeps: no tombstone.
    let overlay = overlay_extending("examples/tenancy-decisioning/base").await;
    write(
        overlay.path(),
        "data/catalogs/banners/default_banner.tombstone.toml",
        "tombstone = true\n",
    )
    .await;
    let err = Package::load(overlay.path().to_string_lossy())
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("governance denies delete of entry default_banner on catalog.banners"),
        "unexpected error: {err}"
    );
}

// --- regional-policy ---------------------------------------------------------

#[tokio::test]
async fn regional_policy_gates_and_values_follow_the_jurisdiction() {
    let package = load("examples/regional-policy").await;

    let eu = context(json!({
        "account": { "id": "a_1", "jurisdiction": "eu", "plan_tier": "free" },
        "message": { "channel": "sms" }
    }));
    let gate = package
        .resolve_variable("sms_marketing_enabled", &eu)
        .unwrap();
    assert_eq!(gate.value, json!(false));
    let retention = package
        .resolve_variable("data_retention_days", &eu)
        .unwrap();
    assert_eq!(retention.value, json!(30));

    let us = context(json!({
        "account": { "id": "a_1", "jurisdiction": "us", "plan_tier": "free" },
        "message": { "channel": "sms" }
    }));
    let gate = package
        .resolve_variable("sms_marketing_enabled", &us)
        .unwrap();
    assert_eq!(gate.value, json!(true));
    let retention = package
        .resolve_variable("data_retention_days", &us)
        .unwrap();
    assert_eq!(retention.value, json!(90));
}

#[tokio::test]
async fn regional_policy_selects_the_active_provider_and_model() {
    let package = load("examples/regional-policy").await;

    // (email, us) has a primary and a backup; priority picks the primary.
    let email_us = context(json!({
        "account": { "id": "a_9", "jurisdiction": "us", "plan_tier": "paid" },
        "message": { "channel": "email" }
    }));
    let provider = package
        .resolve_variable("message_provider", &email_us)
        .unwrap();
    assert_eq!(provider.value["id"], json!("email_us_primary"));

    // Model, prompt version, and parameters travel together as one entry.
    let model = package
        .resolve_variable("assistant_model", &email_us)
        .unwrap();
    assert_eq!(model.value["model_id"], json!("large-2"));
    assert_eq!(model.value["prompt_version"], json!("support-v14"));
}

#[tokio::test]
async fn regional_policy_migration_moves_whole_accounts_deterministically() {
    let package = load("examples/regional-policy").await;

    // a_6 hashes into the migration's on arm (buckets 0-9); a_5 does not.
    let migrated = context(json!({
        "account": { "id": "a_6", "jurisdiction": "us", "plan_tier": "free" },
        "message": { "channel": "email" }
    }));
    let pipeline = package
        .resolve_variable("use_new_delivery_pipeline", &migrated)
        .unwrap();
    assert_eq!(pipeline.value, json!(true));

    let unmigrated = context(json!({
        "account": { "id": "a_5", "jurisdiction": "us", "plan_tier": "free" },
        "message": { "channel": "email" }
    }));
    let pipeline = package
        .resolve_variable("use_new_delivery_pipeline", &unmigrated)
        .unwrap();
    assert_eq!(pipeline.value, json!(false));
}

// --- environments -------------------------------------------------------------

#[tokio::test]
async fn environments_differ_in_values_never_in_contract() {
    let empty = context(json!({}));

    // The base carries production values.
    let base = load("examples/environments/base").await;
    let bucket = base.resolve_variable("storage_bucket", &empty).unwrap();
    assert_eq!(bucket.value, json!("thumbs-prod"));
    let debug = base
        .resolve_variable("enable_debug_endpoints", &empty)
        .unwrap();
    assert_eq!(debug.value, json!(false));

    // Dev overrides values only: same variables, different defaults.
    let dev = load("examples/environments/dev").await;
    let bucket = dev.resolve_variable("storage_bucket", &empty).unwrap();
    assert_eq!(bucket.value, json!("thumbs-dev"));
    let timeout = dev.resolve_variable("origin_timeout_ms", &empty).unwrap();
    assert_eq!(timeout.value, json!(10000));
    let log_level = dev.resolve_variable("log_level", &empty).unwrap();
    assert_eq!(log_level.value, json!("debug"));

    // Staging stays closer to prod: no timeout override.
    let staging = load("examples/environments/staging").await;
    let bucket = staging.resolve_variable("storage_bucket", &empty).unwrap();
    assert_eq!(bucket.value, json!("thumbs-staging"));
    let timeout = staging
        .resolve_variable("origin_timeout_ms", &empty)
        .unwrap();
    assert_eq!(timeout.value, json!(1500));
    let log_level = staging.resolve_variable("log_level", &empty).unwrap();
    assert_eq!(log_level.value, json!("info"));

    // An overlay only carries what differs; untouched knobs pass through.
    for package in [&base, &dev, &staging] {
        let upload = package.resolve_variable("max_upload_mb", &empty).unwrap();
        assert_eq!(upload.value, json!(25));
    }
}
