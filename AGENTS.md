# FeatBit OpenFeature Provider for Rust Engineering Guide

This file applies to the entire repository.

## Product Boundary

This repository contains the OpenFeature adapter for
[`featbit-server-sdk`](https://github.com/featbit/featbit-rust-sdk). It must not copy or fork
FeatBit's evaluator, synchronization, storage, event processor, configuration, or lifecycle code.
One `FeatBitProvider` owns a cloneable `FbClient` handle and delegates every flag evaluation to the
core SDK's public `RawEvaluation` boundary.

The core SDK must remain usable without `open-feature`. Changes to FeatBit wire behavior belong in
`featbit-rust-sdk`; changes to OpenFeature context, status, reason, error, or value mapping belong
here.

## Protocol Authority

Verify public behavior against:

- FeatBit Rust Server SDK: <https://github.com/featbit/featbit-rust-sdk>
- OpenFeature Rust SDK 0.3: <https://github.com/open-feature/rust-sdk>
- OpenFeature specification and Rust guide: <https://openfeature.dev/>
- FeatBit .NET provider precedent:
  <https://github.com/featbit/openfeature-provider-dotnet-server>

Pin a compatible `open-feature` release. A trait or mapping change is a public API change.

## Rust Compatibility

- Use Rust edition 2021.
- The MSRV is 1.95.0.
- Test Rust 1.95, 1.96, and 1.97.
- Stable Rust only.
- Keep `#![forbid(unsafe_code)]` at the crate root.

## Provider Contract

`FeatBitProvider` must implement the official
`open_feature::provider::FeatureProvider` trait and remain `Send + Sync`.

Context mapping:

- `targeting_key` becomes the required FeatBit user key.
- custom `name` becomes the FeatBit user name.
- primitive custom fields become invariant strings.
- datetime becomes RFC 3339.
- supported structured fields become stable JSON.
- unsupported structured values and non-finite floats are ignored with debug logging.
- never downcast with an unchecked assumption.

Resolution mapping:

- disabled flag -> `EvaluationReason::Disabled`;
- direct target or non-split rule -> `EvaluationReason::TargetingMatch`;
- percentage rollout -> `EvaluationReason::Split`;
- ordinary fallthrough -> `EvaluationReason::Default`;
- variation ID -> OpenFeature `variant`;
- flag ID and variation type -> OpenFeature flag metadata;
- unavailable/closed client -> `ProviderNotReady`;
- unknown flag -> `FlagNotFound`;
- missing targeting key -> `TargetingKeyMissing`;
- malformed flag or JSON -> `ParseError`;
- incompatible requested type -> `TypeMismatch`.

Bool, integer, float, string, and struct resolution must use the same core raw evaluation result.
Only call `FbClient::complete_raw_evaluation` after conversion succeeds. Report adapter-side context
and conversion failures through `FbClient::observe_evaluation_error`.

Struct resolution accepts a top-level JSON object and recursively supported values. A top-level
array, null, nested null, invalid JSON, or non-finite number returns a typed error.

## Lifecycle and Tracking

- Preserve `NotReady`, `Ready`, `Stale`, and terminal `Error` status without masking stale data.
- Provider construction may perform the core SDK's bounded initial wait.
- OpenFeature shutdown and FeatBit close are distinct; examples shut down OpenFeature first and
  then close `FbClient`.
- Provider-specific tracking uses the same context mapping as evaluation.
- Tracking never waits for network I/O and returns `false` when delivery is unavailable or the
  bounded queue rejects the event.
- Do not log environment secrets, authorization headers, context identifiers, or event bodies.

## Dependencies

- Depend on `featbit-server-sdk`; never vendor its source.
- Use one official `open-feature` dependency.
- Use the standard `log` facade and never install a global logger.
- Keep TLS defaults and runtime ownership in the core SDK.
- Commit `Cargo.lock` once its pinned core source revision is available.

## Documentation and Tests

README structure follows the FeatBit .NET provider: Introduction, Get Started, provider behavior,
supported versions, support, and related links. Rust-specific lifecycle, context mapping, tracking,
and structured-value behavior must remain documented.

Tests must cover:

- every provider status, reason, and error mapping;
- primitive, datetime, structured, unsupported, and missing-key context fields;
- bool, integer, float, string, and struct resolution;
- recursive struct conversion and rejected shapes;
- variant and flag metadata;
- observer behavior for success and conversion errors;
- tracking extensions, closed clients, and `Send + Sync`.

Examples use placeholders and environment variables, never real secrets.

## Required Workflow

Run from this repository root:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --doc
cargo check --examples --all-features
```

During the coordinated pre-release split, use a local Cargo source patch to test against the sibling
`featbit-rust-sdk` checkout. Do not commit that local patch.

## Definition of Done

A change is complete only when:

- OpenFeature behavior is covered at the appropriate test layer;
- no FeatBit evaluation logic is duplicated;
- standard reasons and typed errors are preserved;
- evaluation remains local and non-panicking;
- shutdown and I/O remain bounded by the core SDK;
- documentation and examples compile;
- formatting, Clippy, tests, doc tests, examples, and supported Rust checks pass.
