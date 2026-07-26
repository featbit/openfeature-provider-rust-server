//! End-to-end coverage for `OpenFeature` evaluation, provider tracking, and lifecycle cleanup.

use std::time::Duration;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::Router;
use featbit_openfeature_provider::FeatBitProvider;
use featbit_server_sdk::{ClientStatus, FbClient, FbOptions};
use futures_util::{SinkExt, StreamExt};
use open_feature::{EvaluationContext, EvaluationReason, OpenFeature};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

const FULL: &str = r#"{
    "messageType":"data-sync",
    "data":{"eventType":"full","featureFlags":[{
        "id":"e2e-id","key":"e2e-flag","updatedAt":1,"variationType":"boolean",
        "variations":[{"id":"enabled","value":"true"}],
        "targetUsers":[],"rules":[],"isEnabled":true,
        "fallthrough":{"includedInExpt":false,"variations":[
            {"id":"enabled","rollout":[0,1],"exptRollout":0}
        ]}
    }],"segments":[]}
}"#;

#[derive(Clone)]
struct EventServerState {
    requests: mpsc::UnboundedSender<Bytes>,
}

struct EventServer {
    url: String,
    requests: mpsc::UnboundedReceiver<Bytes>,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

struct StreamingServer {
    url: String,
    task: JoinHandle<()>,
}

async fn capture_events(State(state): State<EventServerState>, body: Bytes) -> StatusCode {
    let _ignored = state.requests.send(body);
    StatusCode::ACCEPTED
}

async fn start_event_server() -> EventServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("event listener should bind");
    let address = listener
        .local_addr()
        .expect("event listener should have an address");
    let (request_sender, requests) = mpsc::unbounded_channel();
    let (shutdown, shutdown_receiver) = oneshot::channel();
    let router = Router::new()
        .route("/api/public/insight/track", post(capture_events))
        .with_state(EventServerState {
            requests: request_sender,
        });
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ignored = shutdown_receiver.await;
            })
            .await
            .expect("event server should run");
    });
    EventServer {
        url: format!("http://{address}"),
        requests,
        shutdown,
        task,
    }
}

async fn run_streaming_server(listener: TcpListener) {
    let (stream, _) = listener.accept().await.expect("SDK should connect");
    let mut socket = accept_async(stream)
        .await
        .expect("WebSocket handshake should succeed");
    let request = time::timeout(Duration::from_secs(10), socket.next())
        .await
        .expect("SDK should request initial data")
        .expect("SDK stream should remain open")
        .expect("SDK data request should be valid")
        .into_text()
        .expect("SDK data request should be text");
    let request: Value =
        serde_json::from_str(&request).expect("SDK data request should contain JSON");
    assert_eq!(request["messageType"], "data-sync");
    socket
        .send(Message::Text(FULL.into()))
        .await
        .expect("full data should send");

    while let Some(message) = socket.next().await {
        match message.expect("client message should be valid") {
            Message::Close(_) => {
                let _ignored = socket.close(None).await;
                break;
            }
            Message::Ping(payload) => {
                socket
                    .send(Message::Pong(payload))
                    .await
                    .expect("pong should send");
            }
            _ => {}
        }
    }
}

async fn start_streaming_server() -> StreamingServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("streaming listener should bind");
    let address = listener
        .local_addr()
        .expect("streaming listener should have an address");
    let task = tokio::spawn(run_streaming_server(listener));
    StreamingServer {
        url: format!("ws://{address}"),
        task,
    }
}

fn options(
    streaming_url: &str,
    event_url: &str,
    configure: impl FnOnce(featbit_server_sdk::FbOptionsBuilder) -> featbit_server_sdk::FbOptionsBuilder,
) -> FbOptions {
    configure(
        FbOptions::builder("e2e-environment-secret")
            .streaming_url(streaming_url)
            .event_url(event_url)
            .start_wait(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(1))
            .close_timeout(Duration::from_secs(1))
            .auto_flush_interval(Duration::from_mins(1))
            .flush_timeout(Duration::from_secs(10))
            .event_request_timeout(Duration::from_secs(5))
            .max_events_in_queue(16)
            .max_events_per_request(10)
            .max_send_event_attempts(1),
    )
    .build()
    .expect("E2E options should build")
}

async fn start_client(options: FbOptions) -> FbClient {
    tokio::task::spawn_blocking(move || FbClient::with_options(options))
        .await
        .expect("client construction should finish")
}

async fn flush(provider: &FeatBitProvider) {
    let client = provider.client().clone();
    assert!(
        tokio::task::spawn_blocking(move || client.flush_and_wait(Duration::from_secs(10)))
            .await
            .expect("flush task should finish")
    );
}

async fn close(provider: &FeatBitProvider) {
    let client = provider.client().clone();
    tokio::task::spawn_blocking(move || client.close())
        .await
        .expect("close task should finish");
}

async fn next_event_batch(events: &mut EventServer) -> Vec<Value> {
    let body = time::timeout(Duration::from_secs(10), events.requests.recv())
        .await
        .expect("event request should arrive")
        .expect("event server should remain available");
    serde_json::from_slice::<Vec<Value>>(&body).expect("event request should contain a JSON array")
}

fn assert_user(event: &Value) {
    assert_eq!(event["user"]["keyId"], "e2e-user");
    assert_eq!(event["user"]["name"], "E2E User");
    let properties = event["user"]["customizedProperties"]
        .as_array()
        .expect("custom properties should be an array");
    assert!(properties
        .iter()
        .any(|property| property["name"] == "plan" && property["value"] == "pro"));
}

