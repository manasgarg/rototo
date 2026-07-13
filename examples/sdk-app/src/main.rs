use std::error::Error;
use std::path::Path;

use rototo::{EvaluationContext, Package};
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
    let package_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../basic");
    let package = Package::load(package_root.to_string_lossy()).await?;
    let context = EvaluationContext::from_json(serde_json::json!({
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

    let premium_users = package.resolve_variable("premium_users", &context)?;
    let premium_users: bool = serde_json::from_value(premium_users.value)?;
    let enterprise_accounts = package.resolve_variable("enterprise_accounts", &context)?;
    let enterprise_accounts: bool = serde_json::from_value(enterprise_accounts.value)?;

    let checkout = package.resolve_variable("checkout_redesign", &context)?;
    let checkout: CheckoutPage = serde_json::from_value(checkout.value)?;

    let llm_config = package.resolve_variable("llm_agent_config", &context)?;
    let llm_config: LlmConfig = serde_json::from_value(llm_config.value)?;

    let message = package.resolve_variable("premium_message", &context)?;
    let message: String = serde_json::from_value(message.value)?;

    println!("premium_users: {premium_users}");
    println!("enterprise_accounts: {enterprise_accounts}");
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
