//! Cross-crate evaluator compatibility tests through the standard `OpenFeature` client API.

use featbit_openfeature_provider::FeatBitProvider;
use featbit_server_sdk::FbOptions;
use open_feature::{Client, EvaluationContext, EvaluationErrorCode, EvaluationReason, OpenFeature};
use serde_json::{json, Value};

const IS_IN_SEGMENT: &str = "User is in segment";
const IS_NOT_IN_SEGMENT: &str = "User is not in segment";

// Copied from FeatBit .NET Server SDK commit 974e2a7a557095b300e4e89da86df7d6fa894963,
// tests/FeatBit.ServerSdk.Tests/Evaluation/DispatchAlgorithmTests.cs.
const DOTNET_DISPATCH_FIXTURES: [(&str, f64); 3] = [
    ("test-value", 0.146_536_292_042_583_23),
    ("qKPKh1S3FolC", 0.910_591_969_266_533_9),
    (
        "3eacb184-2d79-49df-9ea7-edd4f10e4c6f",
        0.089_944_031_555_205_58,
    ),
];

// Copied from ConditionMatcherTests.cs at the same pinned .NET protocol revision.
const DOTNET_OPERATOR_FIXTURES: [(&str, &str, &str, bool); 41] = [
    ("10", "BiggerThan", "9", true),
    ("10", "BiggerThan", "11", false),
    ("10", "BiggerEqualThan", "10", true),
    ("10", "BiggerEqualThan", "11", false),
    ("10", "LessThan", "11", true),
    ("10", "LessThan", "9", false),
    ("10", "LessEqualThan", "10", true),
    ("10", "LessEqualThan", "9", false),
    ("v1.0.0", "Equal", "v1.0.0", true),
    ("v1.1.0", "Equal", "v1.0.0", false),
    ("v1.1.0", "NotEqual", "v1.1.0", false),
    ("v1.1.0", "NotEqual", "v1.0.0", true),
    ("vvip", "Contains", "vip", true),
    ("vvip", "Contains", "sv", false),
    ("svip", "NotContain", "vv", true),
    ("svip", "NotContain", "vip", false),
    ("abc", "StartsWith", "ab", true),
    ("abc", "StartsWith", "b", false),
    ("abc", "EndsWith", "bc", true),
    ("abc", "EndsWith", "cd", false),
    ("color", "MatchRegex", "colou?r", true),
    ("colour", "MatchRegex", "colorr?", false),
    ("colouur", "NotMatchRegex", "colou?r", true),
    ("color", "NotMatchRegex", "colou?r", false),
    ("a", "IsOneOf", r#"["a", "b"]"#, true),
    ("c", "IsOneOf", r#"["a", "b"]"#, false),
    ("c", "NotOneOf", r#"["a", "b"]"#, true),
    ("a", "NotOneOf", r#"["a", "b"]"#, false),
    ("true", "IsTrue", "", true),
    ("TRue", "IsTrue", "", true),
    ("false", "IsFalse", "", true),
    ("falSE", "IsFalse", "", true),
    ("not-true-string", "IsTrue", "", false),
    ("not-false-string", "IsFalse", "", false),
    ("anything", "UnknownOperator", "anything", false),
    ("anything", "NotMatchRegex", "[", false),
    ("a", "NotOneOf", "not-json", false),
    ("a", "IsOneOf", "null", false),
    ("NaN", "BiggerThan", "0", false),
    ("1", "BiggerThan", "NaN", false),
    ("inf", "LessThan", "1", false),
];

const SEGMENT_FIXTURES: [(&str, &str, bool); 22] = [
    (IS_IN_SEGMENT, r#"["included"]"#, true),
    (IS_NOT_IN_SEGMENT, r#"["included"]"#, false),
    (IS_IN_SEGMENT, r#"["excluded-wins"]"#, false),
    (IS_NOT_IN_SEGMENT, r#"["excluded-wins"]"#, true),
    (IS_IN_SEGMENT, r#"["rule"]"#, true),
    (IS_IN_SEGMENT, r#"["empty-rule"]"#, true),
    (IS_IN_SEGMENT, r#"["other","included"]"#, true),
    (IS_NOT_IN_SEGMENT, r#"["other","included"]"#, false),
    (IS_IN_SEGMENT, "[]", false),
    (IS_NOT_IN_SEGMENT, "[]", true),
    (IS_IN_SEGMENT, "null", false),
    (IS_NOT_IN_SEGMENT, "null", true),
    (IS_IN_SEGMENT, r#"["included","missing"]"#, true),
    (IS_NOT_IN_SEGMENT, r#"["included","missing"]"#, false),
    (IS_IN_SEGMENT, "not-json", false),
    (IS_NOT_IN_SEGMENT, "not-json", false),
    (IS_IN_SEGMENT, r#"["missing"]"#, false),
    (IS_NOT_IN_SEGMENT, r#"["missing"]"#, false),
    (IS_IN_SEGMENT, r#"["archived"]"#, false),
    (IS_NOT_IN_SEGMENT, r#"["archived"]"#, false),
    (IS_IN_SEGMENT, r#"["missing","included"]"#, false),
    (IS_NOT_IN_SEGMENT, r#"["missing","included"]"#, false),
];

fn rollout(variation_id: &str, lower: f64, upper: f64) -> Value {
    json!({
        "id": variation_id,
        "rollout": [lower, upper],
        "exptRollout": 0
    })
}

fn boolean_flag(key: &str) -> Value {
    json!({
        "id": format!("{key}-id"),
        "key": key,
        "updatedAt": 1,
        "variationType": "boolean",
        "variations": [
            {"id": "on", "value": "true"},
            {"id": "off", "value": "false"}
        ],
        "targetUsers": [],
        "rules": [],
        "isEnabled": true,
        "disabledVariationId": "off",
        "fallthrough": {
            "includedInExpt": false,
            "variations": [rollout("on", 0.0, 1.0)]
        },
        "exptIncludeAllTargets": false,
        "isArchived": false
    })
}

fn bootstrap(flags: &[Value], segments: &[Value]) -> String {
    json!({
        "messageType": "data-sync",
        "data": {
            "eventType": "full",
            "featureFlags": flags,
            "segments": segments
        }
    })
    .to_string()
}

async fn open_feature_client(
    flags: Vec<Value>,
    segments: Vec<Value>,
) -> (OpenFeature, Client, FeatBitProvider) {
    let options = FbOptions::builder("provider-compatibility-secret")
        .offline(true)
        .disable_events(true, false)
        .bootstrap_json(bootstrap(&flags, &segments))
        .build()
        .expect("compatibility bootstrap should build");
    let provider = FeatBitProvider::new(options);
    let mut api = OpenFeature::default();
    api.set_provider(provider.clone()).await;
    let client = api.create_client();
    (api, client, provider)
}

async fn shutdown(mut api: OpenFeature, client: Client, provider: FeatBitProvider) {
    drop(client);
    api.shutdown().await;
    provider.client().close();
}

fn evaluation_order_flags() -> Vec<Value> {
    let mut off = boolean_flag("off");
    off["isEnabled"] = json!(false);
    off["targetUsers"] = json!([{"keyIds": ["user-1"], "variationId": "on"}]);

    let mut target = boolean_flag("target");
    target["targetUsers"] = json!([{"keyIds": ["user-1"], "variationId": "off"}]);

    let mut rule = boolean_flag("rule");
    rule["rules"] = json!([{
        "name": "pro users",
        "conditions": [{"property": "tier", "op": "Equal", "value": "pro"}],
        "variations": [rollout("off", 0.0, 1.0)]
    }]);

    let mut and_rule = boolean_flag("and-rule");
    and_rule["rules"] = json!([
        {
            "name": "partial",
            "conditions": [
                {"property": "tier", "op": "Equal", "value": "pro"},
                {"property": "country", "op": "Equal", "value": "us"}
            ],
            "variations": [rollout("off", 0.0, 1.0)]
        },
        {
            "name": "complete",
            "conditions": [
                {"property": "country", "op": "Equal", "value": "cn"}
            ],
            "variations": [rollout("off", 0.0, 1.0)]
        }
    ]);

    let mut archived = boolean_flag("archived");
    archived["isArchived"] = json!(true);

    let mut malformed_off = boolean_flag("malformed-off");
    malformed_off["isEnabled"] = json!(false);
    malformed_off["disabledVariationId"] = json!("missing");

    let mut malformed_fallthrough = boolean_flag("malformed-fallthrough");
    malformed_fallthrough["fallthrough"]["variations"] = json!([]);

    vec![
        off,
        target,
        rule,
        and_rule,
        boolean_flag("fallthrough"),
        archived,
        malformed_off,
        malformed_fallthrough,
    ]
}

#[tokio::test]
async fn standard_client_preserves_evaluation_order_reasons_and_errors() {
    let (api, client, provider) = open_feature_client(evaluation_order_flags(), Vec::new()).await;
    let context = EvaluationContext::default()
        .with_targeting_key("user-1")
        .with_custom_field("tier", "pro")
        .with_custom_field("country", "cn");

    let off_details = client
        .get_bool_details("off", Some(&context), None)
        .await
        .expect("disabled flag should resolve");
    assert!(!off_details.value);
    assert_eq!(off_details.variant.as_deref(), Some("off"));
    assert_eq!(off_details.reason, Some(EvaluationReason::Disabled));

    let target_details = client
        .get_bool_details("target", Some(&context), None)
        .await
        .expect("direct target should resolve");
    assert!(!target_details.value);
    assert_eq!(target_details.variant.as_deref(), Some("off"));
    assert_eq!(
        target_details.reason,
        Some(EvaluationReason::TargetingMatch)
    );

    let rule_details = client
        .get_bool_details("rule", Some(&context), None)
        .await
        .expect("matching rule should resolve");
    assert!(!rule_details.value);
    assert_eq!(rule_details.variant.as_deref(), Some("off"));
    assert_eq!(rule_details.reason, Some(EvaluationReason::TargetingMatch));

    let and_rule_details = client
        .get_bool_details("and-rule", Some(&context), None)
        .await
        .expect("the first fully matching rule should resolve");
    assert!(!and_rule_details.value);
    assert_eq!(and_rule_details.variant.as_deref(), Some("off"));
    assert_eq!(
        and_rule_details.reason,
        Some(EvaluationReason::TargetingMatch)
    );

    let default_details = client
        .get_bool_details("fallthrough", Some(&context), None)
        .await
        .expect("fallthrough should resolve");
    assert!(default_details.value);
    assert_eq!(default_details.variant.as_deref(), Some("on"));
    assert_eq!(default_details.reason, Some(EvaluationReason::Default));

    for key in ["missing", "archived"] {
        let error = client
            .get_bool_details(key, Some(&context), None)
            .await
            .expect_err("missing and archived flags should be hidden");
        assert_eq!(error.code, EvaluationErrorCode::FlagNotFound, "{key}");
    }
    for key in ["malformed-off", "malformed-fallthrough"] {
        let error = client
            .get_bool_details(key, Some(&context), None)
            .await
            .expect_err("malformed flags should not resolve");
        assert_eq!(error.code, EvaluationErrorCode::ParseError, "{key}");
    }

    shutdown(api, client, provider).await;
}

#[tokio::test]
async fn standard_client_matches_all_dotnet_operator_fixtures() {
    let flags = DOTNET_OPERATOR_FIXTURES
        .iter()
        .enumerate()
        .map(|(index, &(_, operator, rule_value, _))| {
            let key = format!("operator-{index}");
            let mut flag = boolean_flag(&key);
            flag["rules"] = json!([{
                "name": operator,
                "conditions": [{
                    "property": "input",
                    "op": operator,
                    "value": rule_value
                }],
                "variations": [rollout("on", 0.0, 1.0)]
            }]);
            flag["fallthrough"]["variations"] = json!([rollout("off", 0.0, 1.0)]);
            flag
        })
        .collect();
    let (api, client, provider) = open_feature_client(flags, Vec::new()).await;

    for (index, &(input, operator, rule_value, expected)) in
        DOTNET_OPERATOR_FIXTURES.iter().enumerate()
    {
        let key = format!("operator-{index}");
        let context = EvaluationContext::default()
            .with_targeting_key(format!("operator-user-{index}"))
            .with_custom_field("input", input);
        let details = client
            .get_bool_details(&key, Some(&context), None)
            .await
            .expect("operator fixture should evaluate");
        assert_eq!(
            details.value, expected,
            "unexpected result for input={input:?} operator={operator} rule={rule_value:?}"
        );
        assert_eq!(
            details.reason,
            Some(if expected {
                EvaluationReason::TargetingMatch
            } else {
                EvaluationReason::Default
            })
        );
    }

    shutdown(api, client, provider).await;
}

#[tokio::test]
async fn standard_client_matches_dotnet_dispatch_vectors_and_range_boundaries() {
    let mut flags = Vec::new();
    for &(dispatch_key, expected) in &DOTNET_DISPATCH_FIXTURES {
        let (flag_key, targeting_key) = dispatch_key.split_at(1);
        let mut flag = boolean_flag(flag_key);
        // The core SDK suite checks these values bit-for-bit. This adapter fixture crosses a JSON
        // bootstrap boundary, whose decimal parser may move a boundary by one ULP, so use a
        // sub-femtoscale envelope around the same fixed vector.
        flag["fallthrough"]["variations"] = json!([
            rollout("on", expected - f64::EPSILON, expected + f64::EPSILON),
            rollout("off", 0.0, 1.0)
        ]);
        flags.push((flag, targeting_key));
    }

    let mut always = boolean_flag("always-");
    always["fallthrough"]["variations"] =
        json!([rollout("on", 0.0, 1.0), rollout("off", 0.0, 1.0)]);
    let mut never = boolean_flag("never-");
    never["fallthrough"]["variations"] = json!([rollout("on", 0.0, 0.0), rollout("off", 0.0, 1.0)]);
    let mut invalid_empty = boolean_flag("invalid-empty-");
    invalid_empty["fallthrough"]["variations"] = json!([
        {"id": "on", "rollout": [], "exptRollout": 0},
        rollout("off", 0.0, 1.0)
    ]);
    let mut invalid_single = boolean_flag("invalid-single-");
    invalid_single["fallthrough"]["variations"] = json!([
        {"id": "on", "rollout": [0.5], "exptRollout": 0},
        rollout("off", 0.0, 1.0)
    ]);
    let mut invalid_reversed = boolean_flag("invalid-reversed-");
    invalid_reversed["fallthrough"]["variations"] =
        json!([rollout("on", 0.8, 0.2), rollout("off", 0.0, 1.0)]);

    let mut default_dispatch = boolean_flag("test-");
    default_dispatch["fallthrough"]["variations"] =
        json!([rollout("on", 0.0, 0.3), rollout("off", 0.3, 1.0)]);
    let mut custom_dispatch = boolean_flag("test");
    custom_dispatch["fallthrough"]["dispatchKey"] = json!("bucket");
    custom_dispatch["fallthrough"]["variations"] =
        json!([rollout("on", 0.0, 0.3), rollout("off", 0.3, 1.0)]);

    let mut provider_flags = flags
        .iter()
        .map(|(flag, _)| flag.clone())
        .collect::<Vec<_>>();
    provider_flags.extend([
        always,
        never,
        invalid_empty,
        invalid_single,
        invalid_reversed,
        default_dispatch,
        custom_dispatch,
    ]);
    let (api, client, provider) = open_feature_client(provider_flags, Vec::new()).await;

    for ((_, targeting_key), &(dispatch_key, _)) in flags.iter().zip(&DOTNET_DISPATCH_FIXTURES) {
        let (flag_key, _) = dispatch_key.split_at(1);
        let context = EvaluationContext::default().with_targeting_key(*targeting_key);
        let details = client
            .get_bool_details(flag_key, Some(&context), None)
            .await
            .expect("exact dispatch boundary should evaluate");
        assert!(details.value, "dispatch vector changed for {dispatch_key}");
        assert_eq!(details.variant.as_deref(), Some("on"));
        assert_eq!(details.reason, Some(EvaluationReason::Split));
    }

    let range_context = EvaluationContext::default().with_targeting_key("range-user");
    let always_details = client
        .get_bool_details("always-", Some(&range_context), None)
        .await
        .expect("full range should evaluate");
    assert!(always_details.value);
    for key in [
        "never-",
        "invalid-empty-",
        "invalid-single-",
        "invalid-reversed-",
    ] {
        let details = client
            .get_bool_details(key, Some(&range_context), None)
            .await
            .expect("invalid first rollout should be skipped");
        assert!(!details.value, "{key}");
        assert_eq!(details.variant.as_deref(), Some("off"));
    }

    let default_context = EvaluationContext::default().with_targeting_key("user-default");
    let default_details = client
        .get_bool_details("test-", Some(&default_context), None)
        .await
        .expect("default dispatch should evaluate");
    assert!(!default_details.value);
    assert_eq!(default_details.variant.as_deref(), Some("off"));
    assert_eq!(default_details.reason, Some(EvaluationReason::Split));

    let custom_context = EvaluationContext::default()
        .with_targeting_key("ignored")
        .with_custom_field("bucket", "-value");
    let custom_details = client
        .get_bool_details("test", Some(&custom_context), None)
        .await
        .expect("custom dispatch should evaluate");
    assert!(custom_details.value);
    assert_eq!(custom_details.variant.as_deref(), Some("on"));
    assert_eq!(custom_details.reason, Some(EvaluationReason::Split));

    shutdown(api, client, provider).await;
}

fn segment_data() -> Vec<Value> {
    vec![
        json!({
            "id": "included",
            "updatedAt": 1,
            "included": ["user-1"]
        }),
        json!({
            "id": "excluded-wins",
            "updatedAt": 1,
            "included": ["user-1"],
            "excluded": ["user-1"],
            "rules": [{
                "conditions": [{
                    "property": "country",
                    "op": "Equal",
                    "value": "cn"
                }]
            }]
        }),
        json!({
            "id": "rule",
            "updatedAt": 1,
            "rules": [{
                "conditions": [{
                    "property": "country",
                    "op": "Equal",
                    "value": "cn"
                }]
            }]
        }),
        json!({
            "id": "empty-rule",
            "updatedAt": 1,
            "rules": [{"conditions": []}]
        }),
        json!({
            "id": "other",
            "updatedAt": 1,
            "included": ["another-user"]
        }),
        json!({
            "id": "archived",
            "updatedAt": 1,
            "included": ["user-1"],
            "isArchived": true
        }),
    ]
}

#[tokio::test]
async fn standard_client_preserves_segment_matching_semantics() {
    let flags = SEGMENT_FIXTURES
        .iter()
        .enumerate()
        .map(|(index, &(property, segment_ids, _))| {
            let key = format!("segment-{index}");
            let mut flag = boolean_flag(&key);
            flag["rules"] = json!([{
                "name": "segment rule",
                "conditions": [{
                    "property": property,
                    "op": "",
                    "value": segment_ids
                }],
                "variations": [rollout("on", 0.0, 1.0)]
            }]);
            flag["fallthrough"]["variations"] = json!([rollout("off", 0.0, 1.0)]);
            flag
        })
        .collect();
    let (api, client, provider) = open_feature_client(flags, segment_data()).await;
    let context = EvaluationContext::default()
        .with_targeting_key("user-1")
        .with_custom_field("country", "cn");

    for (index, &(property, segment_ids, expected)) in SEGMENT_FIXTURES.iter().enumerate() {
        let key = format!("segment-{index}");
        let details = client
            .get_bool_details(&key, Some(&context), None)
            .await
            .expect("segment fixture should evaluate");
        assert_eq!(
            details.value, expected,
            "unexpected result for {property} {segment_ids}"
        );
    }

    shutdown(api, client, provider).await;
}
