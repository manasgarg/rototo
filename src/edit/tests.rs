use serde_json::json;

use super::operation::{AllocationArmInput, EditOperation, EditOptions, EditOutcome};
use super::tree::EditTree;

const CHECKOUT: &str = r#"schema_version = 1

# What runtime behavior this controls.
description = "Checkout page content"
type = "catalog=checkout_redesign"

[resolve]
# The fallback everyone gets.
default = "control" # chosen at launch

[[resolve.rule]]
# Premium users see the redesign.
when = 'variables["premium_users"]'
value = "premium"

[[resolve.rule]]
# EU stays conservative.
when = 'variables["eu_users"]'
value = "control"
"#;

const PLAN_TIERS: &str = r#"schema_version = 1
description = "The sellable plan tiers"
type = "string"

members = ["free", "team", "business"] # ordered cheapest first
"#;

const PRO_ENTRY: &str = r#"enabled_features = ["reporting", "webhooks"] # launched set
monthly_price = 49 # in dollars

[limits]
projects = 50
members = 500
"#;

const CHECKOUT_LAYER: &str = r#"schema_version = 1

description = "Checkout experiments, diverted by user id"
unit = "context.user.id"
buckets = 1000

[[allocation]]
id = "cta_copy_test"
status = "running"
eligibility = '!variables["enterprise_accounts"]'

[[allocation.arm]]
name = "control"
buckets = "0-499"

[[allocation.arm]]
name = "benefit_led"
buckets = "500-999"
"#;

fn tree() -> EditTree {
    EditTree::from_files([
        ("rototo-package.toml", "schema_version = 1\n"),
        ("variables/checkout_redesign.toml", CHECKOUT),
        ("enums/plan_tiers.toml", PLAN_TIERS),
        (
            "model/catalogs/plans.schema.json",
            "{\"type\": \"object\"}\n",
        ),
        ("data/catalogs/plans/pro.toml", PRO_ENTRY),
        ("data/catalogs/plans/free.toml", "monthly_price = 0\n"),
        ("layers/checkout.toml", CHECKOUT_LAYER),
        (
            "model/context/request.schema.json",
            "{\"type\": \"object\"}\n",
        ),
        (
            "model/context/request-samples/premium.json",
            "{\n  \"user\": {\n    \"tier\": \"premium\"\n  }\n}\n",
        ),
    ])
}

fn apply_one(operation: EditOperation) -> EditOutcome {
    apply_all(vec![operation])
}

fn apply_all(operations: Vec<EditOperation>) -> EditOutcome {
    super::apply(&tree(), &operations, &EditOptions::default())
        .unwrap_or_else(|err| panic!("apply should succeed: {err}"))
}

fn apply_err(operation: EditOperation) -> String {
    super::apply(&tree(), &[operation], &EditOptions::default())
        .expect_err("apply should fail")
        .to_string()
}

fn written<'a>(outcome: &'a EditOutcome, path: &str) -> &'a str {
    outcome
        .plan
        .writes
        .iter()
        .find(|write| write.path == path)
        .unwrap_or_else(|| panic!("plan should write {path}"))
        .content
        .as_str()
}

#[test]
fn set_default_splices_one_value_and_keeps_every_comment() {
    let outcome = apply_one(EditOperation::SetDefault {
        variable: "checkout_redesign".to_owned(),
        value: json!("premium"),
    });
    let content = written(&outcome, "variables/checkout_redesign.toml");
    assert_eq!(
        content,
        CHECKOUT.replace(
            "default = \"control\" # chosen at launch",
            "default = \"premium\" # chosen at launch"
        )
    );

    let record = &outcome.records[0];
    assert_eq!(record.operation, "set_default");
    assert_eq!(
        record.address,
        "variable=checkout_redesign#/resolve/default"
    );
    assert_eq!(record.before, Some(json!("control")));
    assert_eq!(record.after, Some(json!("premium")));
}

