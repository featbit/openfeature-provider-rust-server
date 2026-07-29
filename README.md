# FeatBit OpenFeature Provider for Rust Server-Side SDK

## Introduction

This is the [OpenFeature](https://openfeature.dev/) provider for the
[FeatBit Rust Server-Side SDK](https://github.com/featbit/featbit-rust-sdk), built for the 100%
open-source feature flag management platform [FeatBit](https://github.com/featbit/featbit).

The provider implements the official OpenFeature Rust interface and delegates flag evaluation to
one shared FeatBit `FbClient`. Synchronization, local evaluation, analytics, and lifecycle remain
owned by the core SDK.

## Quick Start

### 1. Install

```shell
cargo add featbit-openfeature-provider@0.1.1
```

### 2. Configure

Before using the provider, obtain the environment secret and SDK URLs:

- [How to get the environment secret](https://docs.featbit.co/sdk/faq#how-to-get-the-environment-secret)
- [How to get the SDK URLs](https://docs.featbit.co/sdk/faq#how-to-get-the-sdk-urls)

Use `wss://` and `https://` endpoints in production. Keep the environment secret on trusted
server-side infrastructure.

### 3. Evaluate a Flag

```rust,no_run
use featbit_openfeature_provider::FeatBitProvider;
use open_feature::{EvaluationContext, OpenFeature};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let environment_secret = std::env::var("FEATBIT_ENV_SECRET")?;
    let streaming_url = std::env::var("FEATBIT_STREAMING_URL")?;
    let event_url = std::env::var("FEATBIT_EVENT_URL")?;

    // Construction can perform a bounded initial wait.
    let provider = tokio::task::spawn_blocking(move || {
        FeatBitProvider::from_endpoints(environment_secret, streaming_url, event_url)
    })
    .await??;

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

    drop(client);
    OpenFeature::singleton_mut().await.shutdown().await;
    Ok(())
}
```

## Behavior

- OpenFeature `targeting_key` is the required FeatBit user key; custom `name` becomes the FeatBit
  user name.
- Primitive context fields become invariant strings, datetime becomes RFC 3339, and supported
  structured fields become stable JSON. Unsupported structures and non-finite floats are ignored.
- FeatBit variation IDs become OpenFeature variants. Disabled, targeting, split, and fallthrough
  results map to the corresponding standard OpenFeature reasons.
- Unknown flags, missing targeting keys, malformed values, and incompatible types return standard
  OpenFeature errors.
- Struct flags require a top-level JSON object and reject `null`, nested `null`, and non-finite
  values.
- `NotReady`, `Ready`, `Stale`, and terminal `Error` status are preserved. Drop application clients
  and shut down OpenFeature to release the provider and its synchronization worker.

## Examples

- [OpenFeature-only standalone application](examples/openfeature-only/README.md):
  `cargo run --manifest-path examples/openfeature-only/Cargo.toml -- <flag-key>`
- [Console application](examples/console.rs): `cargo run --example console`
- [Axum application](examples/axum.rs): `cargo run --example axum`
- [Bounded FeatBit Cloud stress test](examples/test/README.md):
  `cargo run --example cloud_axum_extreme`

## FeatBit Features Not Covered by OpenFeature

OpenFeature Rust SDK `0.3.0` does not implement eventing or a tracking API. Delivery-aware flush,
offline bootstrap, bulk flag inspection, and FeatBit lifecycle details are also vendor-specific.

For example, an experiment may need to record exposure only after the evaluated experience was
actually shown, followed by a conversion metric when checkout completes. OpenFeature still performs
the flag evaluation; the FeatBit provider extensions record those two events. For this
application-controlled mode, add `.disable_events(true, true)` to the `FbOptions` builder to disable
automatic evaluation events while allowing explicit tracking:

```rust,no_run
use featbit_openfeature_provider::FeatBitProvider;
use open_feature::{Client, EvaluationContext};

# fn show_checkout(_: bool) {}
# async fn evaluate_checkout(
#     client: &Client,
#     provider: &FeatBitProvider,
#     context: &EvaluationContext,
#     checkout_completed: bool,
# ) -> Result<bool, open_feature::EvaluationError> {
let use_new_checkout = client
    .get_bool_value("new-checkout", Some(context), None)
    .await?;

// Render either the new or current checkout. Record exposure only after
// the selected experience was actually shown to this user.
show_checkout(use_new_checkout);
let exposure_queued =
    provider.track_eval_event_for_flag(context, "new-checkout")?;

if checkout_completed {
    let conversion_queued =
        provider.track_metric_event(context, "checkout-completed", 1.0)?;
    # let _ = conversion_queued;
}

// Both tracking methods are non-blocking. `false` means the event was not
// accepted because delivery is unavailable or the bounded queue is full.
# let _ = exposure_queued;
# Ok(use_new_checkout)
# }
```

`track_eval_event_for_flag` re-evaluates the current local snapshot. When the exact original
variation must be retained across a flag update, use a direct FeatBit detail evaluation and pass its
immutable event to `FeatBitProvider::track_eval_event`.

For every other non-OpenFeature capability, see the
[FeatBit Rust SDK](https://github.com/featbit/featbit-rust-sdk).

## Supported Rust, FeatBit, and OpenFeature Versions

| Component | Supported version |
| --- | --- |
| Rust edition | 2021 |
| Rust toolchain | 1.95.0, 1.96.0, and 1.97.x |
| FeatBit Rust SDK | `0.1.1` |
| `open-feature` | exactly `0.3.0` |

Stable Rust only is supported.

## Getting Support

- Ask FeatBit usage questions in
  [FeatBit Slack](https://join.slack.com/t/featbit/shared_invite/zt-1ew5e2vbb-x6Apan1xZOaYMnFzqZkGNQ).
- Report provider bugs or request features in
  [openfeature-provider-rust-server issues](https://github.com/featbit/openfeature-provider-rust-server/issues/new).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
