use chrono::{DateTime, Duration, Utc};
use color_eyre::Result;
use futures::TryFutureExt;
use reqwest::{header::AUTHORIZATION, Client};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::fmt::{Display, Formatter};
use url::Url;

#[derive(Deserialize, Clone, PartialEq, Debug)]
struct Identifier(String);

#[derive(Deserialize, Debug)]
struct PullRequestAuthor {
    #[serde(rename = "displayName")]
    name: String,
}

#[derive(Deserialize, Debug)]
enum Vote {
    Rejected = -10,
    WaitingForAuthor = -5,
    NoVote = 0,
    ApprovedWithSuggestions = 5,
    Approved = 10,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PullRequestReviewer {
    id: Identifier,
    #[serde(default)]
    is_required: bool,
    vote: Vote,
    has_declined: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PullRequest {
    title: String,
    url: Url,
    created_by: PullRequestAuthor,
    creation_date: DateTime<Utc>,
    reviewers: Vec<PullRequestReviewer>,
}

#[derive(Deserialize, Debug)]
struct TeamMember {
    id: Identifier,
    #[serde(rename = "displayName")]
    name: String,
    #[serde(default)]
    is_container: bool,
}

#[derive(Deserialize, Debug)]
struct TeamMemberContainer {
    identity: TeamMember,
}

#[derive(Deserialize, Debug)]
struct Team {
    name: String,
}

#[derive(Deserialize, Debug)]
struct Response<T> {
    value: T,
}

struct RepoRequests {
    repo_id: String,
    pull_requests: Vec<PullRequest>,
}

pub struct ReviewerRequests {
    pub reviewer_name: String,
    repo_requests: Vec<RepoRequests>,
}

pub struct AzureApi<'a> {
    token: &'a str,
    base_url: &'a Url,
    project: &'a str,
    team_name: &'a str,
    repositories: &'a [String],
    client: Client,
}

enum ApiVersion {
    Six,
    SixPreview3,
}

impl ApiVersion {
    fn query(&self) -> &'static str {
        match self {
            ApiVersion::Six => "api-version=6.0",
            ApiVersion::SixPreview3 => "api-version=6.0-preview.3",
        }
    }
}

impl<'a> AzureApi<'a> {
    pub fn new(
        token: &'a str,
        base_url: &'a Url,
        project: &'a str,
        team_name: &'a str,
        repositories: &'a [String],
    ) -> Self {
        Self {
            token,
            base_url,
            project,
            team_name,
            repositories,
            client: Client::new(),
        }
    }

    pub async fn pull_requests(&self) -> Result<Vec<ReviewerRequests>> {
        let teams = self.get_teams().await?;
        let dev_team = teams.into_iter().find(|v| v.name == self.team_name);
        let Some(dev_team) = dev_team else {
            tracing::info!("Team was not found.");
            return Ok(vec![]);
        };
        let members = self.team_members(Identifier(dev_team.name)).await?;
        let requests_iter = members.into_iter().map(|reviewer| {
            let requests = self.repositories.iter().map(|repo_id| {
                let repo_id = repo_id.clone();
                self.reviewer_pull_requests(repo_id.clone(), reviewer.id.clone())
                    .map_ok(move |mut pull_requests| {
                        pull_requests.sort_by(|a, b| a.creation_date.cmp(&b.creation_date));
                        RepoRequests {
                            repo_id,
                            pull_requests,
                        }
                    })
            });
            let reviewer_name = reviewer.name.clone();
            futures::future::try_join_all(requests).map_ok(|repo_requests| ReviewerRequests {
                reviewer_name,
                repo_requests: repo_requests
                    .into_iter()
                    .filter(|r| !r.pull_requests.is_empty())
                    .collect(),
            })
        });
        futures::future::try_join_all(requests_iter)
            .map_ok(|reviewers| {
                reviewers
                    .into_iter()
                    .filter(|r| !r.repo_requests.is_empty())
                    .collect()
            })
            .await
    }

