//! Standalone microbenchmark for `FeatBit` provider and full `OpenFeature` evaluation paths.

use std::hint::black_box;
use std::time::Instant;

use featbit_openfeature_provider::FeatBitProvider;
use featbit_server_sdk::{FbOptions, FbUser};
use open_feature::provider::FeatureProvider;
use open_feature::{Client, EvaluationContext, OpenFeature};

const READY_BOOTSTRAP: &str = r#"{
    "messageType":"data-sync",
    "data":{"eventType":"full","featureFlags":[{
        "id":"flag-id","key":"enabled","updatedAt":1,"variationType":"boolean",
        "variations":[{"id":"on","value":"true"}],
        "targetUsers":[],"rules":[],"isEnabled":true,
        "fallthrough":{"includedInExpt":false,"variations":[
            {"id":"on","rollout":[0,1],"exptRollout":0}
        ]}
    }],"segments":[]}
}"#;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let Ok(options) = FbOptions::builder("benchmark-secret")
        .offline(true)
        .disable_events(true, false)
        .bootstrap_json(READY_BOOTSTRAP)
        .build()
    else {
        eprintln!("failed to build provider benchmark options");
        return;
    };
    let provider = FeatBitProvider::new(options);
    let featbit_user = FbUser::builder("benchmark-user").build();
    let minimal_context = EvaluationContext::default().with_targeting_key("benchmark-user");
    let rich_context = EvaluationContext::default()
        .with_targeting_key("benchmark-user")
        .with_custom_field("name", "Benchmark User")
        .with_custom_field("country", "CN")
        .with_custom_field("plan", "pro")
        .with_custom_field("age", 42_i64)
        .with_custom_field("active", true)
        .with_custom_field("score", 1.5_f64)
        .with_custom_field(
            "profile",
            open_feature::EvaluationContextFieldValue::new_struct(
                serde_json::json!({"team":"sdk","roles":["admin","developer"]}),
            ),
        );

    benchmark_core(&provider, &featbit_user);
    benchmark_provider(
        "provider_minimal_context",
        &provider,
        &minimal_context,
        10_000,
        500_000,
    )
    .await;
    benchmark_provider(
        "provider_rich_context",
        &provider,
        &rich_context,
        5_000,
        100_000,
    )
    .await;

    let mut api = OpenFeature::default();
    api.set_provider(provider.clone()).await;
    let client = api.create_client();
    benchmark_open_feature(&client, &minimal_context, 5_000, 100_000).await;

    api.shutdown().await;
    provider.client().close();
}

fn benchmark_core(provider: &FeatBitProvider, user: &FbUser) {
    for _ in 0..10_000 {
        black_box(provider.client().bool_variation(
            black_box("enabled"),
            black_box(user),
            black_box(false),
        ));
    }
    let started = Instant::now();
    let iterations = 2_000_000_u64;
    for _ in 0..iterations {
        black_box(provider.client().bool_variation(
            black_box("enabled"),
            black_box(user),
            black_box(false),
        ));
    }
    print_result("featbit_core", iterations, started);
}

async fn benchmark_provider(
    name: &str,
    provider: &FeatBitProvider,
    context: &EvaluationContext,
    warmup: u64,
    iterations: u64,
) {
    for _ in 0..warmup {
        let _evaluation = black_box(
            provider
                .resolve_bool_value(black_box("enabled"), black_box(context))
                .await,
        );
    }
    let started = Instant::now();
    for _ in 0..iterations {
        let _evaluation = black_box(
            provider
                .resolve_bool_value(black_box("enabled"), black_box(context))
                .await,
        );
    }
    print_result(name, iterations, started);
}

async fn benchmark_open_feature(
    client: &Client,
    context: &EvaluationContext,
    warmup: u64,
    iterations: u64,
) {
    for _ in 0..warmup {
        let _evaluation = black_box(
            client
                .get_bool_value(black_box("enabled"), Some(black_box(context)), None)
                .await,
        );
    }
    let started = Instant::now();
    for _ in 0..iterations {
        let _evaluation = black_box(
            client
                .get_bool_value(black_box("enabled"), Some(black_box(context)), None)
                .await,
        );
    }
    print_result("openfeature_minimal_context", iterations, started);
}

fn print_result(name: &str, iterations: u64, started: Instant) {
    let elapsed = started.elapsed();
    let nanos_per_evaluation = elapsed.as_nanos() / u128::from(iterations);
    println!("{name}: {nanos_per_evaluation} ns/evaluation ({iterations} iterations)");
}