#[test]
fn set_type_and_description_edit_in_place() {
    let outcome = apply_all(vec![
        EditOperation::SetType {
            variable: "checkout_redesign".to_owned(),
            variable_type: "string".to_owned(),
        },
        EditOperation::SetDescription {
            target: "variable=checkout_redesign".to_owned(),
            text: Some("Which checkout everyone sees".to_owned()),
        },
    ]);
    let content = written(&outcome, "variables/checkout_redesign.toml");
    assert!(content.contains("type = \"string\""));
    assert!(content.contains("description = \"Which checkout everyone sees\""));
    // Both edits land in one write, and the surrounding comments survive.
    assert_eq!(outcome.plan.writes.len(), 1);
    assert!(content.contains("# What runtime behavior this controls."));
    assert!(content.contains("# Premium users see the redesign."));
}

#[test]
fn clearing_a_description_removes_the_key() {
    let outcome = apply_one(EditOperation::SetDescription {
        target: "variable=checkout_redesign".to_owned(),
        text: None,
    });
    let content = written(&outcome, "variables/checkout_redesign.toml");
    assert!(!content.contains("description"));
    let record = &outcome.records[0];
    assert_eq!(record.before, Some(json!("Checkout page content")));
    assert_eq!(record.after, None);
}

#[test]
fn add_rule_appends_at_the_end_by_default() {
    let outcome = apply_one(EditOperation::AddRule {
        variable: "checkout_redesign".to_owned(),
        position: None,
        when: "context.user.tier == \"pro\"".to_owned(),
        value: json!("premium"),
    });
    let content = written(&outcome, "variables/checkout_redesign.toml");
    let expected = format!(
        "{CHECKOUT}\n[[resolve.rule]]\nwhen = 'context.user.tier == \"pro\"'\nvalue = \"premium\"\n"
    );
    assert_eq!(content, expected);
    assert_eq!(
        outcome.records[0].address,
        "variable=checkout_redesign#/resolve/rule/2"
    );
}

#[test]
fn add_rule_at_a_position_shifts_the_rest_down() {
    let outcome = apply_one(EditOperation::AddRule {
        variable: "checkout_redesign".to_owned(),
        position: Some(0),
        when: "context.urgent".to_owned(),
        value: json!("control"),
    });
    let content = written(&outcome, "variables/checkout_redesign.toml");
    let urgent = content.find("context.urgent").expect("new rule present");
    let premium = content.find("premium_users").expect("old rule present");
    assert!(urgent < premium, "new rule should come first:\n{content}");
    // The old rules keep their comments while sliding down.
    assert!(content.contains("# Premium users see the redesign."));
    assert!(content.contains("# EU stays conservative."));
}

#[test]
fn update_rule_touches_only_the_named_fields() {
    let outcome = apply_one(EditOperation::UpdateRule {
        variable: "checkout_redesign".to_owned(),
        index: 1,
        when: None,
        value: Some(json!("premium")),
    });
    let content = written(&outcome, "variables/checkout_redesign.toml");
    assert!(content.contains("# EU stays conservative."));
    assert!(content.contains("when = 'variables[\"eu_users\"]'"));
    assert!(!content.contains("value = \"control\"\n\n[[resolve.rule]]\n# EU"));
    let record = &outcome.records[0];
    assert_eq!(
        record.before,
        Some(json!({ "when": "variables[\"eu_users\"]", "value": "control" }))
    );
    assert_eq!(
        record.after,
        Some(json!({ "when": "variables[\"eu_users\"]", "value": "premium" }))
    );
}

#[test]
fn remove_rule_takes_its_comment_with_it() {
    let outcome = apply_one(EditOperation::RemoveRule {
        variable: "checkout_redesign".to_owned(),
        index: 0,
    });
    let content = written(&outcome, "variables/checkout_redesign.toml");
    assert!(!content.contains("premium_users"));
    assert!(!content.contains("# Premium users see the redesign."));
    assert!(content.contains("# EU stays conservative."));
    assert_eq!(
        outcome.records[0].before,
        Some(json!({ "when": "variables[\"premium_users\"]", "value": "premium" }))
    );
}

