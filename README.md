# FeatBit OpenFeature Provider for Rust Server-Side SDK

## Introduction

This is the [OpenFeature](https://openfeature.dev/) provider for the
[FeatBit Rust Server-Side SDK](https://github.com/featbit/featbit-rust-sdk), for the 100% open-source
feature flag management platform [FeatBit](https://github.com/featbit/featbit).

The provider is designed for long-lived, multi-user systems such as web servers and backend
applications. It maps the official OpenFeature Rust API onto one shared FeatBit `FbClient`; data
synchronization, evaluation, analytics, and shutdown remain owned by the core SDK.

## Get Started

### Installation

Add the provider, the FeatBit server SDK, and the official OpenFeature SDK:

```toml
[dependencies]
featbit-openfeature-provider = "0.1.0-beta.1"
featbit-server-sdk = "0.1.0-beta.2"
open-feature = "0.3.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Before the first crates.io release, use the repository source:

```toml
featbit-openfeature-provider = {
    git = "https://github.com/featbit/openfeature-provider-rust-server",
    branch = "main"
}
```

This source form requires `featbit-server-sdk` `0.1.0-beta.2` to have been published first. The
Cargo package is imported as `featbit_openfeature_provider`.

### Prerequisite

Before using the provider, obtain the environment secret and SDK URLs:

- [How to get the environment secret](https://docs.featbit.co/sdk/faq#how-to-get-the-environment-secret)
- [How to get the SDK URLs](https://docs.featbit.co/sdk/faq#how-to-get-the-sdk-urls)

Use `wss://` and `https://` endpoints in production.

### Quick Start

```rust,no_run
use featbit_openfeature_provider::FeatBitProvider;
use featbit_server_sdk::FbOptions;
use open_feature::{EvaluationContext, OpenFeature};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let options = FbOptions::builder(std::env::var("FEATBIT_ENV_SECRET")?)
        .streaming_url(std::env::var("FEATBIT_STREAMING_URL")?)
        .event_url(std::env::var("FEATBIT_EVENT_URL")?)
        .build()?;

    // Construction can perform a bounded initial wait, so keep it off async workers.
    let provider =
        tokio::task::spawn_blocking(move || FeatBitProvider::new(options)).await?;
    let featbit = provider.client().clone();

    let client = {
        let mut api = OpenFeature::singleton_mut().await;
        api.set_provider(provider).await;
        api.create_client()
    };

    let context = EvaluationContext::default()
        .with_targeting_key("user-123")
        .with_custom_field("name", "Ada")
        .with_custom_field("country", "CN");
    let enabled = client
        .get_bool_value("new-checkout", Some(&context), None)
        .await
        .unwrap_or(false);

    if enabled {
        // Run the new code path.
    }

    OpenFeature::singleton_mut().await.shutdown().await;
    tokio::task::spawn_blocking(move || featbit.close()).await?;
    Ok(())
}
```

### Examples

- [Console application](examples/console.rs): `cargo run --example console`
- [Axum application](examples/axum.rs): `cargo run --example axum`
- [Bounded FeatBit Cloud stress test](examples/test/README.md):
  `cargo run --example cloud_axum_extreme`

The Axum example shares the OpenFeature client through typed application state, exposes
`GET /health/ready`, and shuts down OpenFeature before closing FeatBit.

## Provider

### FeatBitProvider

`FeatBitProvider` implements `open_feature::provider::FeatureProvider`. Construct it from validated
`FbOptions`, or wrap an existing `FbClient`:

```rust,no_run
use featbit_openfeature_provider::FeatBitProvider;
use featbit_server_sdk::{FbClient, FbOptions};

# fn example(options: FbOptions) {
let client = FbClient::with_options(options);
let provider = FeatBitProvider::from_client(client.clone());

// Both handles share one snapshot, synchronizer, event processor, and lifecycle.
# let _ = provider;
# client.close();
# }
```

Keep a provider or `FbClient` handle when the application needs FeatBit-specific readiness,
delivery-aware flush, tracking, or explicit bounded shutdown.

### Evaluation Context

FeatBit evaluates a single-user context and requires a non-empty OpenFeature `targeting_key`.

| OpenFeature context value | FeatBit user value |
| --- | --- |
| `targeting_key` | required `keyId` |
| custom field `name` | built-in user name |
| boolean, integer, finite float, string | invariant string custom property |
| datetime | RFC 3339 custom property |
| supported structured value | stable JSON custom property |
| unsupported structured value or non-finite float | ignored with debug logging |

```rust
use open_feature::EvaluationContext;

let context = EvaluationContext::default()
    .with_targeting_key("user-123")
    .with_custom_field("name", "Ada")
    .with_custom_field("country", "CN")
    .with_custom_field("age", 42_i64);
```

Missing targeting keys return `EvaluationErrorCode::TargetingKeyMissing`; the provider never
downcasts a structured field with an unchecked assumption.

### Evaluating Flags

Boolean, integer, float, string, and struct resolutions all use the core SDK's single local
evaluation engine. The selected FeatBit variation ID is returned as the OpenFeature `variant`.

| FeatBit outcome | OpenFeature reason or error |
| --- | --- |
| disabled flag | `EvaluationReason::Disabled` |
| direct target or non-split rule | `EvaluationReason::TargetingMatch` |
| percentage rollout | `EvaluationReason::Split` |
| ordinary fallthrough | `EvaluationReason::Default` |
| client not initialized or closed | `ProviderNotReady` |
| unknown or archived flag | `FlagNotFound` |
| malformed remote flag/JSON | `ParseError` |
| incompatible requested type | `TypeMismatch` |

```rust,no_run
use open_feature::{Client, EvaluationContext};

# async fn example(client: &Client, context: &EvaluationContext) {
let details = client
    .get_bool_details("new-checkout", Some(context), None)
    .await;

match details {
    Ok(details) => {
        println!(
            "value={}, variant={:?}, reason={:?}",
            details.value, details.variant, details.reason
        );
    }
    Err(error) => eprintln!("evaluation used its fallback: {}", error.code),
}
# }
```

Struct variations must contain a JSON object. Supported values are converted recursively; arrays
may appear inside objects, but a top-level array, `null`, or any nested `null` returns a typed error.

### Status and Lifecycle

The provider preserves the core SDK's readiness semantics:

| FeatBit client status | OpenFeature provider status |
| --- | --- |
| `NotReady` | `NotReady` |
| `Ready` | `Ready` |
| `Stale` | `STALE` |
| `Closed` | `Error` |

OpenFeature shutdown unregisters providers but OpenFeature 0.3 does not provide FeatBit's
delivery-aware close contract. Shut down OpenFeature first, then call `provider.client().close()`.
For async applications, run the bounded close through `tokio::task::spawn_blocking`.

### Tracking Extensions

OpenFeature 0.3 does not standardize custom metric tracking. The provider exposes FeatBit-specific
extensions using the same context mapping:

```rust,no_run
use std::time::Duration;

use featbit_openfeature_provider::FeatBitProvider;
use open_feature::EvaluationContext;

# fn example(
#     provider: &FeatBitProvider,
#     context: &EvaluationContext,
# ) -> Result<(), open_feature::EvaluationError> {
let _accepted =
    provider.track_metric_event(context, "checkout-completed", 1.0)?;
let _delivered = provider
    .client()
    .flush_and_wait(Duration::from_secs(2));
# Ok(())
# }
```

`track_eval_event_for_flag` re-evaluates the current immutable snapshot because OpenFeature details
cannot carry FeatBit's retained event. Call it promptly after resolution. When an application must
preserve the original result across flag updates, use a direct FeatBit detail evaluation and its
captured event instead.

## Supported Rust and OpenFeature Versions

The provider uses Rust edition 2021, has an MSRV of 1.95.0, and pins the OpenFeature Rust SDK to the
compatible `0.3` release line. CI tests Rust 1.95, 1.96, and 1.97.

## Getting Support

- Ask FeatBit usage questions in [FeatBit Slack](https://join.slack.com/t/featbit/shared_invite/zt-1ew5e2vbb-x6Apan1xZOaYMnFzqZkGNQ).
- Report provider bugs or request features in
  [openfeature-provider-rust-server issues](https://github.com/featbit/openfeature-provider-rust-server/issues/new).

## See Also

- [FeatBit Rust Server-Side SDK](https://github.com/featbit/featbit-rust-sdk)
- [OpenFeature Rust SDK](https://github.com/open-feature/rust-sdk)
- [OpenFeature documentation](https://openfeature.dev/docs/reference/intro/)
- [FeatBit documentation](https://docs.featbit.co/)

## Development

The provider repository deliberately contains only OpenFeature mapping and examples. The FeatBit
evaluation engine remains in `featbit-server-sdk`.

During coordinated local development before the core version is published, clone
`featbit-rust-sdk` and `openfeature-provider-rust-server` as sibling directories and use a temporary
Cargo source patch for the core crates. Do not commit that local patch; the provider manifest keeps
the repositories independent by depending only on released core versions.

Run the full quality gate before submitting a change:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --doc
```

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
