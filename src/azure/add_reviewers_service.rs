use super::api::{
    AzurePullRequestService, AzureTeamService, Identifier, NewPullRequestReviewer,
    PullRequestStatus, TeamMember,
};
use async_trait::async_trait;
use color_eyre::Result;
use itertools::Itertools;
use rand::prelude::*;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize, Debug)]
pub struct AzureTeam {
    name: String,
    required_reviewers_team: Option<String>,
}

pub struct ReviewersConfig<'a> {
    required_reviewers_count: usize,
    teams: &'a [AzureTeam],
}

impl<'a> ReviewersConfig<'a> {
    pub fn new(required_reviewers_count: usize, teams: &'a [AzureTeam]) -> Self {
        Self {
            required_reviewers_count,
            teams,
        }
    }
}

#[async_trait]
pub trait AddReviewersService {
    async fn add_reviewers<F>(&self, is_on_vacation: F) -> Result<()>
    where
        F: Fn(&str) -> bool + Send + Sync;
}

type TeamMembersShuffler =
    fn(Vec<TeamMember>, Vec<TeamMember>) -> (Vec<TeamMember>, Vec<TeamMember>);

pub struct AddReviewersServiceImpl<'a, Api>
where
    Api: AzureTeamService,
    Api: AzurePullRequestService,
{
    api: Api,
    team_name: &'a str,
    pull_request_id: String,
    repository_id: String,
    config: ReviewersConfig<'a>,
    shuffle_teams: TeamMembersShuffler,
}

impl<'a, Api> AddReviewersServiceImpl<'a, Api>
where
    Api: AzureTeamService,
    Api: AzurePullRequestService,
{
    pub fn new(
        api: Api,
        team_name: &'a str,
        pull_request_id: String,
        repository_id: String,
        config: ReviewersConfig<'a>,
    ) -> Self {
        Self {
            api,
            team_name,
            pull_request_id,
            repository_id,
            config,
            shuffle_teams: shuffle_teams_members,
        }
    }

    #[cfg(test)]
    pub fn new_with_shuffler(
        api: Api,
        team_name: &'a str,
        pull_request_id: String,
        repository_id: String,
        config: ReviewersConfig<'a>,
        shuffle_teams: TeamMembersShuffler,
    ) -> Self {
        Self {
            api,
            team_name,
            pull_request_id,
            repository_id,
            config,
            shuffle_teams,
        }
    }

    async fn add_required_reviwers<F>(
        &self,
        reviwers: &mut Vec<Identifier>,
        author_id: &Identifier,
        can_be_added: F,
    ) -> Result<()>
    where
        F: Fn(&TeamMember) -> bool,
    {
        let (team_members, team) = self.find_author_dev_team_members(author_id).await?;

        let required_reviewers_team = team.and_then(|team| team.required_reviewers_team.clone());
        let required_reviewers = if let Some(team_name) = required_reviewers_team {
            tracing::info!("Required reviewers team is {team_name}.");
            self.api.team_members(Identifier(team_name)).await?
        } else {
            tracing::info!("Author's team does not have requied reviwers.");
            vec![]
        };

        let (team_members, required_reviewers) =
            (self.shuffle_teams)(team_members, required_reviewers);

        let team_members_ids = team_members
            .iter()
            .map(|m| m.id.clone())
            .collect::<HashSet<_>>();

        team_members
            .into_iter()
            .filter(|member| can_be_added(member))
            .interleave(
                required_reviewers.into_iter().filter(|member| {
                    can_be_added(member) && !team_members_ids.contains(&member.id)
                }),
            )
            .for_each(|reviwer| reviwers.push(reviwer.id));

        Ok(())
    }

    async fn find_author_dev_team_members(
        &self,
        author_id: &Identifier,
    ) -> Result<(Vec<TeamMember>, Option<&AzureTeam>)> {
        for team in self.config.teams {
            let members = self.api.team_members(Identifier(team.name.clone())).await?;
            if members.iter().any(|member| &member.id == author_id) {
                tracing::info!("PR author found in {} team.", team.name);
                return Ok((members, Some(team)));
            }
        }
        tracing::warn!("Pull request author is not added to any of the dev groups.");
        Ok((Vec::new(), None))
    }
}

