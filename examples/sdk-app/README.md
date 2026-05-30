# Rototo SDK App

This sample embeds the Rototo Rust SDK in an application. It loads the
`examples/basic` workspace once, validates a request context, resolves
qualifiers, resolves variables, and deserializes resolved JSON values into
typed Rust structs.

The SDK load and resolve calls are async, so the sample uses Tokio.

Run it from the `sdk-workspace-api` directory:

```sh
cargo run --manifest-path examples/sdk-app/Cargo.toml
```

The app resolves:

- `premium-users` and `enterprise-accounts` qualifiers
- `checkout-redesign`, an inline schema-backed object variable
- `llm-agent-config`, a schema-backed object variable loaded from
  `llm-agent-config-values/*.toml`
- `directory-backed-message`, a primitive variable loaded from
  `directory-backed-message-values/*.toml`

Applications should use `Workspace::load` instead of shelling out to the CLI so
workspace lint, context validation, qualifier evaluation, and variable
resolution all happen in process with typed error handling.
