use color_eyre::Result;
use serde::Deserialize;
use slack::SlackApi;
use std::fs::File;
use std::{io::Read, path::Path};

mod azure;
mod slack;

#[derive(Deserialize, Debug)]
struct Hosting {
    base_url: url::Url,
    token: String,
    project: String,
    repositories: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct Slack {
    token: String,
    team_id: String,
    usergroup_id: String,
}

#[derive(Deserialize, Debug)]
struct Config {
    azure: Hosting,
    slack: Slack,
}

pub async fn run(config_path: &Path) -> Result<()> {
    let mut file = File::open(config_path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let config = toml::from_str::<Config>(&content)?;

    let slack = &config.slack;
    let slack_api = SlackApi::new(&slack.token, &slack.team_id, &slack.usergroup_id);
    let users = slack_api.obtain_users().await?;
    tracing::info!("Slack users: {users:?}");

    let azure = &config.azure;
    if !azure.project.is_empty() {
        let azure = azure::AzureApi::new(
            &azure.token,
            &azure.base_url,
            &azure.project,
            &azure.repositories,
        );
        let send_requests = azure.pull_requests().await?.into_iter().filter_map(|r| {
            let Some(id) = users.get(&r.reviewer_name) else {
                return None;
            };
            let request = slack_api.send_message(id.clone(), r.to_string());
            Some(request)
        });
        futures::future::try_join_all(send_requests).await?;
        tracing::info!("All messages were sent.")
    }
    Ok(())
}