#[async_trait]
impl<'a, Api> AddReviewersService for AddReviewersServiceImpl<'a, Api>
where
    Api: AzureTeamService + Sync + Send,
    Api: AzurePullRequestService + Sync + Send,
{
    async fn add_reviewers<F>(&self, is_on_vacation: F) -> Result<()>
    where
        F: Fn(&str) -> bool + Send + Sync,
    {
        let all_members = self
            .api
            .team_members(Identifier(self.team_name.to_string()));
        let pull_request = self
            .api
            .obtain_pull_request(&self.repository_id, &self.pull_request_id);
        let (all_members, pull_request) =
            futures::future::try_join(all_members, pull_request).await?;

        if PullRequestStatus::Active != pull_request.status {
            tracing::warn!(
                "Pull request is not active. Current staus is {:?}. Reviewers can not be added.",
                pull_request.status
            );
            return Ok(());
        }

        tracing::info!("Received pull request: {pull_request:?}");

        let author_id = &pull_request.created_by.id;
        let existing_reviewers = pull_request
            .reviewers
            .iter()
            .map(|v| v.id.clone())
            .collect::<HashSet<_>>();

        let mut new_reviewers: Vec<Identifier> = vec![];
        let required_reviwers_count = pull_request
            .reviewers
            .iter()
            .filter(|r| r.is_required)
            .count();

        let required_reviwers_left = self
            .config
            .required_reviewers_count
            .saturating_sub(required_reviwers_count);

        if required_reviwers_left > 0 {
            self.add_required_reviwers(&mut new_reviewers, author_id, |member| {
                let id = &member.id;
                author_id != id
                    && !existing_reviewers.contains(id)
                    && !is_on_vacation(member.name.as_str())
            })
            .await?;
        }

        let new_reviewers_set = new_reviewers.iter().cloned().collect::<HashSet<_>>();

        let (on_vacation, not_on_vacation): (Vec<_>, Vec<_>) = all_members
            .into_iter()
            .partition(|member| is_on_vacation(&member.name));

        not_on_vacation
            .into_iter()
            .chain(on_vacation.into_iter())
            .filter(|member| {
                let id = &member.id;
                author_id != id
                    && !new_reviewers_set.contains(id)
                    && !existing_reviewers.contains(id)
            })
            .for_each(|member| new_reviewers.push(member.id));

        let new_reviwers = new_reviewers
            .into_iter()
            .enumerate()
            .map(|(index, id)| NewPullRequestReviewer {
                id,
                is_required: required_reviwers_left.saturating_sub(index) > 0,
            })
            .collect::<Vec<_>>();

        tracing::info!("New reviewers will be added: {new_reviwers:?}");
        self.api
            .add_reviewers_to_pull_request(&self.repository_id, &self.pull_request_id, new_reviwers)
            .await
    }
}

fn shuffle_teams_members(
    mut first_team: Vec<TeamMember>,
    mut second_team: Vec<TeamMember>,
) -> (Vec<TeamMember>, Vec<TeamMember>) {
    let mut rng = rand::thread_rng();
    first_team.shuffle(&mut rng);
    second_team.shuffle(&mut rng);
    (first_team, second_team)
}

#[cfg(test)]
mod test {
    use crate::azure::api::{PullRequestAuthor, PullRequestReviewer};

    use super::super::api::{
        AzurePullRequestService, AzureTeamService, NewPullRequestReviewer, PullRequest, Team,
        TeamMember, Vote,
    };
    use super::*;
    use async_trait::async_trait;
    use chrono::DateTime;
    use mockall::mock;
    use mockall::predicate::eq;
    use std::ops::Range;
    use url::Url;

