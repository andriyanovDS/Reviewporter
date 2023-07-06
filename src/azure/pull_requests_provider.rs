use super::api::{
    AzurePullRequestsService, AzureTeamService, Identifier, PullRequest, PullRequestReviewer,
    PullRequestSearchCriteria, Vote,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use color_eyre::Result;
use futures::TryFutureExt;
use std::fmt::{Display, Formatter};
use url::Url;

struct RepoRequests {
    repo_id: String,
    pull_requests: Vec<PullRequest>,
}

pub struct ReviewerRequests {
    pub reviewer_name: String,
    waiting_for_review: Vec<RepoRequests>,
    waiting_by_reviewers: Vec<RepoRequests>,
}

#[async_trait]
pub trait ReviewerRequestsProvider {
    async fn pull_requests<F>(&self, include_user: F) -> Result<Vec<ReviewerRequests>>
    where
        F: Fn(&str) -> bool + Send + Sync;
}

pub struct AzureReviewerRequestsProvider<'a, Service>
where
    Service: AzureTeamService,
    Service: AzurePullRequestsService,
{
    api: Service,
    team_name: &'a str,
    repositories: Vec<String>,
}

#[async_trait]
impl<'a, Service> ReviewerRequestsProvider for AzureReviewerRequestsProvider<'a, Service>
where
    Service: AzureTeamService + Send + Sync,
    Service: AzurePullRequestsService + Send + Sync,
{
    async fn pull_requests<F>(&self, include_user: F) -> Result<Vec<ReviewerRequests>>
    where
        F: Fn(&str) -> bool + Send + Sync,
    {
        let teams = self.api.get_teams().await?;
        let dev_team = teams.into_iter().find(|v| v.name == self.team_name);
        let Some(dev_team) = dev_team else {
            tracing::info!("Team was not found.");
            return Ok(vec![]);
        };
        let members = self.api.team_members(Identifier(dev_team.name)).await?;
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
                    self.api
                        .obtain_pull_requests(
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
                    self.api
                        .obtain_pull_requests(
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
}

impl<'a, Service> AzureReviewerRequestsProvider<'a, Service>
where
    Service: AzureTeamService,
    Service: AzurePullRequestsService,
{
    pub fn new(api: Service, team_name: &'a str, repositories: Vec<String>) -> Self {
        Self {
            api,
            team_name,
            repositories,
        }
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
