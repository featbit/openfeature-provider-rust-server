//! Heap-retention regression coverage for repeated standard `OpenFeature` evaluations.

use std::alloc::System;

use featbit_openfeature_provider::FeatBitProvider;
use featbit_server_sdk::FbOptions;
use open_feature::{EvaluationContext, OpenFeature};
use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const BOOTSTRAP: &str = r#"{
    "messageType":"data-sync",
    "data":{"eventType":"full","featureFlags":[{
        "id":"heap-id","key":"heap-flag","updatedAt":1,"variationType":"boolean",
        "variations":[{"id":"on","value":"true"}],
        "targetUsers":[],"rules":[],"isEnabled":true,
        "fallthrough":{"includedInExpt":false,"variations":[
            {"id":"on","rollout":[0,1],"exptRollout":0}
        ]}
    }],"segments":[]}
}"#;

fn options() -> FbOptions {
    FbOptions::builder("heap-stability-secret")
        .offline(true)
        .disable_events(true, false)
        .bootstrap_json(BOOTSTRAP)
        .build()
        .expect("heap test options should build")
}

#[test]
fn repeated_standard_evaluations_and_lifecycles_release_allocations() {
    const WARMUP_EVALUATIONS: usize = 1_000;
    const MEASURED_EVALUATIONS: usize = 20_000;
    const MEASURED_LIFECYCLES: usize = 100;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("heap test runtime should build");
    runtime.block_on(async {
        let provider = FeatBitProvider::new(options());
        let mut api = OpenFeature::default();
        api.set_provider(provider.clone()).await;
        let client = api.create_client();
        let context = EvaluationContext::default()
            .with_targeting_key("heap-user")
            .with_custom_field("plan", "pro")
            .with_custom_field("age", 42_i64);

        for _ in 0..WARMUP_EVALUATIONS {
            assert!(client
                .get_bool_value("heap-flag", Some(&context), None)
                .await
                .expect("warmup evaluation should resolve"));
        }

        let region = Region::new(GLOBAL);
        for _ in 0..MEASURED_EVALUATIONS {
            assert!(client
                .get_bool_value("heap-flag", Some(&context), None)
                .await
                .expect("measured evaluation should resolve"));
        }
        let allocations = region.change();
        assert_eq!(
            allocations.allocations, allocations.deallocations,
            "repeated evaluations retained allocation blocks: {allocations:?}"
        );
        assert_eq!(
            allocations.bytes_allocated, allocations.bytes_deallocated,
            "repeated evaluations retained allocated bytes: {allocations:?}"
        );

        drop(context);
        drop(client);
        api.shutdown().await;
        provider.client().close();
        drop(provider);

        let lifecycle_region = Region::new(GLOBAL);
        for _ in 0..MEASURED_LIFECYCLES {
            let provider = FeatBitProvider::new(options());
            let mut api = OpenFeature::default();
            api.set_provider(provider.clone()).await;
            let client = api.create_client();
            let context = EvaluationContext::default().with_targeting_key("lifecycle-user");
            assert!(client
                .get_bool_value("heap-flag", Some(&context), None)
                .await
                .expect("lifecycle evaluation should resolve"));
            drop(context);
            drop(client);
            api.shutdown().await;
            provider.client().close();
        }
        let lifecycle_allocations = lifecycle_region.change();
        assert_eq!(
            lifecycle_allocations.allocations, lifecycle_allocations.deallocations,
            "provider lifecycles retained allocation blocks: {lifecycle_allocations:?}"
        );
        assert_eq!(
            lifecycle_allocations.bytes_allocated, lifecycle_allocations.bytes_deallocated,
            "provider lifecycles retained allocated bytes: {lifecycle_allocations:?}"
        );
    });
}