#[test]
fn removing_the_last_rule_drops_the_rule_key() {
    let outcome = apply_all(vec![
        EditOperation::RemoveRule {
            variable: "checkout_redesign".to_owned(),
            index: 1,
        },
        EditOperation::RemoveRule {
            variable: "checkout_redesign".to_owned(),
            index: 0,
        },
    ]);
    let content = written(&outcome, "variables/checkout_redesign.toml");
    assert!(!content.contains("resolve.rule"), "{content}");
    assert!(content.contains("default = \"control\" # chosen at launch"));
}

#[test]
fn move_rule_reorders_and_the_comments_travel() {
    let outcome = apply_one(EditOperation::MoveRule {
        variable: "checkout_redesign".to_owned(),
        from: 1,
        to: 0,
    });
    let content = written(&outcome, "variables/checkout_redesign.toml");
    let eu = content.find("eu_users").expect("eu rule present");
    let premium = content.find("premium_users").expect("premium rule present");
    assert!(eu < premium, "eu rule should come first:\n{content}");
    let eu_comment = content.find("# EU stays conservative.").expect("comment");
    assert!(eu_comment < eu, "the comment moved with its rule");
}

#[test]
fn rule_indexes_out_of_range_are_refused_with_the_count() {
    let message = apply_err(EditOperation::RemoveRule {
        variable: "checkout_redesign".to_owned(),
        index: 5,
    });
    assert!(message.contains("index 5 is out of range"), "{message}");
    assert!(message.contains("2 rules"), "{message}");
}

#[test]
fn a_missing_variable_is_a_friendly_refusal() {
    let message = apply_err(EditOperation::SetDefault {
        variable: "missing".to_owned(),
        value: json!(true),
    });
    assert!(
        message.contains("variable `missing` does not exist"),
        "{message}"
    );
    assert!(message.contains("operation 0 (set_default)"), "{message}");
}

#[test]
fn null_values_are_refused_because_toml_has_no_null() {
    let message = apply_err(EditOperation::SetDefault {
        variable: "checkout_redesign".to_owned(),
        value: json!(null),
    });
    assert!(message.contains("TOML has no null"), "{message}");
}

#[test]
fn set_field_replaces_a_scalar_and_keeps_its_comment() {
    let outcome = apply_one(EditOperation::SetField {
        target: "catalog=plans:entry=pro#/monthly_price".to_owned(),
        value: json!(59),
    });
    let content = written(&outcome, "data/catalogs/plans/pro.toml");
    assert!(
        content.contains("monthly_price = 59 # in dollars"),
        "{content}"
    );
    let record = &outcome.records[0];
    assert_eq!(record.address, "catalog=plans:entry=pro#/monthly_price");
    assert_eq!(record.before, Some(json!(49)));
    assert_eq!(record.after, Some(json!(59)));
}

#[test]
fn set_field_walks_into_nested_tables() {
    let outcome = apply_one(EditOperation::SetField {
        target: "catalog=plans:entry=pro#/limits/projects".to_owned(),
        value: json!(100),
    });
    let content = written(&outcome, "data/catalogs/plans/pro.toml");
    assert!(content.contains("projects = 100"), "{content}");
    assert!(content.contains("members = 500"), "{content}");
}

#[test]
fn set_field_creates_missing_intermediate_objects() {
    let outcome = apply_one(EditOperation::SetField {
        target: "catalog=plans:entry=pro#/metadata/support/tier".to_owned(),
        value: json!("dedicated"),
    });
    let content = written(&outcome, "data/catalogs/plans/pro.toml");
    assert!(
        content.contains("[metadata.support]") && content.contains("tier = \"dedicated\""),
        "{content}"
    );
}

