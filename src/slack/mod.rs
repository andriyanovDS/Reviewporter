use color_eyre::Result;
use futures::TryFutureExt;
use reqwest::{header::AUTHORIZATION, Client};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

pub struct SlackApi<'a> {
    token: &'a str,
    team_id: &'a str,
    usergroup_id: &'a str,
    base_url: Url,
    client: Client,
}

#[derive(Deserialize, Debug)]
struct User {
    #[serde(default)]
    id: String,
    #[serde(rename = "real_name")]
    pub name: String,
    status_text: String,
}

impl User {
    fn is_on_vacation(&self) -> bool {
        self.status_text == "Vacationing"
    }
}

#[derive(Deserialize, Debug)]
struct UserContainer {
    profile: User,
}

#[derive(Deserialize, Debug)]
struct UsergroupUsers {
    users: Vec<String>,
}

#[derive(Serialize)]
struct PostMessagePayload {
    text: String,
    channel: String,
}

#[derive(Deserialize)]
struct PostMessageResponse {
    ok: bool,
    error: Option<String>,
}

impl<'a> SlackApi<'a> {
    pub fn new(token: &'a str, team_id: &'a str, usergroup_id: &'a str) -> Self {
        Self {
            token,
            team_id,
            usergroup_id,
            base_url: Url::parse("https://slack.com/api/")
                .expect("Failed to create Slack base URL"),
            client: Client::new(),
        }
    }

    pub async fn send_message(&self, user_id: String, message: String) -> Result<()> {
        let url = self.base_url.join("chat.postMessage")?;
        tracing::info!("Sending message to {user_id}.");

        let payload = PostMessagePayload {
            text: message,
            channel: user_id.clone(),
        };
        let request = self
            .client
            .post(url)
            .json(&payload)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .build()?;

        let response = self.client.execute(request).await?;
        let response = response.json::<PostMessageResponse>().await?;
        if response.ok {
            tracing::info!("Message successfully sent to {user_id}.");
            Ok(())
        } else {
            tracing::info!(
                "Message sent to {user_id} failed with error: {:?}.",
                response.error
            );
            Err(color_eyre::Report::msg(response.error.unwrap()))
        }
    }

    pub async fn obtain_users(&self) -> Result<HashMap<String, String>> {
        let user_list = self.obtain_user_list().await?;
        let requests = user_list
            .into_iter()
            .map(|user_id| self.obtain_user_info(user_id));

        let iter = futures::future::try_join_all(requests)
            .await?
            .into_iter()
            .filter(|user| !user.is_on_vacation())
            .map(|u| (u.name, u.id));

        Ok(iter.collect())
    }

    async fn obtain_user_info(&self, user_id: String) -> Result<User> {
        let mut url = self.base_url.join("users.profile.get")?;
        let query = [("user", user_id.as_str())];
        url.query_pairs_mut().extend_pairs(query);

        self.make_request::<UserContainer>(url)
            .map_ok(move |r| {
                let mut user = r.profile;
                user.id = user_id;
                user
            })
            .await
    }

    async fn obtain_user_list(&self) -> Result<Vec<String>> {
        let mut url = self.base_url.join("usergroups.users.list")?;
        let query = [("usergroup", self.usergroup_id)];
        url.query_pairs_mut().extend_pairs(query);

        self.make_request::<UsergroupUsers>(url)
            .map_ok(|r| r.users)
            .await
    }

    async fn make_request<T: DeserializeOwned>(&self, mut url: Url) -> Result<T> {
        let query = [("team_id", self.team_id)];
        url.query_pairs_mut().extend_pairs(query);
        tracing::info!("Executing GET request with url: {url}");

        let request = self
            .client
            .get(url)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .build()?;

        self.client
            .execute(request)
            .await?
            .json()
            .await
            .map_err(color_eyre::Report::new)
    }
}
