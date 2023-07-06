use async_trait::async_trait;
use chrono::{DateTime, Utc};
use color_eyre::Result;
use futures::TryFutureExt;
use reqwest::RequestBuilder;
use reqwest::{header::AUTHORIZATION, Client};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_repr::Deserialize_repr;
use std::fmt::{Display, Formatter};
use url::Url;

#[derive(Deserialize, Serialize, Clone, PartialEq, Eq, Debug, Hash)]
pub struct Identifier(pub String);

#[derive(Deserialize, Debug)]
pub struct PullRequestAuthor {
    pub id: Identifier,
    #[serde(rename = "displayName")]
    pub name: String,
}

#[derive(Deserialize_repr, Debug, PartialEq)]
#[repr(i32)]
pub enum Vote {
    Rejected = -10,
    WaitingForAuthor = -5,
    NoVote = 0,
    ApprovedWithSuggestions = 5,
    Approved = 10,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestReviewer {
    pub id: Identifier,
    #[serde(rename = "displayName")]
    pub name: String,
    #[serde(default)]
    pub is_required: bool,
    pub vote: Vote,
    pub has_declined: bool,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PullRequestStatus {
    Abandoned,
    Active,
    NotSet,
    All,
    Completed,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    #[serde(rename = "pullRequestId")]
    pub id: usize,
    pub title: String,
    pub url: Url,
    pub created_by: PullRequestAuthor,
    pub creation_date: DateTime<Utc>,
    pub reviewers: Vec<PullRequestReviewer>,
    pub status: PullRequestStatus,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TeamMember {
    pub id: Identifier,
    #[serde(rename = "displayName")]
    pub name: String,
    #[serde(default)]
    pub is_container: bool,
}

#[derive(Deserialize, Debug)]
struct TeamMemberContainer {
    identity: TeamMember,
}

#[derive(Deserialize, Debug)]
pub struct Team {
    pub name: String,
}

pub enum PullRequestSearchCriteria {
    Reviewer(Identifier),
    Creator(Identifier),
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewPullRequestReviewer {
    pub id: Identifier,
    pub is_required: bool,
}

#[derive(Deserialize, Debug)]
struct ListResponse<T> {
    value: T,
}

pub struct AzureApi<'a> {
    token: &'a str,
    base_url: &'a Url,
    project: &'a str,
    client: Client,
}

enum ApiVersion {
    Six,
    SixPreview3,
}

impl ApiVersion {
    fn query(&self) -> (&'static str, &'static str) {
        match self {
            ApiVersion::Six => ("api-version", "6.0"),
            ApiVersion::SixPreview3 => ("api-version", "6.0-preview.3"),
        }
    }
}

#[derive(Debug)]
struct ResponseError {
    response: String,
}

impl Display for ResponseError {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "{}", self.response)
    }
}

impl std::error::Error for ResponseError {}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait AzureTeamService {
    async fn team_members(&self, team_id: Identifier) -> Result<Vec<TeamMember>>;
    async fn get_teams(&self) -> Result<Vec<Team>>;
}

#[async_trait]
pub trait AzurePullRequestsService {
    async fn obtain_pull_requests<F>(
        &self,
        repository_id: &str,
        search_creteria: PullRequestSearchCriteria,
        filter: F,
    ) -> Result<Vec<PullRequest>>
    where
        F: Fn(&PullRequestReviewer) -> bool,
        F: Send;
}
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait AzurePullRequestService {
    async fn obtain_pull_request(
        &self,
        repository_id: &str,
        pull_request_id: &str,
    ) -> Result<PullRequest>;

    async fn add_reviewers_to_pull_request(
        &self,
        repository_id: &str,
        request_id: &str,
        reviewers: Vec<NewPullRequestReviewer>,
    ) -> Result<()>;
}

impl<'a> AzureApi<'a> {
    pub fn new(token: &'a str, base_url: &'a Url, project: &'a str) -> Self {
        Self {
            token,
            base_url,
            project,
            client: Client::new(),
        }
    }

    fn make_pull_requests_url(
        &self,
        repository_id: &str,
        search_creteria: PullRequestSearchCriteria,
    ) -> Result<Url> {
        let mut url = self.base_url.join(&format!(
            "{}/_apis/git/repositories/{}/pullrequests",
            self.project, repository_id
        ))?;
        let queries = [search_creteria.query(), ("searchCriteria.status", "active")];
        url.query_pairs_mut().extend_pairs(queries);
        Ok(url)
    }

    async fn obtain_single_item<T: DeserializeOwned>(
        &self,
        url: Url,
        api_version: ApiVersion,
    ) -> Result<T> {
        let response = self.send_get_request(url, api_version).await?;
        response.json::<T>().await.map_err(color_eyre::Report::new)
    }

    async fn obtain_list<T: DeserializeOwned>(
        &self,
        url: Url,
        api_version: ApiVersion,
    ) -> Result<Vec<T>> {
        let response = self.send_get_request(url, api_version).await?;
        let response = response.json::<ListResponse<Vec<T>>>().await;
        response.map(|v| v.value).map_err(color_eyre::Report::new)
    }

