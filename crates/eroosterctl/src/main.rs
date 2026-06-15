// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! eroosterctl — command-line management tool for Erooster mail server.

#![allow(clippy::missing_panics_doc)]

use clap::{Parser, Subcommand};
use color_eyre::Result;
use erooster_core::panic_handler::EroosterPanicMessage;

mod config_cmd;
mod domain;
mod mailbox;
mod output;
mod queue;
mod status;
mod user;

use output::OutputFormat;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Erooster mail server management CLI",
    long_about = None,
    propagate_version = true
)]
struct Cli {
    /// Path to the configuration file
    #[arg(short, long, default_value = "./config.yml")]
    config: String,

    /// Output format
    #[arg(long, value_enum, default_value = "table")]
    output: OutputFormat,

    /// Disable color output (also respected: `NO_COLOR` env var, `TERM=dumb`)
    #[arg(long)]
    no_color: bool,

    /// Skip interactive confirmation prompts
    #[arg(short, long)]
    yes: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show live health status of all server components
    Status,

    /// Show version and build information
    Version,

    /// Configuration management
    #[command(subcommand, alias = "cfg")]
    Config(config_cmd::ConfigCommands),

    /// User management
    #[command(subcommand, alias = "u")]
    User(user::UserCommands),

    /// Mailbox management
    #[command(subcommand, alias = "mb")]
    Mailbox(mailbox::MailboxCommands),

    /// Outbound queue management
    #[command(subcommand, alias = "q")]
    Queue(queue::QueueCommands),

    /// Domain DNS record checks
    #[command(subcommand, alias = "dns")]
    Domain(domain::DomainCommands),
}

#[tokio::main]
async fn main() -> Result<()> {
    let builder =
        color_eyre::config::HookBuilder::default().panic_message(EroosterPanicMessage);
    let (panic_hook, eyre_hook) = builder.into_hooks();
    eyre_hook.install()?;

    let next = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("{}", panic_hook.panic_report(info));
        next(info);
    }));

    let cli = Cli::parse();

    // Install the aws-lc-rs crypto provider so rustls TLS checks don't panic.
    // This must happen before any TLS code runs (e.g. the status command's IMAP :993 check).
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    tracing_subscriber::fmt::init();

    if let Commands::Version = cli.command {
        print_version(cli.output, cli.no_color);
        return Ok(());
    }

    let config = erooster_core::get_config(cli.config).await?;

    match cli.command {
        Commands::Status => {
            status::run(&config, cli.output, cli.no_color).await?;
        }
        Commands::Config(cmd) => {
            config_cmd::run(cmd, &config, cli.output, cli.no_color).await?;
        }
        Commands::User(cmd) => {
            user::run(cmd, &config, cli.output, cli.yes, cli.no_color).await?;
        }
        Commands::Mailbox(cmd) => {
            mailbox::run(cmd, &config, cli.output).await?;
        }
        Commands::Queue(cmd) => {
            queue::run(cmd, &config, cli.output, cli.yes, cli.no_color).await?;
        }
        Commands::Domain(cmd) => {
            domain::run(cmd, &config, cli.output, cli.no_color).await?;
        }
        Commands::Version => unreachable!(),
    }

    Ok(())
}

fn print_version(format: OutputFormat, no_color: bool) {
    let version = env!("CARGO_PKG_VERSION");

    #[cfg(feature = "postgres")]
    let db_backend = "postgres";
    #[cfg(feature = "sqlite")]
    let db_backend = "sqlite";
    #[cfg(not(any(feature = "postgres", feature = "sqlite")))]
    let db_backend = "unknown";

    if format == OutputFormat::Json {
        let _ = output::print_json(&serde_json::json!({
            "version": version,
            "db_backend": db_backend,
        }));
    } else {
        let _ = output::color_disabled(no_color);
        println!("eroosterctl {version}");
        println!("DB backend:  {db_backend}");
    }
}
