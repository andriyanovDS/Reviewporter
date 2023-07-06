use self::azure::AddReviewersService;
use self::azure::ReviewerRequestsProvider;
use color_eyre::{Report, Result};
use config::Config;
use std::fs::File;
use std::{io::Read, path::Path};

mod azure;
pub mod cli;
mod config;
mod slack;

pub async fn add_reviewers(
    config_path: &Path,
    pull_request_id: String,
    repository_id: String,
) -> Result<()> {
    let config: Config = config_path.try_into()?;

    let slack_api = config.slack_api();
    let users = slack_api.obtain_users().await?;
    tracing::info!("Slack users: {users:?}");

    let add_reviewers_service = config.add_reviewers_service(pull_request_id, repository_id);
    add_reviewers_service
        .add_reviewers(|name| !users.contains_key(name))
        .await
}

pub async fn send_reports(repositories: Vec<String>, config_path: &Path) -> Result<()> {
    let config: Config = config_path.try_into()?;

    let slack_api = config.slack_api();
    let users = slack_api.obtain_users().await?;
    tracing::info!("Slack users: {users:?}");

    let pull_requests_provider = config.pull_requests_provider(repositories);
    let send_requests = pull_requests_provider
        .pull_requests(|name| users.contains_key(name))
        .await?
        .into_iter()
        .filter_map(|r| {
            let Some(id) = users.get(&r.reviewer_name) else {
                return None;
            };
            let request = slack_api.send_message(id.clone(), r.to_string());
            Some(request)
        });
    futures::future::try_join_all(send_requests).await?;
    tracing::info!("All messages were sent.");
    Ok(())
}

impl TryFrom<&Path> for Config {
    type Error = Report;
    fn try_from(value: &Path) -> std::result::Result<Self, Report> {
        let mut file = File::open(value)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        toml::from_str::<Config>(&content).map_err(Report::from)
    }
}
