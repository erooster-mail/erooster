//! Erooster Mail Server
//!
//! Erooster is a rust native imap server build on modern solutions.
//! The goal being easy to setup, use and maintain for smaller mail servers
//! while being also fast and efficient.
//!
#![feature(string_remove_matches)]
#![deny(unsafe_code)]
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

use std::{path::Path, sync::Arc};

use clap::Parser;
use color_eyre::eyre::Result;
use erooster::config::Config;
use tokio::signal;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long, default_value = "./config.yml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    info!("Starting ERooster Imap Server");
    let config = if Path::new(&args.config).exists() {
        Arc::new(Config::load(args.config).await?)
    } else if Path::new("/etc/erooster/config.yml").exists() {
        Arc::new(Config::load("/etc/erooster/config.yml").await?)
    } else if Path::new("/etc/erooster/config.yaml").exists() {
        Arc::new(Config::load("/etc/erooster/config.yaml").await?)
    } else {
        error!("No config file found. Please follow the readme.");
        color_eyre::eyre::bail!("No config file found");
    };
    erooster::imap_servers::start(Arc::clone(&config))?;
    erooster::smtp_servers::start(config)?;

    match signal::ctrl_c().await {
        Ok(()) => {}
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
            // we also shut down in case of error
        }
    }
    Ok(())
}