#[test]
fn set_field_replaces_an_array_element_by_index() {
    let outcome = apply_one(EditOperation::SetField {
        target: "catalog=plans:entry=pro#/enabled_features/1".to_owned(),
        value: json!("sso"),
    });
    let content = written(&outcome, "data/catalogs/plans/pro.toml");
    assert!(
        content.contains("[\"reporting\", \"sso\"] # launched set"),
        "{content}"
    );
}

#[test]
fn unset_field_removes_an_existing_field_only() {
    let outcome = apply_one(EditOperation::UnsetField {
        target: "catalog=plans:entry=pro#/limits/members".to_owned(),
    });
    let content = written(&outcome, "data/catalogs/plans/pro.toml");
    assert!(!content.contains("members"), "{content}");
    assert_eq!(outcome.records[0].before, Some(json!(500)));

    let message = apply_err(EditOperation::UnsetField {
        target: "catalog=plans:entry=pro#/limits/absent".to_owned(),
    });
    assert!(message.contains("has nothing at `absent`"), "{message}");
}

#[test]
fn field_operations_demand_a_pointer_and_an_entry_target() {
    let message = apply_err(EditOperation::SetField {
        target: "catalog=plans:entry=pro".to_owned(),
        value: json!(1),
    });
    assert!(message.contains("need a `#/field` pointer"), "{message}");

    let message = apply_err(EditOperation::SetField {
        target: "variable=checkout_redesign#/type".to_owned(),
        value: json!(1),
    });
    assert!(message.contains("catalog=<id>:entry=<key>"), "{message}");
}

#[test]
fn enum_members_add_and_remove_in_place() {
    let outcome = apply_one(EditOperation::AddMember {
        enum_id: "plan_tiers".to_owned(),
        value: json!("enterprise"),
    });
    let content = written(&outcome, "enums/plan_tiers.toml");
    assert!(
        content.contains(
            "members = [\"free\", \"team\", \"business\", \"enterprise\"] # ordered cheapest first"
        ),
        "{content}"
    );
    assert_eq!(
        outcome.records[0].after,
        Some(json!(["free", "team", "business", "enterprise"]))
    );

    let outcome = apply_one(EditOperation::RemoveMember {
        enum_id: "plan_tiers".to_owned(),
        value: json!("free"),
    });
    let content = written(&outcome, "enums/plan_tiers.toml");
    assert!(
        content.contains("members = [\"team\", \"business\"] # ordered cheapest first"),
        "{content}"
    );
}

#[test]
fn duplicate_and_missing_members_are_refused() {
    let message = apply_err(EditOperation::AddMember {
        enum_id: "plan_tiers".to_owned(),
        value: json!("team"),
    });
    assert!(message.contains("already a member"), "{message}");

    let message = apply_err(EditOperation::RemoveMember {
        enum_id: "plan_tiers".to_owned(),
        value: json!("platinum"),
    });
    assert!(message.contains("not a member"), "{message}");
}

#[test]
fn the_rollout_dial_rewrites_one_bucket_range() {
    let outcome = apply_one(EditOperation::SetArmBuckets {
        layer: "checkout".to_owned(),
        allocation: "cta_copy_test".to_owned(),
        arm: "benefit_led".to_owned(),
        buckets: "200-999".to_owned(),
    });
    let content = written(&outcome, "layers/checkout.toml");
    assert_eq!(
        content,
        CHECKOUT_LAYER.replace("buckets = \"500-999\"", "buckets = \"200-999\"")
    );
    let record = &outcome.records[0];
    assert_eq!(record.address, "layer=checkout#/allocation/0/arm/1/buckets");
    assert_eq!(record.before, Some(json!("500-999")));
    assert_eq!(record.after, Some(json!("200-999")));
}

