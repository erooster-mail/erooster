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

use clap::Parser;
use color_eyre::eyre::Result;
use erooster_core::{
    backend::{database::get_database, storage::get_storage},
    panic_handler::EroosterPanicMessage,
};
use std::borrow::Cow;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info, warn};
use tracing_error::ErrorLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

static COMPRESSED_DEPENDENCY_LIST: &[u8] = auditable::inject_dependency_list!();

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
    let mut _guard;
    if config.sentry {
        let filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
            .or_else(|_| tracing_subscriber::EnvFilter::try_new("info"))?;
        cfg_if::cfg_if! {
            if #[cfg(feature = "jaeger")] {
                let tracer = opentelemetry_jaeger::new_agent_pipeline().with_service_name(env!("CARGO_PKG_NAME")).with_auto_split_batch(true).install_batch(opentelemetry::runtime::Tokio)?;
                tracing_subscriber::Registry::default()
                    .with(sentry::integrations::tracing::layer())
                    .with(filter_layer)
                    .with(tracing_subscriber::fmt::Layer::default())
                    .with(ErrorLayer::default())
                    .with(tracing_opentelemetry::layer().with_tracer(tracer))
                    .init();
            } else {
                 tracing_subscriber::Registry::default()
                    .with(sentry::integrations::tracing::layer().event_filter(|md| match md.level() {
                        &tracing::Level::ERROR => sentry::integrations::tracing::EventFilter::Exception,
                        &tracing::Level::WARN | &tracing::Level::INFO | &tracing::Level::DEBUG => sentry::integrations::tracing::EventFilter::Breadcrumb,
                        &tracing::Level::TRACE => sentry::integrations::tracing::EventFilter::Ignore,
                    }))
                    .with(filter_layer)
                    .with(tracing_subscriber::fmt::Layer::default())
                    .with(ErrorLayer::default())
                    .with(tracing_opentelemetry::layer())
                    .init();
            }
        }

        info!("Sentry logging is enabled. Change the config to disable it.");

        _guard = sentry::init((
            "https://49e511ff807e45ffa19be1c63cfda26c@o105177.ingest.sentry.io/6458648",
            sentry::ClientOptions {
                release: Some(Cow::Owned(format!(
                    "{}@{}:{}",
                    env!("CARGO_PKG_NAME"),
                    env!("CARGO_PKG_VERSION"),
                    env!("VERGEN_GIT_SHA_SHORT")
                ))),
                traces_sample_rate: 1.0,
                enable_profiling: true,
                profiles_sample_rate: 1.0,
                ..Default::default()
            },
        ));
    } else {
        info!("Sentry logging is disabled. Change the config to enable it.");
        let filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
            .or_else(|_| tracing_subscriber::EnvFilter::try_new("info"))?;
        cfg_if::cfg_if! {
            if #[cfg(feature = "jaeger")] {
                let tracer = opentelemetry_jaeger::new_agent_pipeline().with_service_name(env!("CARGO_PKG_NAME")).with_auto_split_batch(true).install_batch(opentelemetry::runtime::Tokio)?;
                tracing_subscriber::Registry::default()
                    .with(sentry::integrations::tracing::layer())
                    .with(filter_layer)
                    .with(tracing_subscriber::fmt::Layer::default())
                    .with(ErrorLayer::default())
                    .with(tracing_opentelemetry::layer().with_tracer(tracer))
                    .init();
            } else {
                 tracing_subscriber::Registry::default()
                    .with(sentry::integrations::tracing::layer())
                    .with(filter_layer)
                    .with(tracing_subscriber::fmt::Layer::default())
                    .with(ErrorLayer::default())
                    .with(tracing_opentelemetry::layer())
                    .init();
            }
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
    let database = Arc::new(get_database(Arc::clone(&config)).await?);
    let storage = Arc::new(get_storage(Arc::clone(&database), Arc::clone(&config)));

    // Startup servers
    erooster_imap::start(
        Arc::clone(&config),
        Arc::clone(&database),
        Arc::clone(&storage),
    )?;
    // We do need the let here to make sure that the runner is bound to the lifetime of main.
    let _runner = erooster_smtp::servers::start(Arc::clone(&config), database, storage).await?;

    erooster_web::start(Arc::clone(&config)).await?;

    match signal::ctrl_c().await {
        Ok(()) => {}
        Err(err) => {
            // Actually use the data to work around a bug in rustc:
            // https://github.com/rust-lang/rust/issues/47384
            // On nightly you can use `test::black_box` instead of `println!`
            println!("{}", COMPRESSED_DEPENDENCY_LIST[0]);
            error!("Unable to listen for shutdown signal: {}", err);
            // we also shut down in case of error
        }
    }
    opentelemetry::global::shutdown_tracer_provider();
    Ok(())
}
