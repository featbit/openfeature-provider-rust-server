//! Adversarial tests for the `OpenFeature` adapter boundary.

use std::process::Command;

use featbit_openfeature_provider::FeatBitProvider;
use featbit_server_sdk::FbOptions;
use open_feature::provider::FeatureProvider;
use open_feature::{
    EvaluationContext, EvaluationContextFieldValue, EvaluationReason, StructValue, Value,
};

const DEEP_CONTEXT_CHILD: &str = "FEATBIT_OF_DEEP_CONTEXT_CHILD";
const DEEP_CONTEXT_DEPTH: usize = 16_384;
const BOOTSTRAP: &str = r#"{
    "messageType":"data-sync",
    "data":{"eventType":"full","featureFlags":[{
        "id":"adversarial-id","key":"adversarial-flag","updatedAt":1,
        "variationType":"boolean","variations":[{"id":"enabled","value":"true"}],
        "targetUsers":[],"rules":[],"isEnabled":true,
        "fallthrough":{"includedInExpt":false,"variations":[
            {"id":"enabled","rollout":[0,1],"exptRollout":0}
        ]}
    }],"segments":[]}
}"#;

fn provider() -> FeatBitProvider {
    let options = FbOptions::builder("adversarial-test-secret")
        .offline(true)
        .bootstrap_json(BOOTSTRAP)
        .build()
        .expect("adversarial test options should build");
    FeatBitProvider::new(options)
}

fn deeply_nested_struct(depth: usize) -> StructValue {
    let mut nested = Value::String("leaf".to_owned());
    for _ in 0..depth {
        let mut parent = StructValue::default();
        parent.fields.insert("nested".to_owned(), nested);
        nested = Value::Struct(parent);
    }
    match nested {
        Value::Struct(value) => value,
        _ => unreachable!("a positive depth always produces a structure"),
    }
}

fn deeply_nested_json(depth: usize) -> serde_json::Value {
    let mut nested = serde_json::Value::String("leaf".to_owned());
    for _ in 0..depth {
        let mut parent = serde_json::Map::new();
        parent.insert("nested".to_owned(), nested);
        nested = serde_json::Value::Object(parent);
    }
    nested
}

fn exercise_deep_context_child() {
    let context = EvaluationContext::default()
        .with_targeting_key("deep-context-user")
        .with_custom_field(
            "open_feature_deep",
            EvaluationContextFieldValue::new_struct(deeply_nested_struct(DEEP_CONTEXT_DEPTH)),
        )
        .with_custom_field(
            "json_deep",
            EvaluationContextFieldValue::new_struct(deeply_nested_json(DEEP_CONTEXT_DEPTH)),
        );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");
    let details = runtime
        .block_on(provider().resolve_bool_value("adversarial-flag", &context))
        .expect("an unsupported context field must not abort flag resolution");
    assert!(details.value);
    assert_eq!(details.reason, Some(EvaluationReason::Default));

    // Dropping an attacker-constructed recursively owned OpenFeature value can itself recurse
    // deeply. This probe isolates the adapter traversal and deliberately leaves that upstream
    // value to process teardown.
    std::mem::forget(context);
}

#[test]
fn deeply_nested_structured_context_cannot_abort_the_provider_process() {
    if std::env::var_os(DEEP_CONTEXT_CHILD).is_some() {
        exercise_deep_context_child();
        return;
    }

    let executable = std::env::current_exe().expect("current test executable should be available");
    let output = Command::new(executable)
        .arg("--exact")
        .arg("deeply_nested_structured_context_cannot_abort_the_provider_process")
        .arg("--nocapture")
        .env(DEEP_CONTEXT_CHILD, "1")
        .env("RUST_BACKTRACE", "0")
        .output()
        .expect("isolated deep-context probe should start");

    assert!(
        output.status.success(),
        "OpenFeature context conversion aborted the child process\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
