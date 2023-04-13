use color_eyre::Result;
use reviewporter;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<()> {
    configure_logging()?;
    let Some(config_path) = std::env::args().skip(1).next() else {
        println!("Usage: revp <path to config>");
        return Ok(());
    };
    tracing::info!("Config path: {config_path}");
    reviewporter::run(&std::path::Path::new(&config_path)).await
}

fn configure_logging() -> Result<()> {
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1")
    }
    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info")
    }
    let format = fmt::format()
        .with_source_location(false)
        .with_file(false)
        .with_target(false)
        .with_timer(fmt::time::SystemTime::default())
        .compact();

    fmt::fmt()
        .event_format(format)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    Ok(())
}
