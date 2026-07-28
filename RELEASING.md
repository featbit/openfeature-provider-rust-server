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

## Automated release

The [`Release`](.github/workflows/release.yml) workflow publishes the crate and creates a GitHub
Release. It is started manually with a SemVer version input and is restricted to the `main` branch.
The workflow writes the input version to `Cargo.toml`, refreshes the root package entry in
`Cargo.lock`, runs the complete quality gate, and commits the version bump back to `main` before
publishing.

Before running it:

1. Ensure the dependency versions and release contents already on `main` are ready to publish.
2. In **Settings > Actions > General**, allow workflows to use read and write permissions so the
   release workflow can commit its version bump to `main`.
3. Create a protected GitHub Environment named `release`. Restrict its deployment branch to `main`
   and configure required reviewers if appropriate.
4. On crates.io, configure a GitHub Trusted Publisher for:
   - Repository owner: `featbit`
   - Repository name: `openfeature-provider-rust-server`
   - Workflow file: `release.yml`
   - Environment: `release`
5. In GitHub, open **Actions > Release > Run workflow**, select `main`, and enter the new version
   without a leading `v`. No local version edit or version-bump push is needed.

The workflow validates the requested version and tag, updates the Cargo files, runs the complete
quality gate, pushes a `chore: prepare v<VERSION>` commit, publishes that exact commit through a
short-lived crates.io OIDC token, and then creates a `v<VERSION>` GitHub Release. Versions
containing a SemVer pre-release suffix, such as `0.1.0-beta.1`, are marked as pre-releases.

crates.io requires the first version of a new crate to be published manually before a Trusted
Publisher can be configured. For that one-time bootstrap, run the verification commands below and
then run `cargo publish --locked` locally with an owner token. Do not create the Git tag yet. After
configuring Trusted Publishing, run the workflow with the same version; it detects the existing
crate version, skips the upload, and creates the GitHub Release. Do not store that bootstrap token
in the repository or as a long-lived GitHub secret.

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

The repository does not store a long-lived crates.io token. Automated releases use crates.io
Trusted Publishing and the protected `release` GitHub Environment.
