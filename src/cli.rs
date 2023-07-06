use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Provides a configuration file
    #[arg(short, long, value_name = "FILE")]
    pub config: PathBuf,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Add reviewers to the pull request
    AddReviewers {
        /// Pull request's repository name
        #[arg(short, long)]
        repository: String,
        /// Pull request's id
        #[arg(long)]
        request_id: String,
    },
    /// Send reports with not reviewed pull requests to reviewers
    SendReports {
        /// List of repositories
        repositories: Vec<String>,
    },
}
