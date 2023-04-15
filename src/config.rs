use crate::slack::SlackApi;
use serde::Deserialize;

use super::azure::AzureApi;

#[derive(Deserialize, Debug)]
struct AzureConfig {
    base_url: url::Url,
    token: String,
    project: String,
    team_name: String,
    repositories: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct SlackConfig {
    token: String,
    team_id: String,
    usergroup_id: String,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    azure: AzureConfig,
    slack: SlackConfig,
}

impl Config {
    pub fn azure_api(&self) -> AzureApi {
        let config = &self.azure;
        AzureApi::new(
            &config.token,
            &config.base_url,
            &config.project,
            &config.team_name,
            &config.repositories,
        )
    }

    pub fn slack_api(&self) -> SlackApi {
        let config = &self.slack;
        SlackApi::new(&config.token, &config.team_id, &config.usergroup_id)
    }
}
