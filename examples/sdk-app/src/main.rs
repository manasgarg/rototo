use std::error::Error;
use std::path::Path;

use rototo::{ResolveContext, Workspace};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CheckoutPage {
    variant: String,
    heading: String,
    subheading: String,
    image_url: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct LlmConfig {
    model: String,
    gateway: String,
    prompt: String,
    max_output_tokens: u32,
    temperature: f32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../basic");
    let workspace = Workspace::load(workspace_root.to_string_lossy()).await?;
    let context = ResolveContext::from_json(serde_json::json!({
        "lane": "prod",
        "user": {
            "id": "user-123",
            "tier": "premium",
            "role": "admin"
        },
        "account": {
            "plan": "enterprise",
            "seats": 250
        },
        "cart": {
            "total_usd": 300
        },
        "request": {
            "country": "DE"
        }
    }))?;

    let premium_users = workspace.resolve_qualifier("premium-users", &context).await?;
    let enterprise_accounts = workspace
        .resolve_qualifier("enterprise-accounts", &context)
        .await?;

    let checkout = workspace
        .resolve_variable("checkout-redesign", &context)
        .await?;
    let checkout: CheckoutPage = serde_json::from_value(checkout.value)?;

    let llm_config = workspace
        .resolve_variable("llm-agent-config", &context)
        .await?;
    let llm_config: LlmConfig = serde_json::from_value(llm_config.value)?;

    let message = workspace
        .resolve_variable("premium-message", &context)
        .await?;
    let message: String = serde_json::from_value(message.value)?;

    println!("premium-users: {}", premium_users.value);
    println!("enterprise-accounts: {}", enterprise_accounts.value);
    println!();
    println!("checkout variant: {}", checkout.variant);
    println!("checkout heading: {}", checkout.heading);
    println!("checkout subheading: {}", checkout.subheading);
    println!("checkout image: {}", checkout.image_url);
    println!("checkout content: {}", checkout.content);
    println!();
    println!("agent model: {}", llm_config.model);
    println!("agent gateway: {}", llm_config.gateway);
    println!("agent max output tokens: {}", llm_config.max_output_tokens);
    println!("agent temperature: {}", llm_config.temperature);
    println!("agent prompt: {}", llm_config.prompt);
    println!();
    println!("message: {message}");

    Ok(())
}