    mock! {
        Api {}

        #[async_trait]
        impl AzureTeamService for Api {
            async fn team_members(&self, team_id: Identifier) -> Result<Vec<TeamMember>>;
            async fn get_teams(&self) -> Result<Vec<Team>>;
        }

        #[async_trait]
        impl AzurePullRequestService for Api {
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
    }

    struct Stubs;

    impl Stubs {
        fn team_name() -> &'static str {
            "fake_team_name"
        }
        fn pull_request_id() -> &'static str {
            "fake_pull_request_id"
        }
        fn repository_id() -> &'static str {
            "fake_repository_id"
        }
        fn required_reviewers_team_id() -> &'static str {
            "fake_required_reviewers_team_id"
        }
        fn teams() -> Vec<AzureTeam> {
            vec![
                AzureTeam {
                    name: "Team_1".to_string(),
                    required_reviewers_team: Some("fake_required_reviewers_team_id".to_string()),
                },
                AzureTeam {
                    name: "Team_2".to_string(),
                    required_reviewers_team: None,
                },
            ]
        }
        fn config(required_reviewers_count: usize, teams: &[AzureTeam]) -> ReviewersConfig<'_> {
            ReviewersConfig {
                required_reviewers_count,
                teams,
            }
        }
    }

    struct MockApiBuilder {
        all_team_members: Range<usize>,
        team_members: Range<usize>,
        required_reviewers: Range<usize>,
        new_reviewers: Vec<NewPullRequestReviewer>,
        pull_request: PullRequest,
    }

    impl MockApiBuilder {
        fn new(
            existing_reviewers: Vec<PullRequestReviewer>,
            new_reviewers: impl Iterator<Item = NewPullRequestReviewer>,
        ) -> Self {
            Self::new_with_on_vacation(existing_reviewers, new_reviewers)
        }

        fn new_with_on_vacation(
            existing_reviewers: Vec<PullRequestReviewer>,
            new_reviewers: impl Iterator<Item = NewPullRequestReviewer>,
        ) -> Self {
            Self {
                all_team_members: 0..10,
                team_members: 0..3,
                required_reviewers: 3..5,
                pull_request: PullRequest::new(existing_reviewers),
                new_reviewers: new_reviewers.collect_vec(),
            }
        }

        fn team_members(mut self, reviewers: Range<usize>) -> Self {
            self.team_members = reviewers;
            self
        }

        fn build(self) -> MockApi {
            let mut api = MockApi::new();
            let all_team_members = self
                .all_team_members
                .into_iter()
                .map(Identifier::from)
                .map(TeamMember::new)
                .collect::<Vec<TeamMember>>();

            let required_reviewers = all_team_members[self.required_reviewers].to_vec();
            api.expect_team_members()
                .with(eq(Identifier(
                    Stubs::required_reviewers_team_id().to_string(),
                )))
                .times(1)
                .return_once(move |_| Ok(required_reviewers));

            let team_reviewers = all_team_members[self.team_members].to_vec();
            api.expect_team_members()
                .with(eq(Identifier(
                    Stubs::teams().first().map(|t| t.name.clone()).unwrap(),
                )))
                .times(1)
                .return_once(move |_| Ok(team_reviewers));

            api.expect_team_members()
                .with(eq(Identifier(Stubs::team_name().to_string())))
                .times(1)
                .return_once(move |_| Ok(all_team_members));

            api.expect_obtain_pull_request()
                .with(eq(Stubs::repository_id()), eq(Stubs::pull_request_id()))
                .times(1)
                .return_once(|_, _| Ok(self.pull_request));

            api.expect_add_reviewers_to_pull_request()
                .with(
                    eq(Stubs::repository_id()),
                    eq(Stubs::pull_request_id()),
                    eq(self.new_reviewers),
                )
                .times(1)
                .returning(|_, _, _| Ok(()));

            api
        }
    }

    #[tokio::test]
    async fn new_reviewers_added() -> Result<()> {
        let expected_reviewers = [1, 3]
            .into_iter()
            .map(|id| NewPullRequestReviewer {
                id: Identifier::from(id),
                is_required: true,
            })
            .chain(
                [2, 4, 5, 6, 7, 8, 9]
                    .into_iter()
                    .map(NewPullRequestReviewer::from),
            );

        run_test(MockApiBuilder::new(vec![], expected_reviewers), |_| false).await
    }

    #[tokio::test]
    async fn existing_reviewers_not_added() -> Result<()> {
        let existing_reviewers = [1, 4, 8]
            .into_iter()
            .map(PullRequestReviewer::from)
            .collect();

        let expected_reviewers = [2, 3]
            .into_iter()
            .map(|id| NewPullRequestReviewer {
                id: Identifier::from(id),
                is_required: true,
            })
            .chain([5, 6, 7, 9].into_iter().map(NewPullRequestReviewer::from));

        run_test(
            MockApiBuilder::new(existing_reviewers, expected_reviewers),
            |_| false,
        )
        .await
    }

    #[tokio::test]
    async fn existing_required_reviewers_taken_into_account() -> Result<()> {
        let existing_reviewers = [(1, false), (4, false), (9, true)]
            .into_iter()
            .map(PullRequestReviewer::from)
            .collect();

        let expected_reviewers = std::iter::once(NewPullRequestReviewer {
            id: Identifier::from(2),
            is_required: true,
        })
        .chain(
            [3, 5, 6, 7, 8]
                .into_iter()
                .map(NewPullRequestReviewer::from),
        );
        run_test(
            MockApiBuilder::new(existing_reviewers, expected_reviewers),
            |_| false,
        )
        .await
    }

    #[tokio::test]
    async fn reviewers_on_vacation_not_required() -> Result<()> {
        let expected_reviewers = [7, 8]
            .into_iter()
            .map(|id| NewPullRequestReviewer {
                id: Identifier::from(id),
                is_required: true,
            })
            .chain(
                std::iter::once(9)
                    .chain((1..=6).into_iter())
                    .map(NewPullRequestReviewer::from),
            );

        run_test(MockApiBuilder::new(vec![], expected_reviewers), |name| {
            name.parse::<usize>().unwrap() <= 6
        })
        .await
    }

    #[tokio::test]
    async fn new_reviewers_unique() -> Result<()> {
        let expected_reviewers = [1, 4]
            .into_iter()
            .map(|id| NewPullRequestReviewer {
                id: Identifier::from(id),
                is_required: true,
            })
            .chain(
                (2..4)
                    .into_iter()
                    .chain((5..10).into_iter())
                    .map(NewPullRequestReviewer::from),
            );

        let builder = MockApiBuilder::new(vec![], expected_reviewers).team_members(0..4);
        run_test(builder, |_| false).await
    }

    async fn run_test<OnVacation>(
        api_builder: MockApiBuilder,
        is_on_vacation: OnVacation,
    ) -> Result<()>
    where
        OnVacation: Fn(&str) -> bool + Send + Sync,
    {
        let developer_teams = Stubs::teams();
        let service = AddReviewersServiceImpl::new_with_shuffler(
            api_builder.build(),
            Stubs::team_name(),
            Stubs::pull_request_id().to_string(),
            Stubs::repository_id().to_string(),
            Stubs::config(2, &developer_teams),
            fake_shuffle_teams,
        );
        let result = service.add_reviewers(is_on_vacation).await;
        assert!(result.is_ok());

        Ok(())
    }

    impl TeamMember {
        fn new(id: Identifier) -> Self {
            let name = id.0.clone();
            Self {
                id,
                name,
                is_container: false,
            }
        }
    }

    impl From<usize> for Identifier {
        fn from(value: usize) -> Self {
            Self(value.to_string())
        }
    }

    impl From<usize> for NewPullRequestReviewer {
        fn from(value: usize) -> Self {
            Self {
                id: Identifier::from(value),
                is_required: false,
            }
        }
    }

    impl From<usize> for PullRequestReviewer {
        fn from(value: usize) -> Self {
            Self {
                id: Identifier::from(value),
                name: value.to_string(),
                is_required: false,
                vote: Vote::NoVote,
                has_declined: false,
            }
        }
    }

    impl From<(usize, bool)> for PullRequestReviewer {
        fn from(value: (usize, bool)) -> Self {
            Self {
                id: Identifier::from(value.0),
                name: value.0.to_string(),
                is_required: value.1,
                vote: Vote::NoVote,
                has_declined: false,
            }
        }
    }

    impl PullRequest {
        fn new(reviewers: Vec<PullRequestReviewer>) -> Self {
            Self {
                id: 0,
                title: Default::default(),
                url: Url::parse("http://some.co").unwrap(),
                created_by: PullRequestAuthor {
                    id: Identifier::from(0),
                    name: Default::default(),
                },
                creation_date: DateTime::default(),
                reviewers,
                status: PullRequestStatus::Active,
            }
        }
    }

    fn fake_shuffle_teams(
        first_team: Vec<TeamMember>,
        second_team: Vec<TeamMember>,
    ) -> (Vec<TeamMember>, Vec<TeamMember>) {
        (first_team, second_team)
    }
}
