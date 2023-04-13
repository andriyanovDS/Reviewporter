use color_eyre::Result;
use serde::Deserialize;
use std::fs::File;
use std::{io::Read, path::Path};

mod azure;

#[derive(Deserialize, Debug)]
struct Hosting {
    base_url: url::Url,
    token: String,
    project: String,
    repositories: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct Config {
    azure: Option<Hosting>,
}

pub async fn run(config_path: &Path) -> Result<()> {
    let mut file = File::open(config_path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let config = toml::from_str::<Config>(&content)?;

    if let Some(azure_hosting) = config.azure {
        if !azure_hosting.project.is_empty() {
            let azure = azure::AzureHostingService::new(
                azure_hosting.token,
                azure_hosting.base_url,
                azure_hosting.project,
                azure_hosting.repositories,
            );
            let results = azure.pull_requests().await?;
            for result in results {
                println!("{result}");
            }
        }
    }
    Ok(())
}
