use super::azure::{self, ReviewerRequestsProvider};
use crate::{
    azure::{AddReviewersService, AzureTeam, ReviewersConfig},
    slack::SlackApi,
};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct AzureConfig {
    base_url: url::Url,
    token: String,
    project: String,
    team_name: String,
    pull_request_reviewers: Option<PullRequestReviewersConfig>,
}

#[derive(Deserialize, Debug)]
struct SlackConfig {
    token: String,
    team_id: String,
    usergroup_id: String,
}

#[derive(Deserialize, Debug)]
struct PullRequestReviewersConfig {
    required_reviewers_count: usize,
    teams: Vec<AzureTeam>,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    azure: AzureConfig,
    slack: SlackConfig,
}

impl Config {
    pub fn pull_requests_provider(
        &self,
        repositories: Vec<String>,
    ) -> impl ReviewerRequestsProvider + '_ {
        let config = &self.azure;
        azure::make_pull_requests_provider(
            &config.token,
            &config.base_url,
            &config.project,
            &config.team_name,
            repositories,
        )
    }

    pub fn add_reviewers_service(
        &self,
        pull_request_id: String,
        repository_id: String,
    ) -> impl AddReviewersService + '_ {
        let azure_config = &self.azure;
        let reviewers_config = azure_config
            .pull_request_reviewers
            .as_ref()
            .expect("Config must have [azure.pull_request_reviewers].");
        azure::make_add_reviewers_service(
            &azure_config.token,
            &azure_config.base_url,
            &azure_config.project,
            &azure_config.team_name,
            pull_request_id,
            repository_id,
            ReviewersConfig::new(
                reviewers_config.required_reviewers_count,
                &reviewers_config.teams,
            ),
        )
    }

    pub fn slack_api(&self) -> SlackApi {
        let config = &self.slack;
        SlackApi::new(&config.token, &config.team_id, &config.usergroup_id)
    }
}
