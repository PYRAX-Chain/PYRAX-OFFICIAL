use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use colored::Colorize;
use std::io;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod commands;
mod config;
mod dashboard;
mod docker;
mod instance;
mod ports;
mod remote;

use commands::{
    dashboard::DashboardArgs, docker::DockerCommand, init::InitArgs, logs::LogsArgs,
    remote::RemoteCommand, service::ServiceCommand, status::StatusArgs,
    cloud::CloudCommand, mesh::MeshCommand, snapshot::SnapshotCommand,
    update::UpdateArgs,
};

const BANNER: &str = r#"
  ___        __                       
 |_ _|_ __  / _| ___ _ __ _ __   ___  
  | || '_ \| |_ / _ \ '__| '_ \ / _ \ 
  | || | | |  _|  __/ |  | | | | (_) |
 |___|_| |_|_|  \___|_|  |_| |_|\___/ 
                                      
"#;

#[derive(Parser)]
#[command(
    name = "inferno",
    author = "PYRAX Chain",
    version,
    about = "🔥 Inferno CLI - Terminal-based node management for PYRAX blockchain",
    long_about = "Inferno CLI provides complete command-line management for PYRAX blockchain nodes.\n\nFeatures:\n  • Multi-instance node management\n  • Automatic port conflict resolution\n  • Docker container support\n  • SSH remote node management\n  • Web-based monitoring dashboard\n  • Live log streaming",
    after_help = "For more information, visit: https://github.com/PYRAX-Chain/PYRAX-OFFICIAL"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Output in JSON format
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new node with interactive setup wizard
    Init(InitArgs),

    /// Start the node
    Start {
        /// Instance number to start (default: 1)
        #[arg(short, long, default_value = "1")]
        instance: u32,

        /// Start all instances
        #[arg(long)]
        all: bool,

        /// Run in foreground (don't daemonize)
        #[arg(short, long)]
        foreground: bool,

        /// Network to connect to
        #[arg(short, long, default_value = "devnet")]
        network: String,
    },

    /// Stop the node
    Stop {
        /// Instance number to stop (default: 1)
        #[arg(short, long, default_value = "1")]
        instance: u32,

        /// Stop all instances
        #[arg(long)]
        all: bool,

        /// Force stop without graceful shutdown
        #[arg(short, long)]
        force: bool,
    },

    /// Restart the node
    Restart {
        /// Instance number to restart (default: 1)
        #[arg(short, long, default_value = "1")]
        instance: u32,

        /// Restart all instances
        #[arg(long)]
        all: bool,
    },

    /// Show node status
    Status(StatusArgs),

    /// List all node instances
    List {
        /// Show detailed information
        #[arg(short, long)]
        detailed: bool,
    },

    /// Stream live logs
    Logs(LogsArgs),

    /// Configuration management
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// Docker container management
    Docker {
        #[command(subcommand)]
        command: DockerCommand,
    },

    /// Remote node management via SSH
    Remote {
        #[command(subcommand)]
        command: RemoteCommand,
    },

    /// System service management (systemd/launchd)
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },

    /// Start the web dashboard
    Dashboard(DashboardArgs),

    /// Show connected peers
    Peers {
        /// Instance number (default: 1)
        #[arg(short, long, default_value = "1")]
        instance: u32,
    },

    /// Show mining status
    Mining {
        /// Instance number (default: 1)
        #[arg(short, long, default_value = "1")]
        instance: u32,

        /// Start mining
        #[arg(long)]
        start: bool,

        /// Stop mining
        #[arg(long)]
        stop: bool,
    },

    /// Attach to a running node console
    Attach {
        /// Instance number (default: 1)
        #[arg(short, long, default_value = "1")]
        instance: u32,
    },

    /// Show version information
    Version,

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Cloud deployment commands
    Cloud {
        #[command(subcommand)]
        command: CloudCommand,
    },

    /// VPN mesh network commands
    Mesh {
        #[command(subcommand)]
        command: MeshCommand,
    },

    /// Snapshot and backup commands
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },

    /// Update inferno-cli to latest or specific version
    Update(UpdateArgs),
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Show current configuration
    Show {
        /// Instance number (default: 1)
        #[arg(short, long, default_value = "1")]
        instance: u32,
    },

    /// Set a configuration value
    Set {
        /// Configuration key (e.g., network.p2p_port)
        key: String,

        /// Value to set
        value: String,

        /// Instance number (default: 1)
        #[arg(short, long, default_value = "1")]
        instance: u32,
    },

    /// Get a configuration value
    Get {
        /// Configuration key
        key: String,

        /// Instance number (default: 1)
        #[arg(short, long, default_value = "1")]
        instance: u32,
    },

    /// Reset configuration to defaults
    Reset {
        /// Instance number (default: 1)
        #[arg(short, long, default_value = "1")]
        instance: u32,

        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// Open configuration in editor
    Edit {
        /// Instance number (default: 1)
        #[arg(short, long, default_value = "1")]
        instance: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    // Handle commands
    match cli.command {
        Commands::Init(args) => {
            print_banner();
            commands::init::run(args).await?;
        }

        Commands::Start {
            instance,
            all,
            foreground,
            network,
        } => {
            commands::node::start(instance, all, foreground, &network).await?;
        }

        Commands::Stop {
            instance,
            all,
            force,
        } => {
            commands::node::stop(instance, all, force).await?;
        }

        Commands::Restart { instance, all } => {
            commands::node::restart(instance, all).await?;
        }

        Commands::Status(args) => {
            commands::status::run(args, cli.json).await?;
        }

        Commands::List { detailed } => {
            commands::node::list(detailed, cli.json).await?;
        }

        Commands::Logs(args) => {
            commands::logs::run(args).await?;
        }

        Commands::Config { command } => match command {
            ConfigCommand::Show { instance } => {
                commands::config::show(instance, cli.json).await?;
            }
            ConfigCommand::Set {
                key,
                value,
                instance,
            } => {
                commands::config::set(&key, &value, instance).await?;
            }
            ConfigCommand::Get { key, instance } => {
                commands::config::get(&key, instance, cli.json).await?;
            }
            ConfigCommand::Reset { instance, yes } => {
                commands::config::reset(instance, yes).await?;
            }
            ConfigCommand::Edit { instance } => {
                commands::config::edit(instance).await?;
            }
        },

        Commands::Docker { command } => {
            commands::docker::run(command).await?;
        }

        Commands::Remote { command } => {
            commands::remote::run(command).await?;
        }

        Commands::Service { command } => {
            commands::service::run(command).await?;
        }

        Commands::Dashboard(args) => {
            print_banner();
            commands::dashboard::run(args).await?;
        }

        Commands::Peers { instance } => {
            commands::node::peers(instance, cli.json).await?;
        }

        Commands::Mining {
            instance,
            start,
            stop,
        } => {
            commands::node::mining(instance, start, stop, cli.json).await?;
        }

        Commands::Attach { instance } => {
            commands::node::attach(instance).await?;
        }

        Commands::Version => {
            print_version();
        }

        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "inferno", &mut io::stdout());
        }

        Commands::Cloud { command } => {
            commands::cloud::run(command).await?;
        }

        Commands::Mesh { command } => {
            commands::mesh::run(command).await?;
        }

        Commands::Snapshot { command } => {
            commands::snapshot::run(command).await?;
        }

        Commands::Update(args) => {
            commands::update::run(args).await?;
        }
    }

    Ok(())
}

fn print_banner() {
    println!("{}", BANNER.bright_red());
    println!(
        "  {} v{}\n",
        "Inferno CLI".bright_white().bold(),
        env!("CARGO_PKG_VERSION")
    );
}

fn print_version() {
    println!(
        "{} {} v{}",
        "🔥".bright_red(),
        "Inferno CLI".bright_white().bold(),
        env!("CARGO_PKG_VERSION")
    );
    println!("  {} PYRAX Chain", "Author:".dimmed());
    println!("  {} {}", "Repo:".dimmed(), env!("CARGO_PKG_REPOSITORY"));
    println!(
        "  {} {}",
        "Platform:".dimmed(),
        std::env::consts::OS.to_uppercase()
    );
    println!("  {} {}", "Arch:".dimmed(), std::env::consts::ARCH);
}