#[test]
fn allocations_add_with_their_arms_and_remove_cleanly() {
    let outcome = apply_one(EditOperation::AddAllocation {
        layer: "checkout".to_owned(),
        id: "shipping_test".to_owned(),
        status: Some("running".to_owned()),
        eligibility: None,
        arms: vec![
            AllocationArmInput {
                name: "control".to_owned(),
                buckets: "0-899".to_owned(),
            },
            AllocationArmInput {
                name: "free_shipping".to_owned(),
                buckets: "900-999".to_owned(),
            },
        ],
    });
    let content = written(&outcome, "layers/checkout.toml");
    assert!(content.contains("id = \"shipping_test\""), "{content}");
    assert!(content.contains("buckets = \"900-999\""), "{content}");
    // The existing allocation is untouched, comments and all.
    assert!(
        content.starts_with(CHECKOUT_LAYER.trim_end_matches('\n')),
        "{content}"
    );
    assert_eq!(outcome.records[0].address, "layer=checkout#/allocation/1");

    let outcome = apply_one(EditOperation::RemoveAllocation {
        layer: "checkout".to_owned(),
        id: "cta_copy_test".to_owned(),
    });
    let content = written(&outcome, "layers/checkout.toml");
    assert!(!content.contains("allocation"), "{content}");
    assert!(content.contains("buckets = 1000"), "{content}");
}

#[test]
fn allocation_status_and_eligibility_edit_in_place() {
    let outcome = apply_all(vec![
        EditOperation::SetAllocationStatus {
            layer: "checkout".to_owned(),
            id: "cta_copy_test".to_owned(),
            status: "paused".to_owned(),
        },
        EditOperation::SetAllocationEligibility {
            layer: "checkout".to_owned(),
            id: "cta_copy_test".to_owned(),
            when: None,
        },
    ]);
    let content = written(&outcome, "layers/checkout.toml");
    assert!(content.contains("status = \"paused\""), "{content}");
    assert!(!content.contains("eligibility"), "{content}");
    assert_eq!(
        outcome.records[1].before,
        Some(json!("!variables[\"enterprise_accounts\"]"))
    );

    let message = apply_err(EditOperation::SetArmBuckets {
        layer: "checkout".to_owned(),
        allocation: "cta_copy_test".to_owned(),
        arm: "surprise".to_owned(),
        buckets: "0-1".to_owned(),
    });
    assert!(message.contains("no arm named `surprise`"), "{message}");
}

#[test]
fn create_variable_writes_the_full_skeleton() {
    let outcome = apply_one(EditOperation::CreateVariable {
        id: "beta_users".to_owned(),
        variable_type: "bool".to_owned(),
        description: Some("Who is in the beta".to_owned()),
        default: json!(false),
    });
    assert_eq!(
        written(&outcome, "variables/beta_users.toml"),
        r#"schema_version = 1

description = "Who is in the beta"
type = "bool"

[resolve]
# The value when no rule matches. Rules run top to bottom; the first
# match wins.
default = false
"#
    );
    let record = &outcome.records[0];
    assert_eq!(record.address, "variable=beta_users");
    assert_eq!(
        record.after,
        Some(json!({
            "schema_version": 1,
            "description": "Who is in the beta",
            "type": "bool",
            "resolve": { "default": false }
        }))
    );
}

#[test]
fn create_enum_and_layer_write_their_skeletons() {
    let outcome = apply_all(vec![
        EditOperation::CreateEnum {
            id: "regions".to_owned(),
            member_type: "string".to_owned(),
            members: vec![json!("eu"), json!("us")],
            description: None,
        },
        EditOperation::CreateLayer {
            id: "search".to_owned(),
            unit: "context.user.id".to_owned(),
            buckets: 1000,
        },
    ]);
    assert_eq!(
        written(&outcome, "enums/regions.toml"),
        "schema_version = 1\ntype = \"string\"\n\nmembers = [\"eu\", \"us\"]\n"
    );
    assert_eq!(
        written(&outcome, "layers/search.toml"),
        "schema_version = 1\n\nunit = \"context.user.id\"\nbuckets = 1000\n"
    );
}