fn assert_evaluation_and_metric(events: &[Value]) {
    assert_eq!(events.len(), 2);
    let evaluation = events
        .iter()
        .find(|event| event.get("variations").is_some())
        .expect("evaluation event should be present");
    assert_eq!(evaluation["variations"][0]["featureFlagKey"], "e2e-flag");
    assert_eq!(evaluation["variations"][0]["variation"]["id"], "enabled");
    assert_eq!(evaluation["variations"][0]["variation"]["value"], "true");
    assert_user(evaluation);

    let metric = events
        .iter()
        .find(|event| event.get("metrics").is_some())
        .expect("metric event should be present");
    assert_eq!(metric["metrics"][0]["eventName"], "e2e-conversion");
    assert_eq!(metric["metrics"][0]["numericValue"], 1.0);
    assert_user(metric);
}

async fn stop_servers(streaming: StreamingServer, events: EventServer) {
    time::timeout(Duration::from_secs(10), streaming.task)
        .await
        .expect("streaming worker should stop")
        .expect("streaming server should stop cleanly");
    let _ignored = events.shutdown.send(());
    events.task.await.expect("event server should stop cleanly");
}

#[tokio::test]
async fn open_feature_evaluation_and_featbit_metric_are_delivered_end_to_end() {
    let mut events = start_event_server().await;
    let streaming = start_streaming_server().await;
    let client = start_client(options(&streaming.url, &events.url, |builder| builder)).await;
    let provider = FeatBitProvider::from_client(client);
    assert_eq!(provider.client().status(), ClientStatus::Ready);

    let mut api = OpenFeature::default();
    api.set_provider(provider.clone()).await;
    let open_feature = api.create_client();
    let context = EvaluationContext::default()
        .with_targeting_key("e2e-user")
        .with_custom_field("name", "E2E User")
        .with_custom_field("plan", "pro");

    let details = open_feature
        .get_bool_details("e2e-flag", Some(&context), None)
        .await
        .expect("OpenFeature evaluation should succeed");
    assert!(details.value);
    assert_eq!(details.variant.as_deref(), Some("enabled"));
    assert_eq!(details.reason, Some(EvaluationReason::Default));
    assert!(provider
        .track_metric_event(&context, "e2e-conversion", 1.0)
        .expect("metric context should be valid"));

    flush(&provider).await;
    assert_evaluation_and_metric(&next_event_batch(&mut events).await);

    drop(open_feature);
    api.shutdown().await;
    close(&provider).await;
    stop_servers(streaming, events).await;
}

#[tokio::test]
async fn manual_exposure_tracking_is_delivered_and_large_events_are_bounded() {
    let mut events = start_event_server().await;
    let streaming = start_streaming_server().await;
    let client = start_client(options(&streaming.url, &events.url, |builder| {
        builder
            .disable_events(true, true)
            .max_event_queue_size_bytes(4_096)
    }))
    .await;
    let provider = FeatBitProvider::from_client(client);
    let mut api = OpenFeature::default();
    api.set_provider(provider.clone()).await;
    let open_feature = api.create_client();
    let context = EvaluationContext::default()
        .with_targeting_key("e2e-user")
        .with_custom_field("name", "E2E User")
        .with_custom_field("plan", "pro");

    assert!(open_feature
        .get_bool_value("e2e-flag", Some(&context), None)
        .await
        .expect("OpenFeature evaluation should succeed"));
    assert!(provider
        .track_eval_event_for_flag(&context, "e2e-flag")
        .expect("exposure context should be valid"));
    assert!(provider
        .track_metric_event(&context, "e2e-conversion", 1.0)
        .expect("metric context should be valid"));

    let oversized = EvaluationContext::default()
        .with_targeting_key("e2e-user")
        .with_custom_field("payload", "x".repeat(8_192));
    assert_eq!(
        provider.track_metric_event(&oversized, "oversized", 1.0),
        Ok(false),
        "the core SDK byte budget should reject an oversized event"
    );

    flush(&provider).await;
    assert_evaluation_and_metric(&next_event_batch(&mut events).await);

    drop(open_feature);
    api.shutdown().await;
    close(&provider).await;
    stop_servers(streaming, events).await;
}

#[tokio::test]
async fn open_feature_shutdown_releases_the_last_provider_and_sync_worker() {
    let events = start_event_server().await;
    let streaming = start_streaming_server().await;
    let provider = FeatBitProvider::from_client(
        start_client(options(&streaming.url, &events.url, |builder| {
            builder.disable_events(true, false)
        }))
        .await,
    );
    let mut api = OpenFeature::default();
    api.set_provider(provider).await;
    let open_feature = api.create_client();
    let context = EvaluationContext::default()
        .with_targeting_key("e2e-user")
        .with_custom_field("name", "E2E User")
        .with_custom_field("plan", "pro");

    assert!(open_feature
        .get_bool_value("e2e-flag", Some(&context), None)
        .await
        .expect("OpenFeature evaluation should succeed"));
    drop(open_feature);

    // No provider or FbClient handle remains outside OpenFeature. Clearing the registry therefore
    // drops the final client handle and closes the synchronization worker.
    api.shutdown().await;
    stop_servers(streaming, events).await;
}
