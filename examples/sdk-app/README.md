# Rototo SDK App

This sample embeds the Rototo Rust SDK in an application. It loads the
`examples/basic` package once, validates an evaluation context, resolves
qualifiers, resolves variables, and deserializes resolved JSON values into
typed Rust structs.

The SDK load and resolve calls are async, so the sample uses Tokio.

Run it from the `sdk-package-api` directory:

```sh
cargo run --manifest-path examples/sdk-app/Cargo.toml
```

The app resolves:

- `premium_users` and `enterprise_accounts` qualifiers
- `checkout_redesign`, a catalog-backed variable
- `llm_agent_config`, a catalog-backed variable loaded from
  `catalogs/llm-agent-config-entries/*.toml`
- `support_banner`, a catalog-backed operational banner variable

Applications should use `Package::load` instead of shelling out to the CLI so
package lint, context validation, qualifier evaluation, and variable
resolution all happen in process with typed error handling.
