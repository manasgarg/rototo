use std::path::Path;

use ring::digest;
use serde::Serialize;

use crate::error::{Result, RototoError};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ActorIdentity {
    GitConfig {
        name: Option<String>,
        email: Option<String>,
    },
    GitHub {
        id: String,
        login: String,
        name: Option<String>,
        avatar_url: Option<String>,
    },
}

impl ActorIdentity {
    pub fn principal_id(&self) -> String {
        match self {
            Self::GitHub { id, .. } => format!("github:{id}"),
            Self::GitConfig { name, email } => {
                let material = format!(
                    "{}\0{}",
                    name.as_deref().unwrap_or_default(),
                    email.as_deref().unwrap_or_default()
                );
                if material != "\0" {
                    return format!("git:{}", short_hash(material.as_bytes()));
                }
                "local:unknown".to_owned()
            }
        }
    }

    pub fn display_login(&self) -> String {
        match self {
            Self::GitHub { login, .. } => login.clone(),
            Self::GitConfig { name, email } => name
                .clone()
                .or_else(|| email.clone())
                .unwrap_or_else(|| "local git".to_owned()),
        }
    }

    pub fn avatar_url(&self) -> Option<String> {
        match self {
            Self::GitHub { avatar_url, .. } => avatar_url.clone(),
            Self::GitConfig { .. } => None,
        }
    }
}

pub async fn resolve_git_config_identity(workdir: Option<&Path>) -> Result<ActorIdentity> {
    let name = git_config_value(workdir, "user.name").await?;
    let email = git_config_value(workdir, "user.email").await?;
    Ok(ActorIdentity::GitConfig { name, email })
}

async fn git_config_value(workdir: Option<&Path>, key: &str) -> Result<Option<String>> {
    let mut command = tokio::process::Command::new("git");
    if let Some(workdir) = workdir {
        command.arg("-C").arg(workdir);
    }
    let output = command
        .args(["config", "--get", key])
        .output()
        .await
        .map_err(|err| RototoError::new(format!("failed to read git config {key}: {err}")))?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!value.is_empty()).then_some(value))
}

fn short_hash(bytes: &[u8]) -> String {
    let digest = digest::digest(&digest::SHA256, bytes);
    digest
        .as_ref()
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