    async fn send_get_request(
        &self,
        url: Url,
        api_version: ApiVersion,
    ) -> Result<reqwest::Response> {
        self.send_request(url, api_version, |client, url| {
            tracing::debug!("Executing GET request with url: {url}.");
            client.get(url)
        })
        .await
    }

    async fn send_post_request<Body>(
        &self,
        url: Url,
        api_version: ApiVersion,
        body: Body,
    ) -> Result<reqwest::Response>
    where
        Body: Serialize,
    {
        self.send_request(url, api_version, |client, url| {
            tracing::debug!("Executing POST request with url: {url}.");
            client.post(url).json(&body)
        })
        .await
    }

    async fn send_request<F>(
        &self,
        mut url: Url,
        api_version: ApiVersion,
        request_builder_factory: F,
    ) -> Result<reqwest::Response>
    where
        F: FnOnce(&Client, Url) -> RequestBuilder,
    {
        let query = [api_version.query()];
        url.query_pairs_mut().extend_pairs(query);

        let request = request_builder_factory(&self.client, url)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .build()?;

        let response = self
            .client
            .execute(request)
            .await
            .map_err(color_eyre::Report::from)?;

        if !response.status().is_success() {
            let response = response.text().await?;
            Err(color_eyre::Report::from(ResponseError { response }))
        } else {
            Ok(response)
        }
    }
}

#[async_trait]
impl<'a> AzureTeamService for AzureApi<'a> {
    async fn team_members(&self, team_id: Identifier) -> Result<Vec<TeamMember>> {
        tracing::info!("Requesting team {} members.", team_id.0);
        let url = self.base_url.join(&format!(
            "_apis/projects/{}/teams/{}/members",
            self.project, team_id.0
        ))?;
        self.obtain_list::<TeamMemberContainer>(url, ApiVersion::Six)
            .await
            .map(|v| {
                v.into_iter()
                    .filter_map(|v| (!v.identity.is_container).then_some(v.identity))
                    .collect()
            })
    }

    async fn get_teams(&self) -> Result<Vec<Team>> {
        tracing::info!("Requesting teams in project {}.", self.project);
        let url = self
            .base_url
            .join(&format!("_apis/projects/{}/teams", self.project))?;
        self.obtain_list::<Team>(url, ApiVersion::SixPreview3).await
    }
}

#[async_trait]
impl<'a> AzurePullRequestService for AzureApi<'a> {
    async fn obtain_pull_request(
        &self,
        repository_id: &str,
        request_id: &str,
    ) -> Result<PullRequest> {
        tracing::info!("Requesting pull request {request_id} in repository {repository_id}.");
        let url = self.base_url.join(&format!(
            "{}/_apis/git/repositories/{}/pullrequests/{}",
            self.project, repository_id, request_id
        ))?;
        self.obtain_single_item::<PullRequest>(url, ApiVersion::Six)
            .await
    }

    async fn add_reviewers_to_pull_request(
        &self,
        repository_id: &str,
        pull_request_id: &str,
        reviewers: Vec<NewPullRequestReviewer>,
    ) -> Result<()> {
        tracing::info!("Creating reviewers for {pull_request_id} in repository {repository_id}. Reviewers: {reviewers:?}");
        let url = self.base_url.join(&format!(
            "{}/_apis/git/repositories/{}/pullrequests/{}/reviewers",
            self.project, repository_id, pull_request_id
        ))?;
        self.send_post_request(url, ApiVersion::Six, reviewers)
            .map_ok(|_| ())
            .await
    }
}

#[async_trait]
impl<'a> AzurePullRequestsService for AzureApi<'a> {
    async fn obtain_pull_requests<F>(
        &self,
        repository_id: &str,
        search_creteria: PullRequestSearchCriteria,
        filter: F,
    ) -> Result<Vec<PullRequest>>
    where
        F: Fn(&PullRequestReviewer) -> bool,
        F: Send,
    {
        let url = self.make_pull_requests_url(repository_id, search_creteria)?;
        let requests = self
            .obtain_list::<PullRequest>(url, ApiVersion::Six)
            .await?;
        let requests = requests
            .into_iter()
            .filter(|v| v.reviewers.iter().any(&filter))
            .map(|mut request| {
                let request_path = format!(
                    "{}/_git/{}/pullrequest/{}",
                    self.project, repository_id, request.id
                );
                request.url = self
                    .base_url
                    .join(&request_path)
                    .expect("Failed to create PR URL");
                request
            })
            .collect();
        Ok(requests)
    }
}

impl PullRequestSearchCriteria {
    fn query(&self) -> (&str, &str) {
        match self {
            Self::Reviewer(id) => ("searchCriteria.reviewerId", id.0.as_str()),
            Self::Creator(id) => ("searchCriteria.creatorId", id.0.as_str()),
        }
    }
}