#[test]
fn create_entry_needs_its_catalog_and_writes_nested_tables() {
    let outcome = apply_one(EditOperation::CreateEntry {
        catalog: "plans".to_owned(),
        key: "enterprise".to_owned(),
        fields: json!({
            "monthly_price": 499,
            "limits": { "projects": 5000 }
        }),
    });
    assert_eq!(
        written(&outcome, "data/catalogs/plans/enterprise.toml"),
        "monthly_price = 499\n\n[limits]\nprojects = 5000\n"
    );

    let message = apply_err(EditOperation::CreateEntry {
        catalog: "absent".to_owned(),
        key: "x".to_owned(),
        fields: json!({}),
    });
    assert!(
        message.contains("catalog `absent` does not exist"),
        "{message}"
    );
}

#[test]
fn create_context_brings_a_starter_sample_along() {
    let outcome = apply_one(EditOperation::CreateContext {
        id: "session".to_owned(),
        schema: json!({ "type": "object" }),
    });
    assert_eq!(
        written(&outcome, "model/context/session.schema.json"),
        "{\n  \"type\": \"object\"\n}\n"
    );
    assert_eq!(
        written(&outcome, "model/context/session-samples/default.json"),
        "{}\n"
    );
    assert_eq!(outcome.records.len(), 2);
    assert_eq!(
        outcome.records[1].address,
        "evaluation-context=session:sample=default"
    );
}

#[test]
fn samples_create_and_replace_as_whole_documents() {
    let outcome = apply_all(vec![
        EditOperation::CreateSample {
            context: "request".to_owned(),
            key: "enterprise".to_owned(),
            content: json!({ "user": { "tier": "enterprise" } }),
        },
        EditOperation::ReplaceSample {
            context: "request".to_owned(),
            key: "premium".to_owned(),
            content: json!({ "user": { "tier": "premium", "id": 7 } }),
        },
    ]);
    assert!(
        written(&outcome, "model/context/request-samples/enterprise.json")
            .contains("\"enterprise\"")
    );
    assert_eq!(
        outcome.records[1].before,
        Some(json!({ "user": { "tier": "premium" } }))
    );
}

#[test]
fn creating_something_that_exists_is_refused() {
    let message = apply_err(EditOperation::CreateVariable {
        id: "checkout_redesign".to_owned(),
        variable_type: "bool".to_owned(),
        description: None,
        default: json!(false),
    });
    assert!(message.contains("already exists"), "{message}");
}

#[test]
fn ids_follow_the_addressing_grammar() {
    let message = apply_err(EditOperation::CreateVariable {
        id: "BetaUsers".to_owned(),
        variable_type: "bool".to_owned(),
        description: None,
        default: json!(false),
    });
    assert!(message.contains("lowercase snake_case"), "{message}");
}

#[test]
fn delete_removes_one_entity_and_records_what_it_held() {
    let outcome = apply_one(EditOperation::Delete {
        target: "variable=checkout_redesign".to_owned(),
    });
    assert_eq!(
        outcome.plan.deletes,
        vec!["variables/checkout_redesign.toml"]
    );
    let record = &outcome.records[0];
    assert_eq!(record.address, "variable=checkout_redesign");
    assert_eq!(
        record.before.as_ref().and_then(|before| before.get("type")),
        Some(&json!("catalog=checkout_redesign"))
    );
}

#[test]
fn deleting_a_catalog_cascades_to_its_entries() {
    let outcome = apply_one(EditOperation::Delete {
        target: "catalog=plans".to_owned(),
    });
    let mut deletes = outcome.plan.deletes.clone();
    deletes.sort();
    assert_eq!(
        deletes,
        vec![
            "data/catalogs/plans/free.toml",
            "data/catalogs/plans/pro.toml",
            "model/catalogs/plans.schema.json",
        ]
    );
    let addresses: Vec<&str> = outcome
        .records
        .iter()
        .map(|record| record.address.as_str())
        .collect();
    assert_eq!(
        addresses,
        vec![
            "catalog=plans",
            "catalog=plans:entry=free",
            "catalog=plans:entry=pro",
        ]
    );
}

