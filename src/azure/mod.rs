use chrono::{DateTime, Duration, Utc};
use color_eyre::Result;
use futures::TryFutureExt;
use reqwest::{header::AUTHORIZATION, Client};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_repr::Deserialize_repr;
use std::fmt::{Display, Formatter};
use url::Url;
use html_escape;

#[derive(Deserialize, Clone, PartialEq, Debug)]
struct Identifier(String);

#[derive(Deserialize, Debug)]
struct PullRequestAuthor {
    #[serde(rename = "displayName")]
    name: String,
}

#[derive(Deserialize_repr, Debug, PartialEq)]
#[repr(i32)]
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
    #[serde(rename = "displayName")]
    name: String,
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
    #[serde(rename = "pullRequestId")]
    id: usize,
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

enum PullRequestSearchCriteria {
    Reviewer(Identifier),
    Creator(Identifier),
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
    waiting_for_review: Vec<RepoRequests>,
    waiting_by_reviewers: Vec<RepoRequests>,
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

    pub async fn pull_requests<F>(&self, include_user: F) -> Result<Vec<ReviewerRequests>>
    where
        F: Fn(&str) -> bool,
    {
        let teams = self.get_teams().await?;
        let dev_team = teams.into_iter().find(|v| v.name == self.team_name);
        let Some(dev_team) = dev_team else {
            tracing::info!("Team was not found.");
            return Ok(vec![]);
        };
        let members = self.team_members(Identifier(dev_team.name)).await?;
        let requests_iter = members
            .into_iter()
            .filter(|member| include_user(&member.name))
            .map(|member| {
                let reviewer_name = member.name;
                let requests = self.repositories.iter().map(|repo_id| {
                    let member_id = member.id.clone();
                    tracing::info!(
                        "Requesting Pull Requests for review in repository {repo_id} for {}.",
                        member.id.0
                    );
                    self.obtain_pull_requests(
                        repo_id,
                        PullRequestSearchCriteria::Reviewer(member_id.clone()),
                        move |r| r.should_be_shown_to_reviewer(&member_id),
                    )
                    .map_ok(move |mut pull_requests| {
                        pull_requests.sort_by(|a, b| a.creation_date.cmp(&b.creation_date));
                        RepoRequests {
                            repo_id: repo_id.clone(),
                            pull_requests,
                        }
                    })
                });
                let member_id = member.id.clone();
                let waiting_by_reviewers = self.repositories.iter().map(move |repo_id| {
                    tracing::info!(
                        "Requesting {} own Pull Requests in repository {repo_id}",
                        member_id.0
                    );
                    self.obtain_pull_requests(
                        repo_id,
                        PullRequestSearchCriteria::Creator(member_id.clone()),
                        |r| r.should_be_shown_to_creator(),
                    )
                    .map_ok(move |mut pull_requests| {
                        pull_requests.sort_by(|a, b| a.creation_date.cmp(&b.creation_date));
                        RepoRequests {
                            repo_id: repo_id.clone(),
                            pull_requests,
                        }
                    })
                });
                futures::future::try_join_all(requests).and_then(|waiting_for_review| {
                    futures::future::try_join_all(waiting_by_reviewers).map_ok(
                        |waiting_by_reviewers| ReviewerRequests {
                            reviewer_name,
                            waiting_for_review: waiting_for_review
                                .into_iter()
                                .filter(|r| !r.pull_requests.is_empty())
                                .collect(),
                            waiting_by_reviewers: waiting_by_reviewers
                                .into_iter()
                                .filter(|r| !r.pull_requests.is_empty())
                                .collect(),
                        },
                    )
                })
            });
        let mut results = Vec::<ReviewerRequests>::new();
        for member_request in requests_iter {
            let result = member_request.await;
            match result {
                Ok(r) if !r.waiting_for_review.is_empty() || !r.waiting_by_reviewers.is_empty() => {
                    results.push(r);
                }
                Ok(r) => {
                    tracing::info!("There're no requests for {:?}", r.reviewer_name);
                }
                Err(error) => {
                    tracing::error!("Failed to obtain Pull Request list with error: {error:?}");
                }
            }
        }
        Ok(results)
    }

