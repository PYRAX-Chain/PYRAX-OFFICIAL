//! PYRAX Chain Observer - Main Entry Point
//!
//! Chain observability and metrics service for the PYRAX blockchain.

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, error};

mod config;
mod error;
mod observer;
mod metrics;
mod api;
mod state;
mod crawler;
mod alert;
mod scheduler;

use config::Config;
use state::AppState;

/// PYRAX Chain Observer - Blockchain Monitoring Service
#[derive(Parser, Debug)]
#[command(name = "pyrax-metrics")]
#[command(author = "PYRAX Team")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Chain Observer and Metrics Service for PYRAX Blockchain")]
struct Args {
    /// Path to configuration file
    #[arg(short, long, env = "PYRAX_METRICS_CONFIG")]
    config: PathBuf,
    
    /// Override log level
    #[arg(long, env = "PYRAX_METRICS_LOG_LEVEL")]
    log_level: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present
    dotenv::dotenv().ok();

    // Parse command line arguments
    let args = Args::parse();
    
    // Load configuration
    let config = Config::load(&args.config)?;
    
    // Initialize logging
    let log_level = args.log_level
        .unwrap_or_else(|| config.logging.level.clone());
    
    init_logging(&log_level, config.logging.json)?;
    
    info!(
        "Starting PYRAX Chain Observer v{}",
        env!("CARGO_PKG_VERSION")
    );
    info!("Network: {}", config.chain.network);
    info!("Monitoring {} node(s)", config.nodes.endpoints.len());
    
    // Create shared application state
    let app_state = AppState::new(config.clone());
    
    // Create the chain observer with shared state
    let observer = observer::ChainObserver::new(config.clone(), app_state.clone());
    
    // Create node discovery crawler
    let node_crawler = crawler::NodeCrawler::new(
        config.crawler.clone(),
        config.nodes.endpoints.clone(),
    ).with_app_state(app_state.clone());
    let discovered_nodes = node_crawler.discovered_nodes();
    app_state.set_discovered_nodes(discovered_nodes);
    
    // Create metrics server with shared state
    let metrics_server = metrics::MetricsServer::new(config.metrics.clone(), app_state.clone());
    
    // Create API server with shared state
    let api_server = api::ApiServer::new(config.api.clone(), app_state.clone());
    
    // Send Startup Notification
    if config.telegram.enabled {
        info!("Sending startup notification to Telegram...");
        app_state.alert_manager().send_alert("🤖 System", "<b>PYRAX Observer Started</b>\nMonitoring initialized.").await;
    }

    // Create scheduler for periodic tasks
    let scheduler = scheduler::Scheduler::new(app_state.clone());

    // Start all services concurrently
    tokio::select! {
        result = observer.run() => {
            if let Err(e) = result {
                error!("Observer error: {}", e);
            }
        }
        result = node_crawler.run() => {
            if let Err(e) = result {
                error!("Crawler error: {}", e);
            }
        }
        result = metrics_server.run() => {
            if let Err(e) = result {
                error!("Metrics server error: {}", e);
            }
        }
        result = api_server.run() => {
            if let Err(e) = result {
                error!("API server error: {}", e);
            }
        }
        result = scheduler.run() => {
            if let Err(e) = result {
                error!("Scheduler error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
        }
    }
    
    info!("PYRAX Chain Observer stopped");
    Ok(())
}

/// Initialize logging with tracing
fn init_logging(level: &str, json: bool) -> Result<()> {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};
    
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));
    
    if json {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer())
            .init();
    }
    
    Ok(())
}
