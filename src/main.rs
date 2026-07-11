mod compiler;
mod config;
mod resolver;
mod routes;
mod templates;

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "typst_letter=info".into()),
        )
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".into());
    let config = config::Config::load(Path::new(&config_path))?;
    config.check_templates_dir()?;
    tracing::info!(
        listen = %config.listen,
        templates_dir = %config.templates_dir.display(),
        "starting typst-letter"
    );

    let pool = compiler::CompilerPool::new(&config)?;
    let state = routes::AppState {
        config: Arc::new(config),
        pool,
    };
    let listener = tokio::net::TcpListener::bind(state.config.listen)
        .await
        .with_context(|| format!("cannot bind {}", state.config.listen))?;
    axum::serve(listener, routes::router(state))
        .await
        .context("server error")?;
    Ok(())
}
