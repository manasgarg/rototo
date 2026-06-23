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
    GitHubRepoIdentity, package_repo_path, parse_repo_spec, repo_identity_from_source,
    stable_package_key,
};