#[test]
fn delete_takes_entities_not_fields_or_subtrees() {
    let message = apply_err(EditOperation::Delete {
        target: "variable=checkout_redesign#/type".to_owned(),
    });
    assert!(message.contains("not a field"), "{message}");

    let message = apply_err(EditOperation::Delete {
        target: "variable=payments/".to_owned(),
    });
    assert!(message.contains("concrete entity"), "{message}");
}

#[test]
fn inherited_entities_are_refused_until_overlay_compilation_lands() {
    let options = EditOptions {
        inherited: ["variable=checkout_redesign".to_owned()].into(),
    };
    let err = super::apply(
        &tree(),
        &[EditOperation::SetDefault {
            variable: "checkout_redesign".to_owned(),
            value: json!("premium"),
        }],
        &options,
    )
    .expect_err("inherited edit should be refused")
    .to_string();
    assert!(err.contains("inherited from a base package"), "{err}");

    // The parent catalog being inherited blocks entry edits too.
    let options = EditOptions {
        inherited: ["catalog=plans".to_owned()].into(),
    };
    let err = super::apply(
        &tree(),
        &[EditOperation::SetField {
            target: "catalog=plans:entry=pro#/monthly_price".to_owned(),
            value: json!(1),
        }],
        &options,
    )
    .expect_err("inherited edit should be refused")
    .to_string();
    assert!(err.contains("`catalog=plans` is inherited"), "{err}");
}

#[test]
fn operations_later_in_the_list_see_earlier_writes() {
    let outcome = apply_all(vec![
        EditOperation::CreateVariable {
            id: "beta_users".to_owned(),
            variable_type: "bool".to_owned(),
            description: None,
            default: json!(false),
        },
        EditOperation::AddRule {
            variable: "beta_users".to_owned(),
            position: None,
            when: "context.user.beta".to_owned(),
            value: json!(true),
        },
    ]);
    assert_eq!(outcome.plan.writes.len(), 1);
    let content = written(&outcome, "variables/beta_users.toml");
    assert!(content.contains("[[resolve.rule]]"), "{content}");
    assert!(
        content.contains("when = \"context.user.beta\""),
        "{content}"
    );
}

#[test]
fn rewriting_a_file_to_its_own_content_plans_no_write() {
    let outcome = apply_one(EditOperation::SetDefault {
        variable: "checkout_redesign".to_owned(),
        value: json!("control"),
    });
    assert!(outcome.plan.writes.is_empty());
    assert!(outcome.plan.deletes.is_empty());
    // The record still tells the truth: the value was already control.
    assert_eq!(outcome.records[0].before, outcome.records[0].after);
}

#[test]
fn operations_round_trip_through_their_wire_shape() {
    let operations = vec![
        EditOperation::SetDefault {
            variable: "checkout_redesign".to_owned(),
            value: json!(true),
        },
        EditOperation::AddRule {
            variable: "x".to_owned(),
            position: Some(1),
            when: "context.a".to_owned(),
            value: json!([1, 2]),
        },
        EditOperation::SetField {
            target: "catalog=plans:entry=pro#/limits/api_calls".to_owned(),
            value: json!({ "burst": 10 }),
        },
        EditOperation::SetArmBuckets {
            layer: "checkout".to_owned(),
            allocation: "cta_copy_test".to_owned(),
            arm: "benefit_led".to_owned(),
            buckets: "0-499".to_owned(),
        },
    ];
    let wire = serde_json::to_value(&operations).expect("operations serialize");
    assert_eq!(wire[0]["op"], "set_default");
    assert_eq!(wire[1]["position"], 1);
    let back: Vec<EditOperation> = serde_json::from_value(wire).expect("operations deserialize");
    assert_eq!(back, operations);

    let unknown = serde_json::from_value::<EditOperation>(json!({
        "op": "set_default",
        "variable": "x",
        "value": 1,
        "surprise": true
    }));
    assert!(unknown.is_err(), "unknown fields should be refused");
}
