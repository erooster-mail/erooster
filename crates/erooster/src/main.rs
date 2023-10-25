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

use clap::Parser;
use color_eyre::eyre::Result;
use erooster_core::{
    backend::{database::get_database, storage::get_storage},
    panic_handler::EroosterPanicMessage,
};
use tokio::signal;
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long, default_value = "./config.yml")]
    config: String,
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
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
    cfg_if::cfg_if! {
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

    // Startup servers
    erooster_imap::start(config.clone(), &database, &storage)?;
    // We do need the let here to make sure that the runner is bound to the lifetime of main.
    erooster_smtp::servers::start(config.clone(), &database, &storage).await?;

    erooster_web::start(&config).await?;

    match signal::ctrl_c().await {
        Ok(()) => {}
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
            // we also shut down in case of error
        }
    }
    opentelemetry::global::shutdown_tracer_provider();
    Ok(())
}
