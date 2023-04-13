use std::error::Error;
use std::fmt::Display;

use chrono::{DateTime, Utc};
use color_eyre::Result;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use url::Url;

struct PullRequest {
    title: String,
    creation_date: DateTime<Utc>,
    link: Url,
}

#[derive(Deserialize, Debug)]
struct Identifier(String);

#[derive(Deserialize, Debug)]
struct Identity {
    id: Identifier,
    #[serde(rename = "displayName")]
    name: String,
}

#[derive(Deserialize, Debug)]
pub struct Reviewer {
    identity: Identity,
}

#[derive(Deserialize, Debug)]
struct Team {
    id: Identifier,
    name: String,
}

#[derive(Deserialize, Debug)]
struct Response<T> {
    value: T,
}

pub struct AzureHostingService {
    token: String,
    base_url: Url,
    project: String,
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

impl AzureHostingService {
    pub fn new(token: String, base_url: Url, project: String) -> Self {
        Self {
            token,
            base_url,
            project,
            client: Client::new(),
        }
    }

    pub async fn pull_requests(&self) -> Result<()> {
        let teams = self.get_teams().await?;
        let dev_team = teams.into_iter().find(|v| v.name == "iOS Developers Team");
        let Some(dev_team) = dev_team else {
            tracing::info!("Team was not found.");
            return Ok(());
        };
        let members = self.team_members(dev_team.name).await?;
        Ok(())
    }

    async fn team_members(&self, team_id: String) -> Result<Vec<Reviewer>> {
        let url = self.base_url.join(&format!(
            "_apis/projects/{}/teams/{}/members",
            self.project, team_id
        ))?;
        self.make_request::<Vec<Reviewer>>(url, ApiVersion::Six)
            .await
            .map(|v| v.value)
    }

    async fn get_teams(&self) -> Result<Vec<Team>> {
        let url = self
            .base_url
            .join(&format!("_apis/projects/{}/teams", self.project))?;
        self.make_request::<Vec<Team>>(url, ApiVersion::SixPreview3)
            .await
            .map(|v| v.value)
    }

    async fn make_request<T: DeserializeOwned>(
        &self,
        mut url: Url,
        api_version: ApiVersion,
    ) -> Result<Response<T>> {
        tracing::info!("Executing GET request with url: {url}");
        url.set_query(Some(api_version.query()));
        let request = self
            .client
            .get(url)
            .header("Authorization", format!("Basic {}", self.token))
            .build()?;
        let response = self.client.execute(request).await?;
        let response = response.json::<Response<T>>().await;
        response.map_err(color_eyre::Report::new)
    }
}