// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Erooster Mail Server
//!
//! Erooster is a rust native imap server build on modern solutions.
//! The goal being easy to setup, use and maintain for smaller mail servers
//! while being also fast and efficient.
//!
#![allow(clippy::missing_panics_doc, clippy::items_after_statements)]

use erooster_core::{
    backend::{database::get_database, storage::get_storage},
    panic_handler::EroosterPanicMessage,
};
use {
    clap::{self, Parser},
    color_eyre::{self, eyre::Result},
    tokio::{
        self,
        signal::unix::{signal, SignalKind},
    },
    tokio_util::sync::CancellationToken,
    tracing::{error, info},
    tracing_error::ErrorLayer,
    tracing_subscriber::{self, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter},
};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long, default_value = "./config.yml")]
    config: String,
}

#[tokio::main]
#[allow(clippy::too_many_lines, clippy::redundant_pub_crate)]
async fn main() -> Result<()> {
    // Setup logging and metrics
    let builder = color_eyre::config::HookBuilder::default().panic_message(EroosterPanicMessage);
    let (panic_hook, eyre_hook) = builder.into_hooks();
    eyre_hook.install()?;

    // Get arfs and config
    let args = Args::parse();
    info!("Starting ERooster Server");
    let config = erooster_core::get_config(args.config).await?;

    // Setup the rest of our logging
    tracing_subscriber::Registry::default()
        .with(ErrorLayer::default())
        .with(tracing_subscriber::fmt::Layer::default())
        .with(EnvFilter::from_default_env())
        .init();

    // Make panics pretty
    let next = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let report = panic_hook.panic_report(panic_info);
        eprintln!("{report}");
        next(panic_info);
    }));

    // Continue loading database and storage
    let database = get_database(&config).await?;
    let storage = get_storage(database.clone(), config.clone());

    // Get SIGTERMs
    let mut sigterms = signal(SignalKind::terminate())?;
    let shutdown_flag = CancellationToken::new();

    // Startup servers
    erooster_imap::start(&config, &database, &storage)?;

    let config_clone = config.clone();
    tokio::spawn(async move {
        if let Err(e) = erooster_web::start(&config_clone).await {
            error!("Unable to start webserver: {e:?}");
        }
    });

    let shutdown_flag_clone = shutdown_flag.clone();
    erooster_smtp::servers::start(config.clone(), &database, &storage, shutdown_flag_clone).await?;

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            cleanup(&shutdown_flag);
        }
        _ = sigterms.recv() => {
            cleanup(&shutdown_flag);
        }
    }

    Ok(())
}

fn cleanup(shutdown_flag: &CancellationToken) {
    info!("Received shutdown signal. Cleaning up");
    shutdown_flag.cancel();
    info!("Shutdown complete");
}
