mod auth_flow;
mod client;
mod error;
mod source;

const GITHUB_USER_AGENT: &str = "rototo-console";
const GITHUB_API: &str = "https://api.github.com";

pub use self::auth_flow::{DevicePoll, exchange_github_code, poll_device_flow, start_device_flow};
pub use self::client::{GitHubClient, RefComparison};
pub use self::error::{GitHubError, GitHubResult, github_error_message};
pub use self::source::{
    parse_repo_spec, stable_workspace_key, workspace_archive_source, workspace_repo_path,
};
