# Releasing to crates.io

The provider and core SDK are separate crates and repositories. For the initial provider release,
the coordinated core crates are already published. Release in this order:

1. Confirm the exact core versions are visible in the crates.io index.
2. Generate `Cargo.lock` so the provider package resolves the published core versions, and run the
   full quality gate.
3. Package and publish `featbit-openfeature-provider`.

For future coordinated releases, publish `featbit-server-sdk` and any required integration crates
before updating and publishing the provider.

The initial versions are:

| Crate | Version |
| --- | --- |
| `featbit-server-sdk` | `0.1.1` |
| `featbit-server-sdk-opentelemetry` | `0.1.1` |
| `featbit-openfeature-provider` | `0.1.0-beta.1` |

## Verification

From a clean, reviewed checkout:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --doc
cargo check --examples --all-features
cargo publish --dry-run --locked
```

Inspect the package contents and dependency sources before publishing:

```text
cargo package --list
cargo tree -d
```

Published versions cannot be overwritten or deleted. If a release is unusable, yank it and publish
a new version:

```text
cargo yank --version <VERSION> featbit-openfeature-provider
```

The repository does not store a long-lived crates.io token. Configure crates.io Trusted Publishing
and a protected GitHub Environment before adding automated publication.
