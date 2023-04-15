use color_eyre::Result;
use config::Config;
use std::fs::File;
use std::{io::Read, path::Path};

mod azure;
mod config;
mod slack;

pub async fn run(config_path: &Path) -> Result<()> {
    let mut file = File::open(config_path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let config = toml::from_str::<Config>(&content)?;

    let slack_api = config.slack_api();
    let users = slack_api.obtain_users().await?;
    tracing::info!("Slack users: {users:?}");

    let azure_api = config.azure_api();
    let send_requests = azure_api
        .pull_requests()
        .await?
        .into_iter()
        .filter_map(|r| {
            let Some(id) = users.get(&r.reviewer_name) else {
            return None;
        };
            let request = slack_api.send_message(id.clone(), r.to_string());
            Some(request)
        });
    futures::future::try_join_all(send_requests).await?;
    tracing::info!("All messages were sent.");
    Ok(())
}