    async fn team_members(&self, team_id: Identifier) -> Result<Vec<TeamMember>> {
        let url = self.base_url.join(&format!(
            "_apis/projects/{}/teams/{}/members",
            self.project, team_id.0
        ))?;
        self.make_request::<Vec<TeamMemberContainer>>(url, ApiVersion::Six)
            .await
            .map(|v| {
                v.into_iter()
                    .filter_map(|v| (!v.identity.is_container).then_some(v.identity))
                    .collect()
            })
    }

    async fn get_teams(&self) -> Result<Vec<Team>> {
        let url = self
            .base_url
            .join(&format!("_apis/projects/{}/teams", self.project))?;
        self.make_request::<Vec<Team>>(url, ApiVersion::SixPreview3)
            .await
    }

    async fn reviewer_pull_requests(
        &self,
        repository_id: String,
        reviewer_id: Identifier,
    ) -> Result<Vec<PullRequest>> {
        let mut url = self.base_url.join(&format!(
            "{}/_apis/git/repositories/{}/pullrequests",
            self.project, repository_id
        ))?;
        let queries = [
            ("searchCriteria.reviewerId", reviewer_id.0.as_str()),
            ("searchCriteria.status", "active"),
        ];
        url.query_pairs_mut().extend_pairs(queries);
        let requests = self
            .make_request::<Vec<PullRequest>>(url, ApiVersion::Six)
            .await?;
        let requests = requests
            .into_iter()
            .filter(|v| v.reviewers.iter().any(|r| r.should_be_shown(&reviewer_id)))
            .collect();
        Ok(requests)
    }

    async fn make_request<T: DeserializeOwned>(
        &self,
        mut url: Url,
        api_version: ApiVersion,
    ) -> Result<T> {
        let query = [api_version.query()];
        url.query_pairs_mut()
            .extend_keys_only::<[&str; 1], &str>(query);
        tracing::info!("Executing GET request with url: {url}");

        let request = self
            .client
            .get(url)
            .header(AUTHORIZATION, format!("Basic {}", self.token))
            .build()?;
        let response = self.client.execute(request).await?;
        let response = response.json::<Response<T>>().await;
        response.map(|v| v.value).map_err(color_eyre::Report::new)
    }
}

impl Display for ReviewerRequests {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Hey!")?;
        writeln!(
            f,
            "Just a friendly reminder that there are pull requests waiting for your review."
        )?;
        writeln!(f)?;
        for repository in &self.repo_requests {
            repository.fmt(f)?;
            writeln!(f)?;
        }
        Ok(())
    }
}

impl Display for RepoRequests {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.pull_requests.is_empty() {
            return Ok(());
        }
        writeln!(f, "Repository: {}", self.repo_id)?;
        let date_now = Utc::now();
        for pull_request in &self.pull_requests {
            write!(
                f,
                "- <{}|{}>. Author: {}.",
                pull_request.url, pull_request.title, pull_request.created_by.name
            )?;
            write_formatted_duration(date_now - pull_request.creation_date, f);
            writeln!(f)?;
        }
        Ok(())
    }
}

fn write_formatted_duration(duration: Duration, f: &mut Formatter<'_>) {
    let mut append_value = |value: i64, label: &str| {
        if value > 0 {
            write!(f, " {}{}", value, label).unwrap();
        }
    };
    let days = duration.num_days();
    append_value(days, "d");
    append_value(duration.num_hours() % 24, "h");
    append_value(duration.num_minutes() % 60, "m");
    write!(f, " ago").unwrap();

    if days > 0 {
        write!(f, " 🔥").unwrap();
    }
}

impl PullRequestReviewer {
    fn should_be_shown(&self, user_id: &Identifier) -> bool {
        if &self.id != user_id || !self.is_required || self.has_declined {
            return false;
        }
        match &self.vote {
            Vote::NoVote | Vote::WaitingForAuthor => true,
            Vote::Rejected | Vote::Approved | Vote::ApprovedWithSuggestions => false,
        }
    }
}
