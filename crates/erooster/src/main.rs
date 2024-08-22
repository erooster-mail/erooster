// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Erooster Mail Server
//!
//! Erooster is a rust native imap server build on modern solutions.
//! The goal being easy to setup, use and maintain for smaller mail servers
//! while being also fast and efficient.
//!
#![feature(string_remove_matches)]
#![deny(unsafe_code, clippy::unwrap_used)]
#![warn(
    clippy::cognitive_complexity,
    clippy::branches_sharing_code,
    clippy::imprecise_flops,
    clippy::missing_const_for_fn,
    clippy::mutex_integer,
    clippy::path_buf_push_overwrite,
    clippy::redundant_pub_crate,
    clippy::pedantic,
    clippy::dbg_macro,
    clippy::todo,
    clippy::fallible_impl_from,
    clippy::filetype_is_file,
    clippy::suboptimal_flops,
    clippy::fn_to_numeric_cast_any,
    clippy::if_then_some_else_none,
    clippy::imprecise_flops,
    clippy::lossy_float_literal,
    clippy::panic_in_result_fn,
    clippy::clone_on_ref_ptr
)]
#![warn(missing_docs)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::items_after_statements)]

use std::sync::Arc;

use erooster_core::{
    backend::{database::get_database, storage::get_storage},
    panic_handler::EroosterPanicMessage,
};
use erooster_deps::{
    cfg_if::cfg_if,
    clap::{self, Parser},
    color_eyre::{self, eyre::Result},
    opentelemetry,
    tokio::{
        self,
        signal::unix::{signal, SignalKind},
        sync::Mutex,
    },
    tokio_util::sync::CancellationToken,
    tracing::{error, info, warn},
    tracing_error, tracing_subscriber,
    yaque::{recovery::recover, Receiver, ReceiverBuilder},
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
    cfg_if! {
        if #[cfg(feature = "jaeger")] {
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;
            use tracing_subscriber::EnvFilter;
            let tracer = opentelemetry_jaeger::new_agent_pipeline()
                .with_service_name(env!("CARGO_PKG_NAME"))
                .with_auto_split_batch(true)
                .install_batch(opentelemetry::runtime::Tokio)?;
            tracing_subscriber::Registry::default()
                .with(ErrorLayer::default())
                .with(tracing_subscriber::fmt::Layer::default())
                .with(EnvFilter::from_default_env())
                .with(tracing_opentelemetry::layer().with_tracer(tracer))
                .init();
        } else {
            use tracing_error::ErrorLayer;
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;
            use tracing_subscriber::EnvFilter;
            tracing_subscriber::Registry::default()
                .with(ErrorLayer::default())
                .with(tracing_subscriber::fmt::Layer::default())
                .with(EnvFilter::from_default_env())
                .init();
        }
    }

    // Make panics pretty
    let next = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let report = panic_hook.panic_report(panic_info);
        if report
            .to_string()
            .contains("Job `erooster_core::smtp_servers::sending::send_email_job` failed")
        {
            error!("Job `erooster_core::smtp_servers::sending::send_email_job` failed");
        } else {
            eprintln!("{report}");
            next(panic_info);
        }
    }));

    // Continue loading database and storage
    let database = get_database(&config).await?;
    let storage = get_storage(database.clone(), config.clone());

    // Start listening for tasks
    let mut receiver = ReceiverBuilder::default()
        .save_every_nth(None)
        .open(config.task_folder.clone());
    if let Err(e) = receiver {
        warn!("Unable to open receiver: {:?}. Trying to recover.", e);
        recover(&config.task_folder)?;
        receiver = ReceiverBuilder::default()
            .save_every_nth(None)
            .open(config.task_folder.clone());
        info!("Recovered queue successfully");
    }

    // Get SIGTERMs
    let mut sigterms = signal(SignalKind::terminate())?;
    let shutdown_flag = CancellationToken::new();

    match receiver {
        Ok(receiver) => {
            let receiver = Arc::new(Mutex::const_new(receiver));

            // Startup servers
            erooster_imap::start(config.clone(), &database, &storage)?;

            let config_clone = config.clone();
            tokio::spawn(async move {
                erooster_web::start(&config_clone)
                    .await
                    .expect("Unable to start webserver");
            });

            let receiver_clone: Arc<Mutex<Receiver>> = Arc::clone(&receiver);
            let shutdown_flag_clone = shutdown_flag.clone();
            // We do need the let here to make sure that the runner is bound to the lifetime of main.
            erooster_smtp::servers::start(
                config.clone(),
                &database,
                &storage,
                shutdown_flag_clone,
                receiver,
            )
                .await?;

            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    cleanup(&receiver_clone, &shutdown_flag).await;
                }
                _ = sigterms.recv() => {
                    cleanup(&receiver_clone, &shutdown_flag).await;
                }
            }
        }
        Err(e) => {
            error!("Unable to open receiver: {:?}. Giving up.", e);
        }
    }
    Ok(())
}

async fn cleanup(receiver: &Arc<Mutex<Receiver>>, shutdown_flag: &CancellationToken) {
    info!("Received ctr-c. Cleaning up");
    shutdown_flag.cancel();
    opentelemetry::global::shutdown_tracer_provider();
    {
        let mut lock = receiver.lock().await;

        info!("Gained lock. Saving queue");
        lock.save().expect("Unable to save queue");
        info!("Saved queue. Exiting");
    };
}
