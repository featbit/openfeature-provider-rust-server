//! Multithreaded stress coverage through the standard `OpenFeature` client API.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use featbit_openfeature_provider::FeatBitProvider;
use featbit_server_sdk::FbOptions;
use open_feature::{EvaluationContext, EvaluationErrorCode, EvaluationReason, OpenFeature};
use tokio::sync::Barrier;

const BOOTSTRAP: &str = r#"{
    "messageType":"data-sync",
    "data":{"eventType":"full","featureFlags":[{
        "id":"concurrent-id","key":"concurrent-flag","updatedAt":1,
        "variationType":"boolean",
        "variations":[{"id":"on","value":"true"}],
        "targetUsers":[],"rules":[],"isEnabled":true,
        "fallthrough":{"includedInExpt":false,"variations":[
            {"id":"on","rollout":[0,1],"exptRollout":0}
        ]}
    }],"segments":[]}
}"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn standard_client_evaluates_concurrently_and_races_close_safely() {
    const READY_WORKERS: usize = 16;
    const READY_EVALUATIONS_PER_WORKER: usize = 2_000;
    const RACING_WORKERS: usize = 8;
    const RACING_EVALUATIONS_PER_WORKER: usize = 1_000;

    let options = FbOptions::builder("concurrency-test-secret")
        .offline(true)
        .disable_events(true, false)
        .bootstrap_json(BOOTSTRAP)
        .build()
        .expect("concurrency options should build");
    let provider = FeatBitProvider::new(options);
    let mut api = OpenFeature::default();
    api.set_provider(provider.clone()).await;
    let client = Arc::new(api.create_client());

    let start = Arc::new(Barrier::new(READY_WORKERS + 1));
    let mut ready_tasks = Vec::with_capacity(READY_WORKERS);
    for worker in 0..READY_WORKERS {
        let client = Arc::clone(&client);
        let start = Arc::clone(&start);
        ready_tasks.push(tokio::spawn(async move {
            let context =
                EvaluationContext::default().with_targeting_key(format!("ready-{worker}"));
            start.wait().await;
            for _ in 0..READY_EVALUATIONS_PER_WORKER {
                let details = client
                    .get_bool_details("concurrent-flag", Some(&context), None)
                    .await
                    .expect("ready concurrent evaluation should resolve");
                assert!(details.value);
                assert_eq!(details.variant.as_deref(), Some("on"));
                assert_eq!(details.reason, Some(EvaluationReason::Default));
            }
        }));
    }
    start.wait().await;
    for task in ready_tasks {
        task.await.expect("ready evaluation worker should finish");
    }

    let race_start = Arc::new(Barrier::new(RACING_WORKERS + 1));
    let successful = Arc::new(AtomicUsize::new(0));
    let not_ready = Arc::new(AtomicUsize::new(0));
    let mut racing_tasks = Vec::with_capacity(RACING_WORKERS);
    for worker in 0..RACING_WORKERS {
        let client = Arc::clone(&client);
        let race_start = Arc::clone(&race_start);
        let successful = Arc::clone(&successful);
        let not_ready = Arc::clone(&not_ready);
        racing_tasks.push(tokio::spawn(async move {
            let context =
                EvaluationContext::default().with_targeting_key(format!("racing-{worker}"));
            race_start.wait().await;
            for _ in 0..RACING_EVALUATIONS_PER_WORKER {
                match client
                    .get_bool_details("concurrent-flag", Some(&context), None)
                    .await
                {
                    Ok(details) => {
                        assert!(details.value);
                        successful.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(error) if error.code == EvaluationErrorCode::ProviderNotReady => {
                        not_ready.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(error) => panic!("unexpected racing evaluation error: {error:?}"),
                }
            }
        }));
    }
    race_start.wait().await;
    tokio::task::yield_now().await;
    provider.client().close();
    for task in racing_tasks {
        task.await.expect("racing evaluation worker should finish");
    }

    assert_eq!(
        successful.load(Ordering::Relaxed) + not_ready.load(Ordering::Relaxed),
        RACING_WORKERS * RACING_EVALUATIONS_PER_WORKER
    );
    let closed_context = EvaluationContext::default().with_targeting_key("closed-user");
    let error = client
        .get_bool_details("concurrent-flag", Some(&closed_context), None)
        .await
        .expect_err("closed provider should reject standard evaluation");
    assert_eq!(error.code, EvaluationErrorCode::ProviderNotReady);

    drop(client);
    api.shutdown().await;
}
