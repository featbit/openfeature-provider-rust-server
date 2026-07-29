# OpenFeature-only consumer example

This standalone application connects the FeatBit provider once and then uses only the standard
OpenFeature client API for contexts, typed flag evaluation, details, provider metadata, and
shutdown. It does not directly depend on or import `featbit-server-sdk`.

The provider package itself is still required: OpenFeature is vendor-neutral and does not know how
to connect to FeatBit without a provider.

## Run against FeatBit Cloud

The application defaults to:

- streaming: `wss://app-eval.featbit.co`
- events: `https://app-eval.featbit.co`

Set the server key only in the process environment. Do not add it to this project.

PowerShell:

```powershell
$env:FEATBIT_SERVER_KEY = '<your server key>'
cargo run --manifest-path .\examples\openfeature-only\Cargo.toml -- `
  '<flag-key>' bool 'real-test-user' country=CN plan=pro
Remove-Item Env:FEATBIT_SERVER_KEY
```

Bash:

```bash
FEATBIT_SERVER_KEY='<your server key>' \
  cargo run --manifest-path examples/openfeature-only/Cargo.toml -- \
  '<flag-key>' bool 'real-test-user' country=CN plan=pro
```

The positional arguments are:

1. flag key (required)
2. flag type: `bool`, `int`, `float`, `string`, or `struct` (default: `bool`)
3. OpenFeature targeting key (default: `openfeature-example-user`)
4. zero or more string context attributes in `name=value` form

For example, evaluate a string flag:

```powershell
cargo run --manifest-path .\examples\openfeature-only\Cargo.toml -- `
  'checkout-message' string 'user-123' country=CN
```

A successful evaluation prints the value, FeatBit variation ID, and OpenFeature reason. A missing
flag, wrong type, invalid key, or failed initial connection exits with the corresponding
OpenFeature error code.

The process stays running after the initial evaluation. It evaluates the flag once per second and
prints a new block only when the value, variation ID, reason, or error state changes. Update the
flag in FeatBit and watch the terminal; press `Ctrl+C` to stop and shut down the provider.

Set `OPENFEATURE_POLL_INTERVAL_MS` to change the polling interval (minimum `100`; default `1000`).
Every poll is a real OpenFeature evaluation and therefore produces normal FeatBit evaluation
analytics when event collection is enabled.

Override the default cloud endpoints with `FEATBIT_STREAMING_URL` and `FEATBIT_EVENT_URL` when
testing another FeatBit deployment.
