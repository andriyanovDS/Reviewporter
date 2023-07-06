pub use self::pull_requests_provider::ReviewerRequestsProvider;
use self::{
    add_reviewers_service::AddReviewersServiceImpl, api::AzureApi,
    pull_requests_provider::AzureReviewerRequestsProvider,
};
pub use add_reviewers_service::{AddReviewersService, AzureTeam, ReviewersConfig};

use url::Url;

mod add_reviewers_service;
mod api;
mod pull_requests_provider;

pub fn make_pull_requests_provider<'a>(
    token: &'a str,
    base_url: &'a Url,
    project: &'a str,
    team_name: &'a str,
    repositories: Vec<String>,
) -> impl ReviewerRequestsProvider + 'a {
    let api = AzureApi::new(token, base_url, project);
    AzureReviewerRequestsProvider::new(api, team_name, repositories)
}

pub fn make_add_reviewers_service<'a>(
    token: &'a str,
    base_url: &'a Url,
    project: &'a str,
    team_name: &'a str,
    pull_request_id: String,
    repository_id: String,
    reviewers_config: ReviewersConfig<'a>,
) -> impl AddReviewersService + 'a {
    let api = AzureApi::new(token, base_url, project);
    AddReviewersServiceImpl::new(
        api,
        team_name,
        pull_request_id,
        repository_id,
        reviewers_config,
    )
}
