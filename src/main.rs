use clap::Parser;
use color_eyre::Result;
use reviewporter::cli::{Cli, Command};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    configure_logging()?;

    tracing::info!("Config path: {:?}", cli.config);
    match cli.command {
        Command::AddReviewers {
            repository,
            request_id,
        } => reviewporter::add_reviewers(&cli.config, request_id, repository).await,
        Command::SendReports { repositories } => {
            reviewporter::send_reports(repositories, &cli.config).await
        }
    }
}

fn configure_logging() -> Result<()> {
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1")
    }
    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "verbose")
    }
    let format = fmt::format()
        .with_source_location(false)
        .with_file(false)
        .with_target(false)
        .with_timer(fmt::time::SystemTime)
        .compact();

    fmt::fmt()
        .event_format(format)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    Ok(())
}