    async fn team_members(&self, team_id: Identifier) -> Result<Vec<TeamMember>> {
        tracing::info!("Requesting team {} members.", team_id.0);
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
        tracing::info!("Requesting teams in project {}.", self.project);
        let url = self
            .base_url
            .join(&format!("_apis/projects/{}/teams", self.project))?;
        self.make_request::<Vec<Team>>(url, ApiVersion::SixPreview3)
            .await
    }

    async fn obtain_pull_requests<F>(
        &self,
        repository_id: &str,
        search_creteria: PullRequestSearchCriteria,
        filter: F,
    ) -> Result<Vec<PullRequest>>
    where
        F: Fn(&PullRequestReviewer) -> bool,
    {
        let url = self.make_pull_requests_url(repository_id, search_creteria)?;
        let requests = self
            .make_request::<Vec<PullRequest>>(url, ApiVersion::Six)
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

    async fn make_request<T: DeserializeOwned>(
        &self,
        mut url: Url,
        api_version: ApiVersion,
    ) -> Result<T> {
        let query = [api_version.query()];
        url.query_pairs_mut()
            .extend_keys_only::<[&str; 1], &str>(query);
        tracing::debug!("Executing GET request with url: {url}.");

        let request = self
            .client
            .get(url)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .build()?;
        let response = self.client.execute(request).await?;
        let response = response.json::<Response<T>>().await;
        response.map(|v| v.value).map_err(color_eyre::Report::new)
    }
}

impl Display for ReviewerRequests {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Hey!")?;
        write!(f, "Just a friendly reminder that there are ")?;
        if !self.waiting_for_review.is_empty() {
            writeln!(f, "Pull Requests waiting for your review:")?;
            writeln!(f)?;
            for repository in &self.waiting_for_review {
                repository.format_for_reviewer(f)?;
                writeln!(f)?;
            }
        }
        if !self.waiting_by_reviewers.is_empty() {
            writeln!(f, "Pull Requests where reviewers are waiting for you:")?;
            writeln!(f)?;
            for repository in &self.waiting_by_reviewers {
                repository.format_for_creator(f)?;
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

impl RepoRequests {
    fn format_for_reviewer(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.pull_requests.is_empty() {
            return Ok(());
        }
        writeln!(f, "{}", self.repo_id)?;
        let date_now = Utc::now();
        for pull_request in &self.pull_requests {
            write!(f, "- ")?;
            write_link(f, &pull_request.url, pull_request.title.as_str());
            write!(f, ". Author: {}.", pull_request.created_by.name)?;
            write_formatted_duration(date_now - pull_request.creation_date, f);
            writeln!(f)?;
        }
        Ok(())
    }

    fn format_for_creator(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        assert!(!self.pull_requests.is_empty());
        writeln!(f, "{}", self.repo_id)?;
        let date_now = Utc::now();
        for pull_request in &self.pull_requests {
            write!(f, "- ")?;
            write_link(f, &pull_request.url, pull_request.title.as_str());
            write_formatted_duration(date_now - pull_request.creation_date, f);
            writeln!(f)?;
            write!(f, "Waiting: ")?;
            let waiting_reviewers = pull_request
                .reviewers
                .iter()
                .filter_map(|r| (r.vote == Vote::WaitingForAuthor).then_some(r.name.as_str()));
            for (index, name) in waiting_reviewers.enumerate() {
                if index != 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", name)?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

fn write_link(f: &mut Formatter<'_>, url: &Url, message: &str) {
    write!(f, "<{url}|{}>", html_escape::encode_text(message)).unwrap();
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
    fn should_be_shown_to_reviewer(&self, user_id: &Identifier) -> bool {
        if &self.id != user_id || !self.is_required || self.has_declined {
            return false;
        }
        match &self.vote {
            Vote::NoVote | Vote::WaitingForAuthor => true,
            Vote::Rejected | Vote::Approved | Vote::ApprovedWithSuggestions => false,
        }
    }

    fn should_be_shown_to_creator(&self) -> bool {
        self.vote == Vote::WaitingForAuthor
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
